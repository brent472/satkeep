//! The real placeholder heir delay (61,320 blocks), mined in full.
//!
//! Takes several minutes, so it is ignored by a plain `cargo test`. Run it
//! with `cargo test -- --include-ignored` before opening a pull request.

mod common;

use common::{Holder, Node, Path, Savings};
use satkeep_savings::bitcoin::Amount;
use satkeep_savings::{DEFAULT_HEIR_DELAY, Delays, Error};

#[test]
#[ignore = "mines 61,320 blocks; run with --include-ignored"]
fn the_default_heir_delay_opens_at_exactly_61320_confirmations() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::with_heir(DEFAULT_HEIR_DELAY).unwrap());
    let funded_at = savings.fund(&node, Amount::from_btc(1.0).unwrap());
    node.close_faucet();
    let heir = u32::from(DEFAULT_HEIR_DELAY);

    savings.age_to(&node, funded_at, heir - 1);
    let early = savings.build(Path::Heir, &node.new_address());
    assert!(matches!(early, Err(Error::TooEarly { needed: 61_320, has: 61_319, .. })), "{:?}", early.err());

    savings.age_to(&node, funded_at, heir);
    let mut psbt = savings.build(Path::Heir, &node.new_address()).unwrap();
    savings.sign(&mut psbt, &[Holder::Heir]);
    let tx = savings.finalize(&psbt).unwrap();
    assert_eq!(savings.path_used(&tx), Path::Heir);
    node.broadcast_and_confirm(&tx);
}
