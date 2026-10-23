//! A PSBT must tell the truth about the coins it spends (`spend::check_inputs`).
//!
//! A SegWit v0 signature (P2WSH Savings, no beneficiary) commits only to the
//! amount of the input it signs, so a PSBT that understates another input
//! would show too small a fee. Two such approvals, each covering a different
//! input, could be combined into a transaction that burns the difference.

mod common;

use common::{Holder, Node, Path, Savings};
use twofold_savings::bitcoin::{Amount, psbt::Psbt};
use twofold_savings::{Delays, Error, spend};

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
    // got one (crates/twofold-cli/tests/approve.rs).
    let node = Node::start();
    let mut savings = two_coins(&node, Delays::none());
    let mut psbt = spend_both(&mut savings, &node);
    psbt.inputs[1].bip32_derivation.clear();
    savings.sign(&mut psbt, &[Holder::Phone]);
    assert_eq!((psbt.inputs[0].partial_sigs.len(), psbt.inputs[1].partial_sigs.len()), (1, 0));
}
