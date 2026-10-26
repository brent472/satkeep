//! The check-in on regtest: it restarts the heir delay, must include every
//! coin, and can only go through the 2-of-3 path.

mod common;

use common::{FEE_RATE, Holder, Node, Path, Savings};
use bdk_wallet::KeychainKind;
use bdk_wallet::descriptor::policy::Condition;
use satkeep_savings::bitcoin::{Amount, FeeRate, Transaction};
use satkeep_savings::checkin::{build_checkin, check_checkin, is_checkin};
use satkeep_savings::spend::FeeCeiling;
use satkeep_savings::{Delays, Error, spend};

fn btc(n: f64) -> Amount {
    Amount::from_btc(n).unwrap()
}

/// Builds, signs as PHONE + HELPER, finalizes, broadcasts and confirms a
/// check-in. Returns it and the height the new coin confirmed at.
fn check_in(savings: &mut Savings, node: &Node) -> (Transaction, u32) {
    let delays = savings.delays();
    let mut psbt = build_checkin(&mut savings.wallet, delays, FEE_RATE, FeeCeiling::DEFAULT).expect("build check-in");
    savings.sign(&mut psbt, &[Holder::Phone, Holder::Helper]);
    check_checkin(&savings.wallet, &psbt, delays, FeeCeiling::DEFAULT).unwrap();
    let tx = savings.finalize(&psbt).unwrap();
    node.broadcast_and_confirm(&tx);
    savings.sync(node);
    (tx, node.height())
}

#[test]
fn a_check_in_restarts_the_heir_delay_for_the_new_coin() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::with_heir(20).unwrap());
    let first = savings.fund(&node, btc(1.0));
    savings.fund(&node, btc(0.5));
    // One block before the first coin's heir path would open.
    savings.age_to(&node, first, 19);

    let (tx, checked_in_at) = check_in(&mut savings, &node);
    assert_eq!(tx.input.len(), 2, "both coins went in");
    assert_eq!(tx.output.len(), 1, "one new coin came out");
    assert_eq!(savings.path_used(&tx), Path::TwoOfThree);
    assert_eq!(savings.wallet.list_unspent().count(), 1);

    // The first coin's heir path would be open now. The new coin's isn't:
    // the delay starts again from its own confirmation.
    let to = node.new_address();
    assert!(matches!(savings.build(Path::Heir, &to), Err(Error::TooEarly { needed: 20, has: 1, .. })));
    let mut early = savings.build_unchecked(Path::Heir, &to);
    savings.sign(&mut early, &[Holder::Heir]);
    assert!(node.accepts(&savings.forced(&early, Path::Heir)).unwrap_err().contains("non-BIP68-final"));

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
    let mut savings = Savings::new(Delays::with_heir(20).unwrap());
    savings.fund(&node, btc(1.0));
    let delays = savings.delays();
    let stale = build_checkin(&mut savings.wallet, delays, FEE_RATE, FeeCeiling::DEFAULT).unwrap();
    assert!(is_checkin(&stale));

    // A coin arrives after the check-in was built: the old one now leaves it out.
    savings.fund(&node, btc(0.5));
    let new_coin = savings.wallet.list_unspent().find(|u| u.txout.value == btc(0.5)).unwrap().outpoint;
    match check_checkin(&savings.wallet, &stale, delays, FeeCeiling::DEFAULT) {
        Err(Error::CheckinMissesCoin { coin }) => assert_eq!(coin, new_coin),
        other => panic!("expected CheckinMissesCoin, got {other:?}"),
    }

    // A hand-made "check-in" that picks one coin is refused the same way.
    let full = build_checkin(&mut savings.wallet, delays, FEE_RATE, FeeCeiling::DEFAULT).unwrap();
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
    assert!(matches!(check_checkin(&savings.wallet, &partial, delays, FeeCeiling::DEFAULT), Err(Error::CheckinMissesCoin { .. })));

    // The rebuilt one includes both and passes.
    assert_eq!(full.unsigned_tx.input.len(), 2);
    assert!(check_checkin(&savings.wallet, &full, delays, FeeCeiling::DEFAULT).is_ok());
}

#[test]
fn a_check_in_must_pay_only_back_into_savings() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::with_heir(20).unwrap());
    savings.fund(&node, btc(1.0));
    let delays = savings.delays();
    let full = build_checkin(&mut savings.wallet, delays, FEE_RATE, FeeCeiling::DEFAULT).unwrap();
    let mut out = savings.build_unchecked(Path::TwoOfThree, &node.new_address());
    out.proprietary = full.proprietary.clone();
    assert!(matches!(check_checkin(&savings.wallet, &out, delays, FeeCeiling::DEFAULT), Err(Error::CheckinPaysOut)));
    // And a payment that isn't marked as a check-in isn't one.
    let plain = savings.build_unchecked(Path::TwoOfThree, &node.new_address());
    assert!(matches!(check_checkin(&savings.wallet, &plain, delays, FeeCeiling::DEFAULT), Err(Error::NotACheckin)));
}

#[test]
fn a_check_in_can_only_use_the_two_of_three_path() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::with_heir(20).unwrap());
    let funded = savings.fund(&node, btc(1.0));
    savings.age_to(&node, funded, 20); // the heir path is open
    let delays = savings.delays();
    let full = build_checkin(&mut savings.wallet, delays, FEE_RATE, FeeCeiling::DEFAULT).unwrap();
    assert_eq!(spend::path_of(&full, delays), Some(Path::TwoOfThree));

    // A self-send through the heir path, marked as a check-in, is refused.
    let own = savings.wallet.reveal_next_address(KeychainKind::External).address;
    let mut delayed = savings.build_unchecked(Path::Heir, &own);
    delayed.proprietary = full.proprietary.clone();
    assert!(matches!(check_checkin(&savings.wallet, &delayed, delays, FeeCeiling::DEFAULT), Err(Error::CheckinUsesDelayedPath)));

    // The check-in itself carries no delay in its sequence, so even with the
    // heir path open, HEIR alone can't approve it, and nor can DEVICE alone.
    // The network agrees: a hand-built spend of it fails on the script.
    for (holder, path) in [(Holder::Heir, Path::Heir), (Holder::Device, Path::TwoOfThree)] {
        let mut alone = full.clone();
        savings.sign(&mut alone, &[holder]);
        assert!(matches!(savings.finalize(&alone), Err(Error::NotEnoughApprovals)), "{holder:?}");
        let reason = node.accepts(&savings.forced(&alone, path)).unwrap_err();
        assert!(reason.contains("script-verify-flag-failed"), "{holder:?}: {reason}");
    }
}

#[test]
fn savings_with_no_beneficiary_has_no_check_in() {
    // No delayed path, so nothing to restart: a check-in is refused, before
    // and after any coin has waited.
    let node = Node::start();
    let mut savings = Savings::new(Delays::none());
    let funded = savings.fund(&node, btc(1.0));
    savings.age_to(&node, funded, 30);
    let delays = savings.delays();
    assert!(matches!(build_checkin(&mut savings.wallet, delays, FEE_RATE, FeeCeiling::DEFAULT), Err(Error::NoCheckinWithoutHeir)));
    let any = savings.build_unchecked(Path::TwoOfThree, &node.new_address());
    assert!(matches!(check_checkin(&savings.wallet, &any, delays, FeeCeiling::DEFAULT), Err(Error::NoCheckinWithoutHeir)));
}

#[test]
fn an_immature_coinbase_coin_holds_up_the_check_in() {
    // Only on test networks, where Savings can be funded by mining: a block
    // reward can't be spent for 100 blocks, so a check-in can't include it
    // and is refused until it can.
    use bdk_bitcoind_rpc::bitcoincore_rpc::RpcApi;
    use serde_json::{Value, json};
    let node = Node::start();
    let mut savings = Savings::new(Delays::with_heir(20).unwrap());
    savings.fund(&node, btc(1.0));
    let address = savings.wallet.reveal_next_address(KeychainKind::External).address;
    node.rpc().call::<Value>("generatetoaddress", &[json!(1), json!(address.to_string())]).unwrap();
    savings.sync(&node);
    let delays = savings.delays();
    assert!(matches!(build_checkin(&mut savings.wallet, delays, FEE_RATE, FeeCeiling::DEFAULT), Err(Error::CheckinMissesCoin { .. })));

    node.mine(99); // the reward now has 100 confirmations and can be spent
    savings.sync(&node);
    let (tx, _) = check_in(&mut savings, &node);
    assert_eq!(tx.input.len(), 2);
}

#[test]
fn an_unconfirmed_incoming_coin_neither_blocks_nor_joins_a_check_in() {
    // Anyone can send to a Savings address, with a low fee or as a
    // replaceable transaction. If the check-in had to include that coin, the
    // sender could stall it or knock it out by replacing the coin.
    let node = Node::start();
    let mut savings = Savings::new(Delays::with_heir(20).unwrap());
    savings.fund(&node, btc(1.0));
    let address = savings.wallet.reveal_next_address(KeychainKind::External).address;
    node.send_to(&address, btc(0.5)); // not mined
    savings.sync(&node);
    let unconfirmed = savings.wallet.list_unspent().find(|u| !u.chain_position.is_confirmed()).expect("seen in mempool");
    let delays = savings.delays();

    let psbt = build_checkin(&mut savings.wallet, delays, FEE_RATE, FeeCeiling::DEFAULT).expect("not blocked by the unconfirmed coin");
    assert_eq!(psbt.unsigned_tx.input.len(), 1, "only the confirmed coin");
    assert!(psbt.unsigned_tx.input.iter().all(|i| i.previous_output != unconfirmed.outpoint), "doesn't join");
    assert!(check_checkin(&savings.wallet, &psbt, delays, FeeCeiling::DEFAULT).is_ok(), "an unconfirmed coin isn't missing");

    // A hand-made check-in that does include it is refused.
    let back = savings.wallet.reveal_next_address(KeychainKind::Internal).address.script_pubkey();
    let mut builder = savings.wallet.build_tx();
    builder.drain_wallet().drain_to(back).fee_rate(FEE_RATE).set_condition(Condition { csv: None, timelock: None });
    let mut with_it = builder.finish().unwrap();
    with_it.proprietary = psbt.proprietary.clone();
    assert_eq!(with_it.unsigned_tx.input.len(), 2);
    assert!(matches!(
        check_checkin(&savings.wallet, &with_it, delays, FeeCeiling::DEFAULT),
        Err(Error::CheckinSpendsUnconfirmed { coin }) if coin == unconfirmed.outpoint
    ));

    // The check-in goes through while the coin is still unconfirmed.
    let mut signed = psbt;
    savings.sign(&mut signed, &[Holder::Phone, Holder::Helper]);
    let tx = savings.finalize(&signed).unwrap();
    assert_eq!(node.accepts(&tx), Ok(()));
}

#[test]
fn a_confirmed_coin_left_out_is_still_refused() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::with_heir(20).unwrap());
    savings.fund(&node, btc(1.0));
    savings.fund(&node, btc(0.5));
    let delays = savings.delays();
    let full = build_checkin(&mut savings.wallet, delays, FEE_RATE, FeeCeiling::DEFAULT).unwrap();
    assert_eq!(full.unsigned_tx.input.len(), 2);

    let coins: Vec<_> = savings.wallet.list_unspent().map(|u| u.outpoint).collect();
    let back = savings.wallet.reveal_next_address(KeychainKind::Internal).address.script_pubkey();
    let mut builder = savings.wallet.build_tx();
    builder
        .add_utxo(coins[0])
        .unwrap()
        .manually_selected_only()
        .drain_to(back)
        .fee_rate(FEE_RATE)
        .set_condition(Condition { csv: None, timelock: None });
    let mut partial = builder.finish().unwrap();
    partial.proprietary = full.proprietary.clone();
    assert!(matches!(
        check_checkin(&savings.wallet, &partial, delays, FeeCeiling::DEFAULT),
        Err(Error::CheckinMissesCoin { coin }) if coin == coins[1]
    ));
}

#[test]
fn no_confirmed_coins_means_nothing_to_check_in() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::with_heir(20).unwrap());
    let address = savings.wallet.reveal_next_address(KeychainKind::External).address;
    node.send_to(&address, btc(1.0)); // not mined
    savings.sync(&node);
    let delays = savings.delays();
    assert!(matches!(build_checkin(&mut savings.wallet, delays, FEE_RATE, FeeCeiling::DEFAULT), Err(Error::CheckinNoCoins)));
}

#[test]
fn a_check_in_whose_fee_is_above_the_ceiling_is_refused() {
    let node = Node::start();
    let mut savings = Savings::new(Delays::with_heir(20).unwrap());
    savings.fund(&node, btc(1.0));
    savings.fund(&node, btc(0.5));
    let delays = savings.delays();

    // A typo in the fee rate, 5,000 sat/vB for 5: over 0.01 BTC for two
    // coins, above the 0.005 BTC ceiling.
    let built = build_checkin(&mut savings.wallet, delays, FeeRate::from_sat_per_vb_u32(5_000), FeeCeiling::DEFAULT);
    assert!(matches!(built, Err(Error::FeeTooHigh { .. })), "{:?}", built.err());

    // A good check-in whose one output was edited down, signed anyway.
    let good = build_checkin(&mut savings.wallet, delays, FEE_RATE, FeeCeiling::DEFAULT).unwrap();
    let mut edited = good.clone();
    edited.unsigned_tx.output[0].value = btc(0.1);
    savings.sign(&mut edited, &[Holder::Phone, Holder::Helper]);
    let checked = check_checkin(&savings.wallet, &edited, delays, FeeCeiling::DEFAULT);
    assert!(
        matches!(checked, Err(Error::FeeTooHigh { fee, spent, .. }) if fee == btc(1.4) && spent == btc(1.5)),
        "{checked:?}"
    );
    // The ceiling is a setting: one that allows it lets it through.
    let generous = FeeCeiling::new(btc(2.0), 100).unwrap();
    assert!(check_checkin(&savings.wallet, &edited, delays, generous).is_ok());
    assert!(check_checkin(&savings.wallet, &good, delays, FeeCeiling::DEFAULT).is_ok());

    // In a small Savings, the percentage ceiling refuses what the amount
    // ceiling allows: 1,000 sat/vB on 0.01 BTC is well over 5%.
    let mut small = Savings::new(Delays::with_heir(20).unwrap());
    small.fund(&node, btc(0.01));
    let small_delays = small.delays();
    let built = build_checkin(&mut small.wallet, small_delays, FeeRate::from_sat_per_vb_u32(1_000), FeeCeiling::DEFAULT);
    assert!(
        matches!(built, Err(Error::FeeTooHigh { fee, .. }) if fee < FeeCeiling::DEFAULT.max_fee()),
        "{:?}",
        built.err()
    );
}
