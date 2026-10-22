//! Checks that the regtest harness itself works, so the spending-path tests
//! can trust it.

mod common;

use common::{Holder, Leaf, Node, Savings};
use twofold_savings::Delays;
use twofold_savings::bitcoin::Amount;

#[test]
fn funds_a_savings_address_and_sees_the_coin() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::new(10, 20).unwrap());
    savings.fund(&node, Amount::from_btc(1.0).unwrap());
    assert_eq!(savings.wallet.balance().confirmed, Amount::from_btc(1.0).unwrap());
    assert_eq!(savings.wallet.latest_checkpoint().height(), node.height());
}

#[test]
fn hand_built_witness_with_enough_signatures_is_accepted() {
    // If this fails, the forbidden-path tests' hand-built transactions can't
    // be trusted: a rejection might be the harness's fault, not the policy's.
    let node = Node::start();
    let mut savings = Savings::new(Delays::new(10, 20).unwrap());
    savings.fund(&node, Amount::from_btc(1.0).unwrap());
    let mut psbt = savings.spend_through(Leaf::TwoOfThree, &node.new_address());
    savings.sign(&mut psbt, &[Holder::Phone, Holder::Helper]);
    assert_eq!(node.accepts(&savings.forced(&psbt, Leaf::TwoOfThree)), Ok(()));
}

