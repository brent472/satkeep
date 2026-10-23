//! Regtest harness for the Savings spending-path tests.
//!
//! Every test starts its own throwaway `bitcoind` (from `BITCOIND_EXE`) in a
//! temporary folder on a random local RPC port with P2P off, and generates
//! fresh keys in memory. Nothing here touches a long-running node, and no
//! private key is ever printed, logged or written to disk.

#![allow(dead_code)] // each test binary uses a different part of the harness

use bdk_bitcoind_rpc::{Emitter, NO_EXPECTED_MEMPOOL_TXS};
use bdk_wallet::descriptor::policy::Condition;
use bdk_wallet::{KeychainKind, SignOptions, Wallet};
use twofold_savings::bitcoin::psbt::Psbt;
use twofold_savings::bitcoin::taproot::{LeafVersion, TapLeafHash};
use twofold_savings::bitcoin::{Address, Amount, FeeRate, Network, Transaction, Witness, XOnlyPublicKey};
use twofold_savings::miniscript::{Miniscript, Segwitv0, Tap};
use twofold_savings::{Delays, HolderKey, SavingsKeys, SavingsPolicy};

/// Fixed fee rate for every test spend. Regtest has no fee estimates; this
/// sits safely above the default relay minimum.
pub const FEE_RATE: FeeRate = FeeRate::from_sat_per_vb_u32(2);

pub use twofold_testenv::Node;

/// The four holders. HEIR's key is only in the wallet when it has a
/// beneficiary; in plain Savings, HEIR is a stranger.
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
    phone: HolderKey,
    device: HolderKey,
    helper: HolderKey,
    heir: HolderKey,
}

impl Savings {
    /// Fresh keys. With an heir delay, Savings with a beneficiary; with
    /// `Delays::none()`, plain 2 of 3.
    pub fn new(delays: Delays) -> Self {
        let keys = [(); 4].map(|_| HolderKey::generate());
        Self::from_keys(keys, delays)
    }

    /// The same PHONE, DEVICE and HELPER, and the same HEIR holder, as a
    /// wallet with a beneficiary: what adding one moves Savings to.
    pub fn with_heir_added(&self, heir_delay: u16) -> Self {
        let copy = |k: &HolderKey| HolderKey::from_secret_str(&k.to_secret_string()).unwrap();
        let keys = [copy(&self.phone), copy(&self.device), copy(&self.helper), copy(&self.heir)];
        Self::from_keys(keys, Delays::with_heir(heir_delay).unwrap())
    }

    fn from_keys([phone, device, helper, heir]: [HolderKey; 4], delays: Delays) -> Self {
        let heir_key = delays.heir().map(|_| heir.public_key(0));
        let keys = SavingsKeys::new(phone.public_key(0), device.public_key(0), helper.public_key(0), heir_key).unwrap();
        let policy = SavingsPolicy::new(keys, delays, Network::Regtest).unwrap();
        let (receive, change) = policy.receive_and_change();
        let wallet = Wallet::create(receive.to_string(), change.to_string())
            .network(Network::Regtest)
            .create_wallet_no_persist()
            .unwrap();
        Self { policy, wallet, phone, device, helper, heir }
    }

    /// The paths this wallet has: 2 of 3, plus the heir path with a
    /// beneficiary.
    pub fn paths(&self) -> Vec<Path> {
        if self.policy.has_heir() { vec![Path::TwoOfThree, Path::Heir] } else { vec![Path::TwoOfThree] }
    }

    /// The heir delay. Panics for plain Savings.
    pub fn heir_delay(&self) -> u32 {
        self.delays().heir().expect("a wallet with a beneficiary").into()
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
        // And the mempool, so the wallet sees unconfirmed incoming coins.
        let mempool = emitter.mempool().expect("read mempool");
        self.wallet.apply_unconfirmed_txs(mempool.update);
        self.wallet.apply_evicted_txs(mempool.evicted);
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
            self.holder(*holder).sign(psbt).expect("sign");
        }
    }

    /// PHONE's key, for the backup tests that back it up and restore it.
    pub fn phone_key(&self) -> &HolderKey {
        &self.phone
    }

    fn holder(&self, holder: Holder) -> &HolderKey {
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
    /// For P2WSH Savings (no beneficiary), the multisig script instead.
    pub fn forced(&self, psbt: &Psbt, path: Path) -> Transaction {
        if !self.policy.has_heir() {
            assert_eq!(path, Path::TwoOfThree, "P2WSH Savings has only the 2-of-3 path");
            return self.forced_p2wsh(psbt);
        }
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

    /// The P2WSH 2-of-3 by hand: CHECKMULTISIG's dummy item, the signatures
    /// the PSBT has in the script's key order, empty slots up to two, then
    /// the script. With fewer than two real signatures the script fails.
    fn forced_p2wsh(&self, psbt: &Psbt) -> Transaction {
        let mut tx = psbt.unsigned_tx.clone();
        for (i, input) in psbt.inputs.iter().enumerate() {
            let script = input.witness_script.as_ref().unwrap_or_else(|| panic!("input {i} has no witness script"));
            let ms = Miniscript::<twofold_savings::bitcoin::PublicKey, Segwitv0>::parse(script).unwrap();
            let mut witness = Witness::new();
            witness.push([]);
            let mut slots = 0;
            for key in ms.iter_pk() {
                if let Some(sig) = input.partial_sigs.get(&key) {
                    if slots < 2 {
                        witness.push(sig.to_vec());
                        slots += 1;
                    }
                }
            }
            for _ in slots..2 {
                witness.push([]);
            }
            witness.push(script.as_bytes());
            tx.input[i].witness = witness;
        }
        tx
    }

    /// Which path a finalized spend used: for Taproot, the leaf it revealed;
    /// for P2WSH, always 2 of 3.
    pub fn path_used(&self, tx: &Transaction) -> Path {
        if !self.policy.has_heir() {
            let script = tx.input[0].witness.last().expect("a witness");
            let ms = Miniscript::<twofold_savings::bitcoin::PublicKey, Segwitv0>::parse(
                twofold_savings::bitcoin::Script::from_bytes(script),
            )
            .expect("a multisig witness script");
            assert!(ms.to_string().starts_with("sortedmulti(2,") || ms.to_string().starts_with("multi(2,"), "{ms}");
            return Path::TwoOfThree;
        }
        let leaf = tx.input[0].witness.taproot_leaf_script().expect("a script-path spend");
        assert_eq!(leaf.version, LeafVersion::TapScript);
        self.leaf_of(leaf.script)
    }

    fn leaf_of(&self, script: &twofold_savings::bitcoin::Script) -> Path {
        let ms = Miniscript::<XOnlyPublicKey, Tap>::parse(script).unwrap().to_string();
        if ms.starts_with("multi_a(2,") {
            Path::TwoOfThree
        } else if self.delays().heir().is_some_and(|hd| ms.ends_with(&format!("older({hd}))"))) {
            Path::Heir
        } else {
            panic!("unexpected leaf {ms}")
        }
    }
}
