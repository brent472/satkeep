//! Every forbidden way to spend Savings fails on regtest, at two levels:
//!
//! 1. The Twofold library refuses to build or to finalize the spend.
//! 2. A spend built by hand, sidestepping BDK and miniscript, is rejected by
//!    the node, so the rule is enforced by the network and not just by us.
//!
//! Each forbidden set of signers is tried through all three leaves.

mod common;

use common::{Holder, Node, Path, Savings};
use twofold_savings::bitcoin::psbt::Psbt;
use twofold_savings::bitcoin::{Amount, Sequence, Transaction};
use twofold_savings::{Delays, Error};

const PATHS: [Path; 3] = [Path::TwoOfThree, Path::Device, Path::Heir];

/// The node's reason for rejecting a spend whose script fails.
const SCRIPT_FAILED: &str = "script-verify-flag-failed";
/// The node's reason for rejecting a spend whose coin is too young for the
/// sequence it uses (BIP 68).
const TOO_EARLY: &str = "non-BIP68-final";

struct Attempt {
    twofold: Result<Transaction, Error>,
    node: Result<(), String>,
}

/// Has `holders` try to spend through `path`, two ways: through the Twofold
/// library (build, sign, finalize), and by hand from a PSBT built with BDK
/// directly, which the node then judges.
fn attempt(savings: &mut Savings, node: &Node, path: Path, holders: &[Holder]) -> Attempt {
    let to = node.new_address();
    let twofold = savings.build(path, &to).and_then(|mut psbt| {
        savings.sign(&mut psbt, holders);
        savings.finalize(&psbt)
    });
    let mut raw = savings.build_unchecked(path, &to);
    savings.sign(&mut raw, holders);
    Attempt { twofold, node: node.accepts(&savings.forced(&raw, path)) }
}

/// Like [`attempt`], but `edit` changes the unsigned transaction before
/// anyone signs. The library can't build such a PSBT, so only its
/// finalizer is tried.
fn attempt_edited(
    savings: &mut Savings,
    node: &Node,
    path: Path,
    holders: &[Holder],
    edit: impl FnOnce(&mut Psbt),
) -> Attempt {
    let mut raw = savings.build_unchecked(path, &node.new_address());
    edit(&mut raw);
    savings.sign(&mut raw, holders);
    Attempt { twofold: savings.finalize(&raw), node: node.accepts(&savings.forced(&raw, path)) }
}

/// How each side refused one path.
struct Refusal {
    path: Path,
    twofold: Error,
    node: String,
}

/// Asserts that neither Twofold nor the node lets `holders` spend through
/// any path right now, and returns how each refused.
fn refused_everywhere(savings: &mut Savings, node: &Node, holders: &[Holder]) -> Vec<Refusal> {
    PATHS
        .iter()
        .map(|&path| {
            let a = attempt(savings, node, path, holders);
            let twofold = a.twofold.err().unwrap_or_else(|| panic!("Twofold finalized {holders:?} through {path:?}"));
            let node = a.node.expect_err(&format!("node accepted {holders:?} through {path:?}"));
            Refusal { path, twofold, node }
        })
        .collect()
}

fn refusal(refusals: &[Refusal], path: Path) -> &Refusal {
    refusals.iter().find(|r| r.path == path).unwrap()
}

/// The coin was too young: Twofold says so, and so does the node (BIP 68).
fn assert_too_early(r: &Refusal) {
    assert!(matches!(r.twofold, Error::TooEarly { .. }), "{:?}: {}", r.path, r.twofold);
    assert!(r.node.contains(TOO_EARLY), "{:?}: {}", r.path, r.node);
}

/// The coin was old enough, but the signatures don't satisfy the leaf.
fn assert_not_enough_keys(r: &Refusal) {
    assert!(matches!(r.twofold, Error::NotEnoughApprovals), "{:?}: {}", r.path, r.twofold);
    assert!(r.node.contains(SCRIPT_FAILED), "{:?}: {}", r.path, r.node);
}

fn setup() -> (Node, Savings, u32) {
    let node = Node::start();
    let mut savings = Savings::new(Delays::new(10, 20).unwrap());
    let funded_at = savings.fund(&node, Amount::from_btc(1.0).unwrap());
    (node, savings, funded_at)
}

#[test]
fn phone_alone_is_refused_even_after_both_delays() {
    let (node, mut savings, funded_at) = setup();
    let young = refused_everywhere(&mut savings, &node, &[Holder::Phone]);
    assert_not_enough_keys(refusal(&young, Path::TwoOfThree));
    // Once both delays have passed, age is no excuse: only keys are missing.
    savings.age_to(&node, funded_at, savings.delays().heir().into());
    refused_everywhere(&mut savings, &node, &[Holder::Phone]).iter().for_each(assert_not_enough_keys);
}

#[test]
fn helper_alone_is_refused_even_after_both_delays() {
    let (node, mut savings, funded_at) = setup();
    let young = refused_everywhere(&mut savings, &node, &[Holder::Helper]);
    assert_not_enough_keys(refusal(&young, Path::TwoOfThree));
    // Once both delays have passed, age is no excuse: only keys are missing.
    savings.age_to(&node, funded_at, savings.delays().heir().into());
    refused_everywhere(&mut savings, &node, &[Holder::Helper]).iter().for_each(assert_not_enough_keys);
}

#[test]
fn device_with_both_its_keys_is_refused_one_block_before_its_delay() {
    // The DEVICE holder signs with DEVICE (2-of-3 leaf) and DEVICE_R (its
    // delayed leaf) alike.
    let (node, mut savings, funded_at) = setup();
    let delay = u32::from(savings.delays().device());
    savings.age_to(&node, funded_at, delay - 1);
    let refusals = refused_everywhere(&mut savings, &node, &[Holder::Device]);
    assert_not_enough_keys(refusal(&refusals, Path::TwoOfThree));
    assert_too_early(refusal(&refusals, Path::Device));

    // One block later the same spend goes through.
    savings.age_to(&node, funded_at, delay);
    let a = attempt(&mut savings, &node, Path::Device, &[Holder::Device]);
    assert!(a.twofold.is_ok());
    assert_eq!(a.node, Ok(()));
}

#[test]
fn heir_alone_is_refused_one_block_before_its_delay() {
    let (node, mut savings, funded_at) = setup();
    let delay = u32::from(savings.delays().heir());
    savings.age_to(&node, funded_at, delay - 1);
    let refusals = refused_everywhere(&mut savings, &node, &[Holder::Heir]);
    assert_not_enough_keys(refusal(&refusals, Path::TwoOfThree));
    assert_too_early(refusal(&refusals, Path::Heir));

    savings.age_to(&node, funded_at, delay);
    let a = attempt(&mut savings, &node, Path::Heir, &[Holder::Heir]);
    assert!(a.twofold.is_ok());
    assert_eq!(a.node, Ok(()));
}

#[test]
fn heir_and_helper_together_are_refused_before_the_heir_delay() {
    // Helper–heir collusion. Checked straight away and at the last block
    // before the heir delay (by which time the device delay has passed).
    let (node, mut savings, funded_at) = setup();
    refused_everywhere(&mut savings, &node, &[Holder::Heir, Holder::Helper]);
    savings.age_to(&node, funded_at, u32::from(savings.delays().heir()) - 1);
    let refusals = refused_everywhere(&mut savings, &node, &[Holder::Heir, Holder::Helper]);
    assert_not_enough_keys(refusal(&refusals, Path::TwoOfThree));
    assert_not_enough_keys(refusal(&refusals, Path::Device));
    assert_too_early(refusal(&refusals, Path::Heir));
}

#[test]
fn heir_and_phone_together_are_refused_before_the_heir_delay() {
    let (node, mut savings, funded_at) = setup();
    refused_everywhere(&mut savings, &node, &[Holder::Heir, Holder::Phone]);
    savings.age_to(&node, funded_at, u32::from(savings.delays().heir()) - 1);
    let refusals = refused_everywhere(&mut savings, &node, &[Holder::Heir, Holder::Phone]);
    assert_not_enough_keys(refusal(&refusals, Path::TwoOfThree));
    assert_not_enough_keys(refusal(&refusals, Path::Device));
    assert_too_early(refusal(&refusals, Path::Heir));
}

#[test]
fn heir_and_device_together_are_refused_before_the_device_delay() {
    // From the device delay on, DEVICE can spend alone, so this pair is only
    // forbidden before it.
    let (node, mut savings, funded_at) = setup();
    refused_everywhere(&mut savings, &node, &[Holder::Heir, Holder::Device]);
    savings.age_to(&node, funded_at, u32::from(savings.delays().device()) - 1);
    let refusals = refused_everywhere(&mut savings, &node, &[Holder::Heir, Holder::Device]);
    assert_not_enough_keys(refusal(&refusals, Path::TwoOfThree));
    assert_too_early(refusal(&refusals, Path::Device));
    assert_too_early(refusal(&refusals, Path::Heir));
}

#[test]
fn device_cannot_skip_its_delay_by_signing_a_shorter_sequence() {
    // The coin is old enough for a 1-block sequence, but the leaf demands
    // the full device delay, so the script itself fails.
    let (node, mut savings, funded_at) = setup();
    savings.age_to(&node, funded_at, u32::from(savings.delays().device()) - 1);
    let a = attempt_edited(&mut savings, &node, Path::Device, &[Holder::Device], |psbt| {
        psbt.unsigned_tx.input[0].sequence = Sequence::from_height(1);
    });
    assert!(matches!(a.twofold, Err(Error::NotEnoughApprovals)), "{:?}", a.twofold.err());
    let reason = a.node.unwrap_err();
    assert!(reason.contains(SCRIPT_FAILED) && reason.contains("Locktime requirement not satisfied"), "{reason}");
}

#[test]
fn heir_cannot_skip_its_delay_by_signing_a_shorter_sequence() {
    let (node, mut savings, funded_at) = setup();
    savings.age_to(&node, funded_at, u32::from(savings.delays().heir()) - 1);
    let a = attempt_edited(&mut savings, &node, Path::Heir, &[Holder::Heir], |psbt| {
        psbt.unsigned_tx.input[0].sequence = Sequence::from_height(1);
    });
    assert!(matches!(a.twofold, Err(Error::NotEnoughApprovals)), "{:?}", a.twofold.err());
    let reason = a.node.unwrap_err();
    assert!(reason.contains(SCRIPT_FAILED) && reason.contains("Locktime requirement not satisfied"), "{reason}");
}

#[test]
fn bdk_alone_would_finalize_an_early_delayed_spend() {
    // Pins down why Twofold checks coin ages itself (see spend::check_delays).
    // BDK accepts a sequence matching the leaf's delay without checking the
    // coin's age; only the network stops it. If this test starts failing,
    // BDK has changed and the README note about it should be updated.
    let (node, mut savings, funded_at) = setup();
    savings.age_to(&node, funded_at, u32::from(savings.delays().device()) - 1);
    let mut psbt = savings.build_unchecked(Path::Device, &node.new_address());
    savings.sign(&mut psbt, &[Holder::Device]);
    let tx = savings.bdk_only_finalize(&psbt).expect("BDK finalizes it");
    assert!(node.accepts(&tx).unwrap_err().contains(TOO_EARLY));
    assert!(matches!(savings.finalize(&psbt), Err(Error::TooEarly { needed: 10, has: 9, .. })));
}

#[test]
fn library_refuses_to_build_a_device_spend_one_block_early() {
    let (node, mut savings, funded_at) = setup();
    let delay = u32::from(savings.delays().device());
    savings.age_to(&node, funded_at, delay - 1);
    let early = savings.build(Path::Device, &node.new_address());
    assert!(matches!(early, Err(Error::TooEarly { input: 0, needed: 10, has: 9 })), "{:?}", early.err());
    savings.age_to(&node, funded_at, delay);
    assert!(savings.build(Path::Device, &node.new_address()).is_ok());
}

#[test]
fn library_refuses_to_build_an_heir_spend_one_block_early() {
    let (node, mut savings, funded_at) = setup();
    let delay = u32::from(savings.delays().heir());
    savings.age_to(&node, funded_at, delay - 1);
    let early = savings.build(Path::Heir, &node.new_address());
    assert!(matches!(early, Err(Error::TooEarly { input: 0, needed: 20, has: 19 })), "{:?}", early.err());
    savings.age_to(&node, funded_at, delay);
    assert!(savings.build(Path::Heir, &node.new_address()).is_ok());
}
