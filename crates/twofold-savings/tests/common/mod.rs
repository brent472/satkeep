//! Regtest harness for the Savings spending-path tests.
//!
//! Every test starts its own throwaway `bitcoind` (from `BITCOIND_EXE`) in a
//! temporary folder on a random local RPC port with P2P off, and generates
//! fresh keys in memory. Nothing here touches a long-running node, and no
//! private key is ever printed, logged or written to disk.

#![allow(dead_code)] // each test binary uses a different part of the harness

use std::str::FromStr;

use bdk_bitcoind_rpc::bitcoincore_rpc::{Auth, Client, RpcApi};
use bdk_bitcoind_rpc::{Emitter, NO_EXPECTED_MEMPOOL_TXS};
use bdk_wallet::descriptor::policy::Condition;
use bdk_wallet::signer::{SignerContext, SignerWrapper, TransactionSigner};
use bdk_wallet::{KeychainKind, SignOptions, Wallet};
use rand::RngCore;
use serde_json::{Value, json};
use twofold_savings::bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use twofold_savings::bitcoin::key::Secp256k1;
use twofold_savings::bitcoin::psbt::Psbt;
use twofold_savings::bitcoin::taproot::{LeafVersion, TapLeafHash};
use twofold_savings::bitcoin::{
    Address, Amount, FeeRate, Network, NetworkKind, Transaction, Txid, Witness, XOnlyPublicKey, consensus,
};
use twofold_savings::miniscript::descriptor::{DescriptorPublicKey, DescriptorXKey, Wildcard};
use twofold_savings::miniscript::{Miniscript, Tap};
use twofold_savings::{Delays, SavingsKeys, SavingsPolicy};

/// Fixed fee rate for every test spend. Regtest has no fee estimates; this
/// sits safely above the default relay minimum.
pub const FEE_RATE: FeeRate = FeeRate::from_sat_per_vb_u32(2);

/// Largest batch of blocks mined per RPC call, to stay well inside the RPC
/// client's timeout.
const MINE_BATCH: u32 = 1_000;

// ---------------------------------------------------------------- node ----

/// A throwaway regtest node with a funded wallet that acts as a faucet.
pub struct Node {
    _process: corepc_node::Node,
    rpc: Client,
    miner: Address,
}

impl Node {
    pub fn start() -> Self {
        let exe = corepc_node::exe_path().expect("set BITCOIND_EXE to the path of bitcoind.exe");
        let process = corepc_node::Node::with_conf(exe, &corepc_node::Conf::default())
            .expect("start a throwaway regtest bitcoind");
        let auth = Auth::CookieFile(process.params.cookie_file.clone());
        let rpc = Client::new(&process.rpc_url_with_wallet("default"), auth).expect("RPC client");
        let miner = parse_address(&rpc.call::<String>("getnewaddress", &[]).expect("getnewaddress"));
        let node = Self { _process: process, rpc, miner };
        node.mine(101); // the first coinbase is spendable after 100 confirmations
        node
    }

    pub fn rpc(&self) -> &Client {
        &self.rpc
    }

    pub fn mine(&self, blocks: u32) {
        let mut left = blocks;
        while left > 0 {
            let n = left.min(MINE_BATCH);
            self.rpc
                .call::<Vec<String>>("generatetoaddress", &[json!(n), json!(self.miner.to_string())])
                .expect("generatetoaddress");
            left -= n;
        }
    }

    pub fn height(&self) -> u32 {
        self.rpc.call::<u32>("getblockcount", &[]).expect("getblockcount")
    }

    pub fn new_address(&self) -> Address {
        parse_address(&self.rpc.call::<String>("getnewaddress", &[]).expect("getnewaddress"))
    }

    /// Sends from the faucet wallet. Not mined yet.
    pub fn send_to(&self, address: &Address, amount: Amount) -> Txid {
        let txid: String = self
            .rpc
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

    /// Broadcasts `tx`, mines it into a block, and checks it confirmed.
    pub fn broadcast_and_confirm(&self, tx: &Transaction) {
        let hex = consensus::encode::serialize_hex(tx);
        let txid: String = self.rpc.call("sendrawtransaction", &[json!(hex)]).expect("sendrawtransaction");
        self.mine(1);
        let info: Value = self.rpc.call("gettransaction", &[json!(txid)]).unwrap_or(Value::Null);
        let confirmations = match info["confirmations"].as_i64() {
            Some(c) => c,
            // Not a faucet-wallet transaction: look it up by txid instead.
            None => self.rpc.call::<Value>("getrawtransaction", &[json!(txid), json!(true)]).unwrap()
                ["confirmations"]
                .as_i64()
                .unwrap_or(0),
        };
        assert_eq!(confirmations, 1, "transaction {txid} should be in the latest block");
    }
}

fn parse_address(s: &str) -> Address {
    Address::from_str(s).unwrap().require_network(Network::Regtest).unwrap()
}

// --------------------------------------------------------------- keys -----

/// One person or device holding keys. The private key lives only in this
/// struct, in memory, for the length of one test. Deliberately not `Debug`.
pub struct KeyHolder {
    master: Xpriv,
}

impl KeyHolder {
    pub fn fresh() -> Self {
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        Self { master: Xpriv::new_master(NetworkKind::Test, &seed).unwrap() }
    }

    /// The public key for `account` as `[fp/87'/1'/account']tpub…/<0;1>/*`.
    /// These derivation paths are test-only.
    pub fn public_key(&self, account: u32) -> DescriptorPublicKey {
        let secp = Secp256k1::new();
        let path = DerivationPath::from_str(&format!("m/87'/1'/{account}'")).unwrap();
        let xpub = Xpub::from_priv(&secp, &self.master.derive_priv(&secp, &path).unwrap());
        let fp = self.master.fingerprint(&secp);
        DescriptorPublicKey::from_str(&format!("[{fp}/87'/1'/{account}']{xpub}/<0;1>/*")).unwrap()
    }

    /// Adds this holder's signatures to every script leaf it has a key in,
    /// for accounts 0 and 1 on both the receive and change branches.
    pub fn sign(&self, psbt: &mut Psbt) {
        let secp = Secp256k1::new();
        let fp = self.master.fingerprint(&secp);
        for account in [0, 1] {
            let origin = DerivationPath::from_str(&format!("m/87'/1'/{account}'")).unwrap();
            let xkey = self.master.derive_priv(&secp, &origin).unwrap();
            for branch in ["m/0", "m/1"] {
                // BDK only signs with a key whose origin and path match the
                // PSBT's exactly, so each branch needs its own signer.
                let key = DescriptorXKey {
                    origin: Some((fp, origin.clone())),
                    xkey,
                    derivation_path: DerivationPath::from_str(branch).unwrap(),
                    wildcard: Wildcard::Unhardened,
                };
                let signer = SignerWrapper::new(key, SignerContext::Tap { is_internal_key: false });
                signer.sign_transaction(psbt, &SignOptions::default(), &secp).expect("sign");
            }
        }
    }
}

/// The four holders in a Savings wallet. DEVICE holds two keys: account 0 for
/// the 2-of-3 path and account 1 for its delayed path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    Phone,
    Device,
    Helper,
    Heir,
}

pub use twofold_savings::spend::Path;

// ------------------------------------------------------------- wallet -----

/// A watch-only BDK wallet for one Savings policy, plus the key holders who
/// can sign for it.
pub struct Savings {
    pub policy: SavingsPolicy,
    pub wallet: Wallet,
    phone: KeyHolder,
    device: KeyHolder,
    helper: KeyHolder,
    heir: KeyHolder,
}

impl Savings {
    pub fn new(delays: Delays) -> Self {
        let (phone, device, helper, heir) =
            (KeyHolder::fresh(), KeyHolder::fresh(), KeyHolder::fresh(), KeyHolder::fresh());
        let keys = SavingsKeys::new(
            phone.public_key(0),
            device.public_key(0),
            device.public_key(1),
            helper.public_key(0),
            heir.public_key(0),
        )
        .unwrap();
        let policy = SavingsPolicy::new(keys, delays, Network::Regtest).unwrap();
        let (receive, change) = policy.receive_and_change();
        let wallet = Wallet::create(receive.to_string(), change.to_string())
            .network(Network::Regtest)
            .create_wallet_no_persist()
            .unwrap();
        Self { policy, wallet, phone, device, helper, heir }
    }

    pub fn delays(&self) -> Delays {
        self.policy.delays()
    }

    /// Reads every block the wallet hasn't seen yet.
    pub fn sync(&mut self, node: &Node) {
        let from = self.wallet.latest_checkpoint().height() + 1;
        self.sync_from(node, from);
    }

    /// Jumps the wallet to the node's tip without reading the blocks in
    /// between. Only for after mining blocks that can't hold Savings coins.
    pub fn sync_skipping_empty_blocks(&mut self, node: &Node) {
        self.sync_from(node, node.height());
    }

    fn sync_from(&mut self, node: &Node, start_height: u32) {
        let mut emitter =
            Emitter::new(node.rpc(), self.wallet.latest_checkpoint(), start_height, NO_EXPECTED_MEMPOOL_TXS);
        while let Some(event) = emitter.next_block().expect("read block") {
            let height = event.block_height();
            self.wallet.apply_block_connected_to(&event.block, height, event.connected_to()).expect("apply block");
        }
    }

    /// Sends `amount` from the faucet to a new Savings address and mines it.
    /// Returns the height of the block the coin confirmed in.
    pub fn fund(&mut self, node: &Node, amount: Amount) -> u32 {
        let address = self.wallet.reveal_next_address(KeychainKind::External).address;
        node.send_to(&address, amount);
        node.mine(1);
        self.sync(node);
        node.height()
    }

    /// Mines until the coin funded at `funded_at` has exactly `confirmations`
    /// confirmations (the funding block counts as the first), then syncs.
    ///
    /// The node accepts a spend with a relative delay of N blocks once the
    /// coin has N confirmations, because that spend could go in the next
    /// block, N blocks after the coin's.
    pub fn age_to(&mut self, node: &Node, funded_at: u32, confirmations: u32) {
        let target = funded_at + confirmations - 1;
        assert!(node.height() <= target, "coin is already older than {confirmations} confirmations");
        node.mine(target - node.height());
        self.sync_skipping_empty_blocks(node);
    }

    /// Asks the Twofold library for a PSBT that sends everything in Savings
    /// to `to` through `path`.
    pub fn build(&mut self, path: Path, to: &Address) -> Result<Psbt, twofold_savings::Error> {
        let delays = self.delays();
        twofold_savings::spend::build_spend(&mut self.wallet, delays, path, |b| {
            b.drain_wallet().drain_to(to.script_pubkey()).fee_rate(FEE_RATE);
        })
    }

    /// The same PSBT built with BDK directly, skipping Twofold's delay check,
    /// as an attacker or a buggy wallet could. Only for showing the network
    /// refuses what Twofold would have refused.
    pub fn build_unchecked(&mut self, path: Path, to: &Address) -> Psbt {
        let csv = path.sequence(self.delays());
        let mut builder = self.wallet.build_tx();
        builder
            .drain_wallet()
            .drain_to(to.script_pubkey())
            .fee_rate(FEE_RATE)
            .set_condition(Condition { csv, timelock: None });
        builder.finish().expect("build PSBT")
    }

    pub fn sign(&self, psbt: &mut Psbt, holders: &[Holder]) {
        for holder in holders {
            self.holder(*holder).sign(psbt);
        }
    }

    fn holder(&self, holder: Holder) -> &KeyHolder {
        match holder {
            Holder::Phone => &self.phone,
            Holder::Device => &self.device,
            Holder::Helper => &self.helper,
            Holder::Heir => &self.heir,
        }
    }

    /// Finalizes through the Twofold library, which checks coin ages before
    /// handing over to BDK.
    pub fn finalize(&self, psbt: &Psbt) -> Result<Transaction, twofold_savings::Error> {
        twofold_savings::spend::finalize(&self.wallet, psbt)
    }

    /// Lets BDK finalize on its own, with no Twofold checks. `None` if BDK
    /// refuses. Used only to pin down how BDK behaves.
    pub fn bdk_only_finalize(&self, psbt: &Psbt) -> Option<Transaction> {
        let mut psbt = psbt.clone();
        let next_block = self.wallet.latest_checkpoint().height() + 1;
        let options = SignOptions { assume_height: Some(next_block), ..Default::default() };
        match self.wallet.finalize_psbt(&mut psbt, options) {
            Ok(true) => Some(psbt.extract_tx().expect("finalized PSBT extracts")),
            _ => None,
        }
    }

    /// Builds the spend by hand, sidestepping BDK and miniscript: puts
    /// whatever signatures the PSBT has for `path`'s leaf into a witness for
    /// that leaf, missing ones as empty, and lets the network judge it.
    pub fn forced(&self, psbt: &Psbt, path: Path) -> Transaction {
        let mut tx = psbt.unsigned_tx.clone();
        for (i, input) in psbt.inputs.iter().enumerate() {
            let (control, (script, _)) = input
                .tap_scripts
                .iter()
                .find(|(_, (script, _))| self.leaf_of(script) == path)
                .unwrap_or_else(|| panic!("input {i} has no {path:?} leaf"));
            let ms = Miniscript::<XOnlyPublicKey, Tap>::parse(script).unwrap();
            let leaf_hash = TapLeafHash::from_script(script, LeafVersion::TapScript);
            let keys: Vec<XOnlyPublicKey> = ms.iter_pk().collect();
            let mut witness = Witness::new();
            // Script checks keys in order, so the first key's signature goes
            // on top of the stack, i.e. last in the witness.
            for key in keys.iter().rev() {
                match input.tap_script_sigs.get(&(*key, leaf_hash)) {
                    Some(sig) => witness.push(sig.to_vec()),
                    None => witness.push([]),
                }
            }
            witness.push(script.as_bytes());
            witness.push(control.serialize());
            tx.input[i].witness = witness;
        }
        tx
    }

    /// Which path a finalized Taproot script-path spend revealed.
    pub fn path_used(&self, tx: &Transaction) -> Path {
        let leaf = tx.input[0].witness.taproot_leaf_script().expect("a script-path spend");
        assert_eq!(leaf.version, LeafVersion::TapScript);
        self.leaf_of(leaf.script)
    }

    fn leaf_of(&self, script: &twofold_savings::bitcoin::Script) -> Path {
        let ms = Miniscript::<XOnlyPublicKey, Tap>::parse(script).unwrap().to_string();
        if ms.starts_with("multi_a(2,") {
            Path::TwoOfThree
        } else if ms.ends_with(&format!("older({}))", self.delays().device())) {
            Path::Device
        } else if ms.ends_with(&format!("older({}))", self.delays().heir())) {
            Path::Heir
        } else {
            panic!("unexpected leaf {ms}")
        }
    }
}
