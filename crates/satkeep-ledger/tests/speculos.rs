//! A Ledger as DEVICE, on regtest, against Ledger's Speculos emulator
//! running the Bitcoin Test app. Both Savings shapes: plain 2 of 3 (P2WSH),
//! and with a beneficiary (Taproot, HEIR alone after the heir delay).
//!
//! Allowed: the Ledger registers each wallet, and the Ledger plus PHONE, or
//! plus HELPER, spends it. Forbidden: the Ledger alone; the Ledger and HEIR
//! before the heir delay, or through 2 of 3, where HEIR doesn't count; the Ledger signing for a wallet it hasn't
//! registered, with an altered approval code, or for another wallet's
//! payment; registering a wallet whose DEVICE isn't this Ledger. Each spend
//! is refused by Satkeep and, built by hand, by the node.
//!
//! Needs Speculos on 127.0.0.1 (APDU port 9999, API port 5000) and
//! `BITCOIND_EXE`, so every test here is ignored by a plain `cargo test`.
//! CI's Speculos job runs them one at a time (one emulator):
//!
//! ```text
//! cargo test -p satkeep-ledger --test speculos --locked -- --ignored --test-threads=1
//! ```
//!
//! Speculos uses its public default seed, which holds nothing. PHONE,
//! HELPER and HEIR are fresh keys made in memory for each test. Nothing
//! prints a key or the Ledger's approval code: failures print redacted
//! screens and error messages, never a PSBT or a registration.

#[path = "../../satkeep-savings/tests/common/mod.rs"]
mod common;
#[path = "support/approver.rs"]
mod approver;

use approver::approving;
use common::{Holder, Node, Path, Savings};
use satkeep_ledger::{Error as LedgerError, Ledger, Registration};
use satkeep_savings::bitcoin::Amount;
use satkeep_savings::miniscript::descriptor::DescriptorPublicKey;
use satkeep_savings::{Delays, Error, HolderKey};

const HEIR_DELAY: u16 = 20;
const SCRIPT_FAILED: &str = "script-verify-flag-failed";
const TOO_EARLY: &str = "non-BIP68-final";

/// The emulated Ledger and its Savings key (approved on its screen).
fn ledger() -> (Ledger, DescriptorPublicKey) {
    let mut ledger = Ledger::connect_speculos().expect("Speculos on 127.0.0.1:9999");
    let key = approving(|| ledger.public_key()).expect("the Ledger's key, approved");
    assert!(key.to_string().contains("/87'/1'/0']tpub"), "a test-network key at m/87'/1'/0'");
    (ledger, key)
}

fn plain(key: &DescriptorPublicKey) -> Savings {
    Savings::with_device_key(key.clone(), Delays::none())
}

fn with_heir(key: &DescriptorPublicKey) -> Savings {
    Savings::with_device_key(key.clone(), Delays::with_heir(HEIR_DELAY).unwrap())
}

fn register(ledger: &mut Ledger, savings: &Savings) -> Registration {
    let registration = approving(|| ledger.register(&savings.policy)).expect("registered");
    assert!(registration.is_for(&savings.policy));
    registration
}

fn ledger_signs(ledger: &mut Ledger, registration: &Registration, psbt: &mut satkeep_savings::bitcoin::psbt::Psbt) {
    let added = approving(|| ledger.sign(registration, psbt)).expect("the Ledger signs");
    assert_eq!(added, psbt.inputs.len(), "one signature per coin");
}

#[test]
#[ignore = "needs Speculos and BITCOIND_EXE; run by CI's Speculos job"]
fn the_ledger_and_phone_or_helper_spend_both_shapes() {
    let (mut ledger, key) = ledger();
    for mut savings in [plain(&key), with_heir(&key)] {
        let node = Node::start();
        let registration = register(&mut ledger, &savings);
        for other in [Holder::Phone, Holder::Helper] {
            savings.fund(&node, Amount::from_btc(1.0).unwrap());
            let mut psbt = savings.build(Path::TwoOfThree, &node.new_address()).unwrap();
            ledger_signs(&mut ledger, &registration, &mut psbt);
            savings.sign(&mut psbt, &[other]);
            let tx = savings.finalize(&psbt).expect("the Ledger and one more key are enough");
            assert_eq!(savings.path_used(&tx), Path::TwoOfThree);
            node.broadcast_and_confirm(&tx);
            savings.sync(&node);
            assert_eq!(savings.wallet.balance().total(), Amount::ZERO, "{other:?}: everything was spent");
        }
    }
}

#[test]
#[ignore = "needs Speculos and BITCOIND_EXE; run by CI's Speculos job"]
fn the_ledger_alone_cant_spend_either_shape_even_after_the_heir_delay() {
    let (mut ledger, key) = ledger();
    for mut savings in [plain(&key), with_heir(&key)] {
        let node = Node::start();
        let registration = register(&mut ledger, &savings);
        let funded_at = savings.fund(&node, Amount::from_btc(1.0).unwrap());
        if savings.policy.has_heir() {
            savings.age_to(&node, funded_at, HEIR_DELAY.into());
        }
        for path in savings.paths() {
            let to = node.new_address();
            // Through Satkeep: the heir path's PSBT can be built once the
            // coin is old enough, but the Ledger's signature alone finalizes
            // neither.
            let mut psbt = savings.build(path, &to).unwrap();
            ledger_signs(&mut ledger, &registration, &mut psbt);
            let err = savings.finalize(&psbt).expect_err("the Ledger alone");
            assert!(matches!(err, Error::NotEnoughApprovals), "{path:?}: {err}");
            // By hand: the node refuses it too.
            let reason = node.accepts(&savings.forced(&psbt, path)).expect_err("the node refuses the Ledger alone");
            assert!(reason.contains(SCRIPT_FAILED), "{path:?}: {reason}");
        }
    }
}

#[test]
#[ignore = "needs Speculos and BITCOIND_EXE; run by CI's Speculos job"]
fn the_ledger_and_heir_cant_spend_before_the_heir_delay() {
    let (mut ledger, key) = ledger();
    let mut savings = with_heir(&key);
    let node = Node::start();
    let registration = register(&mut ledger, &savings);
    let funded_at = savings.fund(&node, Amount::from_btc(1.0).unwrap());
    savings.age_to(&node, funded_at, u32::from(HEIR_DELAY) - 1);

    // Satkeep won't even build the heir path's payment yet.
    let err = savings.build(Path::Heir, &node.new_address()).expect_err("one block early");
    assert!(matches!(err, Error::TooEarly { .. }), "{err}");

    // Built without Satkeep's checks and signed by both, it still fails:
    // Satkeep's finalizer and the node refuse it as too early. (The node
    // refuses any spend with the heir delay's sequence on a coin this young,
    // whichever leaf it uses.)
    let mut psbt = savings.build_unchecked(Path::Heir, &node.new_address());
    ledger_signs(&mut ledger, &registration, &mut psbt);
    savings.sign(&mut psbt, &[Holder::Heir]);
    let err = savings.finalize(&psbt).expect_err("HEIR and the Ledger, one block early");
    assert!(matches!(err, Error::TooEarly { .. }), "{err}");
    let reason = node.accepts(&savings.forced(&psbt, Path::Heir)).expect_err("heir leaf, early");
    assert!(reason.contains(TOO_EARLY), "{reason}");

    // And HEIR's signature never counts towards 2 of 3: with no delay at all,
    // the Ledger and HEIR are still one key short.
    let mut psbt = savings.build(Path::TwoOfThree, &node.new_address()).unwrap();
    ledger_signs(&mut ledger, &registration, &mut psbt);
    savings.sign(&mut psbt, &[Holder::Heir]);
    let err = savings.finalize(&psbt).expect_err("HEIR and the Ledger through 2 of 3");
    assert!(matches!(err, Error::NotEnoughApprovals), "{err}");
    let reason = node.accepts(&savings.forced(&psbt, Path::TwoOfThree)).expect_err("2-of-3 leaf, one key");
    assert!(reason.contains(SCRIPT_FAILED), "{reason}");
}

#[test]
#[ignore = "needs Speculos and BITCOIND_EXE; run by CI's Speculos job"]
fn the_ledger_signs_only_for_the_wallet_it_registered() {
    let (mut ledger, key) = ledger();
    let node = Node::start();
    let mut plain = plain(&key);
    let mut taproot = with_heir(&key);
    let plain_registration = register(&mut ledger, &plain);
    plain.fund(&node, Amount::from_btc(1.0).unwrap());
    taproot.fund(&node, Amount::from_btc(1.0).unwrap());
    let plain_psbt = plain.build(Path::TwoOfThree, &node.new_address()).unwrap();
    let taproot_psbt = taproot.build(Path::TwoOfThree, &node.new_address()).unwrap();

    // An altered approval code: the Ledger refuses, and the payment is
    // left as it was.
    let mut altered = plain_registration.clone();
    altered.hmac[0] ^= 1;
    let mut psbt = plain_psbt.clone();
    let err = approving(|| ledger.sign(&altered, &mut psbt)).expect_err("altered approval code");
    assert!(matches!(err, LedgerError::Refused(_)), "{err}");
    assert!(psbt == plain_psbt, "the payment was changed");

    // A wallet it never registered (the Taproot one, with no code at all
    // from this Ledger): borrowing the plain wallet's code fails.
    let unregistered = Registration { descriptor: taproot.policy.export_descriptor(), ..plain_registration.clone() };
    let mut psbt = taproot_psbt.clone();
    let err = approving(|| ledger.sign(&unregistered, &mut psbt)).expect_err("unregistered wallet");
    assert!(matches!(err, LedgerError::Refused(_)), "{err}");
    assert!(psbt == taproot_psbt, "the payment was changed");

    // Another wallet's payment under this wallet's registration: none of
    // its coins are this wallet's, so the Ledger signs none of them.
    let mut psbt = taproot_psbt.clone();
    let err = approving(|| ledger.sign(&plain_registration, &mut psbt)).expect_err("another wallet's payment");
    assert!(matches!(err, LedgerError::Refused(_) | LedgerError::SignedNothing), "{err}");
    assert!(psbt == taproot_psbt, "the payment was changed");

    // The registration that does fit still works afterwards.
    let mut psbt = plain_psbt.clone();
    ledger_signs(&mut ledger, &plain_registration, &mut psbt);
}

#[test]
#[ignore = "needs Speculos and BITCOIND_EXE; run by CI's Speculos job"]
fn a_wallet_whose_device_isnt_this_ledger_isnt_registered() {
    let (mut ledger, _key) = ledger();
    let stranger = HolderKey::generate().public_key(0);
    let savings = plain(&stranger);
    let err = ledger.register(&savings.policy).expect_err("DEVICE is someone else's key");
    assert!(matches!(err, LedgerError::NotThisLedger(_)), "{err}");
}
