//! `twofold`: a developer command-line tool for the Twofold Savings wallet.
//! Test networks only. See the crate README.

mod node;
mod wallet_dir;

use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use bdk_bitcoind_rpc::bitcoincore_rpc::Client;
use bdk_wallet::KeychainKind;
use clap::{Parser, Subcommand, ValueEnum};
use twofold_savings::bitcoin::psbt::Psbt;
use twofold_savings::bitcoin::{Address, Amount, Denomination, FeeRate};
use twofold_savings::spend::{self, Path as SpendPath};
use twofold_savings::{DEFAULT_DEVICE_DELAY, DEFAULT_HEIR_DELAY, HolderKey};

use wallet_dir::{Config, Opened, TestNetwork};

#[derive(Parser)]
#[command(name = "twofold", version, about = "Twofold Savings wallet developer tool. Test networks only.")]
struct Cli {
    /// Folder holding this Savings wallet's savings.json and database [default: ~/.twofold/<network>]
    #[arg(long, global = true, env = "TWOFOLD_WALLET_DIR")]
    wallet_dir: Option<PathBuf>,

    /// Test network [default: regtest, or the wallet's own network]
    #[arg(long, global = true, value_enum, env = "TWOFOLD_NETWORK")]
    network: Option<TestNetwork>,

    /// Bitcoin Core RPC address [default: 127.0.0.1 on the network's usual port]
    #[arg(long, global = true, env = "TWOFOLD_RPC_URL")]
    rpc_url: Option<String>,

    /// Bitcoin Core RPC cookie file [default: the network's folder in Bitcoin Core's data directory]
    #[arg(long, global = true, env = "TWOFOLD_RPC_COOKIE")]
    rpc_cookie: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Make a new test key for one holder and save it to a file.
    Keygen {
        /// Name for the key, like phone. Saved as ~/.twofold/<network>/keys/<name>.key
        #[arg(long, required_unless_present = "out")]
        name: Option<String>,
        /// Save to this file instead. Must end in .key (the repo's .gitignore
        /// covers *.key).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Create a Savings wallet from five public keys.
    Create {
        /// PHONE's public key, like [fingerprint/87'/1'/0']tpub…/<0;1>/*
        #[arg(long)]
        phone: String,
        /// DEVICE's key for the 2-of-3 path
        #[arg(long)]
        device: String,
        /// DEVICE's key for its delayed path (DEVICE_R)
        #[arg(long = "device-r")]
        device_r: String,
        #[arg(long)]
        helper: String,
        #[arg(long)]
        heir: String,
        /// Blocks before DEVICE can spend alone (placeholder default)
        #[arg(long, default_value_t = DEFAULT_DEVICE_DELAY)]
        device_delay: u16,
        /// Blocks before HEIR can spend alone (placeholder default)
        #[arg(long, default_value_t = DEFAULT_HEIR_DELAY)]
        heir_delay: u16,
    },
    /// Show a new address to receive into Savings.
    Address,
    /// Sync with the node and show what Savings holds.
    Balance,
    /// Print the wallet descriptor, to back up or import into other software.
    Export {
        /// Print separate receive and change descriptors instead of one with <0;1>
        #[arg(long)]
        split: bool,
    },
    /// Prepare a payment from Savings and save it, unsigned, as a PSBT file.
    Send {
        /// Address to pay
        #[arg(long)]
        to: String,
        /// Amount in BTC, like 0.25
        #[arg(long, required_unless_present = "all", conflicts_with = "all")]
        amount: Option<String>,
        /// Send everything in Savings, less the network fee
        #[arg(long)]
        all: bool,
        /// Which approval path the payment uses
        #[arg(long, value_enum, default_value_t = PathArg::TwoOfThree)]
        path: PathArg,
        /// Network fee rate in sat/vB
        #[arg(long, default_value_t = 2)]
        fee_rate: u64,
        /// File to save the payment to. Must not already exist.
        #[arg(long)]
        out: PathBuf,
    },
    /// Show a payment, ask to approve it, then sign with one holder's key.
    /// Updates the PSBT file in place.
    ///
    /// Standard input carries the answer to "Approve? (y/N)" first, then,
    /// with --key-stdin, the key on the next line. A declined payment never
    /// reads the key.
    Sign {
        #[arg(long)]
        psbt: PathBuf,
        /// Approve without asking (for scripts and tests)
        #[arg(long)]
        yes: bool,
        /// File holding the private key (from `twofold keygen`). There is
        /// deliberately no way to pass the key itself as an argument.
        #[arg(long, required_unless_present = "key_stdin", conflicts_with = "key_stdin")]
        key_file: Option<PathBuf>,
        /// Read the private key from standard input instead
        #[arg(long)]
        key_stdin: bool,
    },
    /// Check a payment has the approvals it needs and send it to the network.
    Broadcast {
        #[arg(long)]
        psbt: PathBuf,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum PathArg {
    /// Any 2 of PHONE, DEVICE, HELPER. No wait.
    TwoOfThree,
    /// DEVICE alone, once every coin used has waited the device delay
    Device,
    /// HEIR alone, once every coin used has waited the heir delay
    Heir,
}

impl PathArg {
    fn path(self) -> SpendPath {
        match self {
            PathArg::TwoOfThree => SpendPath::TwoOfThree,
            PathArg::Device => SpendPath::Device,
            PathArg::Heir => SpendPath::Heir,
        }
    }

    fn who_approves(self) -> &'static str {
        match self {
            PathArg::TwoOfThree => "any 2 of PHONE, DEVICE, HELPER",
            PathArg::Device => "DEVICE alone",
            PathArg::Heir => "HEIR alone",
        }
    }
}

fn main() -> ExitCode {
    if private_key_in_arguments() {
        eprintln!(
            "error: a private key was passed on the command line. Keys go in a file (--key-file) or on \
             standard input (--key-stdin). Treat that key as exposed: it may be in your shell history."
        );
        return ExitCode::FAILURE;
    }
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Whether any argument contains an extended private key. Checked before
/// clap parses anything, because clap repeats unexpected arguments in its
/// error messages, and a key on the command line also lands in shell
/// history and the process list.
fn private_key_in_arguments() -> bool {
    std::env::args_os().skip(1).any(|arg| {
        let arg = arg.to_string_lossy();
        arg.contains("tprv8") || arg.contains("xprv9")
    })
}

impl Cli {
    /// The network asked for, or regtest.
    fn network_or_default(&self) -> TestNetwork {
        self.network.unwrap_or(TestNetwork::Regtest)
    }

    /// `--wallet-dir`, or `~/.twofold/<network>`.
    fn wallet_dir(&self) -> Result<PathBuf> {
        match &self.wallet_dir {
            Some(dir) => Ok(dir.clone()),
            None => Ok(twofold_home()?.join(self.network_or_default().core_name())),
        }
    }

    /// Opens the wallet, checking it's on `--network` if one was given.
    fn open(&self) -> Result<Opened> {
        let opened = wallet_dir::open(&self.wallet_dir()?)?;
        if let Some(network) = self.network {
            if network != opened.network() {
                bail!("this wallet is on {}, not {}", opened.network().core_name(), network.core_name());
            }
        }
        Ok(opened)
    }
}

/// `TWOFOLD_HOME`, or `.twofold` in your home folder. Outside any repo, so
/// wallet files and demo keys can't be committed by accident.
fn twofold_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("TWOFOLD_HOME") {
        return Ok(PathBuf::from(home));
    }
    let user = if cfg!(windows) { std::env::var_os("USERPROFILE") } else { std::env::var_os("HOME") };
    user.map(|u| PathBuf::from(u).join(".twofold")).context("can't find your home folder; set TWOFOLD_HOME")
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Keygen { ref name, ref out } => {
            let out = match (out, name) {
                (Some(out), _) => out.clone(),
                (None, Some(name)) => {
                    let simple = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
                    if !simple {
                        bail!("the key name may only use letters, digits, - and _");
                    }
                    twofold_home()?.join(cli.network_or_default().core_name()).join("keys").join(format!("{name}.key"))
                }
                (None, None) => unreachable!("clap requires --name or --out"),
            };
            keygen(&out)?
        }
        Command::Send { ref to, ref amount, all: _, path, fee_rate, ref out } => {
            let (mut opened, _client) = open_and_sync(&cli)?;
            let network = opened.network().network();
            let to = Address::from_str(to)
                .context("that isn't a valid address")?
                .require_network(network)
                .with_context(|| format!("that address isn't for {}", opened.network().core_name()))?;
            let amount = match amount {
                Some(text) => Some(
                    Amount::from_str_in(text, Denomination::Bitcoin).context("the amount must be in BTC, like 0.25")?,
                ),
                None => None, // --all
            };
            let fee_rate = FeeRate::from_sat_per_vb(fee_rate)
                .filter(|r| *r >= FeeRate::from_sat_per_vb_u32(1))
                .context("the fee rate must be at least 1 sat/vB")?;
            let delays = opened.policy.delays();
            let script = to.script_pubkey();
            let psbt = spend::build_spend(&mut opened.wallet, delays, path.path(), |b| {
                match amount {
                    Some(amount) => b.add_recipient(script.clone(), amount),
                    None => b.drain_wallet().drain_to(script.clone()),
                };
                b.fee_rate(fee_rate);
            })?;
            write_new(out, psbt.to_string().as_bytes())?;
            opened.persist()?;
            let paid: Amount =
                psbt.unsigned_tx.output.iter().filter(|o| o.script_pubkey == script).map(|o| o.value).sum();
            println!("Prepared a payment of {} to {to}.", btc(paid));
            println!("Network fee: {}", btc(psbt.fee()?));
            println!("Saved, unsigned, to {}.", out.display());
            println!("Needs approval from {}: run `twofold sign`, then `twofold broadcast`.", path.who_approves());
        }
        Command::Sign { ref psbt, ref key_file, key_stdin, yes } => {
            let opened = cli.open()?;
            let mut payment = read_psbt(psbt)?;
            print_payment(&opened, &payment)?;
            let stdin = std::io::stdin();
            let mut lines = stdin.lock().lines();
            if !yes {
                print!("Approve? (y/N) ");
                std::io::stdout().flush()?;
                let answer = lines.next().transpose().context("reading the answer")?.unwrap_or_default();
                if !matches!(answer.trim(), "y" | "Y" | "yes" | "Yes" | "YES") {
                    println!();
                    bail!("not approved; nothing was signed");
                }
            }
            let text = if key_stdin {
                lines.next().transpose().context("reading the key from standard input")?.unwrap_or_default()
            } else {
                let path = key_file.as_ref().expect("clap requires --key-file or --key-stdin");
                fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
            };
            let holder = HolderKey::from_secret_str(&text)?;
            drop(text);
            let added = holder.sign(&mut payment)?;
            if added == 0 {
                bail!("that key isn't part of this payment, or it has already approved it");
            }
            fs::write(psbt, payment.to_string()).with_context(|| format!("writing {}", psbt.display()))?;
            println!("Added {added} signature{} to {}.", if added == 1 { "" } else { "s" }, psbt.display());
        }
        Command::Broadcast { ref psbt } => {
            let (mut opened, client) = open_and_sync(&cli)?;
            let payment = read_psbt(psbt)?;
            let tx = spend::finalize(&opened.wallet, &payment)?;
            let txid = node::broadcast(&client, &tx)?;
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
            opened.wallet.apply_unconfirmed_txs([(tx, now)]);
            opened.persist()?;
            println!("Sent. Transaction ID: {txid}");
        }
        Command::Create { ref phone, ref device, ref device_r, ref helper, ref heir, device_delay, heir_delay } => {
            let network = cli.network_or_default();
            let dir = cli.wallet_dir()?;
            let client = connect(&cli.rpc_url, &cli.rpc_cookie, network)?;
            let config = Config {
                network,
                device_delay,
                heir_delay,
                phone: phone.clone(),
                device: device.clone(),
                device_r: device_r.clone(),
                helper: helper.clone(),
                heir: heir.clone(),
                birthday: node::height(&client)?,
            };
            let mut opened = wallet_dir::create(&dir, config)?;
            opened.persist()?;
            println!("Created a Savings wallet on {} in {}.", network.core_name(), dir.display());
            println!("Any 2 of PHONE, DEVICE, HELPER can approve at any time.");
            println!("DEVICE alone can approve once a coin has {device_delay} confirmations.");
            println!("HEIR alone can approve once a coin has {heir_delay} confirmations.");
            println!("Settings: {}", wallet_dir::config_path(&dir).display());
        }
        Command::Address => {
            let mut opened = cli.open()?;
            let address = opened.wallet.reveal_next_address(KeychainKind::External);
            opened.persist()?;
            println!("{}", address.address);
        }
        Command::Balance => {
            let (mut opened, client) = open_and_sync(&cli)?;
            print_balance(&opened, &client)?;
            opened.persist()?;
        }
        Command::Export { split } => {
            let opened = cli.open()?;
            if split {
                let (receive, change) = opened.policy.export_receive_and_change();
                println!("{receive}");
                println!("{change}");
            } else {
                println!("{}", opened.policy.export_descriptor());
            }
        }
    }
    Ok(())
}

/// Writes a fresh test-network key to `out`. Refuses anything but a new
/// `.key` file, so even inside a repo `.gitignore` keeps it out of commits.
fn keygen(out: &Path) -> Result<()> {
    if out.extension().and_then(|e| e.to_str()) != Some("key") {
        bail!("the key file's name must end in .key, so .gitignore keeps it out of commits");
    }
    let key = HolderKey::generate();
    let secret = key.to_secret_string();
    if !secret.starts_with("tprv") {
        bail!("refusing to save a key that isn't a test-network key");
    }
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    write_new(out, format!("{secret}\n").as_bytes())?;
    println!("Saved a new test key to {}.", out.display());
    println!("Anyone with this file can approve as its holder. Keep it private and never commit it.");
    println!("Public key (for PHONE, DEVICE, HELPER or HEIR):");
    println!("  {}", key.public_key(0));
    println!("Second public key (DEVICE only, for DEVICE_R):");
    println!("  {}", key.public_key(1));
    Ok(())
}

/// Creates `path` and writes `contents`; refuses if it already exists.
fn write_new(path: &Path, contents: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("creating {} (it must not already exist)", path.display()))?;
    file.write_all(contents).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn read_psbt(path: &Path) -> Result<Psbt> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Psbt::from_str(text.trim()).with_context(|| format!("{} isn't a valid PSBT", path.display()))
}

fn connect(url: &Option<String>, cookie: &Option<PathBuf>, network: TestNetwork) -> Result<Client> {
    let url = url.clone().unwrap_or_else(|| node::default_url(network));
    let cookie = cookie.clone().unwrap_or_else(|| node::default_cookie(network));
    node::connect(&url, &cookie, network)
}

/// Prints what a payment does, so a holder sees it before approving:
/// every output's address and amount (marking money that comes back to
/// Savings), the network fee, and which path it needs. Refuses a payment
/// that doesn't spend this wallet's coins.
fn print_payment(opened: &Opened, payment: &Psbt) -> Result<()> {
    let wallet = &opened.wallet;
    for (i, input) in payment.unsigned_tx.input.iter().enumerate() {
        if wallet.get_utxo(input.previous_output).is_none() {
            bail!(
                "input {i} isn't an unspent coin of this Savings wallet. Run `twofold balance` to sync, \
                 or check --wallet-dir"
            );
        }
    }
    let path = spend::path_of(payment, opened.policy.delays())
        .context("this payment's inputs ask for different approval paths")?;
    let network = opened.network().network();
    println!("Payment to approve:");
    for output in &payment.unsigned_tx.output {
        let amount = btc(output.value);
        let address = Address::from_script(&output.script_pubkey, network)
            .map(|a| a.to_string())
            .unwrap_or_else(|_| format!("script {}", output.script_pubkey.to_hex_string()));
        match wallet.derivation_of_spk(output.script_pubkey.clone()) {
            Some((KeychainKind::Internal, _)) => println!("  {amount} comes back to Savings as change ({address})"),
            Some((KeychainKind::External, _)) => println!("  {amount} to {address} (one of your own Savings addresses)"),
            None => println!("  {amount} to {address}"),
        }
    }
    println!("  Network fee: {}", btc(payment.fee()?));
    let who = match path {
        SpendPath::TwoOfThree => PathArg::TwoOfThree,
        SpendPath::Device => PathArg::Device,
        SpendPath::Heir => PathArg::Heir,
    };
    println!("  Approval path: {}", who.who_approves());
    Ok(())
}

fn open_and_sync(cli: &Cli) -> Result<(Opened, Client)> {
    let mut opened = cli.open()?;
    let client = connect(&cli.rpc_url, &cli.rpc_cookie, opened.network())?;
    node::sync(&client, &mut opened.wallet, opened.config.birthday)?;
    opened.persist()?;
    Ok((opened, client))
}

fn print_balance(opened: &Opened, client: &Client) -> Result<()> {
    let tip = node::height(client)?;
    let balance = opened.wallet.balance();
    println!("Savings on {}, synced to block {tip}", opened.network().core_name());
    println!("Total: {}", btc(balance.total()));
    let waiting = balance.trusted_pending + balance.untrusted_pending;
    if waiting > twofold_savings::bitcoin::Amount::ZERO {
        println!("Waiting for confirmation: {}", btc(waiting));
    }
    let delays = opened.policy.delays();
    for coin in opened.wallet.list_unspent() {
        // Confirmations as of the next block, which is what a delay is
        // checked against: a coin confirmed in block c has tip - c + 1.
        let confirmations = coin.chain_position.confirmation_height_upper_bound().map_or(0, |c| tip + 1 - c);
        println!(
            "  {}  {}  {} confirmations  device path {}  heir path {}",
            btc(coin.txout.value),
            coin.outpoint,
            confirmations,
            opens_in(confirmations, delays.device()),
            opens_in(confirmations, delays.heir()),
        );
    }
    Ok(())
}

fn opens_in(confirmations: u32, delay: u16) -> String {
    match u32::from(delay).checked_sub(confirmations) {
        Some(0) | None => "open".into(),
        Some(n) => format!("opens in {n} blocks"),
    }
}

fn btc(amount: twofold_savings::bitcoin::Amount) -> String {
    format!("{:.8} BTC", amount.to_btc())
}
