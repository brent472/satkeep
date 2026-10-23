//! `twofold add-heir`: plain 2-of-3 Savings gets a beneficiary by moving, in
//! one 2-of-3 payment, to a new wallet with the same PHONE, DEVICE and
//! HELPER plus HEIR.

mod common;

use std::str::FromStr;

use common::{Cli, Holders, Node, file_exists};
use twofold_savings::bitcoin::{Address, Amount, Network};

fn fund(cli: &Cli, node: &Node, btc: f64) {
    let address = cli.ok(&["address"]);
    let address = Address::from_str(address.trim()).unwrap().require_network(Network::Regtest).unwrap();
    node.send_to(&address, Amount::from_btc(btc).unwrap());
    node.mine(1);
}

/// Plain Savings with two confirmed coins, and an empty folder (another
/// `Cli`) for the wallet with the beneficiary.
fn plain_with_coins(node: &Node) -> (Cli<'_>, Cli<'_>, Holders) {
    let old = Cli::new(node);
    let holders = Holders::generate();
    old.create_plain(&holders);
    fund(&old, node, 1.0);
    fund(&old, node, 0.5);
    (old, Cli::new(node), holders)
}

fn add_heir_args<'a>(new: &'a str, heir: &'a str, out: &'a str) -> [&'a str; 13] {
    [
        "add-heir", "--new-wallet-dir", new, "--heir", heir, "--heir-delay", "20",
        "--checkin-interval", "10", "--checkin-margin", "5", "--out", out,
    ]
}

#[test]
fn adding_a_beneficiary_moves_savings_in_one_payment_and_the_heir_path_then_works() {
    let node = Node::start();
    let (old, new, holders) = plain_with_coins(&node);
    let new_dir = new.wallet_dir().display().to_string();
    let heir = holders.public_keys()[3].clone();
    let file = old.path("move.psbt");

    let out = old.ok(&add_heir_args(&new_dir, &heir, &file));
    assert!(out.contains(&format!("Created the new Savings wallet in {new_dir}.")), "{out}");
    assert!(out.contains("HEIR alone can approve once a coin has 20 confirmations."), "{out}");
    assert!(out.contains("Prepared the move of 2 coins, 1.50000000 BTC, into it, in one payment."), "{out}");
    assert!(out.contains("Needs approval from any 2 of PHONE, DEVICE, HELPER"), "{out}");

    // The approval prompt says what this is.
    let args = ["sign", "--psbt", &file, "--key-stdin"];
    let shown = common::expect_ok(&args, old.run_with_stdin(&args, &format!("y\n{}\n", holders.phone.to_secret_string())));
    assert!(shown.contains("Move to approve: every Savings coin goes to a new Savings wallet with the same"), "{shown}");
    assert!(shown.contains("HEIR can also approve alone after 20 blocks"), "{shown}");
    let err = old.fails(&["broadcast", "--psbt", &file]);
    assert!(err.contains("not enough keys have approved"), "one approval isn't enough: {err}");
    old.sign(&file, &holders.helper);
    let sent = old.ok(&["broadcast", "--psbt", &file]);
    assert!(sent.contains("use its folder (--wallet-dir) from now on"), "{sent}");
    node.mine(1);

    assert!(old.ok(&["balance"]).contains("Total: 0.00000000 BTC"));
    let balance = new.ok(&["balance"]);
    assert!(balance.contains("Total: 1.4999"), "{balance}");
    assert!(balance.contains("1 confirmation  heir path opens in 19 blocks"), "{balance}");

    // In the new wallet, HEIR alone can spend once the delay has passed.
    node.mine(19);
    let pay = new.path("heir.psbt");
    new.ok(&["send", "--to", &node.new_address().to_string(), "--all", "--path", "heir", "--out", &pay]);
    new.sign(&pay, &holders.heir);
    new.ok(&["broadcast", "--psbt", &pay]);
}

#[test]
fn add_heir_refuses_bad_input_and_writes_nothing() {
    let node = Node::start();
    let (old, new, holders) = plain_with_coins(&node);
    let new_dir = new.wallet_dir().display().to_string();
    let file = old.path("move.psbt");
    let k = holders.public_keys();

    let err = old.fails(&["add-heir", "--new-wallet-dir", &new_dir, "--out", &file]);
    assert!(err.contains("needs --heir"), "{err}");
    let err = old.fails(&add_heir_args(&new_dir, &k[2], &file));
    assert!(err.contains("HELPER and HEIR keys are the same"), "{err}");
    let mut too_late = add_heir_args(&new_dir, &k[3], &file);
    too_late[8] = "20"; // check-in interval equal to the heir delay
    let err = old.fails(&too_late);
    assert!(err.contains("must be shorter than the heir delay"), "{err}");
    assert!(!file_exists(&file) && !new.wallet_dir().exists(), "nothing written");

    // Not over an existing wallet either.
    new.create_plain(&Holders::generate());
    let err = old.fails(&add_heir_args(&new_dir, &k[3], &file));
    assert!(err.contains("already holds a Savings wallet"), "{err}");
}

#[test]
fn a_move_that_leaves_out_a_coin_is_refused_at_broadcast() {
    let node = Node::start();
    let (old, new, holders) = plain_with_coins(&node);
    let new_dir = new.wallet_dir().display().to_string();
    let file = old.path("move.psbt");
    old.ok(&add_heir_args(&new_dir, &holders.public_keys()[3], &file));
    old.sign(&file, &holders.phone);
    old.sign(&file, &holders.device);

    fund(&old, &node, 0.25); // a coin arrives after the move was prepared
    let err = old.fails(&["broadcast", "--psbt", &file]);
    assert!(err.contains("this move leaves out coin"), "{err}");
}
