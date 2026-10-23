//! Talking to a Bitcoin Core node over RPC.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bdk_bitcoind_rpc::Emitter;
use bdk_bitcoind_rpc::bitcoincore_rpc::{Auth, Client, RpcApi};
use bdk_wallet::Wallet;
use serde_json::Value;

use crate::wallet_dir::TestNetwork;

/// Connects and checks the node is on `network`. Refuses mainnet outright.
pub fn connect(url: &str, cookie: &Path, network: TestNetwork) -> Result<Client> {
    if !cookie.exists() {
        bail!("no RPC cookie at {} (is the node running? pass --rpc-cookie)", cookie.display());
    }
    let client = Client::new(url, Auth::CookieFile(cookie.to_path_buf()))
        .with_context(|| format!("connecting to the node at {url}"))?;
    // Raw JSON rather than bitcoincore-rpc's typed structs, which lag behind
    // newer Core releases.
    let info: Value =
        client.call("getblockchaininfo", &[]).with_context(|| format!("asking the node at {url} which chain it's on"))?;
    let chain = info["chain"].as_str().unwrap_or("unknown");
    if chain == "main" {
        bail!("the node at {url} is on mainnet; Twofold only runs on test networks");
    }
    if chain != network.core_name() {
        bail!("the node at {url} is on {chain}, but this wallet is on {}", network.core_name());
    }
    Ok(client)
}

pub fn height(client: &Client) -> Result<u32> {
    Ok(client.call::<u32>("getblockcount", &[])?)
}

/// Reads new blocks (from the wallet's birthday on) and the mempool into the
/// wallet. The caller persists.
pub fn sync(client: &Client, wallet: &mut Wallet, birthday: u32) -> Result<()> {
    let unconfirmed: Vec<_> = wallet
        .transactions()
        .filter(|t| !t.chain_position.is_confirmed())
        .map(|t| t.tx_node.tx.clone())
        .collect();
    let mut emitter = Emitter::new(client, wallet.latest_checkpoint(), birthday, unconfirmed);
    while let Some(event) = emitter.next_block().context("reading blocks from the node")? {
        let height = event.block_height();
        wallet.apply_block_connected_to(&event.block, height, event.connected_to())?;
    }
    let mempool = emitter.mempool().context("reading the node's mempool")?;
    wallet.apply_unconfirmed_txs(mempool.update);
    wallet.apply_evicted_txs(mempool.evicted);
    Ok(())
}

/// Where Bitcoin Core keeps its RPC cookie for `network` by default.
pub fn default_cookie(network: TestNetwork) -> PathBuf {
    let base = if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Bitcoin")
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Application Support/Bitcoin")
    } else {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".bitcoin")
    };
    base.join(network.core_name()).join(".cookie")
}

/// Bitcoin Core's default RPC address for `network`.
pub fn default_url(network: TestNetwork) -> String {
    match network {
        TestNetwork::Regtest => "http://127.0.0.1:18443".into(),
        TestNetwork::Signet => "http://127.0.0.1:38332".into(),
    }
}
