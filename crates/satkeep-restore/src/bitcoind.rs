//! Handing Savings to the owner's own Bitcoin Core: by RPC import into a new
//! wallet (the default), or as an importdescriptors file.
//!
//! Either way the descriptors carry PHONE's private key, so nothing here
//! ever prints them, and an RPC error from the import is never repeated:
//! Core's message could quote the request.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use bitcoincore_rpc::jsonrpc;
use bitcoincore_rpc::{Client, RpcApi};
use satkeep_backup::setup::Network;
use satkeep_backup::zeroize::Zeroizing;
use serde_json::{Value, json};

use crate::keys::PrivateDescriptor;

/// The importdescriptors rescan can take a while on signet.
const RPC_TIMEOUT: Duration = Duration::from_secs(3600);

/// One descriptor to import, and how.
pub struct Import {
    pub descriptor: PrivateDescriptor,
    /// Only the newest wallet of each shape is active (where Core puts new
    /// addresses); earlier ones are still watched and spendable.
    pub active: bool,
    /// The birthday block's time: Core rescans from there.
    pub timestamp: u64,
}

pub fn default_url(network: Network) -> String {
    match network {
        Network::Regtest => "http://127.0.0.1:18443".into(),
        Network::Signet => "http://127.0.0.1:38332".into(),
    }
}

pub fn default_cookie(network: Network) -> PathBuf {
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Bitcoin")
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Application Support/Bitcoin")
    } else {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".bitcoin")
    };
    dir.join(network.as_str()).join(".cookie")
}

/// Connects to the node at `url` and checks it's on `network`. Refuses
/// mainnet outright.
pub fn connect(url: &str, cookie: &Path, network: Network) -> Result<Client> {
    ensure_local(url)?;
    let client = client(url, cookie)?;
    let info: Value = client.call("getblockchaininfo", &[]).with_context(|| format!("asking the node at {url} which chain it's on"))?;
    let chain = info["chain"].as_str().unwrap_or("unknown");
    if chain == "main" {
        bail!("the node at {url} is on mainnet; the restore tool only runs on test networks");
    }
    if chain != network.as_str() {
        bail!("the node at {url} is on {chain}, but this wallet is on {}", network.as_str());
    }
    Ok(client)
}

/// The import sends PHONE's private key over plain HTTP, so only to a node
/// on this machine: `http://127.0.0.1`, `http://localhost` or `http://[::1]`,
/// with any port.
pub fn ensure_local(url: &str) -> Result<()> {
    let rest = url.strip_prefix("http://").unwrap_or("");
    let host = if rest.starts_with("[::1]") { "[::1]" } else { rest.split([':', '/']).next().unwrap_or("") };
    let after = &rest[host.len()..];
    ensure!(
        matches!(host, "127.0.0.1" | "localhost" | "[::1]") && (after.is_empty() || after.starts_with(':') || after.starts_with('/')),
        "--rpc-url must be a node on this machine (http://127.0.0.1, http://localhost or http://[::1]): the import sends the \
         phone key over plain HTTP"
    );
    Ok(())
}

fn client(url: &str, cookie: &Path) -> Result<Client> {
    let text = std::fs::read_to_string(cookie)
        .with_context(|| format!("reading the RPC cookie at {} (is the node running? pass --rpc-cookie)", cookie.display()))?;
    let (user, pass) = text.trim().split_once(':').context("the RPC cookie file isn't user:password")?;
    let transport = jsonrpc::simple_http::SimpleHttpTransport::builder()
        .url(url)
        .with_context(|| format!("the RPC URL {url}"))?
        .auth(user, Some(pass))
        .timeout(RPC_TIMEOUT)
        .build();
    Ok(Client::from_jsonrpc(jsonrpc::Client::with_transport(transport)))
}

/// The time of the block at `height`, for importdescriptors' timestamp.
pub fn birthday_time(client: &Client, height: u32) -> Result<u64> {
    let tip: u32 = client.call("getblockcount", &[])?;
    ensure!(height <= tip, "the node is at block {tip}, before the wallet's birthday block {height}; let it sync first");
    let hash: Value = client.call("getblockhash", &[json!(height)])?;
    let header: Value = client.call("getblockheader", &[hash, json!(true)])?;
    header["time"].as_u64().context("the node's block header has no time")
}

/// How long the wallet stays unlocked for the import, in seconds.
const UNLOCK_FOR_IMPORT: u64 = 60;

/// Checks a Core wallet name before it goes into a URL.
pub fn check_wallet_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "--core-wallet may only use letters, digits, - and _"
    );
    Ok(())
}

/// Creates a new descriptor wallet called `name` in the node, encrypted with
/// `passphrase`, and imports `imports` into it: unlocked for at most 60 s
/// around the import, and locked again afterwards. Refuses a wallet that
/// already exists. Errors never repeat the passphrase.
pub fn import(client: &Client, url: &str, cookie: &Path, name: &str, passphrase: &str, imports: &[Import]) -> Result<()> {
    check_wallet_name(name)?;
    ensure!(!passphrase.is_empty(), "the wallet passphrase can't be empty");
    // createwallet name, disable_private_keys, blank, passphrase, avoid_reuse, descriptors, load_on_startup
    let args = [json!(name), json!(false), json!(true), json!(passphrase), json!(false), json!(true), json!(false)];
    client
        .call::<Value>("createwallet", &args)
        .map_err(|_| anyhow::anyhow!("couldn't create the wallet {name:?} in Bitcoin Core (a wallet of that name may already exist)"))?;

    let wallet = self::client(&format!("{}/wallet/{name}", url.trim_end_matches('/')), cookie)?;
    wallet
        .call::<Value>("walletpassphrase", &[json!(passphrase), json!(UNLOCK_FOR_IMPORT)])
        .map_err(|_| anyhow::anyhow!("Bitcoin Core couldn't unlock the new wallet for the import"))?;
    // serde_json can't wipe its strings; this request lives only until the
    // call returns (README, "Rules for anything secret").
    let imported: Result<Value, _> = wallet.call("importdescriptors", &[requests(imports)]);
    // Locked again whatever happened.
    let locked = wallet.call::<Value>("walletlock", &[]);
    let results = imported.map_err(|_| anyhow::anyhow!("Bitcoin Core refused the import (no detail: the request holds the phone key)"))?;
    locked.context("Bitcoin Core couldn't lock the wallet after the import; lock it with `bitcoin-cli walletlock`")?;
    let results = results.as_array().context("importdescriptors returned no list")?;
    for (i, result) in results.iter().enumerate() {
        if result["success"] != json!(true) {
            let code = result["error"]["code"].as_i64().unwrap_or(0);
            bail!("Bitcoin Core couldn't import descriptor {} (error {code}; no more detail: it holds the phone key)", i + 1);
        }
    }
    Ok(())
}

/// Writes the importdescriptors request to `path`, a new file readable only
/// by its owner where the system allows. Never overwrites.
pub fn write_file(path: &Path, imports: &[Import]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("creating {} (it must not exist yet)", path.display()))?;
    let text = Zeroizing::new(serde_json::to_string_pretty(&requests(imports)).expect("JSON"));
    let written = file.write_all(text.as_bytes()).and_then(|()| file.write_all(b"\n")).and_then(|()| file.sync_all());
    if let Err(e) = written {
        // Don't leave part of a file holding the key behind.
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e).with_context(|| format!("writing {}", path.display()));
    }
    Ok(())
}

fn requests(imports: &[Import]) -> Value {
    Value::Array(
        imports
            .iter()
            .map(|i| {
                json!({
                    "desc": i.descriptor.descriptor.as_str(),
                    "active": i.active,
                    "internal": i.descriptor.internal,
                    "timestamp": i.timestamp,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_node_on_this_machine() {
        for ok in ["http://127.0.0.1", "http://127.0.0.1:18443", "http://localhost:38332/", "http://[::1]:18443"] {
            assert!(super::ensure_local(ok).is_ok(), "{ok}");
        }
        for refused in [
            "http://example.com:18443",
            "http://127.0.0.1.example.com:18443",
            "http://localhost@example.com:18443",
            "http://10.0.0.5:18443",
            "https://127.0.0.1:18443",
            "127.0.0.1:18443",
            "",
        ] {
            assert!(super::ensure_local(refused).is_err(), "{refused}");
        }
    }
}
