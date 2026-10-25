//! `satkeep sign --ledger` and the wallet's Ledger registration file, with
//! no Ledger attached: everything it checks before it talks to the device.
//! Signing with a Ledger itself is tested against Ledger's emulator in
//! `satkeep-ledger`.

mod common;

use std::fs;
use std::str::FromStr;

use common::{Cli, Holders, Node, expect_failure};
use satkeep_savings::bitcoin::{Address, Amount, Network};

/// A fake approval code: it's never sent anywhere, and must never be shown.
const CODE: &str = "5a7c0de5a7c0de5a7c0de5a7c0de5a7c0de5a7c0de5a7c0de5a7c0de5a7c0de0";

fn payment(node: &Node) -> (Cli<'_>, String) {
    let cli = Cli::new(node);
    cli.create_plain(&Holders::generate());
    let address = cli.ok(&["address"]);
    let address = Address::from_str(address.trim()).unwrap().require_network(Network::Regtest).unwrap();
    node.send_to(&address, Amount::from_btc(1.0).unwrap());
    node.mine(1);
    let psbt = cli.path("pay.psbt");
    cli.ok(&["send", "--to", &node.new_address().to_string(), "--all", "--out", &psbt]);
    (cli, psbt)
}

fn ledger_file(cli: &Cli<'_>, descriptor: &str) {
    let file = serde_json::json!({ "name": "Satkeep Savings", "descriptor": descriptor, "approval_code": CODE });
    fs::write(cli.wallet_dir().join("ledger.json"), file.to_string()).unwrap();
}

#[test]
fn a_wallet_not_registered_on_a_ledger_is_refused_before_looking_for_one() {
    let node = Node::start();
    let (cli, psbt) = payment(&node);
    let unsigned = fs::read_to_string(&psbt).unwrap();
    let err = cli.fails(&["sign", "--psbt", &psbt, "--ledger"]);
    assert!(err.contains("isn't registered on a Ledger yet: run `satkeep ledger-register`"), "{err}");
    assert_eq!(fs::read_to_string(&psbt).unwrap(), unsigned);
}

#[test]
fn another_wallets_registration_is_refused_and_its_code_never_shown() {
    let node = Node::start();
    let (cli, psbt) = payment(&node);
    let other = Cli::new(&node);
    other.create_plain(&Holders::generate());
    let other_descriptor = other.ok(&["export"]);
    ledger_file(&cli, other_descriptor.trim());

    let args = ["sign", "--psbt", &psbt, "--ledger"];
    let out = cli.run(&args);
    let shown = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let err = expect_failure(&args, out);
    assert!(err.contains("is for another wallet: run `satkeep ledger-register`"), "{err}");
    assert!(!shown.contains(CODE) && !shown.contains(&CODE[..16]), "the approval code was shown: {shown}");
}

#[test]
fn an_unreadable_registration_is_refused_without_repeating_it() {
    let node = Node::start();
    let (cli, psbt) = payment(&node);
    let descriptor = cli.ok(&["export"]);
    // Valid JSON with a bad code, then not JSON at all.
    let bad_code = serde_json::json!({ "name": "Satkeep Savings", "descriptor": descriptor.trim(), "approval_code": "zz" });
    for text in [bad_code.to_string(), format!("not json {CODE}")] {
        fs::write(cli.wallet_dir().join("ledger.json"), &text).unwrap();
        let args = ["sign", "--psbt", &psbt, "--ledger"];
        let out = cli.run(&args);
        let shown = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let err = expect_failure(&args, out);
        assert!(err.contains("ledger.json"), "{err}");
        assert!(!shown.contains(CODE), "the file was repeated: {shown}");
    }
}

#[test]
fn ledger_signing_takes_no_key_and_no_yes() {
    let node = Node::start();
    let (cli, psbt) = payment(&node);
    let key_file = cli.path("phone.key");
    for extra in [vec!["--key-file", key_file.as_str()], vec!["--key-stdin"], vec!["--yes"]] {
        let mut args = vec!["sign", "--psbt", &psbt, "--ledger"];
        args.extend(extra);
        let err = cli.fails(&args);
        assert!(err.contains("cannot be used with"), "{args:?}: {err}");
    }
}
