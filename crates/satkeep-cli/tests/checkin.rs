//! `satkeep checkin`, the interval rule, and the due/overdue warnings in
//! `satkeep balance`. Heir delay 20; check-ins every 10 blocks with a
//! 5-block margin. Savings with no beneficiary has no check-in.

mod common;

use std::str::FromStr;

use common::{Cli, Holders, Node, expect_failure};
use satkeep_savings::bitcoin::psbt::Psbt;
use satkeep_savings::bitcoin::{Address, Amount, Network};

fn wallet(node: &Node) -> (Cli<'_>, Holders) {
    let cli = Cli::new(node);
    let holders = Holders::generate();
    cli.create_with_checkin(&holders, 20, 10, 5);
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
fn an_interval_at_or_past_the_heir_delay_is_refused() {
    let node = Node::start();
    let holders = Holders::generate();
    let k = holders.public_keys();
    for (interval, margin, why) in [
        ("20", "1", "equal to the heir delay"),
        ("21", "1", "past the heir delay"),
        ("65535", "1", "far past it"),
        ("19", "2", "leaves less than the margin"),
        ("0", "1", "zero"),
    ] {
        let cli = Cli::new(&node);
        let args = [
            "create", "--phone", &k[0], "--device", &k[1], "--helper", &k[2], "--heir", &k[3], "--heir-delay", "20", "--min-heir-delay", "10",
            "--checkin-interval", interval, "--checkin-margin", margin,
        ];
        let err = cli.fails(&args);
        assert!(err.contains("check-in"), "{why}: {err}");
        assert!(!cli.wallet_dir().join("savings.json").exists(), "{why}: nothing was created");
    }

    // The placeholder defaults don't fit a 20-block heir delay either.
    let cli = Cli::new(&node);
    let err = cli.fails(&[
        "create", "--phone", &k[0], "--device", &k[1], "--helper", &k[2], "--heir", &k[3], "--heir-delay", "20", "--min-heir-delay", "10",
    ]);
    assert!(err.contains("the check-in interval (52560 blocks) must be shorter than the heir delay (20 blocks)"), "{err}");

    // And they do fit the placeholder heir delay: once a year.
    let cli = Cli::new(&node);
    let out = cli.ok(&["create", "--phone", &k[0], "--device", &k[1], "--helper", &k[2], "--heir", &k[3]]);
    assert!(out.contains("HEIR alone can approve once a coin has 61320 confirmations (about 14 months)."), "{out}");
    assert!(out.contains("Check in every 52560 blocks"), "{out}");
}

#[test]
fn savings_with_no_beneficiary_refuses_a_check_in() {
    let node = Node::start();
    let cli = Cli::new(&node);
    cli.create_plain(&Holders::generate());
    fund(&cli, &node, 1.0);
    let err = cli.fails(&["checkin", "--out", &cli.path("c.psbt")]);
    assert!(err.contains("has no beneficiary, so it has no delayed path and needs no check-in"), "{err}");
    assert!(!common::file_exists(&cli.path("c.psbt")));
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
    assert!(out.contains("Next check-in due in 9 blocks (every 10 blocks)."), "{out}");
    assert!(!out.contains("DUE") && !out.contains("OVERDUE"), "{out}");

    node.mine(8);
    let out = at(9);
    assert!(out.contains("Next check-in due in 1 block (every 10 blocks)."), "{out}");
    assert!(out.contains("heir path opens in 11 blocks"), "{out}");
    assert!(!out.contains("DUE"), "one block before the interval: {out}");

    node.mine(1);
    let out = at(10);
    assert!(out.contains("CHECK-IN DUE"), "at the interval: {out}");
    assert!(out.contains("Check-in due: the oldest coin has 10 confirmations, and check-ins are due every 10 blocks."), "{out}");
    assert!(out.contains("Its heir path opens in 10 blocks."), "{out}");
    assert!(!out.contains("OVERDUE"), "{out}");

    node.mine(9);
    let out = at(19);
    assert!(out.contains("CHECK-IN DUE") && !out.contains("OVERDUE"), "one block before the heir delay: {out}");
    assert!(out.contains("Its heir path opens in 1 block."), "{out}");

    node.mine(1);
    let out = at(20);
    assert!(out.contains("heir path open") && out.contains("OVERDUE"), "at the heir delay: {out}");
    assert!(out.contains("WARNING: check-in overdue. HEIR alone can already spend 1 coin."), "{out}");
}

#[test]
fn a_check_in_restarts_the_heir_delay_and_clears_the_warning() {
    let node = Node::start();
    let (cli, holders) = wallet(&node);
    fund(&cli, &node, 1.0);
    fund(&cli, &node, 0.5);
    node.mine(18); // first coin at 20 confirmations: overdue
    assert!(cli.ok(&["balance"]).contains("OVERDUE"));

    let file = cli.path("checkin.psbt");
    let out = prepare_and_approve(&cli, &holders, &file);
    assert!(out.contains("Prepared a check-in of 2 coins, 1.50000000 BTC, into one new Savings address."), "{out}");
    assert!(out.contains("Check-in to approve: every Savings coin goes into one new Savings address,"), "{out}");
    assert!(out.contains("restarting the heir delay (20 blocks, about 3 hours)."), "{out}");
    assert!(out.contains("comes back to Savings as change"), "{out}");
    assert!(out.contains("Approval path: any 2 of PHONE, DEVICE, HELPER"), "{out}");
    cli.ok(&["broadcast", "--psbt", &file]);
    node.mine(1);

    let out = cli.ok(&["balance"]);
    assert!(out.contains("1 confirmation  heir path opens in 19 blocks"), "{out}");
    assert!(out.contains("Next check-in due in 9 blocks"), "{out}");
    assert!(!out.contains("DUE") && !out.contains("OVERDUE"), "{out}");
    assert_eq!(out.matches(" confirmation").count(), 1, "one coin now: {out}");

    // The delay really did restart: HEIR alone is refused for the new coin.
    let to = node.new_address().to_string();
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
fn a_check_in_needs_two_of_three_even_when_the_heir_path_is_open() {
    let node = Node::start();
    let (cli, holders) = wallet(&node);
    fund(&cli, &node, 1.0);
    node.mine(19); // 20 confirmations: the heir path is open
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
    // savings.json, and here the defaults don't fit its 20-block heir
    // delay. balance still lists each coin and still warns when overdue.
    let node = Node::start();
    let (cli, _) = wallet(&node);
    let config = cli.wallet_dir().join("savings.json");
    let text = std::fs::read_to_string(&config).unwrap();
    let old = text.replace("\"checkin_interval\": 10", "\"checkin_interval\": null").replace("\"checkin_margin\": 5", "\"checkin_margin\": null");
    assert_ne!(old, text);
    std::fs::write(&config, old).unwrap();

    fund(&cli, &node, 1.0);
    node.mine(4);
    let out = cli.ok(&["balance"]);
    assert!(out.contains("Check-ins aren't set up for this wallet"), "{out}");
    assert!(out.contains("5 confirmations  heir path opens in 15 blocks\n"), "never 'due': {out}");
    assert!(!out.contains("Next check-in"), "{out}");

    node.mine(15);
    let out = cli.ok(&["balance"]);
    assert!(out.contains("OVERDUE") && out.contains("WARNING: check-in overdue."), "{out}");
}

fn settings_path(cli: &Cli) -> std::path::PathBuf {
    cli.wallet_dir().join("savings.json")
}

/// Sets `field` to `value` in the wallet's settings.
fn set_setting(cli: &Cli, field: &str, value: &str) {
    let path = settings_path(cli);
    let text = std::fs::read_to_string(&path).unwrap();
    let edited = text.replacen('{', &format!("{{
  \"{field}\": {value},"), 1);
    std::fs::write(&path, edited).unwrap();
}

const FEE_TOO_HIGH: &str = "more than this wallet allows";

#[test]
fn a_check_in_whose_fee_is_above_the_ceiling_is_refused() {
    let node = Node::start();
    let (cli, holders) = wallet(&node);
    fund(&cli, &node, 1.0);
    fund(&cli, &node, 0.5);

    // A typo in the fee rate, 5,000 sat/vB for 5: refused, nothing written.
    let file = cli.path("checkin.psbt");
    let err = cli.fails(&["checkin", "--fee-rate", "5000", "--out", &file]);
    assert!(err.contains(FEE_TOO_HIGH) && err.contains("at most 0.00500000 BTC, and at most 5%"), "{err}");
    assert!(!common::file_exists(&file));

    // A good check-in whose one output was edited down: refused at sign.
    cli.ok(&["checkin", "--out", &file]);
    let honest = std::fs::read_to_string(&file).unwrap();
    let edit_down = || {
        let mut psbt = Psbt::from_str(std::fs::read_to_string(&file).unwrap().trim()).unwrap();
        psbt.unsigned_tx.output[0].value = Amount::from_btc(0.1).unwrap();
        std::fs::write(&file, psbt.to_string()).unwrap();
    };
    edit_down();
    let edited = std::fs::read_to_string(&file).unwrap();
    let args = ["sign", "--psbt", &file, "--yes", "--key-stdin"];
    let err = expect_failure(&args, cli.run_with_stdin(&args, &holders.phone.to_secret_string()));
    assert!(err.contains("the network fee is 1.40000000 BTC") && err.contains(FEE_TOO_HIGH), "{err}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), edited, "nothing was signed");

    // Approved honestly, then edited before broadcasting: refused there too.
    std::fs::write(&file, &honest).unwrap();
    cli.sign(&file, &holders.phone);
    cli.sign(&file, &holders.helper);
    edit_down();
    let err = cli.fails(&["broadcast", "--psbt", &file]);
    assert!(err.contains(FEE_TOO_HIGH), "{err}");

    // The ceiling is a setting in savings.json.
    let saved = std::fs::read_to_string(settings_path(&cli)).unwrap();
    assert!(!saved.contains("max_fee"), "the defaults aren't written: {saved}");
    set_setting(&cli, "max_fee_percent", "0");
    let err = cli.fails(&["checkin", "--fee-rate", "5000", "--out", &cli.path("zero.psbt")]);
    assert!(err.contains("max_fee_sat and max_fee_percent") && err.contains("from 1 to 100, not 0"), "{err}");
    std::fs::write(settings_path(&cli), &saved).unwrap();
    set_setting(&cli, "max_fee_sat", "2000000");
    let out = cli.ok(&["checkin", "--fee-rate", "5000", "--out", &cli.path("raised.psbt")]);
    assert!(out.contains("Prepared a check-in of 2 coins"), "{out}");
}
