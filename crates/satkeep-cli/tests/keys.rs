//! Private keys never go on the command line, and `keygen` and the wallet
//! files default to a folder outside the repo.

mod common;

use std::fs;

use common::{Cli, Holders, Node, expect_failure, expect_ok};
use satkeep_savings::HolderKey;

const ON_THE_COMMAND_LINE: &str = "a private key or recovery phrase was passed on the command line";

#[test]
fn a_recovery_phrase_is_refused_on_the_command_line_and_never_repeated() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let psbt = cli.path("pay.psbt");
    fs::write(&psbt, "not a real psbt").unwrap();
    let key = common::new_key().to_secret_string();
    let joined = format!("--key-file={key}");
    let mut unquoted = vec!["sign", "--psbt", &psbt, "--key-stdin"];
    unquoted.extend(key.split(' '));
    let attempts = [
        vec!["sign", "--psbt", &psbt, "--key", &key],
        vec!["sign", "--psbt", &psbt, "--key-file", &key],
        vec!["sign", "--psbt", &psbt, &joined],
        vec!["sign", "--psbt", &psbt, &key],
        unquoted,
        vec!["create", "--phone", &key, "--device", "x", "--helper", "x", "--heir", "x"],
    ];
    for args in attempts {
        // `run` also fails the test if the output contains words of the key.
        let err = expect_failure(&args, cli.run(&args));
        assert!(err.contains(ON_THE_COMMAND_LINE), "{err}");
    }
    assert_eq!(fs::read_to_string(&psbt).unwrap(), "not a real psbt");
}

#[test]
fn an_old_style_private_key_is_still_refused_on_the_command_line() {
    use satkeep_savings::bitcoin::NetworkKind;
    use satkeep_savings::bitcoin::bip32::Xpriv;
    use satkeep_savings::bitcoin::secp256k1::rand::{RngCore, thread_rng};

    let node = Node::start();
    let cli = Cli::new(&node);
    let psbt = cli.path("pay.psbt");
    let mut seed = [0u8; 32];
    thread_rng().fill_bytes(&mut seed);
    let key = Xpriv::new_master(NetworkKind::Test, &seed).unwrap().to_string();
    for args in [vec!["sign", "--psbt", &psbt, "--key-file", &key], vec!["sign", "--psbt", &psbt, &key]] {
        // `run` also fails the test if the output contains `tprv`.
        let err = expect_failure(&args, cli.run(&args));
        assert!(err.contains(ON_THE_COMMAND_LINE), "{err}");
    }
}

#[test]
fn there_is_no_key_flag() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let err = cli.fails(&["sign", "--psbt", &cli.path("pay.psbt"), "--key", &cli.path("phone.key")]);
    assert!(err.contains("unexpected argument '--key'"), "{err}");
}

#[test]
fn keygen_defaults_to_satkeep_home_and_writes_a_test_key() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let out = expect_ok(&["keygen"], cli.run_in_home(&["keygen", "--name", "phone"]));
    let file = cli.home().join("regtest").join("keys").join("phone.key");
    assert!(out.contains(&file.display().to_string()), "{out}");
    let saved = fs::read_to_string(&file).unwrap();
    assert_eq!(saved.split_whitespace().count(), 12, "the file holds a 12-word recovery phrase");
    let key = HolderKey::from_secret_str(&saved).unwrap();
    common::remember(&key);
    assert!(out.contains(&key.public_key(0).to_string()) && !out.contains(&key.public_key(1).to_string()), "one public key: {out}");

    let signet = expect_ok(&["keygen"], cli.run_in_home(&["--network", "signet", "keygen", "--name", "phone"]));
    assert!(signet.contains(&cli.home().join("signet").join("keys").join("phone.key").display().to_string()));
}

#[test]
fn keygen_never_overwrites_a_key() {
    let node = Node::start();
    let cli = Cli::new(&node);
    expect_ok(&["keygen"], cli.run_in_home(&["keygen", "--name", "phone"]));
    let file = cli.home().join("regtest").join("keys").join("phone.key");
    let first = fs::read_to_string(&file).unwrap();
    let err = expect_failure(&["keygen"], cli.run_in_home(&["keygen", "--name", "phone"]));
    assert!(err.contains("must not already exist"), "{err}");
    assert_eq!(fs::read_to_string(&file).unwrap(), first);

    let explicit = cli.path("helper.key");
    cli.ok(&["keygen", "--out", &explicit]);
    let err = cli.fails(&["keygen", "--out", &explicit]);
    assert!(err.contains("must not already exist"), "{err}");
}

#[test]
fn keygen_refuses_mainnet_odd_names_and_non_key_files() {
    let node = Node::start();
    let cli = Cli::new(&node);
    for network in ["bitcoin", "main", "mainnet"] {
        let err = expect_failure(&["keygen"], cli.run_in_home(&["--network", network, "keygen", "--name", "phone"]));
        assert!(err.contains("invalid value"), "{err}");
    }
    for name in ["../phone", "phone/key", "", "pho ne"] {
        let err = expect_failure(&["keygen"], cli.run_in_home(&["keygen", "--name", name]));
        assert!(err.contains("may only use letters, digits"), "{name:?}: {err}");
    }
    let err = cli.fails(&["keygen", "--out", &cli.path("phone.txt")]);
    assert!(err.contains("must end in .key"), "{err}");
    assert!(!cli.home().join("regtest").join("keys").exists(), "nothing was written");
}

#[test]
fn wallet_files_default_to_satkeep_home() {
    let node = Node::start();
    let cli = Cli::new(&node);
    let holders = Holders::generate();
    let keys = holders.public_keys();
    let create = ["create", "--phone", &keys[0], "--device", &keys[1], "--helper", &keys[2], "--heir", &keys[3]];
    let out = expect_ok(&create, cli.run_in_home(&create));
    let dir = cli.home().join("regtest");
    assert!(out.contains(&dir.display().to_string()), "{out}");
    assert!(dir.join("savings.json").exists() && dir.join("wallet.sqlite").exists());
    expect_ok(&["address"], cli.run_in_home(&["address"]));

    // Another network looks in another folder, and an explicit folder must
    // match the network asked for.
    let err = expect_failure(&["address"], cli.run_in_home(&["--network", "signet", "address"]));
    assert!(err.contains("no Savings wallet"), "{err}");
    let dir_text = dir.display().to_string();
    let args = ["--network", "signet", "--wallet-dir", &dir_text, "address"];
    let err = expect_failure(&args, cli.run_in_home(&args));
    assert!(err.contains("this wallet is on regtest, not signet"), "{err}");
}
