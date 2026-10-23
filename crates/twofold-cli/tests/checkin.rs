//! `twofold checkin`, the interval rule, and the due/overdue warnings in
//! `twofold balance`. Delays 10 (device) and 20 (heir); check-ins every 5
//! blocks with a 2-block margin.

mod common;

use std::str::FromStr;

use common::{Cli, Holders, Node, expect_failure};
use twofold_savings::bitcoin::{Address, Amount, Network};

fn wallet(node: &Node) -> (Cli<'_>, Holders) {
    let cli = Cli::new(node);
    let holders = Holders::generate();
    cli.create_with_checkin(&holders, 10, 20, 5, 2);
    (cli, holders)
}

/// Sends `btc` to a new Savings address and mines it: the coin has 1
/// confirmation.
fn fund(cli: &Cli, node: &Node, btc: f64) {
    let address = cli.ok(&["address"]);
    let address = Address::from_str(address.trim()).unwrap().require_network(Network::Regtest).unwrap();
    node.send_to(&address, Amount::from_btc(btc).unwrap());
    node.mine(1);
}

/// Prepares a check-in and approves it as PHONE and HELPER, answering y.
fn prepare_and_approve(cli: &Cli, holders: &Holders, file: &str) -> String {
    let out = cli.ok(&["checkin", "--out", file]);
    let args = ["sign", "--psbt", file, "--key-stdin"];
    let shown = common::expect_ok(&args, cli.run_with_stdin(&args, &format!("y\n{}\n", holders.phone.to_secret_string())));
    cli.sign(file, &holders.helper);
    format!("{out}{shown}")
}

#[test]
fn an_interval_at_or_past_the_device_delay_is_refused() {
    let node = Node::start();
    let holders = Holders::generate();
    let keys = holders.public_keys();
    for (interval, margin, why) in [
        ("10", "1", "equal to the device delay"),
        ("11", "1", "past the device delay"),
        ("65535", "1", "far past it"),
        ("9", "2", "leaves less than the margin"),
        ("0", "1", "zero"),
    ] {
        let cli = Cli::new(&node);
        let args = [
            "create", "--phone", &keys[0], "--device", &keys[1], "--device-r", &keys[2], "--helper", &keys[3],
            "--heir", &keys[4], "--device-delay", "10", "--heir-delay", "20",
            "--checkin-interval", interval, "--checkin-margin", margin,
        ];
        let err = cli.fails(&args);
        assert!(err.contains("check-in"), "{why}: {err}");
        assert!(!cli.wallet_dir().join("savings.json").exists(), "{why}: nothing was created");
    }

    // The placeholder defaults don't fit a 10-block device delay either.
    let cli = Cli::new(&node);
    let err = cli.fails(&[
        "create", "--phone", &keys[0], "--device", &keys[1], "--device-r", &keys[2], "--helper", &keys[3],
        "--heir", &keys[4], "--device-delay", "10", "--heir-delay", "20",
    ]);
    assert!(err.contains("the check-in interval (26280 blocks) must be shorter than the device delay (10 blocks)"), "{err}");

    // And they do fit the placeholder device delay.
    let cli = Cli::new(&node);
    let out = cli.ok(&[
        "create", "--phone", &keys[0], "--device", &keys[1], "--device-r", &keys[2], "--helper", &keys[3], "--heir", &keys[4],
    ]);
    assert!(out.contains("Check in every 26280 blocks"), "{out}");
}

#[test]
fn warnings_appear_at_exactly_the_right_confirmations() {
    let node = Node::start();
    let (cli, _) = wallet(&node);
    fund(&cli, &node, 1.0);

    let at = |confirmations: u32| {
        let out = cli.ok(&["balance"]);
        assert!(out.contains(&format!("  {confirmations} confirmation")), "expected {confirmations}: {out}");
        out
    };

    let out = at(1);
    assert!(out.contains("Next check-in due in 4 blocks (every 5 blocks)."), "{out}");
    assert!(!out.contains("DUE") && !out.contains("OVERDUE"), "{out}");

    node.mine(3);
    let out = at(4);
    assert!(out.contains("Next check-in due in 1 block (every 5 blocks)."), "{out}");
    assert!(out.contains("device path opens in 6 blocks"), "{out}");
    assert!(!out.contains("DUE"), "one block before the interval: {out}");

    node.mine(1);
    let out = at(5);
    assert!(out.contains("CHECK-IN DUE"), "at the interval: {out}");
    assert!(out.contains("Check-in due: the oldest coin has 5 confirmations, and check-ins are due every 5 blocks."), "{out}");
    assert!(out.contains("Its device path opens in 5 blocks."), "{out}");
    assert!(!out.contains("OVERDUE"), "{out}");

    node.mine(4);
    let out = at(9);
    assert!(out.contains("CHECK-IN DUE") && !out.contains("OVERDUE"), "one block before the device delay: {out}");
    assert!(out.contains("Its device path opens in 1 block."), "{out}");

    node.mine(1);
    let out = at(10);
    assert!(out.contains("device path open") && out.contains("OVERDUE"), "at the device delay: {out}");
    assert!(out.contains("WARNING: check-in overdue. DEVICE alone can already spend 1 coin."), "{out}");
}

#[test]
fn a_check_in_restarts_both_delays_and_clears_the_warning() {
    let node = Node::start();
    let (cli, holders) = wallet(&node);
    fund(&cli, &node, 1.0);
    fund(&cli, &node, 0.5);
    node.mine(8); // first coin at 10 confirmations: overdue
    assert!(cli.ok(&["balance"]).contains("OVERDUE"));

    let file = cli.path("checkin.psbt");
    let out = prepare_and_approve(&cli, &holders, &file);
    assert!(out.contains("Prepared a check-in of 2 coins, 1.50000000 BTC, into one new Savings address."), "{out}");
    assert!(out.contains("Check-in to approve: every Savings coin goes into one new Savings address,"), "{out}");
    assert!(out.contains("comes back to Savings as change"), "{out}");
    assert!(out.contains("Approval path: any 2 of PHONE, DEVICE, HELPER"), "{out}");
    cli.ok(&["broadcast", "--psbt", &file]);
    node.mine(1);

    let out = cli.ok(&["balance"]);
    assert!(out.contains("1 confirmation  device path opens in 9 blocks  heir path opens in 19 blocks"), "{out}");
    assert!(out.contains("Next check-in due in 4 blocks"), "{out}");
    assert!(!out.contains("DUE") && !out.contains("OVERDUE"), "{out}");
    assert_eq!(out.matches(" confirmation").count(), 1, "one coin now: {out}");

    // The delays really did restart: DEVICE alone is refused for the new coin.
    let to = node.new_address().to_string();
    let err = cli.fails(&["send", "--to", &to, "--all", "--path", "device", "--out", &cli.path("d.psbt")]);
    assert!(err.contains("needs 10 confirmations") && err.contains("has 1"), "{err}");
    let err = cli.fails(&["send", "--to", &to, "--all", "--path", "heir", "--out", &cli.path("h.psbt")]);
    assert!(err.contains("needs 20 confirmations") && err.contains("has 1"), "{err}");
}

#[test]
fn a_check_in_that_leaves_out_a_coin_is_refused() {
    let node = Node::start();
    let (cli, holders) = wallet(&node);
    fund(&cli, &node, 1.0);
    let file = cli.path("checkin.psbt");
    prepare_and_approve(&cli, &holders, &file);

    // A coin arrives after the check-in was prepared and approved.
    fund(&cli, &node, 0.5);
    let err = cli.fails(&["broadcast", "--psbt", &file]);
    assert!(err.contains("this check-in leaves out coin"), "{err}");
    // Now the wallet knows the new coin, sign refuses it too.
    let args = ["sign", "--psbt", &file, "--yes", "--key-stdin"];
    let err = expect_failure(&args, cli.run_with_stdin(&args, &holders.device.to_secret_string()));
    assert!(err.contains("this check-in leaves out coin"), "{err}");

    // A fresh check-in takes both coins.
    let again = cli.path("checkin2.psbt");
    let out = prepare_and_approve(&cli, &holders, &again);
    assert!(out.contains("check-in of 2 coins"), "{out}");
    cli.ok(&["broadcast", "--psbt", &again]);
}

#[test]
fn a_check_in_needs_two_of_three_even_when_the_delayed_paths_are_open() {
    let node = Node::start();
    let (cli, holders) = wallet(&node);
    fund(&cli, &node, 1.0);
    node.mine(19); // 20 confirmations: device and heir paths both open
    for (name, holder) in [("DEVICE", &holders.device), ("HEIR", &holders.heir), ("HELPER", &holders.helper)] {
        let file = cli.path(&format!("{name}.psbt"));
        cli.ok(&["checkin", "--out", &file]);
        if name != "HEIR" {
            cli.sign(&file, holder); // HEIR has no key in the 2-of-3 leaf
        }
        let err = cli.fails(&["broadcast", "--psbt", &file]);
        assert!(err.contains("not enough keys have approved"), "{name} alone: {err}");
    }
}

#[test]
fn nothing_to_check_in() {
    let node = Node::start();
    let (cli, _) = wallet(&node);
    let err = cli.fails(&["checkin", "--out", &cli.path("c.psbt")]);
    assert!(err.contains("no confirmed Savings coins to check in"), "{err}");
}

#[test]
fn an_unconfirmed_incoming_coin_doesnt_hold_up_a_check_in() {
    let node = Node::start();
    let (cli, holders) = wallet(&node);
    fund(&cli, &node, 1.0);
    let address = cli.ok(&["address"]);
    let address = Address::from_str(address.trim()).unwrap().require_network(Network::Regtest).unwrap();
    node.send_to(&address, Amount::from_btc(0.5).unwrap()); // not mined

    let file = cli.path("checkin.psbt");
    let out = prepare_and_approve(&cli, &holders, &file);
    assert!(out.contains("Prepared a check-in of 1 coin, 1.00000000 BTC"), "{out}");
    assert!(out.contains("Left out 1 unconfirmed coin: its delays haven't started yet."), "{out}");
    let sent = cli.ok(&["broadcast", "--psbt", &file]);
    assert!(sent.starts_with("Sent."), "{sent}");

    node.mine(1);
    let balance = cli.ok(&["balance"]);
    assert_eq!(balance.matches("1 confirmation  ").count(), 2, "the check-in's coin and the newcomer: {balance}");
}

#[test]
fn balance_works_for_a_wallet_without_a_check_in_rule() {
    // A wallet made before check-ins existed has no interval in
    // savings.json, and here the defaults don't fit its 10-block device
    // delay. balance still lists each coin and still warns when overdue.
    let node = Node::start();
    let (cli, _) = wallet(&node);
    let config = cli.wallet_dir().join("savings.json");
    let text = std::fs::read_to_string(&config).unwrap();
    let old = text.replace("\"checkin_interval\": 5", "\"checkin_interval\": null").replace("\"checkin_margin\": 2", "\"checkin_margin\": null");
    assert_ne!(old, text);
    std::fs::write(&config, old).unwrap();

    fund(&cli, &node, 1.0);
    node.mine(4);
    let out = cli.ok(&["balance"]);
    assert!(out.contains("Check-ins aren't set up for this wallet"), "{out}");
    assert!(out.contains("5 confirmations  device path opens in 5 blocks  heir path opens in 15 blocks\n"), "never 'due': {out}");
    assert!(!out.contains("Next check-in"), "{out}");

    node.mine(5);
    let out = cli.ok(&["balance"]);
    assert!(out.contains("OVERDUE") && out.contains("WARNING: check-in overdue."), "{out}");
}
