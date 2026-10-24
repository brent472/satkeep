//! The Satkeep restore tool: the phone key back from the exit kit with the
//! second key, and Savings handed to Bitcoin Core or Liana, with no Satkeep
//! app, server or helper. Test networks only. See docs/restore-tool.md.
//!
//! 1. Read the kit: the PDF, its armoured text, or the raw backup.
//! 2. Show what it holds, before anything is unlocked.
//! 3. Unlock it with the second key, and refuse a kit whose plain wallet
//!    setup differs from the locked one.
//! 4. Check the phone key is PHONE in every Savings descriptor (T5), so a
//!    backup and a setup that don't belong together export nothing.
//! 5. Export: import into Bitcoin Core, write Core's import file, or print
//!    the public descriptors for Liana (and the 12 words only on request).

pub mod bitcoind;
pub mod keys;

use std::path::Path;

use anyhow::{Context, Result, bail};
use satkeep_backup::bip39::Mnemonic;
use satkeep_backup::kit::{self, Kit};
use satkeep_backup::setup::WalletSetup;
use satkeep_backup::{LockKind, SecurityDevice};

pub use keys::{AccountKey, Alone};

/// What unlocking a kit gives back. Deliberately not `Debug`: it holds the
/// phone key.
pub struct Unlocked {
    pub phrase: Mnemonic,
    pub phone: AccountKey,
    /// The wallet setup from inside the locked backup: the authenticated one.
    pub setup: WalletSetup,
}

/// Reads a kit from `path`: a PDF, the armoured text, or the raw backup.
pub fn read_kit(path: &Path) -> Result<Kit> {
    let size = std::fs::metadata(path).with_context(|| format!("reading {}", path.display()))?.len();
    if size > kit::MAX_INPUT as u64 {
        bail!("{} is too large to be an exit kit", path.display());
    }
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(kit::read(&bytes)?)
}

/// What the kit holds, before anything is unlocked. Nothing secret: the
/// date, the second keys that can unlock it, and the plain wallet setup,
/// marked unverified.
pub fn describe(kit: &Kit) -> Result<String> {
    let header = satkeep_backup::read_header(&kit.backup)?;
    let mut out = String::new();
    out += &format!("Phone key backup made {} (Unix time {})\n", date(header.created), header.created);
    out += &format!("Wallet id: {}\n", hex(&header.wallet_id));
    for entry in &header.entries {
        let kind = match entry.kind {
            LockKind::Device => "second key",
            LockKind::Contact => "recovery contact (reserved; can't be opened)",
        };
        out += &format!("Unlocks with: {kind} {}…, method {:?}\n", hex(&entry.device_id.0[..8]), entry.method);
    }
    match &kit.plain_setup {
        Some(setup) => {
            out += &format!("Wallet setup (plain copy, UNVERIFIED until the second key unlocks the backup): {}\n", setup.network.as_str());
            out += &wallets(setup);
        }
        None => out += "No plain wallet setup: this is the raw backup. The setup is inside it, locked.\n",
    }
    Ok(out)
}

/// Opens the kit with `device`, which must have been unlocked with its own
/// PIN, and checks the phone key is PHONE in every Savings wallet.
pub fn unlock(kit: &Kit, device: &mut dyn SecurityDevice) -> Result<Unlocked> {
    let restored = kit::restore_kit(kit, device)?;
    let phone = AccountKey::from_phrase(&restored.phrase);
    for (i, wallet) in restored.wallet_setup.savings.iter().enumerate() {
        if !phone.is_in(&wallet.descriptor)? {
            bail!(
                "the phone key in this backup isn't in Savings wallet {} of the wallet setup, so the backup and the \
                 setup don't belong together; nothing was exported",
                i + 1
            );
        }
    }
    Ok(Unlocked { phrase: restored.phrase, phone, setup: restored.wallet_setup })
}

/// The Bitcoin Core imports for every Savings wallet: receive and change,
/// with PHONE's private key, from each wallet's birthday. Only the newest
/// wallet of each shape is active.
pub fn core_imports(unlocked: &Unlocked, birthday_time: impl Fn(u32) -> Result<u64>) -> Result<Vec<bitcoind::Import>> {
    let savings = &unlocked.setup.savings;
    let mut imports = Vec::new();
    for (i, wallet) in savings.iter().enumerate() {
        let shape = shape(&wallet.descriptor);
        let newest_of_shape = !savings[i + 1..].iter().any(|later| self::shape(&later.descriptor) == shape);
        let timestamp = birthday_time(wallet.birthday)?;
        for descriptor in unlocked.phone.with_private_key(&wallet.descriptor)? {
            imports.push(bitcoind::Import { descriptor, active: newest_of_shape, timestamp });
        }
    }
    Ok(imports)
}

/// A Savings wallet the heir can claim.
pub struct HeirWallet {
    /// Its position in the setup, from 1.
    pub number: usize,
    pub count: usize,
    /// Its public descriptor, for Liana.
    pub descriptor: String,
    /// The heir delay in blocks, from the descriptor's heir path (what the
    /// coins follow).
    pub heir_delay: u16,
    /// The heir delay the setup lists for it, when that differs.
    pub setup_says: Option<u16>,
}

/// What the heir's route found in the kit's plain setup.
pub struct HeirReport {
    /// Savings wallets in the setup.
    pub count: usize,
    /// The wallets the heir can claim alone after a block delay.
    pub claimable: Vec<HeirWallet>,
    /// Wallets that hold HEIR's key, but not in a path the heir can use
    /// alone: (number, why). Not exported.
    pub not_alone: Vec<(usize, &'static str)>,
}

/// The heir's route: the Savings wallets in the kit's **plain** setup with
/// one Taproot leaf where `heir` alone spends after a block delay
/// (`and_v(v:pk(HEIR),older(N))`). Never asks for the second key and never
/// opens the backup, so the setup is unverified; HEIR's key can only ever
/// sign for wallets that contain it.
pub fn heir_wallets(kit: &Kit, heir: &AccountKey) -> Result<HeirReport> {
    let setup = kit.plain_setup.as_ref().context(
        "this is the raw backup, with no plain wallet setup: the heir's route needs the kit itself (its PDF or its \
         armoured text)",
    )?;
    let count = setup.savings.len();
    let mut report = HeirReport { count, claimable: Vec::new(), not_alone: Vec::new() };
    for (i, wallet) in setup.savings.iter().enumerate() {
        if !heir.is_in(&wallet.descriptor)? {
            continue;
        }
        match heir.alone_after(&wallet.descriptor)? {
            Alone::After(heir_delay) => {
                let setup_says = wallet.heir.map(|h| h.heir_delay).filter(|listed| *listed != heir_delay);
                report.claimable.push(HeirWallet {
                    number: i + 1,
                    count,
                    descriptor: wallet.descriptor.clone(),
                    heir_delay,
                    setup_says,
                });
            }
            Alone::Nowhere => report.not_alone.push((i + 1, "not in a path the heir can use alone")),
            Alone::TimeBased => report.not_alone.push((i + 1, "its heir delay is time-based, which Satkeep never uses")),
            Alone::MoreThanOne => report.not_alone.push((i + 1, "in more than one heir path, which Satkeep never makes")),
        }
    }
    Ok(report)
}

/// Reads a holder's recovery phrase from the text of a key file or stdin:
/// 12 English BIP39 words. The error never repeats the text.
pub fn read_phrase(text: &str) -> Result<Mnemonic> {
    // Windows tools may put a byte-order mark in front of the text.
    let text = text.trim().trim_start_matches('\u{feff}').trim();
    anyhow::ensure!(!(text.starts_with("tprv") || text.starts_with("xprv")), "that's an extended key, not 12 words");
    let phrase = Mnemonic::parse_in(satkeep_backup::bip39::Language::English, text)
        .map_err(|_| anyhow::anyhow!("that isn't a 12-word recovery phrase (the words aren't repeated here)"))?;
    anyhow::ensure!(phrase.word_count() == satkeep_backup::PHRASE_WORDS, "that isn't a 12-word recovery phrase");
    Ok(phrase)
}

/// Roughly how long `blocks` take, at 10 minutes a block.
pub fn about(blocks: u32) -> String {
    let blocks = f64::from(blocks);
    let hours = blocks / 6.0;
    if hours < 48.0 {
        let n = hours.round().max(1.0);
        format!("about {n} hour{}", if n == 1.0 { "" } else { "s" })
    } else if hours < 24.0 * 60.0 {
        format!("about {} days", (hours / 24.0).round())
    } else {
        format!("about {} months", (hours / 24.0 / 30.44).round())
    }
}

/// The Savings wallets, one per line, with nothing secret.
pub fn wallets(setup: &WalletSetup) -> String {
    let mut out = String::new();
    let count = setup.savings.len();
    for (i, wallet) in setup.savings.iter().enumerate() {
        let what = match wallet.heir {
            None => "no beneficiary: any 2 of PHONE, DEVICE, HELPER (P2WSH)".to_owned(),
            Some(h) => format!("with a beneficiary: any 2 of 3, or HEIR alone after {} blocks (Taproot)", h.heir_delay),
        };
        let current = if i + 1 == count { ", current" } else { "" };
        out += &format!("  Savings wallet {} of {count}{current}: {what}; birthday block {}\n", i + 1, wallet.birthday);
    }
    if setup.pocket.is_some() {
        out += "  The everyday pocket: not handled by this version of the tool.\n";
    }
    out
}

/// "wsh" or "tr": Core keeps one active descriptor per shape.
fn shape(descriptor: &str) -> &str {
    descriptor.split('(').next().unwrap_or("")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A Unix time as a UTC date, with no date library: days since 1970 to a
/// civil date (Howard Hinnant's algorithm).
fn date(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn durations() {
        assert_eq!(super::about(20), "about 3 hours");
        assert_eq!(super::about(1), "about 1 hour");
        assert_eq!(super::about(1_008), "about 7 days");
        assert_eq!(super::about(61_320), "about 14 months");
        assert_eq!(super::about(52_560), "about 12 months");
    }

    #[test]
    fn dates() {
        assert_eq!(super::date(0), "1970-01-01");
        assert_eq!(super::date(1_792_713_600), "2026-10-23");
        assert_eq!(super::date(951_782_400), "2000-02-29");
    }
}
