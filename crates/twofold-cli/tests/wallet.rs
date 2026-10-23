//! `twofold create`, `address`, `balance` and `export`, in both shapes:
//! Savings with a beneficiary and plain 2 of 3.

mod common;

use std::str::FromStr;

use common::{Cli, Holders, Node};
use twofold_savings::bitcoin::{Address, Amount, Network};
use twofold_savings::{Delays, SavingsKeys, SavingsPolicy};

fn receive(cli: &Cli, node: &Node) {
    let address = cli.ok(&["address"]);
    let address = Address::from_str(address.trim()).unwrap().require_network(Network::Regtest).unwrap();
    node.send_to(&address, Amount::from_btc(1.0).unwrap());
}

#[test]
fn creates_a_wallet_with_a_beneficiary_receives_and_shows_the_balance() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let out = cli.create(&Holders::generate(), 20);
    assert!(out.contains("HEIR alone can approve once a coin has 20 confirmations"), "{out}");
    assert!(!out.contains("DEVICE alone"), "no card-only path: {out}");

    receive(&cli, &node);
    let pending = cli.ok(&["balance"]);
    assert!(pending.contains("Waiting for confirmation: 1.00000000 BTC"), "{pending}");

    node.mine(1);
    let out = cli.ok(&["balance"]);
    assert!(out.contains("1 confirmation  heir path opens in 19 blocks"), "{out}");
    node.mine(2);
    let out = cli.ok(&["balance"]);
    assert!(out.contains("Total: 1.00000000 BTC"), "{out}");
    assert!(out.contains("3 confirmations  heir path opens in 17 blocks"), "{out}");
}

#[test]
fn creates_a_wallet_with_no_beneficiary_where_nothing_opens_with_time() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let out = cli.create_plain(&Holders::generate());
    assert!(out.contains("No beneficiary yet: no key can approve alone, ever"), "{out}");
    assert!(!out.contains("Check in every"), "{out}");
    let saved = std::fs::read_to_string(cli.wallet_dir().join("savings.json")).unwrap();
    let no_beneficiary = ["\"heir\"", "\"heir_delay\"", "checkin", "moving_to"].iter().all(|k| !saved.contains(k));
    assert!(no_beneficiary && saved.contains("\"min_heir_delay\": 10"), "{saved}");

    receive(&cli, &node);
    node.mine(30);
    let out = cli.ok(&["balance"]);
    assert!(out.contains("30 confirmations\n"), "no heir path to count down: {out}");
    assert!(out.contains("nothing opens with time and no check-in is needed"), "{out}");
    assert!(!out.contains("OVERDUE") && !out.contains("CHECK-IN DUE"), "{out}");
}

#[test]
fn check_in_settings_need_a_beneficiary() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let k = Holders::generate().public_keys();
    for flag in ["--heir-delay", "--checkin-interval", "--checkin-margin"] {
        let err = cli.fails(&["create", "--phone", &k[0], "--device", &k[1], "--helper", &k[2], flag, "10"]);
        assert!(err.contains("--heir"), "{flag}: {err}");
    }
}

#[test]
fn hands_out_a_new_address_each_time() {
    let node = Node::start();
    let cli = Cli::new(&node);
    cli.create(&Holders::generate(), 20);
    assert_ne!(cli.ok(&["address"]), cli.ok(&["address"]));
}

#[test]
fn export_matches_the_library_in_both_shapes() {
    let node = Node::start();
    let holders = Holders::generate();
    let keys = |heir| {
        SavingsKeys::new(holders.phone.public_key(0), holders.device.public_key(0), holders.helper.public_key(0), heir)
            .unwrap()
    };
    let with_heir = Cli::new(&node);
    with_heir.create(&holders, 20);
    let plain = Cli::new(&node);
    plain.create_plain(&holders);
    let cases = [
        (with_heir, SavingsPolicy::new(keys(Some(holders.heir.public_key(0))), Delays::with_heir(20).unwrap(), Network::Regtest)),
        (plain, SavingsPolicy::new(keys(None), Delays::none(), Network::Regtest)),
    ];
    for (cli, policy) in cases {
        let policy = policy.unwrap();
        assert_eq!(cli.ok(&["export"]).trim(), policy.export_descriptor());
        let (receive, change) = policy.export_receive_and_change();
        assert_eq!(cli.ok(&["export", "--split"]), format!("{receive}\n{change}\n"));
    }
}

#[test]
fn refuses_a_node_on_another_network() {
    let node = Node::start(); // regtest
    let cli = Cli::new(&node);
    let k = Holders::generate().public_keys();
    let err = cli.fails(&["create", "--network", "signet", "--phone", &k[0], "--device", &k[1], "--helper", &k[2], "--heir", &k[3]]);
    assert!(err.contains("is on regtest, but this wallet is on signet"), "{err}");
    assert!(!cli.wallet_dir().join("savings.json").exists());
}

#[test]
fn has_no_mainnet_option() {
    let node = Node::start();
    let cli = Cli::new(&node);
    for network in ["bitcoin", "main", "mainnet"] {
        let err = cli.fails(&["create", "--network", network, "--phone", "x", "--device", "x", "--helper", "x"]);
        assert!(err.contains("invalid value"), "{err}");
    }
}

#[test]
fn has_no_card_only_path_option() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let k = Holders::generate().public_keys();
    for flag in ["--device-r", "--device-delay"] {
        let err = cli.fails(&["create", "--phone", &k[0], "--device", &k[1], "--helper", &k[2], flag, "10"]);
        assert!(err.contains(&format!("unexpected argument '{flag}'")), "{err}");
    }
}

#[test]
fn refuses_to_create_over_an_existing_wallet() {
    let node = Node::start();
    let cli = Cli::new(&node);
    cli.create(&Holders::generate(), 20);
    let first = std::fs::read_to_string(cli.wallet_dir().join("savings.json")).unwrap();
    let k = Holders::generate().public_keys();
    let err = cli.fails(&["create", "--network", "regtest", "--phone", &k[0], "--device", &k[1], "--helper", &k[2]]);
    assert!(err.contains("already holds a Savings wallet"), "{err}");
    assert_eq!(std::fs::read_to_string(cli.wallet_dir().join("savings.json")).unwrap(), first);
}

#[test]
fn refuses_a_policy_the_library_refuses() {
    // The CLI goes through SavingsKeys::new, Delays and CheckinRule, so the
    // library's checks apply.
    let node = Node::start();
    let cli = Cli::new(&node);
    let k = Holders::generate().public_keys();
    let base = ["create", "--network", "regtest", "--phone", &k[0], "--device", &k[1], "--helper", &k[2], "--min-heir-delay", "10"];
    let err = cli.fails(&[&base[..], &["--heir", &k[2]]].concat());
    assert!(err.contains("HELPER and HEIR keys are the same"), "{err}");
    let err = cli.fails(&[&base[..], &["--heir", &k[3], "--heir-delay", "0"]].concat());
    assert!(err.contains("heir delay must be at least 1 block"), "{err}");
    let err = cli.fails(&[&base[..], &["--heir", &k[3], "--heir-delay", "20", "--checkin-interval", "20"]].concat());
    assert!(err.contains("must be shorter than the heir delay"), "{err}");
}

#[test]
fn refuses_a_key_with_the_same_receive_and_change_path() {
    // Found by fuzzing savings.json: a `<1;1>` key used to crash the policy
    // builder instead of being refused.
    let node = Node::start();
    let cli = Cli::new(&node);
    let k = Holders::generate().public_keys();
    let same_twice = k[2].replace("/<0;1>/*", "/<1;1>/*");
    let err = cli.fails(&["create", "--phone", &k[0], "--device", &k[1], "--helper", &same_twice]);
    assert!(err.contains("HELPER key must be an extended public key with receive and change paths"), "{err}");
    assert!(!err.contains("panicked"), "{err}");
}

#[test]
fn a_wallet_made_with_the_card_only_path_gets_a_clear_error() {
    let node = Node::start();
    let cli = Cli::new(&node);
    cli.create(&Holders::generate(), 20);
    let path = cli.wallet_dir().join("savings.json");
    let mut config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    config["device_r"] = config["device"].clone();
    std::fs::write(&path, config.to_string()).unwrap();
    let err = cli.fails(&["address"]);
    assert!(err.contains("card-only delayed path (DEVICE_R), which Twofold no longer has"), "{err}");
}
