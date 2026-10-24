//! The restore tool end to end, on throwaway regtest nodes: a kit made by
//! the app side (satkeep-savings) and a simulated second key restore Savings
//! into Bitcoin Core, where PHONE (from the tool) and DEVICE spend both
//! shapes; and every forbidden case is refused. Keys and devices are fresh
//! in each test; nothing secret is printed, and every command's output is
//! checked for key material.

use std::path::{Path, PathBuf};
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::str::FromStr;

use bitcoincore_rpc::{Auth, Client, RpcApi};
use satkeep_savings::backup::{self, HeirSettings, SavingsWallet, SimulatedDevice, WalletSetup, kit, setup};
use satkeep_savings::bitcoin::psbt::Psbt;
use satkeep_savings::bitcoin::{Address, Amount, Network};
use satkeep_savings::{Delays, HolderKey, SavingsKeys, SavingsPolicy};
use satkeep_testenv::Node;
use serde_json::{Value, json};

const PIN: &str = "2468";
/// 2026-10-23, as Unix seconds.
const CREATED: u64 = 1_792_713_600;
const HEIR_DELAY: u16 = 20;
/// The new Core wallet's passphrase, for the tests only.
const PASSPHRASE: &str = "a test passphrase";

/// A folder for one test, removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = u64::from_be_bytes(backup::new_wallet_id()[..8].try_into().unwrap());
        let path = std::env::temp_dir().join(format!("satkeep-restore-test-{id:016x}"));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The owner's wallets, funded, and a kit and second key for them.
struct Owner {
    node: Node,
    dir: TempDir,
    phone: HolderKey,
    device: HolderKey,
    setup: WalletSetup,
    plain: SavingsPolicy,
    heir: SavingsPolicy,
    /// The simulated second key's secret.
    secret: [u8; 32],
}

fn address(policy: &SavingsPolicy) -> Address {
    policy.receive_and_change().0.at_derivation_index(0).unwrap().address(Network::Regtest).unwrap()
}

fn random_secret() -> [u8; 32] {
    let mut secret = [0u8; 32];
    secret[..16].copy_from_slice(&backup::new_wallet_id());
    secret[16..].copy_from_slice(&backup::new_wallet_id());
    secret
}

impl Owner {
    /// Savings with no beneficiary (1 BTC) and, after adding one, Savings
    /// with a beneficiary (2 BTC), both since the same birthday.
    fn new() -> Self {
        let node = Node::start();
        let [phone, device, helper, heir_key] = [(); 4].map(|_| HolderKey::generate());
        let keys = |heir| SavingsKeys::new(phone.public_key(0), device.public_key(0), helper.public_key(0), heir).unwrap();
        let plain = SavingsPolicy::new(keys(None), Delays::none(), Network::Regtest).unwrap();
        let heir = SavingsPolicy::new(keys(Some(heir_key.public_key(0))), Delays::with_heir(HEIR_DELAY).unwrap(), Network::Regtest).unwrap();
        let birthday = node.height();
        node.send_to(&address(&plain), Amount::from_btc(1.0).unwrap());
        node.send_to(&address(&heir), Amount::from_btc(2.0).unwrap());
        node.mine(1);
        let setup = WalletSetup {
            network: setup::Network::Regtest,
            pocket: None,
            savings: vec![
                SavingsWallet { descriptor: plain.export_descriptor(), birthday, heir: None },
                SavingsWallet {
                    descriptor: heir.export_descriptor(),
                    birthday,
                    heir: Some(HeirSettings { heir_delay: HEIR_DELAY, checkin_interval: 10, checkin_margin: 5, refresh_window: None }),
                },
            ],
        };
        Self { node, dir: TempDir::new(), phone, device, setup, plain, heir, secret: random_secret() }
    }

    /// A kit PDF for `phone` and `setup`, locked to the second key with
    /// `secret`, and its plain copy of `plain_setup`.
    fn kit_for(&self, name: &str, phone: &HolderKey, setup: &WalletSetup, plain_setup: &WalletSetup, secret: [u8; 32]) -> PathBuf {
        let mut device = SimulatedDevice::from_secret(secret, PIN);
        device.enter_pin(PIN).unwrap();
        let file = backup::make_backup(phone, setup, backup::new_wallet_id(), CREATED, &mut device).unwrap();
        let text = kit::encode(&file, plain_setup).unwrap();
        // As in the app's PDF: the armoured text as an uncompressed attachment.
        let mut pdf = b"%PDF-1.7\n1 0 obj << /Type /EmbeddedFile >> stream\n".to_vec();
        pdf.extend_from_slice(text.as_bytes());
        pdf.extend_from_slice(b"endstream\nendobj\n%%EOF\n");
        let path = self.dir.file(name);
        std::fs::write(&path, pdf).unwrap();
        path
    }

    fn kit(&self) -> PathBuf {
        self.kit_for("kit.pdf", &self.phone, &self.setup, &self.setup, self.secret)
    }

    fn device_file(&self, name: &str, secret: [u8; 32], pin: &str) -> PathBuf {
        let path = self.dir.file(name);
        let hex: String = secret.iter().map(|b| format!("{b:02x}")).collect();
        std::fs::write(&path, format!("{hex}\n{pin}\n")).unwrap();
        path
    }

    fn second_key(&self) -> PathBuf {
        self.device_file("device.txt", self.secret, PIN)
    }

    fn rpc_args(&self) -> Vec<String> {
        vec!["--rpc-url".into(), self.node.rpc_url(), "--rpc-cookie".into(), self.node.cookie_file().display().to_string()]
    }

    fn wallet(&self, name: &str) -> Client {
        Client::new(&format!("{}/wallet/{name}", self.node.rpc_url()), Auth::CookieFile(self.node.cookie_file())).unwrap()
    }

    /// No output may hold a private key or three words in a row of the phone
    /// key's phrase, unless the phrase was asked for.
    fn check_no_secrets(&self, out: &Output, phrase_allowed: bool) {
        for (name, stream) in [("stdout", &out.stdout), ("stderr", &out.stderr)] {
            let text = String::from_utf8_lossy(stream);
            assert!(!text.contains("tprv") && !text.contains("xprv"), "{name} holds a private key");
            if phrase_allowed && name == "stdout" {
                continue;
            }
            let words: Vec<String> = self.phone.to_secret_string().split(' ').map(str::to_owned).collect();
            for three in words.windows(3) {
                assert!(!text.contains(&three.join(" ")), "{name} repeats part of the phone key's phrase");
            }
        }
    }

    /// Runs the tool, typing the PIN and then the new wallet passphrase
    /// twice (lines a command doesn't ask for are never read).
    fn run(&self, args: &[&str]) -> Output {
        self.run_typing(args, &format!("{PIN}\n{PASSPHRASE}\n{PASSPHRASE}"))
    }

    /// Runs the tool with `typed` on its standard input.
    fn run_typing(&self, args: &[&str], typed: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_satkeep-restore"))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // The tool may exit before reading (a refusal), so a failed write is fine.
        let _ = child.stdin.take().unwrap().write_all(format!("{typed}\n").as_bytes());
        child.wait_with_output().unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        self.check_no_secrets(&out, args.contains(&"--show-phrase"));
        assert!(out.status.success(), "{args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    fn fails(&self, args: &[&str]) -> String {
        let out = self.run(args);
        self.check_no_secrets(&out, false);
        assert!(!out.status.success(), "{args:?} should have failed");
        String::from_utf8(out.stderr).unwrap()
    }
}

fn s(path: &Path) -> String {
    path.display().to_string()
}

// ------------------------------------------------------------- works ----

#[test]
fn show_tells_what_the_kit_holds_without_unlocking_anything() {
    let owner = Owner::new();
    let out = owner.ok(&["show", "--kit", &s(&owner.kit())]);
    assert!(out.contains("2026-10-23"), "{out}");
    assert!(out.contains("UNVERIFIED"), "the plain setup is marked unverified: {out}");
    assert!(out.contains("Savings wallet 1 of 2: no beneficiary"), "{out}");
    assert!(out.contains(&format!("Savings wallet 2 of 2, current: with a beneficiary: any 2 of 3, or HEIR alone after {HEIR_DELAY} blocks")), "{out}");
    assert!(out.contains("Unlocks with: second key"), "{out}");
}

#[test]
fn core_imports_both_shapes_and_phone_with_device_spends_them() {
    let owner = Owner::new();
    let (kit, key) = (owner.kit(), owner.second_key());
    let mut args = vec!["core", "--kit", &kit_s(&kit), "--simulated-device", &kit_s(&key), "--core-wallet", "restored"]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    args.extend(owner.rpc_args());
    let out = owner.ok(&args.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(out.contains("The phone key is PHONE in every Savings wallet"), "{out}");
    assert!(out.contains("\"restored\""), "{out}");
    assert!(out.contains("The wallet is encrypted"), "{out}");

    // Core found both coins from the birthday on.
    let wallet = owner.wallet("restored");
    let balances: Value = wallet.call("getbalances", &[]).unwrap();
    assert_eq!(balances["mine"]["trusted"], json!(3.0), "{balances}");

    // Encrypted and locked: Core can't sign as PHONE until it's unlocked.
    let info: Value = wallet.call("getwalletinfo", &[]).unwrap();
    assert_eq!(info["unlocked_until"], json!(0), "an encrypted wallet, locked again after the import: {info}");
    let coin: Vec<Value> = wallet.call("listunspent", &[]).unwrap();
    let input = json!([{"txid": coin[0]["txid"], "vout": coin[0]["vout"]}]);
    let to = owner.node.new_address().to_string();
    let funded: Value =
        wallet.call("walletcreatefundedpsbt", &[input, json!([{ to: 0.5 }]), json!(0), json!({"add_inputs": false})]).unwrap();
    let locked: Result<Value, _> = wallet.call("walletprocesspsbt", &[funded["psbt"].clone(), json!(true), json!("ALL")]);
    assert!(locked.is_err(), "a locked wallet signed");
    let _: Value = wallet.call("walletpassphrase", &[json!(PASSPHRASE), json!(60)]).unwrap();

    // Spend each coin, one shape per payment as Satkeep does: Core signs as
    // PHONE, DEVICE adds its signature, and the node accepts it.
    let coins: Vec<Value> = wallet.call("listunspent", &[]).unwrap();
    let mut shapes: Vec<&str> = coins.iter().map(|c| c["desc"].as_str().unwrap().split('(').next().unwrap()).collect();
    shapes.sort();
    assert_eq!(shapes, ["tr", "wsh"], "one coin of each shape");
    for coin in &coins {
        let amount = coin["amount"].as_f64().unwrap();
        let to = owner.node.new_address().to_string();
        let input = json!([{"txid": coin["txid"], "vout": coin["vout"]}]);
        let funded: Value = wallet
            .call("walletcreatefundedpsbt", &[input, json!([{ to: amount - 0.001 }]), json!(0), json!({"add_inputs": false, "fee_rate": 2})])
            .unwrap();
        let processed: Value =
            wallet.call("walletprocesspsbt", &[funded["psbt"].clone(), json!(true), json!("ALL"), json!(true), json!(false)]).unwrap();
        let mut psbt = Psbt::from_str(processed["psbt"].as_str().unwrap()).unwrap();
        let phone: usize = psbt.inputs.iter().map(|i| i.partial_sigs.len() + i.tap_script_sigs.len()).sum();
        assert_eq!(phone, 1, "Core signed as PHONE");
        let alone: Value = owner.node.rpc().call("finalizepsbt", &[json!(psbt.to_string())]).unwrap();
        assert_eq!(alone["complete"], json!(false), "the restored phone key alone can't spend Savings");
        assert_eq!(owner.device.sign(&mut psbt).unwrap(), 1, "DEVICE signed");
        let finalized: Value = owner.node.rpc().call("finalizepsbt", &[json!(psbt.to_string())]).unwrap();
        assert_eq!(finalized["complete"], json!(true), "PHONE + DEVICE is 2 of 3");
        let txid: Value = owner.node.rpc().call("sendrawtransaction", &[finalized["hex"].clone()]).unwrap();
        owner.node.mine(1);
        let tx: Value = wallet.call("gettransaction", &[txid]).unwrap();
        assert_eq!(tx["confirmations"], json!(1));
    }
}

#[test]
fn core_needs_the_same_non_empty_passphrase_twice() {
    let owner = Owner::new();
    let (kit, key) = (owner.kit(), owner.second_key());
    let mut args: Vec<String> =
        ["core", "--kit", &kit_s(&kit), "--simulated-device", &kit_s(&key), "--core-wallet", "nopass"].map(str::to_owned).to_vec();
    args.extend(owner.rpc_args());
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    for (typed, why) in [
        (format!("{PIN}\none passphrase\nanother passphrase"), "don't match"),
        (format!("{PIN}\n\n"), "can't be empty"),
    ] {
        let out = owner.run_typing(&argv, &typed);
        owner.check_no_secrets(&out, false);
        assert!(!out.status.success(), "{why}: accepted");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(why), "{err}");
        assert!(!err.contains("passphrase\n") && !err.contains("one passphrase"), "the passphrase is never repeated");
    }
    let wallets: Vec<String> = owner.node.rpc().call("listwallets", &[]).unwrap();
    assert!(!wallets.contains(&"nopass".to_owned()), "no wallet was created");
}

#[test]
fn core_out_writes_the_import_file_once_and_it_imports() {
    let owner = Owner::new();
    let (kit, key, file) = (owner.kit(), owner.second_key(), owner.dir.file("import.json"));
    let mut args: Vec<String> = ["core", "--kit", &kit_s(&kit), "--simulated-device", &kit_s(&key), "--out", &kit_s(&file)]
        .map(str::to_owned)
        .to_vec();
    args.extend(owner.rpc_args());
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = owner.ok(&argv);
    assert!(out.contains("Wrote Bitcoin Core's import request"), "{out}");
    assert!(out.contains("UNENCRYPTED: delete it once it's imported"), "{out}");

    let request: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let entries = request.as_array().unwrap();
    assert_eq!(entries.len(), 4, "receive and change for each wallet");
    assert!(entries.iter().all(|e| e["desc"].as_str().unwrap().contains("tprv")), "each holds PHONE's private key");
    assert_eq!(entries.iter().map(|e| e["internal"].as_bool().unwrap()).collect::<Vec<_>>(), [false, true, false, true]);
    assert!(entries.iter().all(|e| e["active"] == json!(true)), "one wallet of each shape: both active");

    // The file imports into a blank descriptor wallet as it is.
    owner.node.rpc().call::<Value>("createwallet", &[json!("fromfile"), json!(false), json!(true)]).unwrap();
    let results: Value = owner.wallet("fromfile").call("importdescriptors", &[request]).unwrap();
    assert!(results.as_array().unwrap().iter().all(|r| r["success"] == json!(true)));
    let balances: Value = owner.wallet("fromfile").call("getbalances", &[]).unwrap();
    assert_eq!(balances["mine"]["trusted"], json!(3.0));

    // Never overwritten.
    let err = owner.fails(&argv);
    assert!(err.contains("must not exist"), "{err}");
}

#[test]
fn liana_prints_the_descriptors_and_the_phrase_only_on_request() {
    let owner = Owner::new();
    let (kit, key) = (owner.kit(), owner.second_key());
    let out = owner.ok(&["liana", "--kit", &kit_s(&kit), "--simulated-device", &kit_s(&key)]);
    assert!(out.contains(&format!("Liana's main descriptor:\n{}", owner.heir.export_descriptor())), "{out}");
    assert!(out.contains("Liana can't hold this shape"), "{out}");
    assert!(out.contains(&owner.plain.export_descriptor()), "{out}");
    assert!(out.contains("--show-phrase"), "{out}");

    let shown = owner.run(&["liana", "--kit", &kit_s(&kit), "--simulated-device", &kit_s(&key), "--show-phrase"]);
    owner.check_no_secrets(&shown, true);
    let stdout = String::from_utf8(shown.stdout).unwrap();
    // assert!, not assert_eq!: a failure must not print the phrase.
    assert!(stdout.contains(&owner.phone.to_secret_string()), "the 12 words are shown when asked for");
    assert!(String::from_utf8_lossy(&shown.stderr).contains("warning"), "after a warning");
}

// ----------------------------------------------------------- forbidden ----

#[test]
fn the_wrong_second_key_or_pin_unlocks_nothing() {
    let owner = Owner::new();
    let kit = owner.kit();
    let other = owner.device_file("other.txt", random_secret(), PIN);
    let err = owner.fails(&["liana", "--kit", &kit_s(&kit), "--simulated-device", &kit_s(&other)]);
    assert!(err.contains("that key can't unlock this backup"), "{err}");
    let key = owner.second_key();
    let out = owner.run_typing(&["liana", "--kit", &kit_s(&kit), "--simulated-device", &kit_s(&key)], "0000");
    owner.check_no_secrets(&out, false);
    assert!(!out.status.success(), "a wrong PIN unlocked it");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("wrong PIN"), "{err}");
    let err = owner.fails(&["liana", "--kit", &kit_s(&kit)]);
    assert!(err.contains("can't talk to a second key"), "no second key given: {err}");
}

#[test]
fn a_changed_kit_or_a_backup_from_another_phone_exports_nothing() {
    let owner = Owner::new();
    let key = owner.second_key();

    // The plain setup changed (a shorter heir delay): refused after unlocking.
    let mut changed = owner.setup.clone();
    changed.savings[1].heir.as_mut().unwrap().heir_delay = 1;
    let kit = owner.kit_for("changed.pdf", &owner.phone, &owner.setup, &changed, owner.secret);
    let err = owner.fails(&["liana", "--kit", &kit_s(&kit), "--simulated-device", &kit_s(&key)]);
    assert!(err.contains("doesn't match the one locked in its backup"), "{err}");

    // Another phone key's backup, with this setup and this second key: the
    // PHONE check (T5) refuses it.
    let stranger = HolderKey::generate();
    let kit = owner.kit_for("stranger.pdf", &stranger, &owner.setup, &owner.setup, owner.secret);
    let out = owner.run(&["liana", "--kit", &kit_s(&kit), "--simulated-device", &kit_s(&key)]);
    owner.check_no_secrets(&out, false);
    assert_no_part_of(&out, &stranger);
    assert!(!out.status.success(), "another phone's backup was exported");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("isn't in Savings wallet 1"), "{err}");
}

/// No three words in a row of `key`'s phrase in either output stream.
fn assert_no_part_of(out: &Output, key: &HolderKey) {
    let words: Vec<String> = key.to_secret_string().split(' ').map(str::to_owned).collect();
    for stream in [&out.stdout, &out.stderr] {
        let text = String::from_utf8_lossy(stream);
        assert!(!words.windows(3).any(|three| text.contains(&three.join(" "))), "the output repeats part of a phrase");
    }
}

#[test]
fn a_node_on_another_network_or_an_existing_core_wallet_is_refused() {
    let owner = Owner::new();
    let key = owner.second_key();
    let mut signet = owner.setup.clone();
    signet.network = setup::Network::Signet;
    let kit = owner.kit_for("signet.pdf", &owner.phone, &signet, &signet, owner.secret);
    let mut args: Vec<String> = ["core", "--kit", &kit_s(&kit), "--simulated-device", &kit_s(&key)].map(str::to_owned).to_vec();
    args.extend(owner.rpc_args());
    let err = owner.fails(&args.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(err.contains("is on regtest, but this wallet is on signet"), "{err}");

    // Only a node on this machine: the import sends the phone key over HTTP.
    let kit = owner.kit();
    let remote = ["core", "--kit", &kit_s(&kit), "--simulated-device", &kit_s(&key), "--rpc-url", "http://example.com:18443"];
    let err = owner.fails(&remote);
    assert!(err.contains("must be a node on this machine"), "{err}");

    let kit = owner.kit();
    let mut args: Vec<String> =
        ["core", "--kit", &kit_s(&kit), "--simulated-device", &kit_s(&key), "--core-wallet", "taken"].map(str::to_owned).to_vec();
    args.extend(owner.rpc_args());
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    owner.ok(&argv);
    let err = owner.fails(&argv);
    assert!(err.contains("may already exist"), "{err}");
}

#[test]
fn a_key_on_the_command_line_is_refused_and_never_repeated() {
    let owner = Owner::new();
    let phrase = owner.phone.to_secret_string();
    // A partial phrase too: 7 of its words, as if cut short or mistyped.
    let partial = phrase.split(' ').take(7).collect::<Vec<_>>().join(" ");
    for args in [
        vec!["show", "--kit", phrase.as_str()],
        phrase.split(' ').chain(["show"]).collect(),
        vec!["show", "--kit", partial.as_str()],
        vec!["show", "--kit", "tprv8ZgxMBicQKsPdNotARealKey"],
    ] {
        let out = owner.run(&args);
        owner.check_no_secrets(&out, false);
        assert_eq!(out.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&out.stderr).contains("never takes keys on the command line"));
    }
}

fn kit_s(path: &Path) -> String {
    s(path)
}
