//! Every allowed way to spend Savings works on regtest: any 2 of 3, DEVICE
//! alone once its delay has passed, HEIR alone once its delay has passed.

mod common;

use common::{Holder, Leaf, Node, Savings};
use twofold_savings::Delays;
use twofold_savings::bitcoin::Amount;

fn short_delays() -> Delays {
    Delays::new(10, 20).unwrap()
}

fn one_btc() -> Amount {
    Amount::from_btc(1.0).unwrap()
}

/// Signs with `holders`, lets BDK finalize, and checks the network accepts
/// and mines the spend.
fn spends(savings: &mut Savings, node: &Node, leaf: Leaf, holders: &[Holder]) {
    let mut psbt = savings.spend_through(leaf, &node.new_address());
    savings.sign(&mut psbt, holders);
    let tx = savings.bdk_finalize(&psbt).unwrap_or_else(|| panic!("BDK refused {holders:?} through {leaf:?}"));
    assert_eq!(savings.leaf_used(&tx), leaf, "BDK chose a different leaf");
    assert_eq!(node.accepts(&tx), Ok(()), "{holders:?} through {leaf:?}");
    node.broadcast_and_confirm(&tx);
    savings.sync(node);
    assert_eq!(savings.wallet.balance().total(), Amount::ZERO, "Savings should be empty");
}

#[test]
fn phone_and_device_can_spend() {
    let node = Node::start();
    let mut savings = Savings::new(short_delays());
    savings.fund(&node, one_btc());
    spends(&mut savings, &node, Leaf::TwoOfThree, &[Holder::Phone, Holder::Device]);
}

#[test]
fn phone_and_helper_can_spend() {
    let node = Node::start();
    let mut savings = Savings::new(short_delays());
    savings.fund(&node, one_btc());
    spends(&mut savings, &node, Leaf::TwoOfThree, &[Holder::Phone, Holder::Helper]);
}

#[test]
fn device_and_helper_can_spend() {
    let node = Node::start();
    let mut savings = Savings::new(short_delays());
    savings.fund(&node, one_btc());
    spends(&mut savings, &node, Leaf::TwoOfThree, &[Holder::Device, Holder::Helper]);
}

#[test]
fn device_alone_can_spend_once_its_delay_has_passed() {
    let node = Node::start();
    let mut savings = Savings::new(short_delays());
    let funded_at = savings.fund(&node, one_btc());
    savings.age_to(&node, funded_at, savings.delays().device().into());
    spends(&mut savings, &node, Leaf::Device, &[Holder::Device]);
}

#[test]
fn heir_alone_can_spend_once_its_delay_has_passed() {
    let node = Node::start();
    let mut savings = Savings::new(short_delays());
    let funded_at = savings.fund(&node, one_btc());
    savings.age_to(&node, funded_at, savings.delays().heir().into());
    spends(&mut savings, &node, Leaf::Heir, &[Holder::Heir]);
}
