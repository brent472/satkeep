//! `twofold`: a developer command-line tool for the Twofold Savings wallet.
//! Test networks only. See the crate README.

mod node;
mod wallet_dir;

use std::fs;
use std::io::{Read, Write};
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
    /// Folder holding this Savings wallet's public data. Never holds private keys.
    #[arg(long, global = true, env = "TWOFOLD_WALLET_DIR", default_value = "wallet-data")]
    wallet_dir: PathBuf,

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
        /// File to create. Must end in .key (the repo's .gitignore covers
        /// *.key) and must not already exist.
        #[arg(long)]
        out: PathBuf,
    },
    /// Create a Savings wallet from five public keys.
    Create {
        #[arg(long, value_enum)]
        network: TestNetwork,
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
    /// Approve a payment with one holder's key. Updates the PSBT file in place.
    Sign {
        #[arg(long)]
        psbt: PathBuf,
        /// File holding the private key (from `twofold keygen`)
        #[arg(long, required_unless_present = "key_stdin", conflicts_with = "key_stdin")]
        key: Option<PathBuf>,
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
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Keygen { ref out } => keygen(out)?,
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
        Command::Sign { ref psbt, ref key, key_stdin } => {
            let mut payment = read_psbt(psbt)?;
            let mut text = String::new();
            if key_stdin {
                std::io::stdin().read_to_string(&mut text).context("reading the key from standard input")?;
            } else {
                let path = key.as_ref().expect("clap requires --key or --key-stdin");
                text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
            }
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
        Command::Create { network, phone, device, device_r, helper, heir, device_delay, heir_delay } => {
            let client = connect(&cli.rpc_url, &cli.rpc_cookie, network)?;
            let config = Config {
                network,
                device_delay,
                heir_delay,
                phone,
                device,
                device_r,
                helper,
                heir,
                birthday: node::height(&client)?,
            };
            let mut opened = wallet_dir::create(&cli.wallet_dir, config)?;
            opened.persist()?;
            println!("Created a Savings wallet on {} in {}.", network.core_name(), cli.wallet_dir.display());
            println!("Any 2 of PHONE, DEVICE, HELPER can approve at any time.");
            println!("DEVICE alone can approve once a coin has {device_delay} confirmations.");
            println!("HEIR alone can approve once a coin has {heir_delay} confirmations.");
            println!("Settings: {}", wallet_dir::config_path(&cli.wallet_dir).display());
        }
        Command::Address => {
            let mut opened = wallet_dir::open(&cli.wallet_dir)?;
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
            let opened = wallet_dir::open(&cli.wallet_dir)?;
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

/// Writes a fresh key to `out`. Refuses anything but a new `.key` file, so
/// the repo's `.gitignore` keeps it out of commits.
fn keygen(out: &Path) -> Result<()> {
    if out.extension().and_then(|e| e.to_str()) != Some("key") {
        bail!("the key file's name must end in .key, so .gitignore keeps it out of commits");
    }
    let key = HolderKey::generate();
    write_new(out, format!("{}\n", key.to_secret_string()).as_bytes())?;
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

fn open_and_sync(cli: &Cli) -> Result<(Opened, Client)> {
    let mut opened = wallet_dir::open(&cli.wallet_dir)?;
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
