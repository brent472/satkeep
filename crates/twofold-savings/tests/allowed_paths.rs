//! Every allowed way to spend Savings works on regtest: any 2 of 3 in both
//! shapes, and HEIR alone once its delay has passed in Savings with a
//! beneficiary.

mod common;

use common::{Holder, Node, Path, Savings};
use twofold_savings::Delays;
use twofold_savings::bitcoin::Amount;

fn with_heir() -> Delays {
    Delays::with_heir(20).unwrap()
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

const PAIRS: [[Holder; 2]; 3] =
    [[Holder::Phone, Holder::Device], [Holder::Phone, Holder::Helper], [Holder::Device, Holder::Helper]];

#[test]
fn any_two_of_three_can_spend_with_a_beneficiary() {
    let node = Node::start();
    for pair in PAIRS {
        let mut savings = Savings::new(with_heir());
        savings.fund(&node, one_btc());
        spends(&mut savings, &node, Path::TwoOfThree, &pair);
    }
}

#[test]
fn any_two_of_three_can_spend_without_a_beneficiary() {
    let node = Node::start();
    for pair in PAIRS {
        let mut savings = Savings::new(Delays::none());
        savings.fund(&node, one_btc());
        spends(&mut savings, &node, Path::TwoOfThree, &pair);
    }
}

#[test]
fn heir_alone_can_spend_once_its_delay_has_passed() {
    let node = Node::start();
    let mut savings = Savings::new(with_heir());
    let funded_at = savings.fund(&node, one_btc());
    savings.age_to(&node, funded_at, savings.heir_delay());
    spends(&mut savings, &node, Path::Heir, &[Holder::Heir]);
}
