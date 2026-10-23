//! The wallet folder: the Savings policy's public data and BDK's database.
//! Nothing in it can spend; private keys never go here.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bdk_wallet::rusqlite::Connection;
use bdk_wallet::{KeychainKind, PersistedWallet, Wallet};
use serde::{Deserialize, Serialize};
use twofold_savings::bitcoin::Network;
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
#[derive(Serialize, Deserialize)]
pub struct Config {
    pub network: TestNetwork,
    pub device_delay: u16,
    pub heir_delay: u16,
    pub phone: String,
    pub device: String,
    pub device_r: String,
    pub helper: String,
    pub heir: String,
    /// Block height when the wallet was created. Sync starts here.
    pub birthday: u32,
}

impl Config {
    pub fn policy(&self) -> Result<SavingsPolicy> {
        let key = |name: &str, text: &str| -> Result<DescriptorPublicKey> {
            text.parse().map_err(|e| anyhow::anyhow!("the {name} key isn't a valid public key: {e}"))
        };
        let keys = SavingsKeys::new(
            key("PHONE", &self.phone)?,
            key("DEVICE", &self.device)?,
            key("DEVICE_R", &self.device_r)?,
            key("HELPER", &self.helper)?,
            key("HEIR", &self.heir)?,
        )?;
        let delays = Delays::new(self.device_delay, self.heir_delay)?;
        Ok(SavingsPolicy::new(keys, delays, self.network.network())?)
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
