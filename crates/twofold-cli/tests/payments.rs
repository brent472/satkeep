//! `twofold send`, `sign` and `broadcast`, end to end on regtest.

mod common;

use std::str::FromStr;

use common::{Cli, Holders, Node, file_exists};
use twofold_savings::HolderKey;
use twofold_savings::bitcoin::key::Secp256k1;
use twofold_savings::bitcoin::{Address, Amount, Network};

/// Creates a wallet with 10/20-block delays and funds it with 1 BTC in a
/// coin that has 1 confirmation.
fn funded(node: &Node) -> (Cli<'_>, Holders) {
    let cli = Cli::new(node);
    let holders = Holders::generate();
    cli.create(&holders, 10, 20);
    let address = cli.ok(&["address"]);
    let address = Address::from_str(address.trim()).unwrap().require_network(Network::Regtest).unwrap();
    node.send_to(&address, Amount::from_btc(1.0).unwrap());
    node.mine(1);
    (cli, holders)
}

fn to(node: &Node) -> String {
    node.new_address().to_string()
}

#[test]
fn two_of_three_payment_needs_two_approvals_and_returns_change() {
    let node = Node::start();
    let (cli, holders) = funded(&node);
    let psbt = cli.path("pay.psbt");
    let out = cli.ok(&["send", "--to", &to(&node), "--amount", "0.4", "--out", &psbt]);
    assert!(out.contains("Prepared a payment of 0.40000000 BTC"), "{out}");
    assert!(out.contains("Needs approval from any 2 of PHONE, DEVICE, HELPER"), "{out}");

    let err = cli.fails(&["broadcast", "--psbt", &psbt]);
    assert!(err.contains("not enough keys have approved"), "{err}");

    assert!(cli.sign(&psbt, &holders.phone).contains("Added 1 signature"));
    let err = cli.fails(&["broadcast", "--psbt", &psbt]);
    assert!(err.contains("not enough keys have approved"), "one approval is not enough: {err}");

    let args = ["sign", "--psbt", &psbt, "--yes", "--key-stdin"];
    let again = common::expect_failure(&args, cli.run_with_stdin(&args, &holders.phone.to_secret_string()));
    assert!(again.contains("already approved"), "{again}");

    cli.sign(&psbt, &holders.helper);
    let out = cli.ok(&["broadcast", "--psbt", &psbt]);
    assert!(out.starts_with("Sent. Transaction ID: "), "{out}");

    node.mine(1);
    let balance = cli.ok(&["balance"]);
    assert!(balance.contains("Total: 0.5999"), "about 0.6 BTC of change should remain: {balance}");
}

#[test]
fn device_alone_waits_for_its_delay() {
    let node = Node::start();
    let (cli, holders) = funded(&node);
    let psbt = cli.path("device.psbt");
    let send = ["send", "--to", &to(&node), "--all", "--path", "device", "--out", &psbt];

    let err = cli.fails(&send);
    assert!(err.contains("needs 10 confirmations") && err.contains("has 1"), "{err}");
    node.mine(8);
    let err = cli.fails(&send);
    assert!(err.contains("has 9"), "one block early is still refused: {err}");
    assert!(!file_exists(&psbt), "no payment file is written when refused");

    node.mine(1);
    let out = cli.ok(&send);
    assert!(out.contains("Needs approval from DEVICE alone"), "{out}");
    cli.sign(&psbt, &holders.device);
    cli.ok(&["broadcast", "--psbt", &psbt]);
    node.mine(1);
    assert!(cli.ok(&["balance"]).contains("Total: 0.00000000 BTC"));
}

#[test]
fn heir_alone_waits_for_its_delay() {
    let node = Node::start();
    let (cli, holders) = funded(&node);
    let psbt = cli.path("heir.psbt");
    let send = ["send", "--to", &to(&node), "--all", "--path", "heir", "--out", &psbt];

    node.mine(18);
    let err = cli.fails(&send);
    assert!(err.contains("needs 20 confirmations") && err.contains("has 19"), "{err}");
    node.mine(1);
    cli.ok(&send);
    cli.sign(&psbt, &holders.heir);
    cli.ok(&["broadcast", "--psbt", &psbt]);
}

#[test]
fn helper_alone_cannot_send() {
    let node = Node::start();
    let (cli, holders) = funded(&node);
    let psbt = cli.path("pay.psbt");
    cli.ok(&["send", "--to", &to(&node), "--all", "--out", &psbt]);
    cli.sign(&psbt, &holders.helper);
    let err = cli.fails(&["broadcast", "--psbt", &psbt]);
    assert!(err.contains("not enough keys have approved"), "{err}");

    // Nor through a delayed path once it's open. A PSBT carries every leaf,
    // so HELPER can still add its 2-of-3 signature, but that alone
    // satisfies nothing.
    node.mine(19);
    let heir_psbt = cli.path("heir.psbt");
    cli.ok(&["send", "--to", &to(&node), "--all", "--path", "heir", "--out", &heir_psbt]);
    cli.sign(&heir_psbt, &holders.helper);
    let err = cli.fails(&["broadcast", "--psbt", &heir_psbt]);
    assert!(err.contains("not enough keys have approved"), "{err}");
}

#[test]
fn a_key_from_another_wallet_adds_nothing() {
    let node = Node::start();
    let (cli, _holders) = funded(&node);
    let psbt = cli.path("pay.psbt");
    cli.ok(&["send", "--to", &to(&node), "--all", "--out", &psbt]);
    let stranger = HolderKey::generate();
    let args = ["sign", "--psbt", &psbt, "--yes", "--key-stdin"];
    let err = common::expect_failure(&args, cli.run_with_stdin(&args, &stranger.to_secret_string()));
    assert!(err.contains("isn't part of this payment"), "{err}");
}

#[test]
fn send_refuses_bad_input() {
    let node = Node::start();
    let (cli, _holders) = funded(&node);
    let psbt = cli.path("pay.psbt");

    let secp = Secp256k1::new();
    let (secret, _) = secp.generate_keypair(&mut twofold_savings::bitcoin::secp256k1::rand::thread_rng());
    let signet = Address::p2tr(&secp, secret.public_key(&secp).x_only_public_key().0, None, Network::Signet);
    let err = cli.fails(&["send", "--to", &signet.to_string(), "--all", "--out", &psbt]);
    assert!(err.contains("isn't for regtest"), "{err}");

    let err = cli.fails(&["send", "--to", &to(&node), "--amount", "lots", "--out", &psbt]);
    assert!(err.contains("must be in BTC"), "{err}");
    let err = cli.fails(&["send", "--to", &to(&node), "--all", "--fee-rate", "0", "--out", &psbt]);
    assert!(err.contains("at least 1 sat/vB"), "{err}");
    let err = cli.fails(&["send", "--to", &to(&node), "--amount", "5", "--out", &psbt]);
    assert!(err.to_lowercase().contains("insufficient"), "{err}");
    assert!(!file_exists(&psbt));

    cli.ok(&["send", "--to", &to(&node), "--all", "--out", &psbt]);
    let err = cli.fails(&["send", "--to", &to(&node), "--all", "--out", &psbt]);
    assert!(err.contains("must not already exist"), "{err}");
}

#[test]
fn keygen_keys_can_create_a_wallet_and_approve() {
    // The one test that writes private keys to disk: fresh keys, into a
    // temporary folder deleted when the test ends.
    let node = Node::start();
    let cli = Cli::new(&node);

    let err = cli.fails(&["keygen", "--out", &cli.path("phone.txt")]);
    assert!(err.contains("must end in .key"), "{err}");

    let mut public = Vec::new();
    for name in ["phone", "device", "helper", "heir"] {
        let file = cli.path(&format!("{name}.key"));
        let out = cli.ok(&["keygen", "--out", &file]);
        let keys: Vec<&str> = out.lines().filter(|l| l.starts_with("  ")).map(str::trim).collect();
        let saved = HolderKey::from_secret_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(keys, [saved.public_key(0).to_string(), saved.public_key(1).to_string()]);
        public.push((keys[0].to_string(), keys[1].to_string()));
    }
    let err = cli.fails(&["keygen", "--out", &cli.path("phone.key")]);
    assert!(err.contains("must not already exist"), "{err}");

    let [(phone, _), (device, device_r), (helper, _), (heir, _)] = <[_; 4]>::try_from(public).unwrap();
    cli.ok(&[
        "create", "--network", "regtest", "--phone", &phone, "--device", &device, "--device-r", &device_r,
        "--helper", &helper, "--heir", &heir, "--device-delay", "10", "--heir-delay", "20",
    ]);
    let address = cli.ok(&["address"]);
    let address = Address::from_str(address.trim()).unwrap().require_network(Network::Regtest).unwrap();
    node.send_to(&address, Amount::from_btc(1.0).unwrap());
    node.mine(1);

    let psbt = cli.path("pay.psbt");
    cli.ok(&["send", "--to", &to(&node), "--all", "--out", &psbt]);
    cli.ok(&["sign", "--psbt", &psbt, "--yes", "--key-file", &cli.path("phone.key")]);
    cli.ok(&["sign", "--psbt", &psbt, "--yes", "--key-file", &cli.path("device.key")]);
    cli.ok(&["broadcast", "--psbt", &psbt]);
}
