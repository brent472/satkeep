//! `satkeep add-heir`: plain 2-of-3 Savings gets a beneficiary by moving, in
//! one 2-of-3 payment, to a new wallet with the same PHONE, DEVICE and
//! HELPER plus HEIR. `sign` and `broadcast` check where a move goes against
//! this wallet's own settings, never the PSBT's word.

mod common;

use std::fs;
use std::str::FromStr;

use common::{Cli, Holders, Node, expect_failure, file_exists};
use satkeep_savings::bitcoin::psbt::Psbt;
use satkeep_savings::bitcoin::{Address, Amount, Network};

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

fn add_heir_args<'a>(new: &'a str, heir: &'a str, out: &'a str) -> [&'a str; 15] {
    [
        "add-heir", "--new-wallet-dir", new, "--heir", heir, "--heir-delay", "20",
        "--checkin-interval", "10", "--checkin-margin", "5", "--refresh-window", common::REFRESH_WINDOW, "--out", out,
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
    assert!(out.contains("HEIR alone can approve once a coin has 20 confirmations (about 3 hours)."), "{out}");
    assert!(out.contains("Heir delay: 20 blocks, about 3 hours"), "{out}");
    assert!(out.contains("Prepared the move of 2 coins, 1.50000000 BTC, into it, in one payment."), "{out}");
    assert!(out.contains("Needs approval from any 2 of PHONE, DEVICE, HELPER"), "{out}");
    assert!(out.contains(&format!("HEIR's key: {heir}")), "{out}");
    let first = new.ok(&["address"]).trim().to_string();
    assert!(out.contains(&format!("It goes to {first}, the new wallet's first address.")), "{out}");

    // The approval prompt says what this is.
    let args = ["sign", "--psbt", &file, "--key-stdin"];
    let shown = common::expect_ok(&args, old.run_with_stdin(&args, &format!("y\n{}\n", holders.phone.to_secret_string())));
    assert!(shown.contains("Move to approve: every Savings coin goes to the new Savings wallet set up in this"), "{shown}");
    assert!(shown.contains("Heir delay: 20 blocks, about 3 hours"), "{shown}");
    assert!(shown.contains(&format!("HEIR's key: {heir}")), "the approver sees exactly which HEIR: {shown}");
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

/// This wallet's settings, to put back after a stand-in for whoever prepares
/// a move has run `add-heir` with other values on the same folder.
fn settings(cli: &Cli) -> String {
    fs::read_to_string(cli.wallet_dir().join("savings.json")).unwrap()
}

fn restore(cli: &Cli, settings: &str) {
    fs::write(cli.wallet_dir().join("savings.json"), settings).unwrap();
}

/// Signing `file` as PHONE is refused with `why`, and the file is unchanged.
fn sign_refused(cli: &Cli, holders: &Holders, file: &str, why: &str) {
    let before = fs::read_to_string(file).unwrap();
    let args = ["sign", "--psbt", file, "--yes", "--key-stdin"];
    let err = expect_failure(&args, cli.run_with_stdin(&args, &holders.phone.to_secret_string()));
    assert!(err.contains(why), "{err}");
    assert_eq!(fs::read_to_string(file).unwrap(), before, "nothing was signed");
}

fn read(file: &str) -> Psbt {
    Psbt::from_str(fs::read_to_string(file).unwrap().trim()).unwrap()
}

fn edit_psbt(file: &str, edit: impl FnOnce(&mut Psbt)) {
    let mut psbt = read(file);
    edit(&mut psbt);
    fs::write(file, psbt.to_string()).unwrap();
}

const NOT_AS_SET_UP: &str = "names a different beneficiary or heir delay than the one set up in this wallet's settings";
const WRONG_SHAPE: &str = "send everything to the new wallet's first address";

#[test]
fn a_move_to_a_wallet_with_a_different_heir_key_is_refused() {
    // The beneficiary set up here is HEIR; whoever prepared this move made
    // it, complete and consistent, for a HEIR key they hold.
    let node = Node::start();
    let (old, new, holders) = plain_with_coins(&node);
    let honest = old.path("move.psbt");
    old.ok(&add_heir_args(&new.wallet_dir().display().to_string(), &holders.public_keys()[3], &honest));
    let ours = settings(&old);
    let stranger = Holders::generate().public_keys()[3].clone();
    let evil = old.path("evil.psbt");
    old.ok(&add_heir_args(&old.path("evil-wallet"), &stranger, &evil));
    restore(&old, &ours);

    sign_refused(&old, &holders, &evil, NOT_AS_SET_UP);
    // Claiming the right HEIR doesn't help: the output still goes elsewhere.
    edit_psbt(&evil, |p| p.proprietary = read(&honest).proprietary);
    sign_refused(&old, &holders, &evil, WRONG_SHAPE);
}

#[test]
fn a_move_to_a_wallet_with_a_shorter_heir_delay_is_refused() {
    let node = Node::start();
    let (old, new, holders) = plain_with_coins(&node);
    let heir = holders.public_keys()[3].clone();
    old.ok(&add_heir_args(&new.wallet_dir().display().to_string(), &heir, &old.path("move.psbt")));
    let ours = settings(&old);
    let short = old.path("short.psbt");
    let short_wallet = old.path("short-wallet");
    let mut args = add_heir_args(&short_wallet, &heir, &short);
    args[6] = "15";
    args[8] = "5"; // check-ins every 5 blocks still fit
    old.ok(&args);
    restore(&old, &ours);
    sign_refused(&old, &holders, &short, NOT_AS_SET_UP);

    // Signed anyway by two signers whose settings were changed to match,
    // it's refused at broadcast by settings that weren't.
    let changed = ours.replace("\"heir_delay\": 20", "\"heir_delay\": 15");
    assert_ne!(changed, ours);
    restore(&old, &changed);
    old.sign(&short, &holders.phone);
    old.sign(&short, &holders.device);
    restore(&old, &ours);
    let err = old.fails(&["broadcast", "--psbt", &short]);
    assert!(err.contains(NOT_AS_SET_UP), "{err}");
}

#[test]
fn a_move_with_one_extra_output_is_refused_at_sign_and_broadcast() {
    let node = Node::start();
    let (old, new, holders) = plain_with_coins(&node);
    let file = old.path("move.psbt");
    old.ok(&add_heir_args(&new.wallet_dir().display().to_string(), &holders.public_keys()[3], &file));
    let honest = fs::read_to_string(&file).unwrap();
    let thief = node.new_address().script_pubkey();
    let add_output = |p: &mut Psbt| {
        let mut extra = p.unsigned_tx.output[0].clone();
        extra.value = Amount::from_sat(100_000);
        p.unsigned_tx.output[0].value -= extra.value;
        extra.script_pubkey = thief.clone();
        p.unsigned_tx.output.push(extra);
        p.outputs.push(Default::default());
    };
    edit_psbt(&file, add_output);
    sign_refused(&old, &holders, &file, WRONG_SHAPE);
    // Unmarked, it still pays the new wallet set up here, so it's still
    // checked as a move.
    edit_psbt(&file, |p| p.proprietary.clear());
    sign_refused(&old, &holders, &file, "isn't a move to a new Savings wallet");

    // Approved honestly, then altered before broadcasting.
    fs::write(&file, &honest).unwrap();
    old.sign(&file, &holders.phone);
    old.sign(&file, &holders.helper);
    edit_psbt(&file, add_output);
    let err = old.fails(&["broadcast", "--psbt", &file]);
    assert!(err.contains(WRONG_SHAPE), "{err}");
}

#[test]
fn a_move_is_refused_where_no_move_is_set_up() {
    // A signer whose settings have no move has nothing to check it against.
    let node = Node::start();
    let (old, new, holders) = plain_with_coins(&node);
    let ours = settings(&old);
    let file = old.path("move.psbt");
    old.ok(&add_heir_args(&new.wallet_dir().display().to_string(), &holders.public_keys()[3], &file));
    assert!(settings(&old).contains("moving_to"), "add-heir sets the move up");
    restore(&old, &ours);
    sign_refused(&old, &holders, &file, "no move is set up in this wallet's settings");
}

#[test]
fn an_heir_delay_below_the_minimum_is_refused() {
    let node = Node::start();
    let k = Holders::generate().public_keys();

    // The placeholder minimum, about 6 months.
    let cli = Cli::new(&node);
    let base = ["create", "--phone", &k[0], "--device", &k[1], "--helper", &k[2], "--heir", &k[3]];
    let err = cli.fails(&[&base[..], &["--heir-delay", "26279", "--checkin-interval", "17000"]].concat());
    assert!(err.contains("the heir delay is 26279 blocks, below the minimum of 26280 blocks"), "{err}");
    assert!(!cli.wallet_dir().join("savings.json").exists());
    let out = cli.ok(&[&base[..], &["--heir-delay", "26280", "--checkin-interval", "17000"]].concat());
    assert!(out.contains("HEIR alone can approve once a coin has 26280 confirmations (about 6 months)."), "{out}");

    // add-heir keeps the wallet's minimum (10 in these tests).
    let (old, new, holders) = plain_with_coins(&node);
    let file = old.path("move.psbt");
    let new_dir = new.wallet_dir().display().to_string();
    let heir = holders.public_keys()[3].clone();
    let mut args = add_heir_args(&new_dir, &heir, &file);
    args[6] = "9";
    args[8] = "5";
    args[10] = "1";
    let err = old.fails(&args);
    assert!(err.contains("below the minimum of 10 blocks"), "{err}");
    assert!(!file_exists(&file) && !new.wallet_dir().exists(), "nothing written");

    // A move set up below the minimum is refused, even if the minimum was
    // raised after it was set up.
    args[6] = "10";
    old.ok(&args);
    let raised = settings(&old).replace("\"min_heir_delay\": 10", "\"min_heir_delay\": 11");
    assert_ne!(raised, settings(&old));
    restore(&old, &raised);
    sign_refused(&old, &holders, &file, "below the minimum of 11 blocks");
}

#[test]
fn a_move_whose_check_ins_cant_beat_the_heir_delay_is_refused() {
    // add-heir refuses such a wallet outright (the check-in rule must fit),
    // and the move set up here keeps the new wallet's check-ins, so sign
    // and broadcast check them too.
    let node = Node::start();
    let (old, new, holders) = plain_with_coins(&node);
    let file = old.path("move.psbt");
    let new_dir = new.wallet_dir().display().to_string();
    let heir = holders.public_keys()[3].clone();
    let mut args = add_heir_args(&new_dir, &heir, &file);
    args[8] = "16"; // every 16 blocks + a 5-block margin > the 20-block heir delay
    let err = old.fails(&args);
    assert!(err.contains("must be shorter than the heir delay (20 blocks) by at least the margin (5 blocks)"), "{err}");
    assert!(!file_exists(&file) && !new.wallet_dir().exists(), "nothing written");

    args[8] = "10";
    old.ok(&args);
    let ours = settings(&old);
    assert!(ours.contains("\"checkin_interval\": 10") && ours.contains("\"checkin_margin\": 5"), "{ours}");

    // Settings edited so the new wallet's check-ins come too late.
    restore(&old, &ours.replace("\"checkin_interval\": 10", "\"checkin_interval\": 16"));
    sign_refused(&old, &holders, &file, "must be shorter than the heir delay (20 blocks)");

    // A move set up by an earlier version has no check-ins saved: it gets
    // the default margin, which leaves no room in a 20-block heir delay.
    let earlier = ours.replace(",\n    \"checkin_interval\": 10", "").replace(",\n    \"checkin_margin\": 5", "");
    assert!(!earlier.contains("checkin_"), "{earlier}");
    restore(&old, &earlier);
    sign_refused(&old, &holders, &file, "the heir delay (20 blocks) must be longer than the check-in margin (8760 blocks)");

    // Signed under the settings add-heir wrote, it's still refused at
    // broadcast by settings whose check-ins don't fit.
    restore(&old, &ours);
    old.sign(&file, &holders.phone);
    old.sign(&file, &holders.device);
    restore(&old, &ours.replace("\"checkin_interval\": 10", "\"checkin_interval\": 16"));
    let err = old.fails(&["broadcast", "--psbt", &file]);
    assert!(err.contains("must be shorter than the heir delay (20 blocks)"), "{err}");
}

#[test]
fn a_move_whose_fee_is_above_the_ceiling_is_refused() {
    let node = Node::start();
    let (old, new, holders) = plain_with_coins(&node);
    let file = old.path("move.psbt");
    let new_dir = new.wallet_dir().display().to_string();
    let heir = holders.public_keys()[3].clone();

    // A typo in the fee rate: refused, and nothing written.
    let mut args = add_heir_args(&new_dir, &heir, &file).to_vec();
    args.extend(["--fee-rate", "5000"]);
    let err = old.fails(&args);
    assert!(err.contains("more than this wallet allows"), "{err}");
    assert!(!file_exists(&file) && !new.wallet_dir().exists(), "nothing written");

    // A good move whose one output was edited down: refused at sign and at
    // broadcast.
    old.ok(&add_heir_args(&new_dir, &heir, &file));
    let honest = fs::read_to_string(&file).unwrap();
    let edit_down = |p: &mut Psbt| p.unsigned_tx.output[0].value = Amount::from_btc(0.1).unwrap();
    edit_psbt(&file, edit_down);
    sign_refused(&old, &holders, &file, "the network fee is 1.40000000 BTC");
    fs::write(&file, &honest).unwrap();
    old.sign(&file, &holders.phone);
    old.sign(&file, &holders.device);
    edit_psbt(&file, edit_down);
    let err = old.fails(&["broadcast", "--psbt", &file]);
    assert!(err.contains("more than this wallet allows"), "{err}");
}
