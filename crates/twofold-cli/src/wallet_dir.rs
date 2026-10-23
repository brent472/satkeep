//! The wallet folder: the Savings policy's public data and BDK's database.
//! Nothing in it can spend; private keys never go here.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bdk_wallet::rusqlite::Connection;
use bdk_wallet::{KeychainKind, PersistedWallet, Wallet};
use serde::{Deserialize, Serialize};
use twofold_savings::bitcoin::Network;
use twofold_savings::checkin::{CheckinRule, DEFAULT_CHECKIN_INTERVAL, DEFAULT_CHECKIN_MARGIN};
use twofold_savings::miniscript::descriptor::DescriptorPublicKey;
use twofold_savings::{Delays, SavingsKeys, SavingsPolicy};

const CONFIG_FILE: &str = "savings.json";
const DATABASE_FILE: &str = "wallet.sqlite";

/// The test networks Twofold runs on. There is deliberately no mainnet.
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
    /// Only in wallets made before the card-only delayed path was removed.
    /// Read so such a wallet gets a clear error; never written.
    #[serde(default, skip_serializing)]
    pub device_r: Option<String>,
}

impl Config {
    pub fn policy(&self) -> Result<SavingsPolicy> {
        if self.device_r.is_some() {
            bail!(
                "this wallet was made with the card-only delayed path (DEVICE_R), which Twofold no longer has; \
                 make a new wallet with `twofold create` (test networks only, so its coins aren't real)"
            );
        }
        let key = |name: &str, text: &str| -> Result<DescriptorPublicKey> {
            text.parse().map_err(|e| anyhow::anyhow!("the {name} key isn't a valid public key: {e}"))
        };
        let heir = self.heir.as_deref().map(|h| key("HEIR", h)).transpose()?;
        let keys = SavingsKeys::new(key("PHONE", &self.phone)?, key("DEVICE", &self.device)?, key("HELPER", &self.helper)?, heir)?;
        Ok(SavingsPolicy::new(keys, self.delays()?, self.network.network())?)
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
    fs::write(&config_path, serde_json::to_string_pretty(&config)?)
        .with_context(|| format!("writing {}", config_path.display()))?;
    Ok(Opened { config, policy, wallet, db })
}

/// Opens an existing wallet folder.
pub fn open(dir: &Path) -> Result<Opened> {
    let config_path = dir.join(CONFIG_FILE);
    let text = fs::read_to_string(&config_path).with_context(|| {
        format!("no Savings wallet in {} (run `twofold create` first, or pass --wallet-dir)", dir.display())
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
    Ok(Opened { config, policy, wallet, db })
}

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}
