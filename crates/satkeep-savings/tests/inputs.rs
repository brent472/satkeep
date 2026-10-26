//! A PSBT must tell the truth about the coins it spends (`spend::check_inputs`).
//!
//! A SegWit v0 signature (P2WSH Savings, no beneficiary) commits only to the
//! amount of the input it signs, so a PSBT that understates another input
//! would show too small a fee. Two such approvals, each covering a different
//! input, could be combined into a transaction that burns the difference.

mod common;

use common::{Holder, Node, Path, Savings};
use satkeep_savings::bitcoin::{Amount, psbt::Psbt};
use satkeep_savings::{Delays, Error, spend};

fn two_coins(node: &Node, delays: Delays) -> Savings {
    let mut savings = Savings::new(delays);
    savings.fund(node, Amount::from_btc(1.0).unwrap());
    savings.fund(node, Amount::from_btc(1.0).unwrap());
    savings
}

fn spend_both(savings: &mut Savings, node: &Node) -> Psbt {
    let psbt = savings.build(Path::TwoOfThree, &node.new_address()).unwrap();
    assert_eq!(psbt.unsigned_tx.input.len(), 2);
    psbt
}

#[test]
fn an_honest_payment_passes_in_both_shapes() {
    let node = Node::start();
    for delays in [Delays::none(), Delays::with_heir(20).unwrap()] {
        let mut savings = two_coins(&node, delays);
        let psbt = spend_both(&mut savings, &node);
        assert!(spend::check_inputs(&savings.wallet, &psbt).is_ok());
    }
}

#[test]
fn an_understated_input_amount_is_refused() {
    let node = Node::start();
    let mut savings = two_coins(&node, Delays::none());
    let mut psbt = spend_both(&mut savings, &node);
    let real_fee = psbt.fee().unwrap();
    psbt.inputs[1].witness_utxo.as_mut().unwrap().value = Amount::from_sat(1_000);
    assert!(psbt.fee().is_err() || psbt.fee().unwrap() < real_fee, "the PSBT now claims a smaller fee");
    assert!(matches!(spend::check_inputs(&savings.wallet, &psbt), Err(Error::InputMismatch { input: 1 })));
}

#[test]
fn a_swapped_previous_transaction_is_refused() {
    let node = Node::start();
    let mut savings = two_coins(&node, Delays::none());
    let mut psbt = spend_both(&mut savings, &node);
    if psbt.inputs[0].non_witness_utxo.is_none() {
        psbt.inputs[0].non_witness_utxo = psbt.inputs[1].non_witness_utxo.clone();
    }
    psbt.inputs[1].non_witness_utxo = psbt.inputs[0].non_witness_utxo.clone();
    if let Some(tx) = psbt.inputs[1].non_witness_utxo.as_mut() {
        tx.output.reverse();
    }
    assert!(matches!(spend::check_inputs(&savings.wallet, &psbt), Err(Error::InputMismatch { input: 1 })));
}

#[test]
fn a_holder_only_signs_inputs_it_can_see_which_is_why_the_cli_checks_every_input() {
    // Strip input 1's key data: the holder signs input 0 only. The library
    // reports the signatures it added; the CLI refuses unless every input
    // got one (crates/satkeep-cli/tests/approve.rs).
    let node = Node::start();
    let mut savings = two_coins(&node, Delays::none());
    let mut psbt = spend_both(&mut savings, &node);
    psbt.inputs[1].bip32_derivation.clear();
    savings.sign(&mut psbt, &[Holder::Phone]);
    assert_eq!((psbt.inputs[0].partial_sigs.len(), psbt.inputs[1].partial_sigs.len()), (1, 0));
}

#[test]
fn a_substituted_witness_script_is_refused() {
    // A different script than the coin's address commits to: a signature
    // over it would be useless, and nothing should be signed.
    let node = Node::start();
    let mut savings = two_coins(&node, Delays::none());
    let mut psbt = spend_both(&mut savings, &node);
    let other = psbt.inputs[0].witness_script.clone().unwrap();
    let mut changed = other.to_bytes();
    changed[0] = if changed[0] == 0x52 { 0x51 } else { 0x52 }; // OP_2 <-> OP_1: 1 of 3
    psbt.inputs[1].witness_script = Some(satkeep_savings::bitcoin::ScriptBuf::from_bytes(changed));
    assert!(matches!(spend::check_inputs(&savings.wallet, &psbt), Err(Error::InputMismatch { input: 1 })));
}

#[test]
fn every_p2wsh_input_carries_its_previous_transaction() {
    // What Satkeep builds: payments, and the move to a wallet with a
    // beneficiary, include each P2WSH coin's whole previous transaction,
    // and it hashes to the input's txid.
    let node = Node::start();
    let mut savings = two_coins(&node, Delays::none());
    let payment = spend_both(&mut savings, &node);
    let target = savings.with_heir_added(20);
    let moved = satkeep_savings::migrate::build_move(&mut savings.wallet, &savings.policy, &target.policy, common::MIN_HEIR_DELAY, common::FEE_RATE, satkeep_savings::spend::FeeCeiling::DEFAULT).unwrap();
    for psbt in [&payment, &moved] {
        for (txin, input) in psbt.unsigned_tx.input.iter().zip(&psbt.inputs) {
            let prev = input.non_witness_utxo.as_ref().expect("the previous transaction is included");
            assert_eq!(prev.compute_txid(), txin.previous_output.txid);
        }
    }
}

#[test]
fn a_missing_previous_transaction_is_refused() {
    let node = Node::start();
    let mut savings = two_coins(&node, Delays::none());
    let mut psbt = spend_both(&mut savings, &node);
    psbt.inputs[1].non_witness_utxo = None;
    assert!(matches!(spend::check_inputs(&savings.wallet, &psbt), Err(Error::MissingPreviousTransaction { input: 1 })));
}

#[test]
fn a_previous_transaction_that_doesnt_hash_to_the_txid_is_refused() {
    // Same amount and script in the right output, but not the real
    // transaction: one byte of its lock time changed.
    let node = Node::start();
    let mut savings = two_coins(&node, Delays::none());
    let mut psbt = spend_both(&mut savings, &node);
    let prev = psbt.inputs[1].non_witness_utxo.as_mut().unwrap();
    prev.lock_time = satkeep_savings::bitcoin::absolute::LockTime::from_consensus(prev.lock_time.to_consensus_u32() ^ 1);
    assert!(matches!(spend::check_inputs(&savings.wallet, &psbt), Err(Error::InputMismatch { input: 1 })));
}
