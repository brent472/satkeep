//! `twofold sign` shows what is being approved and signs only on "y".

mod common;

use std::fs;
use std::str::FromStr;

use common::{Cli, Holders, Node, expect_failure, expect_ok};
use twofold_savings::bitcoin::{Address, Amount, Network};

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

#[test]
fn shows_outputs_change_fee_and_path_then_signs_on_y() {
    let node = Node::start();
    let (cli, holders) = funded(&node);
    let psbt = cli.path("pay.psbt");
    let to = node.new_address().to_string();
    cli.ok(&["send", "--to", &to, "--amount", "0.4", "--out", &psbt]);

    let args = ["sign", "--psbt", &psbt, "--key-stdin"];
    let input = format!("y\n{}\n", holders.phone.to_secret_string());
    let out = expect_ok(&args, cli.run_with_stdin(&args, &input));
    assert!(out.contains(&format!("  0.40000000 BTC to {to}\n")), "{out}");
    assert!(out.contains(" BTC comes back to Savings as change (bcrt1"), "{out}");
    assert!(out.contains("  Network fee: 0.0000"), "{out}");
    assert!(out.contains("  Approval path: any 2 of PHONE, DEVICE, HELPER"), "{out}");
    let (shown, signed) = out.split_once("Approve? (y/N) ").expect("asks before signing");
    assert!(shown.contains("Payment to approve:"));
    assert!(signed.contains("Added 1 signature"), "{out}");
}

#[test]
fn does_not_sign_unless_the_answer_is_y() {
    let node = Node::start();
    let (cli, holders) = funded(&node);
    let psbt = cli.path("pay.psbt");
    cli.ok(&["send", "--to", &node.new_address().to_string(), "--all", "--out", &psbt]);
    let unsigned = fs::read_to_string(&psbt).unwrap();
    let key = holders.phone.to_secret_string();

    let args = ["sign", "--psbt", &psbt, "--key-stdin"];
    for answer in ["n", "N", "no", "", "maybe", " ", "yy"] {
        let err = expect_failure(&args, cli.run_with_stdin(&args, &format!("{answer}\n{key}\n")));
        assert!(err.contains("not approved; nothing was signed"), "answer {answer:?}: {err}");
        assert_eq!(fs::read_to_string(&psbt).unwrap(), unsigned, "answer {answer:?} changed the PSBT");
    }
    // No input at all (end of file) is a no too.
    let err = expect_failure(&args, cli.run_with_stdin(&args, ""));
    assert!(err.contains("not approved"), "{err}");
    assert_eq!(fs::read_to_string(&psbt).unwrap(), unsigned);
}

#[test]
fn a_declined_payment_never_reads_the_key() {
    // The key file doesn't exist: if sign tried to read it, the error would
    // say so. Declining must stop before that.
    let node = Node::start();
    let (cli, _holders) = funded(&node);
    let psbt = cli.path("pay.psbt");
    cli.ok(&["send", "--to", &node.new_address().to_string(), "--all", "--out", &psbt]);
    let missing = cli.path("missing.key");
    let args = ["sign", "--psbt", &psbt, "--key-file", &missing];
    let err = expect_failure(&args, cli.run_with_stdin(&args, "n\n"));
    assert!(err.contains("not approved") && !err.contains("missing.key"), "{err}");
}

#[test]
fn names_the_delayed_path_and_your_own_addresses() {
    let node = Node::start();
    let (cli, holders) = funded(&node);
    node.mine(9);
    let own = cli.ok(&["address"]).trim().to_string();
    let psbt = cli.path("device.psbt");
    cli.ok(&["send", "--to", &own, "--all", "--path", "device", "--out", &psbt]);
    let args = ["sign", "--psbt", &psbt, "--key-stdin"];
    let out = expect_ok(&args, cli.run_with_stdin(&args, &format!("y\n{}\n", holders.device.to_secret_string())));
    assert!(out.contains(&format!("to {own} (one of your own Savings addresses)")), "{out}");
    assert!(out.contains("Approval path: DEVICE alone"), "{out}");
}

#[test]
fn refuses_a_payment_from_another_wallet() {
    let node = Node::start();
    let (ours, holders) = funded(&node);
    let (theirs, _) = funded(&node);
    let psbt = theirs.path("theirs.psbt");
    theirs.ok(&["send", "--to", &node.new_address().to_string(), "--all", "--out", &psbt]);
    let args = ["sign", "--psbt", &psbt, "--key-stdin"];
    let err = expect_failure(&args, ours.run_with_stdin(&args, &format!("y\n{}\n", holders.phone.to_secret_string())));
    assert!(err.contains("isn't an unspent coin of this Savings wallet"), "{err}");
}
