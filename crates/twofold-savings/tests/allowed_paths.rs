//! Every allowed way to spend Savings works on regtest: any 2 of 3, DEVICE
//! alone once its delay has passed, HEIR alone once its delay has passed.

mod common;

use common::{Holder, Node, Path, Savings};
use twofold_savings::Delays;
use twofold_savings::bitcoin::Amount;

fn short_delays() -> Delays {
    Delays::new(10, 20).unwrap()
}

fn one_btc() -> Amount {
    Amount::from_btc(1.0).unwrap()
}

/// Signs with `holders`, finalizes through the Twofold library, and checks
/// the network accepts and mines the spend.
fn spends(savings: &mut Savings, node: &Node, path: Path, holders: &[Holder]) {
    let mut psbt = savings.build(path, &node.new_address()).expect("Twofold builds the spend");
    savings.sign(&mut psbt, holders);
    let tx = savings.finalize(&psbt).unwrap_or_else(|e| panic!("Twofold refused {holders:?} through {path:?}: {e}"));
    assert_eq!(savings.path_used(&tx), path, "finalized through a different path");
    assert_eq!(node.accepts(&tx), Ok(()), "{holders:?} through {path:?}");
    node.broadcast_and_confirm(&tx);
    savings.sync(node);
    assert_eq!(savings.wallet.balance().total(), Amount::ZERO, "Savings should be empty");
}

#[test]
fn phone_and_device_can_spend() {
    let node = Node::start();
    let mut savings = Savings::new(short_delays());
    savings.fund(&node, one_btc());
    spends(&mut savings, &node, Path::TwoOfThree, &[Holder::Phone, Holder::Device]);
}

#[test]
fn phone_and_helper_can_spend() {
    let node = Node::start();
    let mut savings = Savings::new(short_delays());
    savings.fund(&node, one_btc());
    spends(&mut savings, &node, Path::TwoOfThree, &[Holder::Phone, Holder::Helper]);
}

#[test]
fn device_and_helper_can_spend() {
    let node = Node::start();
    let mut savings = Savings::new(short_delays());
    savings.fund(&node, one_btc());
    spends(&mut savings, &node, Path::TwoOfThree, &[Holder::Device, Holder::Helper]);
}

#[test]
fn device_alone_can_spend_once_its_delay_has_passed() {
    let node = Node::start();
    let mut savings = Savings::new(short_delays());
    let funded_at = savings.fund(&node, one_btc());
    savings.age_to(&node, funded_at, savings.delays().device().into());
    spends(&mut savings, &node, Path::Device, &[Holder::Device]);
}

#[test]
fn heir_alone_can_spend_once_its_delay_has_passed() {
    let node = Node::start();
    let mut savings = Savings::new(short_delays());
    let funded_at = savings.fund(&node, one_btc());
    savings.age_to(&node, funded_at, savings.delays().heir().into());
    spends(&mut savings, &node, Path::Heir, &[Holder::Heir]);
}
