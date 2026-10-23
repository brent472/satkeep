//! The real placeholder delays (39,420 and 61,320 blocks), mined in full.
//!
//! Takes several minutes, so it is ignored by a plain `cargo test`. Run it
//! with `cargo test -- --include-ignored` before opening a pull request.

mod common;

use common::{Holder, Node, Path, Savings};
use twofold_savings::bitcoin::Amount;
use twofold_savings::{DEFAULT_DEVICE_DELAY, DEFAULT_HEIR_DELAY, Delays, Error};

/// Builds, signs and finalizes through the library, and asks the node
/// whether it would accept the result. Nothing is broadcast.
fn try_spend(savings: &mut Savings, node: &Node, path: Path, holder: Holder) -> Result<(), String> {
    let mut psbt = savings.build(path, &node.new_address()).map_err(|e| e.to_string())?;
    savings.sign(&mut psbt, &[holder]);
    let tx = savings.finalize(&psbt).map_err(|e| e.to_string())?;
    node.accepts(&tx)
}

#[test]
#[ignore = "mines 61,320 blocks; run with --include-ignored"]
fn default_delays_open_at_exactly_39420_and_61320_confirmations() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::default());
    let funded_at = savings.fund(&node, Amount::from_btc(1.0).unwrap());
    node.close_faucet();
    let (device, heir) = (u32::from(DEFAULT_DEVICE_DELAY), u32::from(DEFAULT_HEIR_DELAY));

    savings.age_to(&node, funded_at, device - 1);
    let early = savings.build(Path::Device, &node.new_address());
    assert!(matches!(early, Err(Error::TooEarly { needed: 39_420, has: 39_419, .. })), "{:?}", early.err());

    savings.age_to(&node, funded_at, device);
    assert_eq!(try_spend(&mut savings, &node, Path::Device, Holder::Device), Ok(()));
    assert!(try_spend(&mut savings, &node, Path::Heir, Holder::Heir).is_err(), "heir still waits");

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
