//! The wallet folder: the Savings policy's public data and BDK's database.
//! Nothing in it can spend; private keys never go here.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bdk_wallet::rusqlite::Connection;
use bdk_wallet::{KeychainKind, PersistedWallet, Wallet};
use serde::{Deserialize, Serialize};
use satkeep_savings::bitcoin::Network;
use satkeep_savings::checkin::{
    CheckinRule, DEFAULT_CHECKIN_INTERVAL, DEFAULT_CHECKIN_MARGIN, DEFAULT_REFRESH_WINDOW, HeirDelayRules, PaymentRule,
};
use satkeep_savings::miniscript::descriptor::DescriptorPublicKey;
use satkeep_savings::{DEFAULT_MIN_HEIR_DELAY, Delays, SavingsKeys, SavingsPolicy, migrate};

const CONFIG_FILE: &str = "savings.json";
const DATABASE_FILE: &str = "wallet.sqlite";

/// The test networks Satkeep runs on. There is deliberately no mainnet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum TestNetwork {
    Regtest,
    Signet,
}

impl TestNetwork {
    pub fn network(self) -> Network {
        match self {
            TestNetwork::Regtest => Network::Regtest,
            TestNetwork::Signet => Network::Signet,
        }
    }

    /// The name Bitcoin Core reports for this chain.
    pub fn core_name(self) -> &'static str {
        match self {
            TestNetwork::Regtest => "regtest",
            TestNetwork::Signet => "signet",
        }
    }
}

/// What `savings.json` holds: everything needed to rebuild the policy.
///
/// `heir` and `heir_delay` are both there in Savings with a beneficiary and
/// both missing in Savings with none.
#[derive(Serialize, Deserialize)]
pub struct Config {
    pub network: TestNetwork,
    pub phone: String,
    pub device: String,
    pub helper: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heir_delay: Option<u16>,
    /// Block height when the wallet was created. Sync starts here.
    pub birthday: u32,
    /// Blocks between check-ins, with a beneficiary. Missing in wallets made
    /// before check-ins existed; see [`Config::checkin_rule`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkin_interval: Option<u16>,
    /// Blocks the interval must leave before the heir delay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkin_margin: Option<u16>,
    /// The payment rule, with a beneficiary: whether 2-of-3 payments also
    /// spend the coins close to needing a check-in. Missing means on. Set it
    /// to `false` to keep coins separate, for privacy; only check-ins then
    /// refresh coins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_with_payments: Option<bool>,
    /// Blocks before a coin's check-in is due that a payment refreshes it.
    /// Missing means [`DEFAULT_REFRESH_WINDOW`], a placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_window: Option<u16>,
    /// The shortest heir delay this wallet accepts, here and in any move.
    /// Missing means [`DEFAULT_MIN_HEIR_DELAY`], a placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_heir_delay: Option<u16>,
    /// The beneficiary set up for moving Savings to a new wallet (by
    /// `satkeep add-heir`). `sign` and `broadcast` rebuild the new wallet
    /// from this and this wallet's own keys, and refuse a move that goes
    /// anywhere else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moving_to: Option<MovingTo>,
    /// Only in wallets made before the card-only delayed path was removed.
    /// Read so such a wallet gets a clear error; never written.
    #[serde(default, skip_serializing)]
    pub device_r: Option<String>,
}

/// Where a move set up in this wallet goes: this wallet's PHONE, DEVICE and
/// HELPER plus this beneficiary, in a wallet with these check-ins.
#[derive(Clone, Serialize, Deserialize)]
pub struct MovingTo {
    pub heir: String,
    pub heir_delay: u16,
    /// The new wallet's check-in interval and margin, which the heir delay
    /// must leave room for. Missing in moves set up by earlier versions,
    /// which get the defaults, if they fit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkin_interval: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkin_margin: Option<u16>,
}

impl Config {
    pub fn policy(&self) -> Result<SavingsPolicy> {
        if self.device_r.is_some() {
            bail!(
                "this wallet was made with the card-only delayed path (DEVICE_R), which Satkeep no longer has; \
                 make a new wallet with `satkeep create` (test networks only, so its coins aren't real)"
            );
        }
        let key = |name: &str, text: &str| -> Result<DescriptorPublicKey> {
            text.parse().map_err(|e| anyhow::anyhow!("the {name} key isn't a valid public key: {e}"))
        };
        let heir = self.heir.as_deref().map(|h| key("HEIR", h)).transpose()?;
        let keys = SavingsKeys::new(key("PHONE", &self.phone)?, key("DEVICE", &self.device)?, key("HELPER", &self.helper)?, heir)?;
        let delays = self.delays()?;
        delays.require_heir_at_least(self.min_heir_delay())?;
        Ok(SavingsPolicy::new(keys, delays, self.network.network())?)
    }

    /// The payment rule, or `None` with no beneficiary or with the setting
    /// off.
    pub fn payment_rule(&self) -> Result<Option<PaymentRule>> {
        let Some(checkin) = self.checkin_rule()? else { return Ok(None) };
        if self.refresh_with_payments == Some(false) {
            return Ok(None);
        }
        let window = self.refresh_window.unwrap_or(DEFAULT_REFRESH_WINDOW);
        PaymentRule::new(window, checkin).map(Some).map_err(|e| {
            if self.refresh_window.is_none() {
                anyhow::anyhow!("this wallet has no refresh window set and the default doesn't fit it ({e})")
            } else {
                e.into()
            }
        })
    }

    /// The shortest heir delay this wallet accepts.
    pub fn min_heir_delay(&self) -> u16 {
        self.min_heir_delay.unwrap_or(DEFAULT_MIN_HEIR_DELAY)
    }

    /// What this wallet's heir delay must allow: its minimum, and room for
    /// its check-ins (the defaults for any not set).
    pub fn heir_delay_rules(&self) -> HeirDelayRules {
        HeirDelayRules {
            min_heir_delay: self.min_heir_delay(),
            checkin_interval: self.checkin_interval.unwrap_or(DEFAULT_CHECKIN_INTERVAL),
            checkin_margin: self.checkin_margin.unwrap_or(DEFAULT_CHECKIN_MARGIN),
        }
    }

    /// What the heir delay of the move set up here must allow: this
    /// wallet's minimum, and room for the new wallet's check-ins. `None` if
    /// no move is set up.
    pub fn move_rules(&self) -> Option<HeirDelayRules> {
        let moving_to = self.moving_to.as_ref()?;
        Some(HeirDelayRules {
            min_heir_delay: self.min_heir_delay(),
            checkin_interval: moving_to.checkin_interval.unwrap_or(DEFAULT_CHECKIN_INTERVAL),
            checkin_margin: moving_to.checkin_margin.unwrap_or(DEFAULT_CHECKIN_MARGIN),
        })
    }

    /// The wallet a move set up here goes to, rebuilt from this wallet's
    /// own keys and `moving_to`, or `None` if no move is set up. Refused if
    /// its heir delay is below this wallet's minimum or leaves no room for
    /// the new wallet's check-ins.
    pub fn move_target(&self, from: &SavingsPolicy) -> Result<Option<SavingsPolicy>> {
        let Some(moving_to) = &self.moving_to else { return Ok(None) };
        let heir: DescriptorPublicKey = moving_to
            .heir
            .parse()
            .map_err(|e| anyhow::anyhow!("the HEIR key set up for the move isn't a valid public key: {e}"))?;
        let delays = Delays::with_heir(moving_to.heir_delay)?;
        let rules = self.move_rules().expect("a move is set up");
        let to = migrate::wallet_with_heir(from, heir, delays, rules)
            .context("the move set up in this wallet's settings")?;
        Ok(Some(to))
    }

    pub fn delays(&self) -> Result<Delays> {
        Ok(match self.heir_delay {
            Some(blocks) => Delays::with_heir(blocks)?,
            None => Delays::none(),
        })
    }

    /// The check-in rule, or `None` for Savings with no beneficiary, which
    /// needs no check-in. A wallet made before check-ins existed gets the
    /// defaults, if they fit its heir delay.
    pub fn checkin_rule(&self) -> Result<Option<CheckinRule>> {
        let delays = self.delays()?;
        if delays.heir().is_none() {
            return Ok(None);
        }
        let interval = self.checkin_interval.unwrap_or(DEFAULT_CHECKIN_INTERVAL);
        let margin = self.checkin_margin.unwrap_or(DEFAULT_CHECKIN_MARGIN);
        CheckinRule::new(interval, margin, delays).map(Some).map_err(|e| {
            if self.checkin_interval.is_none() {
                anyhow::anyhow!("this wallet has no check-in interval set and the defaults don't fit it ({e})")
            } else {
                e.into()
            }
        })
    }
}

/// An opened wallet folder.
pub struct Opened {
    pub dir: PathBuf,
    pub config: Config,
    pub policy: SavingsPolicy,
    pub wallet: PersistedWallet<Connection>,
    db: Connection,
}

impl Opened {
    pub fn network(&self) -> TestNetwork {
        self.config.network
    }

    pub fn persist(&mut self) -> Result<()> {
        self.wallet.persist(&mut self.db).context("saving the wallet database")?;
        Ok(())
    }

    /// Saves changed settings. The policy fields can't change: the wallet
    /// database is tied to them.
    pub fn save_config(&self) -> Result<()> {
        write_config(&self.dir, &self.config)
    }
}

/// Creates a new wallet folder. Refuses if one is already there.
pub fn create(dir: &Path, config: Config) -> Result<Opened> {
    let config_path = dir.join(CONFIG_FILE);
    if config_path.exists() {
        bail!("{} already holds a Savings wallet", dir.display());
    }
    let policy = config.policy()?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut db = Connection::open(dir.join(DATABASE_FILE)).context("creating the wallet database")?;
    let (receive, change) = policy.receive_and_change();
    let wallet = Wallet::create(receive.to_string(), change.to_string())
        .network(config.network.network())
        .create_wallet(&mut db)
        .context("creating the wallet")?;
    write_config(dir, &config)?;
    Ok(Opened { dir: dir.to_path_buf(), config, policy, wallet, db })
}

/// Opens an existing wallet folder.
pub fn open(dir: &Path) -> Result<Opened> {
    let config_path = dir.join(CONFIG_FILE);
    let text = fs::read_to_string(&config_path).with_context(|| {
        format!("no Savings wallet in {} (run `satkeep create` first, or pass --wallet-dir)", dir.display())
    })?;
    let config: Config = serde_json::from_str(&text).with_context(|| format!("reading {}", config_path.display()))?;
    let policy = config.policy()?;
    let mut db = Connection::open(dir.join(DATABASE_FILE)).context("opening the wallet database")?;
    let (receive, change) = policy.receive_and_change();
    let wallet = Wallet::load()
        .descriptor(KeychainKind::External, Some(receive.to_string()))
        .descriptor(KeychainKind::Internal, Some(change.to_string()))
        .check_network(config.network.network())
        .load_wallet(&mut db)
        .context("loading the wallet database")?
        .with_context(|| format!("the wallet database in {} is empty", dir.display()))?;
    Ok(Opened { dir: dir.to_path_buf(), config, policy, wallet, db })
}

fn write_config(dir: &Path, config: &Config) -> Result<()> {
    let config_path = dir.join(CONFIG_FILE);
    fs::write(&config_path, serde_json::to_string_pretty(config)?)
        .with_context(|| format!("writing {}", config_path.display()))
}

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}
