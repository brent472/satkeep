//! The check-in on regtest: it restarts both delays, must include every
//! coin, and can only go through the 2-of-3 path.

mod common;

use common::{FEE_RATE, Holder, Node, Path, Savings};
use bdk_wallet::KeychainKind;
use bdk_wallet::descriptor::policy::Condition;
use twofold_savings::bitcoin::{Amount, Transaction};
use twofold_savings::checkin::{build_checkin, check_checkin, is_checkin};
use twofold_savings::{Delays, Error, spend};

fn btc(n: f64) -> Amount {
    Amount::from_btc(n).unwrap()
}

/// Builds, signs as PHONE + HELPER, finalizes, broadcasts and confirms a
/// check-in. Returns it and the height the new coin confirmed at.
fn check_in(savings: &mut Savings, node: &Node) -> (Transaction, u32) {
    let delays = savings.delays();
    let mut psbt = build_checkin(&mut savings.wallet, delays, FEE_RATE).expect("build check-in");
    savings.sign(&mut psbt, &[Holder::Phone, Holder::Helper]);
    check_checkin(&savings.wallet, &psbt, delays).unwrap();
    let tx = savings.finalize(&psbt).unwrap();
    node.broadcast_and_confirm(&tx);
    savings.sync(node);
    (tx, node.height())
}

#[test]
fn a_check_in_restarts_both_delays_for_the_new_coin() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::new(10, 20).unwrap());
    let first = savings.fund(&node, btc(1.0));
    savings.fund(&node, btc(0.5));
    // One block before the first coin's device path would open.
    savings.age_to(&node, first, 9);

    let (tx, checked_in_at) = check_in(&mut savings, &node);
    assert_eq!(tx.input.len(), 2, "both coins went in");
    assert_eq!(tx.output.len(), 1, "one new coin came out");
    assert_eq!(savings.path_used(&tx), Path::TwoOfThree);
    assert_eq!(savings.wallet.list_unspent().count(), 1);

    // The first coin's device path would be open now. The new coin's isn't:
    // both delays start again from its own confirmation.
    let to = node.new_address();
    assert!(matches!(savings.build(Path::Device, &to), Err(Error::TooEarly { needed: 10, has: 1, .. })));
    assert!(matches!(savings.build(Path::Heir, &to), Err(Error::TooEarly { needed: 20, has: 1, .. })));
    let mut early = savings.build_unchecked(Path::Device, &to);
    savings.sign(&mut early, &[Holder::Device]);
    assert!(node.accepts(&savings.forced(&early, Path::Device)).unwrap_err().contains("non-BIP68-final"));

    // DEVICE alone opens at exactly 10 confirmations of the new coin.
    savings.age_to(&node, checked_in_at, 9);
    assert!(matches!(savings.build(Path::Device, &to), Err(Error::TooEarly { has: 9, .. })));
    savings.age_to(&node, checked_in_at, 10);
    let mut device = savings.build(Path::Device, &to).unwrap();
    savings.sign(&mut device, &[Holder::Device]);
    assert_eq!(node.accepts(&savings.finalize(&device).unwrap()), Ok(()));

    // HEIR alone at exactly 20.
    savings.age_to(&node, checked_in_at, 19);
    assert!(matches!(savings.build(Path::Heir, &to), Err(Error::TooEarly { has: 19, .. })));
    savings.age_to(&node, checked_in_at, 20);
    let mut heir = savings.build(Path::Heir, &to).unwrap();
    savings.sign(&mut heir, &[Holder::Heir]);
    assert_eq!(node.accepts(&savings.finalize(&heir).unwrap()), Ok(()));
}

#[test]
fn a_check_in_that_leaves_out_a_coin_is_refused() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::new(10, 20).unwrap());
    savings.fund(&node, btc(1.0));
    let delays = savings.delays();
    let stale = build_checkin(&mut savings.wallet, delays, FEE_RATE).unwrap();
    assert!(is_checkin(&stale));

    // A coin arrives after the check-in was built: the old one now leaves it out.
    savings.fund(&node, btc(0.5));
    let new_coin = savings.wallet.list_unspent().find(|u| u.txout.value == btc(0.5)).unwrap().outpoint;
    match check_checkin(&savings.wallet, &stale, delays) {
        Err(Error::CheckinMissesCoin { coin }) => assert_eq!(coin, new_coin),
        other => panic!("expected CheckinMissesCoin, got {other:?}"),
    }

    // A hand-made "check-in" that picks one coin is refused the same way.
    let full = build_checkin(&mut savings.wallet, delays, FEE_RATE).unwrap();
    let back = savings.wallet.reveal_next_address(KeychainKind::Internal).address.script_pubkey();
    let mut builder = savings.wallet.build_tx();
    builder
        .add_utxo(new_coin)
        .unwrap()
        .manually_selected_only()
        .drain_to(back)
        .fee_rate(FEE_RATE)
        .set_condition(Condition { csv: None, timelock: None });
    let mut partial = builder.finish().unwrap();
    partial.proprietary = full.proprietary.clone();
    assert!(matches!(check_checkin(&savings.wallet, &partial, delays), Err(Error::CheckinMissesCoin { .. })));

    // The rebuilt one includes both and passes.
    assert_eq!(full.unsigned_tx.input.len(), 2);
    assert!(check_checkin(&savings.wallet, &full, delays).is_ok());
}

#[test]
fn a_check_in_must_pay_only_back_into_savings() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::new(10, 20).unwrap());
    savings.fund(&node, btc(1.0));
    let delays = savings.delays();
    let full = build_checkin(&mut savings.wallet, delays, FEE_RATE).unwrap();
    let mut out = savings.build_unchecked(Path::TwoOfThree, &node.new_address());
    out.proprietary = full.proprietary.clone();
    assert!(matches!(check_checkin(&savings.wallet, &out, delays), Err(Error::CheckinPaysOut)));
    // And a payment that isn't marked as a check-in isn't one.
    let plain = savings.build_unchecked(Path::TwoOfThree, &node.new_address());
    assert!(matches!(check_checkin(&savings.wallet, &plain, delays), Err(Error::NotACheckin)));
}

#[test]
fn a_check_in_can_only_use_the_two_of_three_path() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::new(10, 20).unwrap());
    let funded = savings.fund(&node, btc(1.0));
    savings.age_to(&node, funded, 20); // both delayed paths open
    let delays = savings.delays();
    let full = build_checkin(&mut savings.wallet, delays, FEE_RATE).unwrap();
    assert_eq!(spend::path_of(&full, delays), Some(Path::TwoOfThree));

    // A self-send through a delayed path, marked as a check-in, is refused.
    let own = savings.wallet.reveal_next_address(KeychainKind::External).address;
    for path in [Path::Device, Path::Heir] {
        let mut delayed = savings.build_unchecked(path, &own);
        delayed.proprietary = full.proprietary.clone();
        assert!(matches!(check_checkin(&savings.wallet, &delayed, delays), Err(Error::CheckinUsesDelayedPath)));
    }

    // The check-in itself carries no delay in its sequence, so even with the
    // delayed paths open, DEVICE or HEIR alone can't approve it. The network
    // agrees: a hand-built delayed-leaf spend of it fails on the script.
    for (holder, path) in [(Holder::Device, Path::Device), (Holder::Heir, Path::Heir)] {
        let mut alone = full.clone();
        savings.sign(&mut alone, &[holder]);
        assert!(matches!(savings.finalize(&alone), Err(Error::NotEnoughApprovals)), "{holder:?}");
        let reason = node.accepts(&savings.forced(&alone, path)).unwrap_err();
        assert!(reason.contains("script-verify-flag-failed"), "{holder:?}: {reason}");
    }
}

#[test]
fn an_immature_coinbase_coin_holds_up_the_check_in() {
    // Only on test networks, where Savings can be funded by mining: a block
    // reward can't be spent for 100 blocks, so a check-in can't include it
    // and is refused until it can.
    use bdk_bitcoind_rpc::bitcoincore_rpc::RpcApi;
    use serde_json::{Value, json};
    let node = Node::start();
    let mut savings = Savings::new(Delays::new(10, 20).unwrap());
    savings.fund(&node, btc(1.0));
    let address = savings.wallet.reveal_next_address(KeychainKind::External).address;
    node.rpc().call::<Value>("generatetoaddress", &[json!(1), json!(address.to_string())]).unwrap();
    savings.sync(&node);
    let delays = savings.delays();
    assert!(matches!(build_checkin(&mut savings.wallet, delays, FEE_RATE), Err(Error::CheckinMissesCoin { .. })));

    node.mine(99); // the reward now has 100 confirmations and can be spent
    savings.sync(&node);
    let (tx, _) = check_in(&mut savings, &node);
    assert_eq!(tx.input.len(), 2);
}
