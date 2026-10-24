//! Throwaway regtest nodes for Satkeep's tests.
//!
//! [`Node::start`] runs `bitcoind` (from `BITCOIND_EXE`) in a temporary
//! folder on a random local RPC port with P2P off, and stops it when the
//! `Node` is dropped. It never touches a node you already run.

use std::path::PathBuf;
use std::str::FromStr;

use bdk_bitcoind_rpc::bitcoincore_rpc::{Auth, Client, RpcApi};
use bdk_wallet::bitcoin::key::Secp256k1;
use bdk_wallet::bitcoin::{Address, Amount, Network, Transaction, Txid, consensus};
use serde_json::{Value, json};

/// Largest batch of blocks mined per RPC call, to stay well inside the RPC
/// client's 15-second timeout. 500 blocks took longer than that on a busy
/// machine (other tests building and running alongside); 100 leaves a wide
/// margin.
const MINE_BATCH: u32 = 100;

/// A throwaway regtest node with a funded wallet that acts as a faucet.
pub struct Node {
    process: corepc_node::Node,
    rpc: Client,
    faucet: Client,
    /// `raw(OP_TRUE)` with its checksum. Blocks are mined to this script
    /// rather than to a wallet address because the node wallet slows mining
    /// down several times over.
    burn: String,
    /// The node's clock, in seconds. Each block must be timestamped after
    /// the median of the 11 before it and no more than 2 hours ahead of the
    /// node's clock, so mining thousands of blocks in seconds needs the
    /// clock moved forward too: 10 minutes per block, as on the real network.
    clock: std::cell::Cell<u64>,
}

impl Node {
    pub fn start() -> Self {
        let exe = corepc_node::exe_path().expect("set BITCOIND_EXE to the path of bitcoind.exe");
        let process = corepc_node::Node::with_conf(exe, &corepc_node::Conf::default())
            .expect("start a throwaway regtest bitcoind");
        let auth = || Auth::CookieFile(process.params.cookie_file.clone());
        let rpc = Client::new(&process.rpc_url(), auth()).expect("RPC client");
        let faucet = Client::new(&process.rpc_url_with_wallet("default"), auth()).expect("RPC client");
        let info: Value = rpc.call("getdescriptorinfo", &[json!("raw(51)")]).expect("getdescriptorinfo");
        let burn = format!("raw(51)#{}", info["checksum"].as_str().unwrap());
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let node = Self { process, rpc, faucet, burn, clock: std::cell::Cell::new(now) };

        // One 50-coin block for the faucet, then 100 more so it can be spent.
        let address: String = node.faucet.call("getnewaddress", &[]).expect("getnewaddress");
        node.rpc.call::<Value>("generatetoaddress", &[json!(1), json!(address)]).expect("generatetoaddress");
        node.mine(100);
        node
    }

    pub fn rpc(&self) -> &Client {
        &self.rpc
    }

    /// The node's RPC address, like `http://127.0.0.1:PORT`.
    pub fn rpc_url(&self) -> String {
        self.process.rpc_url()
    }

    /// The node's RPC cookie file.
    pub fn cookie_file(&self) -> PathBuf {
        self.process.params.cookie_file.clone()
    }

    pub fn mine(&self, blocks: u32) {
        let mut left = blocks;
        while left > 0 {
            let n = left.min(MINE_BATCH);
            self.clock.set(self.clock.get() + u64::from(n) * 600);
            self.rpc.call::<Value>("setmocktime", &[json!(self.clock.get())]).expect("setmocktime");
            self.rpc.call::<Value>("generatetodescriptor", &[json!(n), json!(self.burn)]).expect("generatetodescriptor");
            left -= n;
        }
    }

    /// Unloads the faucet wallet, which makes mining faster still. After
    /// this, [`Node::send_to`] no longer works.
    pub fn close_faucet(&self) {
        self.rpc.call::<Value>("unloadwallet", &[json!("default")]).expect("unloadwallet");
    }

    pub fn height(&self) -> u32 {
        self.rpc.call::<u32>("getblockcount", &[]).expect("getblockcount")
    }

    /// An address nobody will spend from, for sending test spends to. Its
    /// key is random and thrown away at once.
    pub fn new_address(&self) -> Address {
        let secp = Secp256k1::new();
        let (secret, _) = secp.generate_keypair(&mut bdk_wallet::bitcoin::secp256k1::rand::thread_rng());
        let (xonly, _) = secret.public_key(&secp).x_only_public_key();
        Address::p2tr(&secp, xonly, None, Network::Regtest)
    }

    /// Sends from the faucet wallet. Not mined yet.
    pub fn send_to(&self, address: &Address, amount: Amount) -> Txid {
        let txid: String = self
            .faucet
            .call("sendtoaddress", &[json!(address.to_string()), json!(amount.to_btc())])
            .expect("sendtoaddress");
        Txid::from_str(&txid).unwrap()
    }

    /// Asks the node whether it would accept `tx` into its mempool right now,
    /// without broadcasting. Returns the node's reject reason if not.
    pub fn accepts(&self, tx: &Transaction) -> Result<(), String> {
        let hex = consensus::encode::serialize_hex(tx);
        let res: Vec<Value> = self.rpc.call("testmempoolaccept", &[json!([hex])]).expect("testmempoolaccept");
        match res[0]["allowed"].as_bool() {
            Some(true) => Ok(()),
            _ => Err(res[0]["reject-reason"].as_str().unwrap_or("no reason given").to_string()),
        }
    }

    /// Broadcasts `tx`, mines one block, and checks `tx` is in it.
    pub fn broadcast_and_confirm(&self, tx: &Transaction) {
        let hex = consensus::encode::serialize_hex(tx);
        let txid: String = self.rpc.call("sendrawtransaction", &[json!(hex)]).expect("sendrawtransaction");
        self.mine(1);
        let tip: String = self.rpc.call("getbestblockhash", &[]).unwrap();
        let block: Value = self.rpc.call("getblock", &[json!(tip)]).unwrap();
        let mined = block["tx"].as_array().unwrap().iter().any(|t| t.as_str() == Some(&txid));
        assert!(mined, "transaction {txid} should be in the latest block");
    }
}

