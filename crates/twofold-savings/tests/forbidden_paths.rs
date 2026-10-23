//! Every forbidden way to spend Savings fails on regtest, at two levels:
//!
//! 1. The Twofold library refuses to build or to finalize the spend.
//! 2. A spend built by hand, sidestepping BDK and miniscript, is rejected by
//!    the node, so the rule is enforced by the network and not just by us.
//!
//! Each forbidden set of signers is tried through every leaf the wallet has,
//! in both shapes: Savings with a beneficiary (2 of 3, plus HEIR after the
//! heir delay) and plain Savings with none (2 of 3 only).

mod common;

use common::{Holder, Node, Path, Savings};
use twofold_savings::bitcoin::psbt::Psbt;
use twofold_savings::bitcoin::{Amount, Sequence, Transaction};
use twofold_savings::{Delays, Error};

/// The node's reason for rejecting a spend whose script fails.
const SCRIPT_FAILED: &str = "script-verify-flag-failed";
/// The node's reason for rejecting a spend whose coin is too young for the
/// sequence it uses (BIP 68).
const TOO_EARLY: &str = "non-BIP68-final";

const HEIR_DELAY: u16 = 20;

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
/// any of the wallet's paths right now, and returns how each refused.
fn refused_everywhere(savings: &mut Savings, node: &Node, holders: &[Holder]) -> Vec<Refusal> {
    savings
        .paths()
        .into_iter()
        .map(|path| {
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

fn setup(delays: Delays) -> (Node, Savings, u32) {
    let node = Node::start();
    let mut savings = Savings::new(delays);
    let funded_at = savings.fund(&node, Amount::from_btc(1.0).unwrap());
    (node, savings, funded_at)
}

fn with_heir() -> (Node, Savings, u32) {
    setup(Delays::with_heir(HEIR_DELAY).unwrap())
}

fn plain() -> (Node, Savings, u32) {
    setup(Delays::none())
}

// -------------------------------------------------- with a beneficiary ----

#[test]
fn phone_device_or_helper_alone_is_refused_even_after_the_heir_delay() {
    // DEVICE included: the card-only delayed path is gone, so DEVICE alone
    // never spends, however long the coin waits.
    let (node, mut savings, funded_at) = with_heir();
    for holder in [Holder::Phone, Holder::Device, Holder::Helper] {
        let young = refused_everywhere(&mut savings, &node, &[holder]);
        assert_not_enough_keys(refusal(&young, Path::TwoOfThree));
    }
    // Once the heir delay has passed, age is no excuse: only keys are missing.
    savings.age_to(&node, funded_at, savings.heir_delay() + 5);
    for holder in [Holder::Phone, Holder::Device, Holder::Helper] {
        refused_everywhere(&mut savings, &node, &[holder]).iter().for_each(assert_not_enough_keys);
    }
}

#[test]
fn heir_alone_is_refused_one_block_before_its_delay() {
    let (node, mut savings, funded_at) = with_heir();
    let delay = savings.heir_delay();
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
fn heir_with_any_one_other_key_is_refused_before_the_heir_delay() {
    // Helper–heir collusion, and HEIR with either of the owner's keys.
    // Checked straight away and at the last block before the heir delay.
    for other in [Holder::Helper, Holder::Phone, Holder::Device] {
        let (node, mut savings, funded_at) = with_heir();
        refused_everywhere(&mut savings, &node, &[Holder::Heir, other]);
        savings.age_to(&node, funded_at, savings.heir_delay() - 1);
        let refusals = refused_everywhere(&mut savings, &node, &[Holder::Heir, other]);
        assert_not_enough_keys(refusal(&refusals, Path::TwoOfThree));
        assert_too_early(refusal(&refusals, Path::Heir));
    }
}

#[test]
fn heir_cannot_skip_its_delay_by_signing_a_shorter_sequence() {
    // The coin is old enough for a 1-block sequence, but the leaf demands
    // the full heir delay, so the script itself fails.
    let (node, mut savings, funded_at) = with_heir();
    savings.age_to(&node, funded_at, savings.heir_delay() - 1);
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
    let (node, mut savings, funded_at) = with_heir();
    savings.age_to(&node, funded_at, savings.heir_delay() - 1);
    let mut psbt = savings.build_unchecked(Path::Heir, &node.new_address());
    savings.sign(&mut psbt, &[Holder::Heir]);
    let tx = savings.bdk_only_finalize(&psbt).expect("BDK finalizes it");
    assert!(node.accepts(&tx).unwrap_err().contains(TOO_EARLY));
    assert!(matches!(savings.finalize(&psbt), Err(Error::TooEarly { needed: HEIR_DELAY, has: 19, .. })));
}

#[test]
fn library_refuses_to_build_an_heir_spend_one_block_early() {
    let (node, mut savings, funded_at) = with_heir();
    let delay = savings.heir_delay();
    savings.age_to(&node, funded_at, delay - 1);
    let early = savings.build(Path::Heir, &node.new_address());
    assert!(matches!(early, Err(Error::TooEarly { input: 0, needed: HEIR_DELAY, has: 19 })), "{:?}", early.err());
    savings.age_to(&node, funded_at, delay);
    assert!(savings.build(Path::Heir, &node.new_address()).is_ok());
}

#[test]
fn bdk_alone_would_finalize_a_delayed_leaf_whose_sequence_has_no_delay() {
    // The same OR, the other way round: once the coin is old enough, BDK
    // uses the heir leaf even though the input's sequence carries no delay,
    // so CSV fails on the network. Twofold finalizes with miniscript's
    // PSBT-only finalizer instead, which refuses.
    let (node, mut savings, funded_at) = with_heir();
    savings.age_to(&node, funded_at, savings.heir_delay());
    let mut psbt = savings.build_unchecked(Path::TwoOfThree, &node.new_address());
    savings.sign(&mut psbt, &[Holder::Heir]);
    let tx = savings.bdk_only_finalize(&psbt).expect("BDK finalizes it");
    assert_eq!(savings.path_used(&tx), Path::Heir);
    assert!(node.accepts(&tx).unwrap_err().contains(SCRIPT_FAILED));
    assert!(matches!(savings.finalize(&psbt), Err(Error::NotEnoughApprovals)));
}

// ------------------------------------------------- no beneficiary yet ----

#[test]
fn without_a_beneficiary_no_single_key_ever_spends() {
    // Plain 2 of 3 has no delayed path: no single key spends, however long
    // the coin waits. HEIR here is a stranger whose key isn't in the wallet.
    let (node, mut savings, funded_at) = plain();
    assert_eq!(savings.paths(), [Path::TwoOfThree]);
    let all = [Holder::Phone, Holder::Device, Holder::Helper, Holder::Heir];
    for holder in all {
        refused_everywhere(&mut savings, &node, &[holder]).iter().for_each(assert_not_enough_keys);
    }
    savings.age_to(&node, funded_at, u32::from(HEIR_DELAY) * 2);
    for holder in all {
        refused_everywhere(&mut savings, &node, &[holder]).iter().for_each(assert_not_enough_keys);
    }
    // A stranger plus one real key is still only one real key.
    for holder in [Holder::Phone, Holder::Device, Holder::Helper] {
        refused_everywhere(&mut savings, &node, &[Holder::Heir, holder]).iter().for_each(assert_not_enough_keys);
    }
}

#[test]
fn without_a_beneficiary_the_library_refuses_an_heir_path() {
    let (node, mut savings, _) = plain();
    assert!(matches!(savings.build(Path::Heir, &node.new_address()), Err(Error::NoHeirPath)));
}

#[test]
fn without_a_beneficiary_a_long_sequence_buys_nothing() {
    // One key, with a long relative delay on an old enough coin: there's no
    // leaf that delay could open, so the 2-of-3 script still fails.
    let (node, mut savings, funded_at) = plain();
    savings.age_to(&node, funded_at, u32::from(HEIR_DELAY));
    let a = attempt_edited(&mut savings, &node, Path::TwoOfThree, &[Holder::Device], |psbt| {
        psbt.unsigned_tx.input[0].sequence = Sequence::from_height(HEIR_DELAY);
    });
    assert!(matches!(a.twofold, Err(Error::NotEnoughApprovals)), "{:?}", a.twofold.err());
    assert!(a.node.unwrap_err().contains(SCRIPT_FAILED));
}
