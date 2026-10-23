//! Runs the real `twofold` binary against a throwaway regtest node.
//!
//! Keys are generated in memory with `HolderKey`. Private keys reach the
//! binary only through stdin, except in the one test of `--key FILE`, which
//! writes a fresh key into a temporary folder deleted when the test ends.

#![allow(dead_code)] // each test binary uses a different part of this

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;
use twofold_savings::HolderKey;
pub use twofold_testenv::Node;

pub struct Holders {
    pub phone: HolderKey,
    pub device: HolderKey,
    pub helper: HolderKey,
    pub heir: HolderKey,
}

impl Holders {
    pub fn generate() -> Self {
        Self {
            phone: HolderKey::generate(),
            device: HolderKey::generate(),
            helper: HolderKey::generate(),
            heir: HolderKey::generate(),
        }
    }

    /// PHONE, DEVICE, DEVICE_R, HELPER, HEIR, as `create` takes them.
    pub fn public_keys(&self) -> [String; 5] {
        [
            self.phone.public_key(0),
            self.device.public_key(0),
            self.device.public_key(1),
            self.helper.public_key(0),
            self.heir.public_key(0),
        ]
        .map(|k| k.to_string())
    }
}

/// One wallet folder plus the node it talks to.
pub struct Cli<'a> {
    pub node: &'a Node,
    pub dir: TempDir,
}

impl<'a> Cli<'a> {
    pub fn new(node: &'a Node) -> Self {
        Self { node, dir: TempDir::new().unwrap() }
    }

    pub fn path(&self, name: &str) -> String {
        self.dir.path().join(name).display().to_string()
    }

    /// The stand-in for `~/.twofold` in this test, so no test can touch the
    /// real one.
    pub fn home(&self) -> std::path::PathBuf {
        self.dir.path().join("home")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = self.command_in_home(&[]);
        cmd.arg("--wallet-dir").arg(self.wallet_dir()).args(args);
        cmd
    }

    /// Like `command`, but without `--wallet-dir`, so the default applies.
    fn command_in_home(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_twofold"));
        cmd.env("TWOFOLD_HOME", self.home())
            .arg("--rpc-url")
            .arg(self.node.rpc_url())
            .arg("--rpc-cookie")
            .arg(self.node.cookie_file())
            .args(args)
            .env_remove("TWOFOLD_WALLET_DIR")
            .env_remove("TWOFOLD_NETWORK")
            .env_remove("TWOFOLD_RPC_URL")
            .env_remove("TWOFOLD_RPC_COOKIE");
        cmd
    }

    pub fn run(&self, args: &[&str]) -> Output {
        check_no_secrets(self.command(args).output().expect("run twofold"))
    }

    /// Runs with `stdin` piped in, for `--key-stdin`.
    pub fn run_with_stdin(&self, args: &[&str], stdin: &str) -> Output {
        let mut child = self.command(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        check_no_secrets(child.wait_with_output().unwrap())
    }

    /// Runs with no `--wallet-dir`, so it uses the default under `home()`.
    pub fn run_in_home(&self, args: &[&str]) -> Output {
        check_no_secrets(self.command_in_home(args).output().expect("run twofold"))
    }

    /// Runs and expects success; returns stdout.
    pub fn ok(&self, args: &[&str]) -> String {
        expect_ok(args, self.run(args))
    }

    /// Runs and expects failure; returns stderr.
    pub fn fails(&self, args: &[&str]) -> String {
        expect_failure(args, self.run(args))
    }

    /// Signs the PSBT at `psbt` as `holder` without the approval prompt,
    /// passing the key on stdin.
    pub fn sign(&self, psbt: &str, holder: &HolderKey) -> String {
        let args = ["sign", "--psbt", psbt, "--yes", "--key-stdin"];
        expect_ok(&args, self.run_with_stdin(&args, &holder.to_secret_string()))
    }

    /// `twofold create` with these holders' public keys. Check-ins every
    /// half the device delay, with a 1-block margin.
    pub fn create(&self, holders: &Holders, device_delay: u16, heir_delay: u16) -> String {
        self.create_with_checkin(holders, device_delay, heir_delay, device_delay / 2, 1)
    }

    pub fn create_with_checkin(
        &self,
        holders: &Holders,
        device_delay: u16,
        heir_delay: u16,
        checkin_interval: u16,
        checkin_margin: u16,
    ) -> String {
        let [dd, hd, ci, cm] = [device_delay, heir_delay, checkin_interval, checkin_margin].map(|n| n.to_string());
        let keys = holders.public_keys();
        self.ok(&[
            "create", "--network", "regtest",
            "--phone", &keys[0], "--device", &keys[1], "--device-r", &keys[2],
            "--helper", &keys[3], "--heir", &keys[4],
            "--device-delay", &dd, "--heir-delay", &hd,
            "--checkin-interval", &ci, "--checkin-margin", &cm,
        ])
    }

    pub fn wallet_dir(&self) -> std::path::PathBuf {
        self.dir.path().join("wallet")
    }
}

pub fn expect_ok(args: &[&str], out: Output) -> String {
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "twofold {} failed:\n{stderr}{stdout}", args.join(" "));
    stdout.into_owned()
}

pub fn expect_failure(args: &[&str], out: Output) -> String {
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(!out.status.success(), "twofold {} should have failed:\n{stdout}", args.join(" "));
    stderr.into_owned()
}

/// No command may ever print a private key.
fn check_no_secrets(out: Output) -> Output {
    for stream in [&out.stdout, &out.stderr] {
        let text = String::from_utf8_lossy(stream);
        assert!(!text.contains("tprv") && !text.contains("xprv"), "twofold printed a private key");
    }
    out
}

pub fn file_exists(path: &str) -> bool {
    Path::new(path).exists()
}
