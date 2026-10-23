//! `twofold`: a developer command-line tool for the Twofold Savings wallet.
//! Test networks only. See the crate README.

mod node;
mod wallet_dir;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use bdk_bitcoind_rpc::bitcoincore_rpc::Client;
use bdk_wallet::KeychainKind;
use clap::{Parser, Subcommand};
use twofold_savings::{DEFAULT_DEVICE_DELAY, DEFAULT_HEIR_DELAY};

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
