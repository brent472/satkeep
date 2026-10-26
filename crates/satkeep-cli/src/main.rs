//! `satkeep`: a developer command-line tool for the Satkeep Savings wallet.
//! Test networks only. See the crate README.

use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use bdk_bitcoind_rpc::bitcoincore_rpc::Client;
use bdk_wallet::KeychainKind;
use clap::{Parser, Subcommand, ValueEnum};
use satkeep_savings::bitcoin::psbt::Psbt;
use satkeep_savings::bitcoin::{Address, Amount, Denomination, FeeRate};
use satkeep_savings::checkin::{self, Reminder};
use satkeep_savings::spend::{self, Path as SpendPath};
use satkeep_savings::{DEFAULT_HEIR_DELAY, HolderKey, SavingsPolicy, contains_recovery_phrase, migrate};
use zeroize::Zeroizing;

use satkeep_cli::wallet_dir::{self, Config, MovingTo, Opened, TestNetwork};
use satkeep_cli::node;

#[derive(Parser)]
#[command(name = "satkeep", version, about = "Satkeep Savings wallet developer tool. Test networks only.")]
struct Cli {
    /// Folder holding this Savings wallet's savings.json and database [default: ~/.satkeep/<network>]
    #[arg(long, global = true, env = "SATKEEP_WALLET_DIR")]
    wallet_dir: Option<PathBuf>,

    /// Test network [default: regtest, or the wallet's own network]
    #[arg(long, global = true, value_enum, env = "SATKEEP_NETWORK")]
    network: Option<TestNetwork>,

    /// Bitcoin Core RPC address [default: 127.0.0.1 on the network's usual port]
    #[arg(long, global = true, env = "SATKEEP_RPC_URL")]
    rpc_url: Option<String>,

    /// Bitcoin Core RPC cookie file [default: the network's folder in Bitcoin Core's data directory]
    #[arg(long, global = true, env = "SATKEEP_RPC_COOKIE")]
    rpc_cookie: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Make a new test key for one holder and save it to a file.
    Keygen {
        /// Name for the key, like phone. Saved as ~/.satkeep/<network>/keys/<name>.key
        #[arg(long, required_unless_present = "out")]
        name: Option<String>,
        /// Save to this file instead. Must end in .key (the repo's .gitignore
        /// covers *.key).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Create a Savings wallet: any 2 of PHONE, DEVICE, HELPER. With --heir,
    /// HEIR can also spend alone after the heir delay; without it, there's
    /// no delayed path and no check-ins (add a beneficiary later with
    /// `satkeep add-heir`).
    Create {
        /// PHONE's public key, like [fingerprint/87'/1'/0']tpub…/<0;1>/*
        #[arg(long)]
        phone: String,
        #[arg(long)]
        device: String,
        #[arg(long)]
        helper: String,
        #[command(flatten)]
        heir: HeirArgs,
        /// The shortest heir delay this wallet will accept, now and in any
        /// later move (placeholder default, about 6 months). Kept by
        /// add-heir.
        #[arg(long)]
        min_heir_delay: Option<u16>,
    },
    /// Add a beneficiary, or change it: create a new Savings wallet with the
    /// same PHONE, DEVICE and HELPER plus HEIR, and prepare one 2-of-3
    /// payment moving every confirmed coin there. Saved unsigned.
    AddHeir {
        /// Folder for the new wallet. Must not already hold one.
        #[arg(long)]
        new_wallet_dir: PathBuf,
        #[command(flatten)]
        heir: HeirArgs,
        /// Network fee rate in sat/vB
        #[arg(long, default_value_t = 2)]
        fee_rate: u64,
        /// File to save the move to. Must not already exist.
        #[arg(long)]
        out: PathBuf,
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
    /// Prepare a check-in: every Savings coin, through the 2-of-3 path, into
    /// one new Savings address, restarting the heir delay. Saved unsigned.
    /// Only for Savings with a beneficiary.
    Checkin {
        /// Network fee rate in sat/vB
        #[arg(long, default_value_t = 2)]
        fee_rate: u64,
        /// File to save the check-in to. Must not already exist.
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
        /// File holding the private key (from `satkeep keygen`). There is
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

/// The beneficiary, for `create` and `add-heir`.
#[derive(clap::Args)]
struct HeirArgs {
    /// HEIR's public key. Required by add-heir; optional for create.
    #[arg(long)]
    heir: Option<String>,
    /// Blocks before HEIR can spend alone (placeholder default, about 14 months)
    #[arg(long, requires = "heir")]
    heir_delay: Option<u16>,
    /// Blocks between check-ins (placeholder default, about 12 months)
    #[arg(long, requires = "heir")]
    checkin_interval: Option<u16>,
    /// Blocks the check-in interval must leave before the heir delay
    /// (placeholder default, about 2 months)
    #[arg(long, requires = "heir")]
    checkin_margin: Option<u16>,
    /// Payments also spend every coin within this many blocks of needing a
    /// check-in, so they count as its check-in (placeholder default, about
    /// 2 months: coins older than about 10 months)
    #[arg(long, requires = "heir")]
    refresh_window: Option<u16>,
    /// Don't refresh coins with payments. Keeps coins separate, for
    /// privacy; only check-ins then restart the heir delay.
    #[arg(long, requires = "heir", conflicts_with = "refresh_window")]
    no_refresh_with_payments: bool,
}

impl HeirArgs {
    /// Fills a config's beneficiary fields, with the defaults for anything
    /// not given. Leaves them empty without --heir.
    fn apply(&self, config: &mut Config) {
        config.heir = self.heir.clone();
        if self.heir.is_some() {
            config.heir_delay = Some(self.heir_delay.unwrap_or(DEFAULT_HEIR_DELAY));
            config.checkin_interval = Some(self.checkin_interval.unwrap_or(checkin::DEFAULT_CHECKIN_INTERVAL));
            config.checkin_margin = Some(self.checkin_margin.unwrap_or(checkin::DEFAULT_CHECKIN_MARGIN));
            if self.no_refresh_with_payments {
                config.refresh_with_payments = Some(false);
            } else {
                config.refresh_with_payments = Some(true);
                config.refresh_window = Some(self.refresh_window.unwrap_or(checkin::DEFAULT_REFRESH_WINDOW));
            }
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum PathArg {
    /// Any 2 of PHONE, DEVICE, HELPER. No wait.
    TwoOfThree,
    /// HEIR alone, once every coin used has waited the heir delay. Only in
    /// Savings with a beneficiary.
    Heir,
}

impl PathArg {
    fn path(self) -> SpendPath {
        match self {
            PathArg::TwoOfThree => SpendPath::TwoOfThree,
            PathArg::Heir => SpendPath::Heir,
        }
    }

    fn who_approves(self) -> &'static str {
        match self {
            PathArg::TwoOfThree => "any 2 of PHONE, DEVICE, HELPER",
            PathArg::Heir => "HEIR alone",
        }
    }
}

fn main() -> ExitCode {
    if private_key_in_arguments() {
        eprintln!(
            "error: a private key or recovery phrase was passed on the command line. Keys go in a file \
             (--key-file) or on standard input (--key-stdin). Treat that key as exposed: it may be in your \
             shell history."
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

/// Whether the arguments contain an extended private key or a recovery
/// phrase, whether quoted as one argument or spread over several. Checked
/// before clap parses anything, because clap repeats unexpected arguments in
/// its error messages, and a key on the command line also lands in shell
/// history and the process list.
fn private_key_in_arguments() -> bool {
    let args: Vec<String> = std::env::args_os().skip(1).map(|arg| arg.to_string_lossy().into_owned()).collect();
    args.iter().any(|arg| arg.contains("tprv8") || arg.contains("xprv9")) || contains_recovery_phrase(&args.join(" "))
}

impl Cli {
    /// The network asked for, or regtest.
    fn network_or_default(&self) -> TestNetwork {
        self.network.unwrap_or(TestNetwork::Regtest)
    }

    /// `--wallet-dir`, or `~/.satkeep/<network>`.
    fn wallet_dir(&self) -> Result<PathBuf> {
        match &self.wallet_dir {
            Some(dir) => Ok(dir.clone()),
            None => Ok(satkeep_home()?.join(self.network_or_default().core_name())),
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

/// `SATKEEP_HOME`, or `.satkeep` in your home folder. Outside any repo, so
/// wallet files and demo keys can't be committed by accident.
fn satkeep_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("SATKEEP_HOME") {
        return Ok(PathBuf::from(home));
    }
    let user = if cfg!(windows) { std::env::var_os("USERPROFILE") } else { std::env::var_os("HOME") };
    user.map(|u| PathBuf::from(u).join(".satkeep")).context("can't find your home folder; set SATKEEP_HOME")
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
                    satkeep_home()?.join(cli.network_or_default().core_name()).join("keys").join(format!("{name}.key"))
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
            let rule = opened.config.payment_rule()?;
            let psbt = spend::build_payment(&mut opened.wallet, delays, path.path(), rule.as_ref(), |b| {
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
            if let Some(rule) = rule.filter(|_| matches!(path, PathArg::TwoOfThree)) {
                print_refresh(&opened, &psbt, &rule, "");
            }
            println!("Saved, unsigned, to {}.", out.display());
            println!("Needs approval from {}: run `satkeep sign`, then `satkeep broadcast`.", path.who_approves());
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
                if !matches!(without_bom(&answer).trim(), "y" | "Y" | "yes" | "Yes" | "YES") {
                    println!();
                    bail!("not approved; nothing was signed");
                }
            }
            // Wiped from memory when dropped, like the phrase inside HolderKey.
            let text = Zeroizing::new(if key_stdin {
                lines.next().transpose().context("reading the key from standard input")?.unwrap_or_default()
            } else {
                let path = key_file.as_ref().expect("clap requires --key-file or --key-stdin");
                fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
            });
            let holder = HolderKey::from_secret_str(&text)?;
            drop(text);
            let before = signatures_per_input(&payment);
            let added = holder.sign(&mut payment)?;
            if added == 0 {
                bail!("that key isn't part of this payment, or it has already approved it");
            }
            // A payment that hides an input from this key (by leaving out the
            // data it needs to sign it) is refused. This only proves the key
            // added a signature to each input, not that each one is usable;
            // what protects the value is check_inputs, run before anything is
            // shown: every amount matches the wallet, so the fee shown is the
            // fee paid.
            let after = signatures_per_input(&payment);
            if before.iter().zip(&after).any(|(b, a)| a <= b) {
                bail!(
                    "that key couldn't sign every coin in this payment, so nothing was saved; \
                     the payment may have been altered"
                );
            }
            fs::write(psbt, payment.to_string()).with_context(|| format!("writing {}", psbt.display()))?;
            println!("Added {added} signature{} to {}.", if added == 1 { "" } else { "s" }, psbt.display());
        }
        Command::Checkin { fee_rate, ref out } => {
            let (mut opened, _client) = open_and_sync(&cli)?;
            if opened.config.checkin_rule()?.is_none() {
                bail!("this Savings has no beneficiary, so it has no delayed path and needs no check-in");
            }
            let fee_rate = FeeRate::from_sat_per_vb(fee_rate)
                .filter(|r| *r >= FeeRate::from_sat_per_vb_u32(1))
                .context("the fee rate must be at least 1 sat/vB")?;
            let delays = opened.policy.delays();
            let psbt = checkin::build_checkin(&mut opened.wallet, delays, fee_rate, opened.config.fee_ceiling()?)?;
            write_new(out, psbt.to_string().as_bytes())?;
            opened.persist()?;
            let total: Amount = psbt.inputs.iter().filter_map(|i| i.witness_utxo.as_ref()).map(|o| o.value).sum();
            let coins = psbt.unsigned_tx.input.len();
            println!(
                "Prepared a check-in of {coins} coin{}, {}, into one new Savings address.",
                if coins == 1 { "" } else { "s" },
                btc(total)
            );
            println!("Network fee: {}", btc(psbt.fee()?));
            println!("Saved, unsigned, to {}.", out.display());
            println!("Once confirmed, the heir delay restarts for all of it.");
            let waiting = opened.wallet.list_unspent().filter(|u| !u.chain_position.is_confirmed()).count();
            if waiting > 0 {
                println!(
                    "Left out {waiting} unconfirmed coin{}: {} delays haven't started yet.",
                    if waiting == 1 { "" } else { "s" },
                    if waiting == 1 { "its" } else { "their" }
                );
            }
            println!("Needs approval from any 2 of PHONE, DEVICE, HELPER: run `satkeep sign`, then `satkeep broadcast`.");
        }
        Command::Broadcast { ref psbt } => {
            let (mut opened, client) = open_and_sync(&cli)?;
            let payment = read_psbt(psbt)?;
            // Synced just now: refuses if a coin arrived since it was built.
            if checkin::is_checkin(&payment) {
                checkin::check_checkin(&opened.wallet, &payment, opened.policy.delays(), opened.config.fee_ceiling()?)?;
            }
            let moved_to = checked_move(&opened, &payment)?;
            spend::check_inputs(&opened.wallet, &payment)?;
            let tx = spend::finalize(&opened.wallet, &payment)?;
            let txid = node::broadcast(&client, &tx)?;
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
            opened.wallet.apply_unconfirmed_txs([(tx, now)]);
            opened.persist()?;
            println!("Sent. Transaction ID: {txid}");
            if moved_to.is_some() {
                println!("Once it confirms, Savings is in the new wallet: use its folder (--wallet-dir) from now on.");
            }
        }
        Command::Create { ref phone, ref device, ref helper, ref heir, min_heir_delay } => {
            let network = cli.network_or_default();
            let dir = cli.wallet_dir()?;
            let client = connect(&cli.rpc_url, &cli.rpc_cookie, network)?;
            let mut config = Config {
                network,
                phone: phone.clone(),
                device: device.clone(),
                helper: helper.clone(),
                heir: None,
                heir_delay: None,
                birthday: node::height(&client)?,
                checkin_interval: None,
                checkin_margin: None,
                refresh_with_payments: None,
                refresh_window: None,
                min_heir_delay,
                moving_to: None,
                max_fee_sat: None,
                max_fee_percent: None,
                device_r: None,
            };
            heir.apply(&mut config);
            config.policy()?;
            config.checkin_rule()?;
            config.payment_rule()?;
            let mut opened = wallet_dir::create(&dir, config)?;
            opened.persist()?;
            println!("Created a Savings wallet on {} in {}.", network.core_name(), dir.display());
            print_rules(&opened.config);
            println!("Settings: {}", wallet_dir::config_path(&dir).display());
        }
        Command::AddHeir { ref new_wallet_dir, ref heir, fee_rate, ref out } => {
            if heir.heir.is_none() {
                bail!("add-heir needs --heir, the beneficiary's public key");
            }
            let (mut opened, client) = open_and_sync(&cli)?;
            let fee_rate = FeeRate::from_sat_per_vb(fee_rate)
                .filter(|r| *r >= FeeRate::from_sat_per_vb_u32(1))
                .context("the fee rate must be at least 1 sat/vB")?;
            let mut config = Config {
                network: opened.network(),
                phone: opened.config.phone.clone(),
                device: opened.config.device.clone(),
                helper: opened.config.helper.clone(),
                heir: None,
                heir_delay: None,
                birthday: node::height(&client)?,
                checkin_interval: None,
                checkin_margin: None,
                refresh_with_payments: None,
                refresh_window: None,
                min_heir_delay: opened.config.min_heir_delay,
                moving_to: None,
                max_fee_sat: opened.config.max_fee_sat,
                max_fee_percent: opened.config.max_fee_percent,
                device_r: None,
            };
            heir.apply(&mut config);
            let to = config.policy()?;
            config.checkin_rule()?;
            config.payment_rule()?;
            let min_heir_delay = opened.config.min_heir_delay();
            let ceiling = opened.config.fee_ceiling()?;
            let psbt = migrate::build_move(&mut opened.wallet, &opened.policy, &to, min_heir_delay, fee_rate, ceiling)?;
            let mut new = wallet_dir::create(new_wallet_dir, config)?;
            new.persist()?;
            write_new(out, psbt.to_string().as_bytes())?;
            opened.persist()?;
            // What sign and broadcast check the move against, here.
            let heir_delay = to.delays().heir().expect("a wallet with a beneficiary");
            opened.config.moving_to = Some(MovingTo { heir: new.config.heir.clone().expect("set above"), heir_delay });
            opened.save_config()?;
            let total: Amount = psbt.inputs.iter().filter_map(|i| i.witness_utxo.as_ref()).map(|o| o.value).sum();
            println!("Created the new Savings wallet in {}.", new_wallet_dir.display());
            print_rules(&new.config);
            println!(
                "Prepared the move of {} coin{}, {}, into it, in one payment.",
                psbt.unsigned_tx.input.len(),
                if psbt.unsigned_tx.input.len() == 1 { "" } else { "s" },
                btc(total)
            );
            println!("Network fee: {}", btc(psbt.fee()?));
            let destination = Address::from_script(&migrate::destination(&to), opened.network().network())?;
            println!("It goes to {destination}, the new wallet's first address.");
            println!("HEIR's key: {}", to.keys().get(satkeep_savings::Role::Heir).expect("checked above"));
            println!("Heir delay: {}", blocks_and_about(heir_delay.into()));
            println!(
                "Set up in this wallet's settings ({}), so `satkeep sign` and `satkeep broadcast` \
                 refuse a move that goes anywhere else.",
                wallet_dir::config_path(&opened.dir).display()
            );
            println!("Saved, unsigned, to {}.", out.display());
            println!(
                "Needs approval from any 2 of PHONE, DEVICE, HELPER: run `satkeep sign`, then `satkeep broadcast`, \
                 with this wallet. Then use --wallet-dir {}.",
                new_wallet_dir.display()
            );
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

/// Writes a fresh key's recovery phrase to `out`. Refuses anything but a new
/// `.key` file, so even inside a repo `.gitignore` keeps it out of commits.
/// The phrase is never printed.
fn keygen(out: &Path) -> Result<()> {
    if out.extension().and_then(|e| e.to_str()) != Some("key") {
        bail!("the key file's name must end in .key, so .gitignore keeps it out of commits");
    }
    let key = HolderKey::generate();
    let secret = Zeroizing::new(format!("{}\n", key.to_secret_string()));
    // What gets written must read back as the same key.
    if HolderKey::from_secret_str(&secret)?.public_key(0) != key.public_key(0) {
        bail!("refusing to save a key that doesn't read back as itself");
    }
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    write_new(out, secret.as_bytes())?;
    println!("Saved a new test key to {}.", out.display());
    println!("Anyone with this file can approve as its holder. Keep it private and never commit it.");
    println!("Public key (for PHONE, DEVICE, HELPER or HEIR):");
    println!("  {}", key.public_key(0));
    Ok(())
}

/// How many signatures each input carries, Taproot and SegWit v0 alike.
fn signatures_per_input(psbt: &Psbt) -> Vec<usize> {
    psbt.inputs.iter().map(|i| i.tap_script_sigs.len() + i.partial_sigs.len()).collect()
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
    Psbt::from_str(without_bom(&text).trim()).with_context(|| format!("{} isn't a valid PSBT", path.display()))
}

/// Drops a leading byte-order mark. Windows PowerShell 5.1 puts one in front
/// of anything it pipes to a program, and Notepad can save one too.
fn without_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
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
                "input {i} isn't an unspent coin of this Savings wallet. Run `satkeep balance` to sync, \
                 or check --wallet-dir"
            );
        }
    }
    // Every amount must match the wallet's own record, so the fee shown
    // below is the fee paid.
    spend::check_inputs(wallet, payment)?;
    let path = spend::path_of(payment, opened.policy.delays())
        .context("this payment's inputs ask for different approval paths")?;
    let network = opened.network().network();
    if checkin::is_checkin(payment) {
        checkin::check_checkin(wallet, payment, opened.policy.delays(), opened.config.fee_ceiling()?)?;
        let heir_delay = opened.policy.delays().heir().expect("check-ins need a beneficiary");
        println!("Check-in to approve: every Savings coin goes into one new Savings address,");
        println!("restarting the heir delay ({}).", blocks_and_about(heir_delay.into()));
    } else if let Some(to) = checked_move(opened, payment)? {
        let heir_delay = to.delays().heir().expect("a move goes to a wallet with a beneficiary");
        let heir = to.keys().get(satkeep_savings::Role::Heir).expect("a move goes to a wallet with a beneficiary");
        println!("Move to approve: every Savings coin goes to the new Savings wallet set up in this");
        println!("wallet's settings, with the same PHONE, DEVICE and HELPER, where HEIR can also");
        println!("approve alone after the heir delay.");
        // Show exactly which key and delay, so they can be checked against
        // the beneficiary's own before approving.
        println!("  HEIR's key: {heir}");
        println!("  Heir delay: {}", blocks_and_about(heir_delay.into()));
        println!("  Check this is your beneficiary's key and the delay is the one you chose.");
    } else {
        println!("Payment to approve:");
    }
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
    // Only a plain 2-of-3 payment: a check-in or move already spends
    // everything, and the rule never applies to the heir path.
    let plain_payment = !checkin::is_checkin(payment) && !migrate::is_move(payment);
    if plain_payment && path == SpendPath::TwoOfThree {
        if let Some(rule) = opened.config.payment_rule()? {
            print_refresh(opened, payment, &rule, "  ");
        }
    }
    let who = match path {
        SpendPath::TwoOfThree => PathArg::TwoOfThree,
        SpendPath::Heir => PathArg::Heir,
    };
    println!("  Approval path: {}", who.who_approves());
    Ok(())
}

/// For a move, the wallet it goes to, rebuilt from this wallet's own
/// settings (its keys plus the beneficiary set up by `add-heir`), after
/// checking the move goes there and nowhere else. A PSBT is treated as a
/// move if it's marked as one or pays that wallet. `None` for any other
/// payment.
fn checked_move(opened: &Opened, payment: &Psbt) -> Result<Option<SavingsPolicy>> {
    let to = opened.config.move_target(&opened.policy)?;
    let is_move = migrate::is_move(payment) || to.as_ref().is_some_and(|to| migrate::pays_to(payment, to));
    if !is_move {
        return Ok(None);
    }
    let to = to.context(
        "this is a move to a new Savings wallet, but no move is set up in this wallet's settings, \
         so there's nothing to check where it goes against (set one up with `satkeep add-heir`)",
    )?;
    let ceiling = opened.config.fee_ceiling()?;
    migrate::check_move(&opened.wallet, payment, &opened.policy, &to, opened.config.min_heir_delay(), ceiling)?;
    Ok(Some(to))
}

/// What a 2-of-3 payment does under the payment rule: which coins due for a
/// refresh it also spends, restarting their heir delay, and any it leaves
/// out (a payment someone else prepared, or a coin that aged since).
fn print_refresh(opened: &Opened, payment: &Psbt, rule: &checkin::PaymentRule, indent: &str) {
    let refresh = checkin::refresh_of(&opened.wallet, payment, rule);
    let amount = |coins: &[satkeep_savings::bitcoin::OutPoint]| -> Amount {
        coins.iter().filter_map(|c| opened.wallet.get_utxo(*c)).map(|u| u.txout.value).sum()
    };
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    if !refresh.refreshed.is_empty() {
        let n = refresh.refreshed.len();
        let change = payment.unsigned_tx.output.iter().any(|o| {
            matches!(opened.wallet.derivation_of_spk(o.script_pubkey.clone()), Some((KeychainKind::Internal, _)))
        });
        println!(
            "{indent}Also refreshes {n} coin{} ({}) close to needing a check-in: their heir delay restarts \
             once this confirms.{}",
            plural(n),
            btc(amount(&refresh.refreshed)),
            if change { " Change comes back to Savings." } else { "" }
        );
    }
    if !refresh.left_out.is_empty() {
        let n = refresh.left_out.len();
        println!(
            "{indent}Leaves out {n} coin{} ({}) due for a refresh: they still need a check-in or a later payment.",
            plural(n),
            btc(amount(&refresh.left_out))
        );
    }
}

fn open_and_sync(cli: &Cli) -> Result<(Opened, Client)> {
    let mut opened = cli.open()?;
    let client = connect(&cli.rpc_url, &cli.rpc_cookie, opened.network())?;
    node::sync(&client, &mut opened.wallet, opened.config.birthday)?;
    opened.persist()?;
    Ok((opened, client))
}

/// Who can approve what, in this wallet's shape.
fn print_rules(config: &Config) {
    println!("Any 2 of PHONE, DEVICE, HELPER can approve at any time.");
    match (config.heir_delay, config.checkin_interval) {
        (Some(heir_delay), interval) => {
            println!(
                "HEIR alone can approve once a coin has {heir_delay} confirmations ({}).",
                about(heir_delay.into())
            );
            if let Some(interval) = interval {
                println!("Check in every {interval} blocks: each check-in restarts the heir delay (`satkeep checkin`).");
            }
            match (config.refresh_with_payments, config.payment_rule()) {
                (Some(false), _) => println!(
                    "Payments don't refresh coins (turned off, for privacy): only check-ins restart the heir delay."
                ),
                (_, Ok(Some(rule))) => println!(
                    "Payments also refresh coins with {} or more confirmations ({}), so they count as check-ins.",
                    rule.refresh_age(),
                    about(rule.refresh_age().into())
                ),
                _ => {}
            }
        }
        (None, _) => {
            println!("No beneficiary yet: no key can approve alone, ever, and no check-ins are needed.");
            println!("Add one with `satkeep add-heir`, which moves Savings to a new wallet in one payment.");
        }
    }
}

fn print_balance(opened: &Opened, client: &Client) -> Result<()> {
    let tip = node::height(client)?;
    let balance = opened.wallet.balance();
    println!("Savings on {}, synced to block {tip}", opened.network().core_name());
    println!("Total: {}", btc(balance.total()));
    let waiting = balance.trusted_pending + balance.untrusted_pending;
    if waiting > satkeep_savings::bitcoin::Amount::ZERO {
        println!("Waiting for confirmation: {}", btc(waiting));
    }
    let delays = opened.policy.delays();
    // Without a rule nothing is "due", but each coin's heir delay and
    // "overdue" still show: they don't depend on the rule.
    let rule = match opened.config.checkin_rule() {
        Ok(rule) => rule,
        Err(e) => {
            println!("Check-ins aren't set up for this wallet: {e:#}");
            None
        }
    };
    let coins = checkin::coins(&opened.wallet, delays, rule);
    for coin in &coins {
        let s = coin.status;
        let flag = if s.overdue {
            "  OVERDUE"
        } else if s.checkin_due {
            "  CHECK-IN DUE"
        } else {
            ""
        };
        let heir = s.heir_opens_in.map(|n| format!("  heir path {}", opens_in(n))).unwrap_or_default();
        println!(
            "  {}  {}  {} confirmation{}{heir}{flag}",
            btc(coin.amount),
            coin.outpoint,
            s.confirmations,
            if s.confirmations == 1 { "" } else { "s" },
        );
    }
    if !opened.policy.has_heir() {
        println!();
        println!("No beneficiary yet, so nothing opens with time and no check-in is needed.");
        return Ok(());
    }
    let oldest = coins.iter().filter(|c| c.status.confirmations > 0).max_by_key(|c| c.status.confirmations);
    match (checkin::reminder(&coins), oldest, rule) {
        (Reminder::Overdue, _, _) => {
            let open = coins.iter().filter(|c| c.status.overdue).count();
            println!();
            println!(
                "WARNING: check-in overdue. HEIR alone can already spend {open} coin{}.",
                if open == 1 { "" } else { "s" }
            );
            println!("Run `satkeep checkin` now.");
        }
        (Reminder::Due, Some(oldest), Some(rule)) => {
            println!();
            println!(
                "Check-in due: the oldest coin has {} confirmations, and check-ins are due every {} blocks.",
                oldest.status.confirmations,
                rule.interval()
            );
            let heir = oldest.status.heir_opens_in.expect("a wallet with a beneficiary");
            println!("Its heir path {}. Run `satkeep checkin`.", opens_in(heir));
        }
        (_, Some(oldest), Some(rule)) => {
            let due_in = u32::from(rule.interval()) - oldest.status.confirmations;
            println!();
            println!("Next check-in due in {} (every {}).", blocks(due_in), blocks(rule.interval().into()));
        }
        _ => {}
    }
    Ok(())
}

fn opens_in(n: u32) -> String {
    match n {
        0 => "open".into(),
        n => format!("opens in {}", blocks(n)),
    }
}

fn blocks(n: u32) -> String {
    if n == 1 { "1 block".into() } else { format!("{n} blocks") }
}

/// About how long `n` blocks take, at one block every 10 minutes on average.
fn about(n: u32) -> String {
    let minutes = u64::from(n) * 10;
    let (amount, unit) = if minutes < 120 {
        (minutes, "minute")
    } else if minutes < 48 * 60 {
        ((minutes + 30) / 60, "hour")
    } else if minutes < 60 * 24 * 60 {
        ((minutes + 12 * 60) / (24 * 60), "day")
    } else {
        // 525,960 minutes in an average year of 365.25 days.
        ((minutes * 12 + 525_960 / 2) / 525_960, "month")
    };
    format!("about {amount} {unit}{}", if amount == 1 { "" } else { "s" })
}

fn blocks_and_about(n: u32) -> String {
    format!("{}, {}", blocks(n), about(n))
}

fn btc(amount: satkeep_savings::bitcoin::Amount) -> String {
    format!("{:.8} BTC", amount.to_btc())
}

#[cfg(test)]
mod tests {
    use super::about;

    #[test]
    fn says_about_how_long_a_number_of_blocks_takes() {
        assert_eq!(about(1), "about 10 minutes");
        assert_eq!(about(20), "about 3 hours");
        assert_eq!(about(144), "about 24 hours");
        assert_eq!(about(288), "about 2 days");
        assert_eq!(about(8_760), "about 2 months");
        assert_eq!(about(26_280), "about 6 months");
        assert_eq!(about(52_560), "about 12 months");
        assert_eq!(about(61_320), "about 14 months");
        assert_eq!(about(65_535), "about 15 months");
    }
}
