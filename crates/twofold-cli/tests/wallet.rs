//! `twofold create`, `address`, `balance` and `export`.

mod common;

use std::str::FromStr;

use common::{Cli, Holders, Node};
use twofold_savings::bitcoin::{Address, Amount, Network};
use twofold_savings::{Delays, SavingsKeys, SavingsPolicy};

#[test]
fn creates_a_wallet_receives_and_shows_the_balance() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let out = cli.create(&Holders::generate(), 10, 20);
    assert!(out.contains("DEVICE alone can approve once a coin has 10 confirmations"), "{out}");

    let address = cli.ok(&["address"]);
    let address = Address::from_str(address.trim()).unwrap().require_network(Network::Regtest).unwrap();
    node.send_to(&address, Amount::from_btc(1.0).unwrap());

    let pending = cli.ok(&["balance"]);
    assert!(pending.contains("Waiting for confirmation: 1.00000000 BTC"), "{pending}");

    node.mine(1);
    let out = cli.ok(&["balance"]);
    assert!(out.contains("1 confirmation  device path opens in 9 blocks"), "{out}");
    node.mine(2);
    let out = cli.ok(&["balance"]);
    assert!(out.contains("Total: 1.00000000 BTC"), "{out}");
    assert!(out.contains("3 confirmations  device path opens in 7 blocks  heir path opens in 17 blocks"), "{out}");
    node.mine(7);
    let out = cli.ok(&["balance"]);
    assert!(out.contains("device path open  heir path opens in 10 blocks"), "{out}");
}

#[test]
fn hands_out_a_new_address_each_time() {
    let node = Node::start();
    let cli = Cli::new(&node);
    cli.create(&Holders::generate(), 10, 20);
    assert_ne!(cli.ok(&["address"]), cli.ok(&["address"]));
}

#[test]
fn export_matches_the_library() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let holders = Holders::generate();
    cli.create(&holders, 10, 20);
    let keys = SavingsKeys::new(
        holders.phone.public_key(0),
        holders.device.public_key(0),
        holders.device.public_key(1),
        holders.helper.public_key(0),
        holders.heir.public_key(0),
    )
    .unwrap();
    let policy = SavingsPolicy::new(keys, Delays::new(10, 20).unwrap(), Network::Regtest).unwrap();

    assert_eq!(cli.ok(&["export"]).trim(), policy.export_descriptor());
    let (receive, change) = policy.export_receive_and_change();
    assert_eq!(cli.ok(&["export", "--split"]), format!("{receive}\n{change}\n"));
}

#[test]
fn refuses_a_node_on_another_network() {
    let node = Node::start(); // regtest
    let cli = Cli::new(&node);
    let holders = Holders::generate();
    let keys = [holders.phone.public_key(0), holders.device.public_key(0), holders.device.public_key(1), holders.helper.public_key(0), holders.heir.public_key(0)]
        .map(|k| k.to_string());
    let err = cli.fails(&[
        "create", "--network", "signet",
        "--phone", &keys[0], "--device", &keys[1], "--device-r", &keys[2], "--helper", &keys[3], "--heir", &keys[4],
    ]);
    assert!(err.contains("is on regtest, but this wallet is on signet"), "{err}");
    assert!(!cli.wallet_dir().join("savings.json").exists());
}

#[test]
fn has_no_mainnet_option() {
    let node = Node::start();
    let cli = Cli::new(&node);
    for network in ["bitcoin", "main", "mainnet"] {
        let err = cli.fails(&["create", "--network", network, "--phone", "x", "--device", "x", "--device-r", "x", "--helper", "x", "--heir", "x"]);
        assert!(err.contains("invalid value"), "{err}");
    }
}

#[test]
fn refuses_to_create_over_an_existing_wallet() {
    let node = Node::start();
    let cli = Cli::new(&node);
    cli.create(&Holders::generate(), 10, 20);
    let first = std::fs::read_to_string(cli.wallet_dir().join("savings.json")).unwrap();
    let holders = Holders::generate();
    let keys = [holders.phone.public_key(0), holders.device.public_key(0), holders.device.public_key(1), holders.helper.public_key(0), holders.heir.public_key(0)]
        .map(|k| k.to_string());
    let err = cli.fails(&[
        "create", "--network", "regtest",
        "--phone", &keys[0], "--device", &keys[1], "--device-r", &keys[2], "--helper", &keys[3], "--heir", &keys[4],
    ]);
    assert!(err.contains("already holds a Savings wallet"), "{err}");
    assert_eq!(std::fs::read_to_string(cli.wallet_dir().join("savings.json")).unwrap(), first);
}

#[test]
fn refuses_a_policy_the_library_refuses() {
    // The CLI goes through SavingsKeys::new and Delays::new, so the library's
    // checks apply: here, HEIR reusing HELPER's key.
    let node = Node::start();
    let cli = Cli::new(&node);
    let holders = Holders::generate();
    let keys = [holders.phone.public_key(0), holders.device.public_key(0), holders.device.public_key(1), holders.helper.public_key(0)]
        .map(|k| k.to_string());
    let err = cli.fails(&[
        "create", "--network", "regtest",
        "--phone", &keys[0], "--device", &keys[1], "--device-r", &keys[2], "--helper", &keys[3], "--heir", &keys[3],
    ]);
    assert!(err.contains("HELPER and HEIR keys are the same"), "{err}");
    let err = cli.fails(&[
        "create", "--network", "regtest",
        "--phone", &keys[0], "--device", &keys[1], "--device-r", &keys[2], "--helper", &keys[3],
        "--heir", &holders.heir.public_key(0).to_string(), "--device-delay", "20", "--heir-delay", "20",
    ]);
    assert!(err.contains("must be longer than the device delay"), "{err}");
}

#[test]
fn refuses_a_key_with_the_same_receive_and_change_path() {
    // Found by fuzzing savings.json: a `<1;1>` key used to crash the policy
    // builder instead of being refused.
    let node = Node::start();
    let cli = Cli::new(&node);
    let keys = Holders::generate().public_keys();
    let same_twice = keys[2].replace("/<0;1>/*", "/<1;1>/*");
    let err = cli.fails(&[
        "create", "--phone", &keys[0], "--device", &keys[1], "--device-r", &same_twice,
        "--helper", &keys[3], "--heir", &keys[4],
    ]);
    assert!(err.contains("DEVICE_R key must be an extended public key with receive and change paths"), "{err}");
    assert!(!err.contains("panicked"), "{err}");
}
