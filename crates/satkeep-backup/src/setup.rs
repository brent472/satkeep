//! The wallet setup: everything public needed to find and spend the
//! owner's coins with other software. It travels inside the locked backup
//! and, as a plain copy, in the exit kit.
//!
//! It's text, one `key value` per line, in a fixed order, so it's readable
//! and so each setup has exactly one form: two setups are the same exactly
//! when their texts are. See docs/backup-format.md for the grammar.
//!
//! ```text
//! satkeep wallet setup 1
//! network signet
//! savings
//! descriptor wsh(sortedmulti(2,…))#f2jk34gy
//! birthday 327699
//! savings
//! descriptor tr(…)#wv60dvd2
//! birthday 327710
//! heir-delay 12
//! checkin-interval 6
//! checkin-margin 3
//! refresh-window 2
//! ```
//!
//! Only the form is checked here. Whether a descriptor parses, holds the
//! phone key, or has delays the Savings rules accept is for the code that
//! uses it (the restore tool checks the phone key; `satkeep-savings` builds
//! the policy).

use crate::BackupError;

/// The first line, which carries the version.
pub const FIRST_LINE: &str = "satkeep wallet setup 1";
/// Savings wallets in one setup, current and earlier ones together.
pub const MAX_WALLETS: usize = 16;
/// Bytes in one descriptor.
pub const MAX_DESCRIPTOR: usize = 4096;
/// Bytes in the whole setup text.
pub const MAX_SETUP: usize = 16 * 1024;

/// The test networks a setup can be on. Mainnet is refused until the launch
/// review; adding it later needs no new version, and older readers refuse
/// it, which is the safe direction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Network {
    Regtest,
    Signet,
}

impl Network {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Regtest => "regtest",
            Self::Signet => "signet",
        }
    }
}

/// The owner's wallets, all public.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WalletSetup {
    pub network: Network,
    /// The everyday pocket. Reserved: its descriptor isn't designed yet
    /// (brief 3.11), so nothing makes one, but the format has its place.
    pub pocket: Option<PocketWallet>,
    /// Every Savings wallet that may still hold coins, oldest first; the
    /// last is the current one. Earlier ones stay after a move, because
    /// coins sent to them later stay there.
    pub savings: Vec<SavingsWallet>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PocketWallet {
    pub descriptor: String,
    /// The block height to scan from.
    pub birthday: u32,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SavingsWallet {
    /// The public descriptor, as `satkeep export` writes it (with checksum).
    pub descriptor: String,
    /// The block height to scan from.
    pub birthday: u32,
    /// Present once a beneficiary is added.
    pub heir: Option<HeirSettings>,
}

/// A Savings wallet's heir and check-in settings, in blocks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HeirSettings {
    pub heir_delay: u16,
    pub checkin_interval: u16,
    pub checkin_margin: u16,
    /// The payment rule's window; `None` when the rule is turned off.
    pub refresh_window: Option<u16>,
}

impl WalletSetup {
    /// The setup's one text form. Refuses a setup that wouldn't read back
    /// the same: no Savings wallet, too many, a descriptor with a space or
    /// other character outside printable ASCII, a zero delay.
    pub fn to_text(&self) -> Result<String, BackupError> {
        let text = self.write();
        match Self::from_text(&text) {
            Ok(back) if back == *self => Ok(text),
            _ => Err(BackupError::BadSetup),
        }
    }

    /// Reads a setup in exactly its one text form. Anything else, including
    /// a stray space, a Windows line ending or a missing final newline, is
    /// refused.
    pub fn from_text(text: &str) -> Result<Self, BackupError> {
        parse(text).ok_or(BackupError::BadSetup)
    }

    fn write(&self) -> String {
        let mut out = format!("{FIRST_LINE}\nnetwork {}\n", self.network.as_str());
        if let Some(pocket) = &self.pocket {
            out += &format!("pocket\ndescriptor {}\nbirthday {}\n", pocket.descriptor, pocket.birthday);
        }
        for wallet in &self.savings {
            out += &format!("savings\ndescriptor {}\nbirthday {}\n", wallet.descriptor, wallet.birthday);
            if let Some(heir) = wallet.heir {
                out += &format!(
                    "heir-delay {}\ncheckin-interval {}\ncheckin-margin {}\n",
                    heir.heir_delay, heir.checkin_interval, heir.checkin_margin
                );
                if let Some(window) = heir.refresh_window {
                    out += &format!("refresh-window {window}\n");
                }
            }
        }
        out
    }
}

fn parse(text: &str) -> Option<WalletSetup> {
    if text.len() > MAX_SETUP {
        return None;
    }
    let mut lines = Lines(text.strip_suffix('\n')?.split('\n').peekable());
    if lines.0.next()? != FIRST_LINE {
        return None;
    }
    let network = match lines.value("network")? {
        "regtest" => Network::Regtest,
        "signet" => Network::Signet,
        _ => return None,
    };
    let pocket = if lines.bare("pocket") {
        Some(PocketWallet { descriptor: descriptor(lines.value("descriptor")?)?, birthday: lines.number("birthday")? })
    } else {
        None
    };
    let mut savings = Vec::new();
    while lines.bare("savings") {
        let descriptor = descriptor(lines.value("descriptor")?)?;
        let birthday = lines.number("birthday")?;
        let heir = if lines.next_is("heir-delay") {
            Some(HeirSettings {
                heir_delay: lines.delay("heir-delay")?,
                checkin_interval: lines.delay("checkin-interval")?,
                checkin_margin: lines.delay("checkin-margin")?,
                refresh_window: if lines.next_is("refresh-window") { Some(lines.delay("refresh-window")?) } else { None },
            })
        } else {
            None
        };
        savings.push(SavingsWallet { descriptor, birthday, heir });
    }
    if lines.0.next().is_some() || savings.is_empty() || savings.len() > MAX_WALLETS {
        return None;
    }
    Some(WalletSetup { network, pocket, savings })
}

struct Lines<'a>(std::iter::Peekable<std::str::Split<'a, char>>);

impl<'a> Lines<'a> {
    /// The value of the next line, which must be `key value`.
    fn value(&mut self, key: &str) -> Option<&'a str> {
        let (k, v) = self.0.next()?.split_once(' ')?;
        (k == key).then_some(v)
    }

    /// Takes the next line if it's exactly `word`.
    fn bare(&mut self, word: &str) -> bool {
        self.0.next_if(|line| *line == word).is_some()
    }

    fn next_is(&mut self, key: &str) -> bool {
        self.0.peek().and_then(|line| line.split_once(' ')).is_some_and(|(k, _)| k == key)
    }

    fn number(&mut self, key: &str) -> Option<u32> {
        decimal(self.value(key)?)
    }

    /// A delay in blocks: 1 to 65,535.
    fn delay(&mut self, key: &str) -> Option<u16> {
        u16::try_from(self.number(key)?).ok().filter(|blocks| *blocks > 0)
    }
}

/// Decimal digits with no sign and no leading zero (except "0" itself).
fn decimal(text: &str) -> Option<u32> {
    let canonical = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) && (text == "0" || !text.starts_with('0'));
    if canonical { text.parse().ok() } else { None }
}

/// Printable ASCII with no spaces, which every descriptor is.
fn descriptor(text: &str) -> Option<String> {
    let ok = !text.is_empty() && text.len() <= MAX_DESCRIPTOR && text.bytes().all(|b| (0x21..=0x7e).contains(&b));
    ok.then(|| text.to_owned())
}
