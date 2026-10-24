//! `satkeep-restore`: get the phone key back from a Satkeep exit kit with
//! the second key, and hand Savings to Bitcoin Core or Liana. Test networks
//! only. A developer tool: its wording isn't product copy.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
use satkeep_backup::bip39::Language;
use satkeep_backup::kit::Kit;
use satkeep_backup::zeroize::Zeroizing;
use satkeep_restore::{bitcoind, describe, read_kit, unlock, wallets};

#[derive(Parser)]
#[command(name = "satkeep-restore", version, about = "Restore Satkeep Savings from an exit kit and the second key. Test networks only.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show what the kit holds, without unlocking anything.
    Show {
        /// The exit kit: its PDF, its armoured text, or the raw backup file.
        #[arg(long)]
        kit: PathBuf,
    },
    /// Unlock the kit and import Savings into your own Bitcoin Core, in a new
    /// wallet holding the phone key. Or, with --out, write Core's import file.
    Core {
        #[command(flatten)]
        unlock: UnlockArgs,
        /// Bitcoin Core's RPC URL [default: 127.0.0.1, the network's port]
        #[arg(long)]
        rpc_url: Option<String>,
        /// Bitcoin Core's RPC cookie [default: the network's .cookie in Core's data folder]
        #[arg(long)]
        rpc_cookie: Option<PathBuf>,
        /// The new wallet to create in Bitcoin Core [default: satkeep-restore-<wallet id>]
        #[arg(long)]
        core_wallet: Option<String>,
        /// Write the importdescriptors request to this new file instead of
        /// importing it. The file holds the phone key: keep it safe.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// The heir's route: with HEIR's key and the kit's plain wallet setup,
    /// and no second key, show the Savings wallets the heir can claim, for
    /// Liana. Never unlocks the backup, so the setup is unverified.
    Heir {
        #[command(flatten)]
        heir: HeirArgs,
    },
    /// Unlock the kit and print the Savings descriptors for Liana. The phone
    /// key's 12 words only with --show-phrase.
    Liana {
        #[command(flatten)]
        unlock: UnlockArgs,
        /// Also print the phone key's 12 words, to type into Liana as a hot
        /// signer. Anyone who sees them holds the phone key.
        #[arg(long)]
        show_phrase: bool,
    },
}

#[derive(Args)]
struct HeirArgs {
    /// The exit kit: its PDF or its armoured text (the raw backup has no
    /// plain wallet setup).
    #[arg(long)]
    kit: PathBuf,
    /// File holding the heir's 12 words. They're never taken on the command
    /// line and never printed.
    #[arg(long, required_unless_present = "key_stdin", conflicts_with = "key_stdin")]
    key_file: Option<PathBuf>,
    /// Read the heir's 12 words from standard input instead.
    #[arg(long)]
    key_stdin: bool,
}

#[derive(Args)]
struct UnlockArgs {
    /// The exit kit: its PDF, its armoured text, or the raw backup file.
    #[arg(long)]
    kit: PathBuf,
    /// TEST BUILDS ONLY: a simulated second key, from a file holding its
    /// state: its secret (64 hex digits) and, on the next line, its PIN. The
    /// PIN to try is then read from standard input, as if typed on it.
    #[cfg(feature = "simulated-device")]
    #[arg(long)]
    simulated_device: Option<PathBuf>,
}

fn main() -> ExitCode {
    // Before parsing: an argument holding a key or a phrase is refused and
    // never repeated (the parser would quote an unexpected argument).
    // args_os, not args: args() panics on an argument that isn't valid
    // text, and the panic message would print it whole.
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    if looks_like_a_key(&args) {
        eprintln!(
            "error: an argument looks like a private key or a recovery phrase. This tool never takes keys on the \
             command line; treat that key as exposed."
        );
        return ExitCode::from(2);
    }
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
        Command::Show { kit } => {
            print!("{}", describe(&read_kit(&kit)?)?);
        }
        Command::Core { unlock: args, rpc_url, rpc_cookie, core_wallet, out } => {
            let kit = read_kit(&args.kit)?;
            let wallet_id = satkeep_backup::read_header(&kit.backup)?.wallet_id;
            let unlocked = unlock_with(&kit, &args)?;
            let network = unlocked.setup.network;
            let url = rpc_url.unwrap_or_else(|| bitcoind::default_url(network));
            let cookie = rpc_cookie.unwrap_or_else(|| bitcoind::default_cookie(network));
            let client = bitcoind::connect(&url, &cookie, network)?;
            let imports = satkeep_restore::core_imports(&unlocked, |height| bitcoind::birthday_time(&client, height))?;
            print!("Unlocked. The phone key is PHONE in every Savings wallet:\n{}", wallets(&unlocked.setup));
            match out {
                Some(path) => {
                    bitcoind::write_file(&path, &imports)?;
                    println!("Wrote Bitcoin Core's import request to {}.", path.display());
                    println!("It holds the phone key UNENCRYPTED: delete it once it's imported.");
                    println!("Import it into a new blank, encrypted descriptor wallet with `bitcoin-cli importdescriptors`.");
                }
                None => {
                    let name = core_wallet.unwrap_or_else(|| format!("satkeep-restore-{}", hex(&wallet_id[..4])));
                    bitcoind::check_wallet_name(&name)?;
                    let passphrase = new_passphrase()?;
                    bitcoind::import(&client, &url, &cookie, &name, &passphrase, &imports)?;
                    println!("Imported into a new Bitcoin Core wallet, {name:?}, which holds the phone key.");
                    println!("The wallet is encrypted: Bitcoin Core needs its passphrase (walletpassphrase) to sign as PHONE.");
                    println!("Spending Savings still needs a second key: sign the PSBT with DEVICE, then broadcast.");
                }
            }
        }
        Command::Heir { heir: args } => heir(&args)?,
        Command::Liana { unlock: args, show_phrase } => {
            let kit = read_kit(&args.kit)?;
            let unlocked = unlock_with(&kit, &args)?;
            print!("Unlocked. The phone key is PHONE in every Savings wallet:\n{}", wallets(&unlocked.setup));
            for (i, wallet) in unlocked.setup.savings.iter().enumerate() {
                if wallet.heir.is_some() {
                    println!("\nSavings wallet {}: Liana's main descriptor:\n{}", i + 1, wallet.descriptor);
                } else {
                    println!(
                        "\nSavings wallet {}: Liana can't hold this shape (it needs a recovery path). Use Bitcoin Core \
                         (`satkeep-restore core`) or Sparrow, with this descriptor:\n{}",
                        i + 1,
                        wallet.descriptor
                    );
                }
            }
            if show_phrase {
                eprintln!("warning: the phone key's 12 words follow. Anyone who sees them holds the phone key.");
                let phrase = Zeroizing::new(unlocked.phrase.to_string());
                println!("\nPhone key (12 words, for Liana's hot signer): {}", phrase.as_str());
            } else {
                println!("\nFor Liana's hot signer, run again with --show-phrase to see the phone key's 12 words.");
            }
        }
    }
    Ok(())
}

/// The heir's route. Reads only the kit's plain setup: no second key, no
/// unlocking. The heir's words are never printed.
fn heir(args: &HeirArgs) -> Result<()> {
    use anyhow::Context;
    use std::io::{BufRead, Read};
    /// Far more than 12 words, far less than anything that could exhaust memory.
    const MAX_KEY_TEXT: u64 = 4096;
    let kit = read_kit(&args.kit)?;
    // Room for the whole limit up front, so the buffer never grows and leaves
    // an unwiped copy behind.
    let mut text = Zeroizing::new(String::with_capacity(MAX_KEY_TEXT as usize + 1));
    if args.key_stdin {
        std::io::stdin().lock().take(MAX_KEY_TEXT).read_line(&mut text).context("reading the heir's 12 words from standard input")?;
    } else {
        let path = args.key_file.as_ref().expect("clap requires --key-file or --key-stdin");
        let file = std::fs::File::open(path).with_context(|| format!("reading the heir's key file {}", path.display()))?;
        anyhow::ensure!(file.metadata()?.len() <= MAX_KEY_TEXT, "{} is too large to be a key file", path.display());
        file.take(MAX_KEY_TEXT).read_to_string(&mut text).with_context(|| format!("reading the heir's key file {}", path.display()))?;
    }
    let phrase = satkeep_restore::read_phrase(&text)?;
    drop(text);
    let key = satkeep_restore::AccountKey::from_phrase(&phrase);
    drop(phrase);

    println!(
        "UNVERIFIED: the heir's route reads the kit's plain wallet setup. Without the second key nobody can check it \
         against the copy locked in the backup, so anyone with access to the cloud drive could have changed it. The \
         heir's key can only ever sign for wallets that contain it."
    );
    let report = satkeep_restore::heir_wallets(&kit, &key)?;
    let count = report.count;
    for (number, why) in &report.not_alone {
        println!("\nSavings wallet {number} of {count} holds this heir key, but {why}, so it isn't offered.");
    }
    if report.claimable.is_empty() {
        if report.not_alone.is_empty() {
            bail!(
                "none of the {count} Savings wallet(s) in this kit's setup holds this heir key, so there's nothing to \
                 export. Check that these are the heir's 12 words and that this is the right kit."
            );
        }
        bail!("none of the {count} Savings wallet(s) in this kit's setup lets this heir key move coins alone, so there's nothing to export.");
    }
    for wallet in &report.claimable {
        println!(
            "\nSavings wallet {} of {} holds this heir key. The heir can move its coins alone after {} blocks ({}), \
             and only coins that haven't moved for at least that long.",
            wallet.number,
            wallet.count,
            wallet.heir_delay,
            satkeep_restore::about(u32::from(wallet.heir_delay))
        );
        if let Some(listed) = wallet.setup_says {
            println!(
                "  (The setup lists its heir delay as {listed} blocks, but the descriptor says {}: the descriptor is what \
                 the coins follow.)",
                wallet.heir_delay
            );
        }
        println!("Liana's main descriptor (UNVERIFIED):\n{}", wallet.descriptor);
    }
    println!(
        "\nNext: in Liana, create a wallet from that descriptor and add the heir's 12 words as a hot signer. Liana's \
         recovery offers the coins once they're old enough."
    );
    Ok(())
}

fn unlock_with(kit: &Kit, args: &UnlockArgs) -> Result<satkeep_restore::Unlocked> {
    #[cfg(feature = "simulated-device")]
    if let Some(path) = &args.simulated_device {
        let mut device = simulated_device(path)?;
        return unlock(kit, &mut device);
    }
    let _ = (kit, args, unlock);
    bail!(
        "this build of satkeep-restore can't talk to a second key yet: Trezor and the second phone come in later \
         versions. `show` works without one."
    )
}

#[cfg(feature = "simulated-device")]
fn simulated_device(path: &std::path::Path) -> Result<satkeep_backup::SimulatedDevice> {
    use anyhow::Context;
    let text = Zeroizing::new(std::fs::read_to_string(path).context("reading the simulated device file")?);
    let mut lines = text.lines();
    let secret_hex = lines.next().context("the simulated device file has no secret")?.trim();
    let pin = lines.next().context("the simulated device file has no PIN")?.trim();
    let mut secret = [0u8; 32];
    // Checked before slicing: a non-ASCII character would make the slice
    // panic, and the panic message would print the secret.
    anyhow::ensure!(
        secret_hex.len() == 64 && secret_hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "the simulated device's secret isn't 64 hex digits"
    );
    for (i, byte) in secret.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&secret_hex[2 * i..2 * i + 2], 16).context("the simulated device's secret isn't hex")?;
    }
    let mut device = satkeep_backup::SimulatedDevice::from_secret(secret, pin);
    // The PIN is typed on the device, as on a real one: here, a line on
    // standard input.
    eprint!("PIN for the simulated second key: ");
    let mut typed = Zeroizing::new(String::new());
    std::io::stdin().read_line(&mut typed).context("reading the PIN")?;
    device.enter_pin(typed.trim()).map_err(anyhow::Error::from)?;
    Ok(device)
}

/// A new passphrase for the Core wallet, typed twice on standard input. It
/// shows as it's typed on a plain terminal (README); it's never printed or
/// put in an error.
fn new_passphrase() -> Result<Zeroizing<String>> {
    use anyhow::Context;
    let read = |prompt: &str| -> Result<Zeroizing<String>> {
        eprint!("{prompt}");
        let mut line = Zeroizing::new(String::new());
        std::io::stdin().read_line(&mut line).context("reading the wallet passphrase")?;
        Ok(Zeroizing::new(line.trim_end_matches(['\r', '\n']).to_owned()))
    };
    let first = read("New passphrase for the Bitcoin Core wallet: ")?;
    anyhow::ensure!(!first.is_empty(), "the wallet passphrase can't be empty");
    let second = read("The same passphrase again: ")?;
    anyhow::ensure!(*first == *second, "the two passphrases don't match");
    Ok(first)
}

/// BIP39 words in a row that count as (part of) a phrase. Lower than 12 so a
/// phrase with a typo, or cut short, is still caught; no path or option this
/// tool takes has six BIP39 words in a row.
const PHRASE_RUN: usize = 6;

/// An argument holding an extended private key, or PHRASE_RUN BIP39 words
/// in a row across the arguments.
fn looks_like_a_key(args: &[String]) -> bool {
    if args.iter().any(|a| a.contains("tprv") || a.contains("xprv")) {
        return true;
    }
    let mut run = 0;
    for word in args.join(" ").split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()) {
        run = if Language::English.find_word(&word.to_ascii_lowercase()).is_some() { run + 1 } else { 0 };
        if run == PHRASE_RUN {
            return true;
        }
    }
    false
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
