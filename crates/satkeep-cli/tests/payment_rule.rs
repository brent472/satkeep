//! The payment rule through `satkeep`: with a beneficiary, `send` also
//! spends coins close to needing a check-in, `sign` says so, and their heir
//! delay restarts once the payment confirms. Heir delay 20, check-ins every
//! 10 blocks, refresh window 4: coins with 6 or more confirmations.

mod common;

use std::str::FromStr;

use common::{Cli, Holders, Node};
use satkeep_savings::bitcoin::{Address, Amount, Network};

fn fund(cli: &Cli, node: &Node, btc: f64) {
    let address = cli.ok(&["address"]);
    let address = Address::from_str(address.trim()).unwrap().require_network(Network::Regtest).unwrap();
    node.send_to(&address, Amount::from_btc(btc).unwrap());
    node.mine(1);
}

/// A coin with 8 confirmations (due for a refresh) and one with 1.
fn old_and_young(cli: &Cli, node: &Node) {
    fund(cli, node, 0.3);
    node.mine(6);
    fund(cli, node, 1.0);
}

fn create(cli: &Cli, holders: &Holders, extra: &[&str]) -> String {
    let k = holders.public_keys();
    let args = [
        &["create", "--network", "regtest", "--phone", &k[0], "--device", &k[1], "--helper", &k[2], "--heir", &k[3]][..],
        &["--heir-delay", "20", "--checkin-interval", "10", "--checkin-margin", "5", "--min-heir-delay", "10"],
        extra,
    ]
    .concat();
    cli.ok(&args)
}

#[test]
fn a_payment_refreshes_old_coins_and_their_heir_delay_restarts() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let holders = Holders::generate();
    let out = create(&cli, &holders, &["--refresh-window", "4"]);
    assert!(out.contains("Payments also refresh coins with 6 or more confirmations (about 60 minutes)"), "{out}");
    old_and_young(&cli, &node);

    let file = cli.path("pay.psbt");
    let out = cli.ok(&["send", "--to", &node.new_address().to_string(), "--amount", "0.01", "--out", &file]);
    assert!(out.contains("Also refreshes 1 coin (0.30000000 BTC) close to needing a check-in"), "{out}");
    let args = ["sign", "--psbt", &file, "--key-stdin"];
    let shown = common::expect_ok(&args, cli.run_with_stdin(&args, &format!("y\n{}\n", holders.phone.to_secret_string())));
    assert!(shown.contains("  Also refreshes 1 coin (0.30000000 BTC)"), "sign shows it before approving: {shown}");
    cli.sign(&file, &holders.helper);
    cli.ok(&["broadcast", "--psbt", &file]);
    node.mine(1);

    // The old coin had 9 confirmations at the payment. Eleven blocks later
    // it would have reached the heir delay; instead nothing is anywhere near.
    node.mine(11);
    let balance = cli.ok(&["balance"]);
    assert!(!balance.contains("OVERDUE"), "{balance}");
    assert!(balance.contains("12 confirmations  heir path opens in 8 blocks"), "the change's clock started at the payment: {balance}");
    assert!(!balance.contains("0.30000000 BTC"), "the old coin was spent: {balance}");
    let err = cli.fails(&["send", "--to", &node.new_address().to_string(), "--all", "--path", "heir", "--out", &cli.path("heir.psbt")]);
    assert!(err.contains("needs 20 confirmations before it can be spent this way"), "HEIR alone still can't spend: {err}");
}

#[test]
fn with_the_setting_off_payments_dont_refresh_coins() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let holders = Holders::generate();
    let out = create(&cli, &holders, &["--no-refresh-with-payments"]);
    assert!(out.contains("Payments don't refresh coins (turned off, for privacy)"), "{out}");
    let saved = std::fs::read_to_string(cli.wallet_dir().join("savings.json")).unwrap();
    assert!(saved.contains("\"refresh_with_payments\": false"), "{saved}");
    old_and_young(&cli, &node);
    let out = cli.ok(&["send", "--to", &node.new_address().to_string(), "--amount", "0.01", "--out", &cli.path("pay.psbt")]);
    assert!(!out.contains("refreshes") && !out.contains("Leaves out"), "nothing is forced in or reported: {out}");
}

#[test]
fn turning_it_on_again_in_the_settings_brings_it_back() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let holders = Holders::generate();
    create(&cli, &holders, &["--no-refresh-with-payments"]);
    let path = cli.wallet_dir().join("savings.json");
    let saved = std::fs::read_to_string(&path).unwrap();
    let on = saved.replace("\"refresh_with_payments\": false", "\"refresh_with_payments\": true,\n  \"refresh_window\": 4");
    assert_ne!(on, saved);
    std::fs::write(&path, on).unwrap();
    old_and_young(&cli, &node);
    let out = cli.ok(&["send", "--to", &node.new_address().to_string(), "--amount", "0.01", "--out", &cli.path("pay.psbt")]);
    assert!(out.contains("Also refreshes 1 coin"), "{out}");
}

#[test]
fn the_refresh_window_must_fit_inside_the_check_in_interval() {
    let node = Node::start();
    let holders = Holders::generate();
    for (window, why) in [("10", "equal to the interval"), ("11", "longer"), ("0", "zero")] {
        let cli = Cli::new(&node);
        let k = holders.public_keys();
        let err = cli.fails(&[
            "create", "--phone", &k[0], "--device", &k[1], "--helper", &k[2], "--heir", &k[3], "--heir-delay", "20",
            "--checkin-interval", "10", "--checkin-margin", "5", "--min-heir-delay", "10", "--refresh-window", window,
        ]);
        assert!(err.contains("refresh window"), "{why}: {err}");
        assert!(!cli.wallet_dir().join("savings.json").exists(), "{why}: nothing was created");
    }
    // The placeholder default (8,760 blocks) doesn't fit a 10-block interval.
    let cli = Cli::new(&node);
    let err = cli.fails(&[
        "create", "--phone", &holders.public_keys()[0], "--device", &holders.public_keys()[1],
        "--helper", &holders.public_keys()[2], "--heir", &holders.public_keys()[3], "--heir-delay", "20",
        "--checkin-interval", "10", "--checkin-margin", "5", "--min-heir-delay", "10",
    ]);
    assert!(err.contains("the refresh window (8760 blocks) must be shorter than the check-in interval (10 blocks)"), "{err}");
    // And it does fit the placeholder interval: coins older than about 10 months.
    let out = Cli::new(&node).ok(&[
        "create", "--phone", &holders.public_keys()[0], "--device", &holders.public_keys()[1],
        "--helper", &holders.public_keys()[2], "--heir", &holders.public_keys()[3],
    ]);
    assert!(out.contains("Payments also refresh coins with 43800 or more confirmations (about 10 months)"), "{out}");
    // Plain Savings has no check-in, so no payment rule either.
    let out = Cli::new(&node).create_plain(&holders);
    assert!(!out.contains("refresh"), "{out}");
}
