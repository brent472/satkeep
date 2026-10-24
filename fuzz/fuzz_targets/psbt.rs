//! PSBT bytes, or a valid Savings PSBT edited by the input, through the
//! functions that judge an outside PSBT: `spend::path_of`,
//! `checkin::is_checkin`, `checkin::check_checkin`, `migrate::check_move`
//! and `spend::check_delays`.
//!
//! They run against a fixed test wallet whose keys are generated inside the
//! target when it starts. Nothing is ever printed.

#![no_main]

use bdk_wallet::descriptor::policy::Condition;
use bdk_wallet::{KeychainKind, Wallet};
use libfuzzer_sys::arbitrary::{Result as ArbResult, Unstructured};
use libfuzzer_sys::fuzz_target;
use satkeep_savings::bitcoin::block::{Header, Version as BlockVersion};
use satkeep_savings::bitcoin::hashes::Hash;
use satkeep_savings::bitcoin::key::Secp256k1;
use satkeep_savings::bitcoin::psbt::Psbt;
use satkeep_savings::bitcoin::secp256k1::XOnlyPublicKey;
use satkeep_savings::bitcoin::transaction::Version;
use satkeep_savings::bitcoin::{
    Address, Amount, Block, CompactTarget, FeeRate, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn,
    TxMerkleNode, TxOut, Txid, absolute,
};
use satkeep_savings::checkin::{self, build_checkin, check_checkin};
use satkeep_savings::migrate;
use satkeep_savings::spend::{self, Path};
use satkeep_savings::{Delays, HolderKey, SavingsKeys, SavingsPolicy};

struct Fixture {
    wallet: Wallet,
    policy: SavingsPolicy,
    delays: Delays,
    /// Where the settings say a move goes: the same keys with another HEIR.
    next: SavingsPolicy,
    /// Valid PSBTs built by the library: 2-of-3, heir, drain, move, a move
    /// somewhere the settings don't say, check-in (last).
    templates: Vec<Psbt>,
    scripts: Vec<ScriptBuf>,
}

/// The minimum heir delay in the fixture.
const MIN: u16 = 10;

thread_local! {
    static FIXTURE: &'static Fixture = Box::leak(Box::new(build_fixture()));
}

/// A Savings wallet with a beneficiary and coins of different ages:
/// confirmed at heights 1, 12 and 24 of a 25-block chain, plus one
/// unconfirmed. Heir delay 20.
fn build_fixture() -> Fixture {
    let [phone, device, helper, heir, next_heir, stranger] = std::array::from_fn(|_| HolderKey::generate());
    let keys = SavingsKeys::new(phone.public_key(0), device.public_key(0), helper.public_key(0), Some(heir.public_key(0)))
        .expect("fresh keys");
    let delays = Delays::with_heir(20).expect("valid delay");
    let policy = SavingsPolicy::new(keys, delays, Network::Regtest).expect("valid policy");
    let next = migrate::wallet_with_heir(&policy, next_heir.public_key(0), Delays::with_heir(30).expect("valid delay"), MIN)
        .expect("a changed beneficiary");
    let elsewhere = migrate::wallet_with_heir(&policy, stranger.public_key(0), Delays::with_heir(15).expect("valid delay"), MIN)
        .expect("another beneficiary");
    let (receive, change) = policy.receive_and_change();
    let mut wallet = Wallet::create(receive.to_string(), change.to_string())
        .network(Network::Regtest)
        .create_wallet_no_persist()
        .expect("wallet");

    let pay = |wallet: &mut Wallet, n: u8| Transaction {
        version: Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![TxIn { previous_output: OutPoint::new(Txid::from_byte_array([n; 32]), 0), ..Default::default() }],
        output: vec![TxOut {
            value: Amount::from_sat(100_000_000),
            script_pubkey: wallet.reveal_next_address(KeychainKind::External).address.script_pubkey(),
        }],
    };
    let mut parent = wallet.latest_checkpoint().block_id();
    for height in 1..=25u32 {
        let txdata = if [1, 12, 24].contains(&height) { vec![pay(&mut wallet, height as u8)] } else { vec![] };
        let block = Block {
            header: Header {
                version: BlockVersion::TWO,
                prev_blockhash: parent.hash,
                merkle_root: TxMerkleNode::all_zeros(),
                time: height,
                bits: CompactTarget::from_consensus(0x207f_ffff),
                nonce: height,
            },
            txdata,
        };
        wallet.apply_block_connected_to(&block, height, parent).expect("apply block");
        parent = bdk_wallet::chain::BlockId { height, hash: block.block_hash() };
    }
    let unconfirmed = pay(&mut wallet, 99);
    wallet.apply_unconfirmed_txs([(unconfirmed, 0)]);

    let secp = Secp256k1::verification_only();
    let nums = XOnlyPublicKey::from_slice(&[
        0x50, 0x92, 0x9b, 0x74, 0xc1, 0xa0, 0x49, 0x54, 0xb7, 0x8b, 0x4b, 0x60, 0x35, 0xe9, 0x7a, 0x5e, 0x07, 0x8a, 0x5a,
        0x0f, 0x28, 0xec, 0x96, 0xd5, 0x47, 0xbf, 0xee, 0x9a, 0xce, 0x80, 0x3a, 0xc0,
    ])
    .expect("NUMS point");
    let foreign = Address::p2tr(&secp, nums, None, Network::Regtest).script_pubkey();
    let oldest = wallet.list_unspent().find(|u| u.chain_position.confirmation_height_upper_bound() == Some(1)).expect("coin").outpoint;
    let fee = FeeRate::from_sat_per_vb_u32(2);

    let mut templates = Vec::new();
    for path in [Path::TwoOfThree, Path::Heir] {
        let to = foreign.clone();
        let psbt = spend::build_spend(&mut wallet, delays, path, |b| {
            b.add_utxo(oldest).expect("known coin").manually_selected_only().drain_to(to).fee_rate(fee);
        })
        .expect("template spend");
        templates.push(psbt);
    }
    // A plain 2-of-3 spend of everything, unconfirmed coin included.
    let mut b = wallet.build_tx();
    b.drain_wallet().drain_to(foreign.clone()).fee_rate(fee).set_condition(Condition::default());
    templates.push(b.finish().expect("drain spend"));
    templates.push(migrate::build_move(&mut wallet, &policy, &next, MIN, fee).expect("template move"));
    templates.push(migrate::build_move(&mut wallet, &policy, &elsewhere, MIN, fee).expect("template move elsewhere"));
    templates.push(build_checkin(&mut wallet, delays, fee).expect("template check-in"));

    let scripts = vec![
        foreign,
        wallet.reveal_next_address(KeychainKind::External).address.script_pubkey(),
        wallet.reveal_next_address(KeychainKind::Internal).address.script_pubkey(),
        migrate::destination(&next),
        migrate::destination(&elsewhere),
        ScriptBuf::new(),
    ];
    Fixture { wallet, policy, delays, next, templates, scripts }
}

/// Edits a valid PSBT, driven by the fuzzer's bytes.
fn edit(psbt: &mut Psbt, f: &Fixture, u: &mut Unstructured) -> ArbResult<()> {
    for _ in 0..u.int_in_range(0..=8)? {
        let tx = &mut psbt.unsigned_tx;
        match u.int_in_range(0..=9)? {
            0 if !tx.input.is_empty() => {
                let i = u.choose_index(tx.input.len())?;
                tx.input[i].sequence = Sequence::from_consensus(u.arbitrary()?);
            }
            1 => tx.version = Version(u.arbitrary()?),
            2 => tx.lock_time = absolute::LockTime::from_consensus(u.arbitrary()?),
            3 if !tx.input.is_empty() => {
                let i = u.choose_index(tx.input.len())?;
                tx.input.remove(i);
                if i < psbt.inputs.len() {
                    psbt.inputs.remove(i);
                }
            }
            4 if !tx.input.is_empty() => {
                let i = u.choose_index(tx.input.len())?;
                tx.input.push(tx.input[i].clone());
                if let Some(input) = psbt.inputs.get(i).cloned() {
                    psbt.inputs.push(input);
                }
            }
            5 if !tx.output.is_empty() => {
                let i = u.choose_index(tx.output.len())?;
                tx.output.remove(i);
                if i < psbt.outputs.len() {
                    psbt.outputs.remove(i);
                }
            }
            6 if !tx.output.is_empty() => {
                let i = u.choose_index(tx.output.len())?;
                tx.output[i].script_pubkey = f.scripts[u.choose_index(f.scripts.len())?].clone();
            }
            7 if !tx.output.is_empty() => {
                let i = u.choose_index(tx.output.len())?;
                tx.output[i].value = Amount::from_sat(u.arbitrary()?);
            }
            8 => {
                // Swap in the check-in marker, either move marker, or none.
                let n = f.templates.len();
                psbt.proprietary = match u.int_in_range(0..=3)? {
                    0 => f.templates[n - 1].proprietary.clone(),
                    1 => f.templates[n - 2].proprietary.clone(),
                    2 => f.templates[n - 3].proprietary.clone(),
                    _ => Default::default(),
                };
            }
            9 if !tx.input.is_empty() => {
                let i = u.choose_index(tx.input.len())?;
                tx.input[i].previous_output.vout = u.arbitrary()?;
            }
            _ => {}
        }
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    FIXTURE.with(|f| {
        let mut u = Unstructured::new(data);
        let psbt = match u.arbitrary::<u8>().unwrap_or(0) % 2 {
            0 => match Psbt::deserialize(u.take_rest()) {
                Ok(psbt) => psbt,
                Err(_) => return,
            },
            _ => {
                let Ok(i) = u.choose_index(f.templates.len()) else { return };
                let mut psbt = f.templates[i].clone();
                let _ = edit(&mut psbt, f, &mut u);
                psbt
            }
        };

        let path = spend::path_of(&psbt, f.delays);
        let marked = checkin::is_checkin(&psbt);
        let complete = check_checkin(&f.wallet, &psbt, f.delays);
        let moved = migrate::check_move(&f.wallet, &psbt, &f.policy, &f.next, MIN);
        let _ = spend::check_delays(&f.wallet, &psbt);

        // A check-in that passes must really be one: marked, 2-of-3, and a
        // single output back to the wallet.
        if complete.is_ok() {
            assert!(marked, "check_checkin accepted an unmarked PSBT");
            assert_eq!(path, Some(Path::TwoOfThree), "check_checkin accepted a delayed path");
            assert_eq!(psbt.unsigned_tx.output.len(), 1, "check_checkin accepted more than one output");
            assert!(
                f.wallet.is_mine(psbt.unsigned_tx.output[0].script_pubkey.clone()),
                "check_checkin accepted an output that isn't ours"
            );
        }

        // A move that passes must really be one: marked, 2-of-3, and a
        // single output to the first address of the wallet the settings
        // say, whatever the PSBT claims.
        if moved.is_ok() {
            assert!(migrate::is_move(&psbt), "check_move accepted an unmarked PSBT");
            assert_eq!(path, Some(Path::TwoOfThree), "check_move accepted a delayed path");
            assert_eq!(psbt.unsigned_tx.output.len(), 1, "check_move accepted more than one output");
            assert_eq!(psbt.unsigned_tx.output[0].script_pubkey, migrate::destination(&f.next), "check_move: wrong output");
        }
    });
});
