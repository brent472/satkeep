//! Adding a beneficiary on regtest: plain 2-of-3 Savings moves, in one
//! 2-of-3 transaction, to a new wallet with the same PHONE, DEVICE and
//! HELPER plus HEIR, and the heir path then works there. Where it goes comes
//! from the signer's own settings, never the PSBT, and every way of bending
//! the move is refused.

mod common;

use common::{FEE_RATE, Holder, MIN_HEIR_DELAY, Node, Path, Savings};
use bdk_wallet::descriptor::policy::Condition;
use satkeep_savings::bitcoin::{Amount, FeeRate};
use satkeep_savings::bitcoin::psbt::Psbt;
use satkeep_savings::migrate::{build_move, check_move, destination, is_move, pays_to, wallet_with_heir};
use satkeep_savings::spend::FeeCeiling;
use satkeep_savings::{DEFAULT_MIN_HEIR_DELAY, Delays, Error, Role};

const HEIR_DELAY: u16 = 20;

fn btc(n: f64) -> Amount {
    Amount::from_btc(n).unwrap()
}

/// Plain Savings with two confirmed coins, and the wallet that adding the
/// beneficiary moves it to: what the signers' settings say.
fn plain_and_target(node: &Node) -> (Savings, Savings) {
    let mut plain = Savings::new(Delays::none());
    plain.fund(node, btc(1.0));
    plain.fund(node, btc(0.5));
    let target = plain.with_heir_added(HEIR_DELAY);
    (plain, target)
}

fn prepare(plain: &mut Savings, target: &Savings) -> Psbt {
    build_move(&mut plain.wallet, &plain.policy, &target.policy, MIN_HEIR_DELAY, FEE_RATE, FeeCeiling::DEFAULT).expect("build the move")
}

/// `check_move` against the wallet the signers' settings say.
fn check(plain: &Savings, target: &Savings, psbt: &Psbt) -> Result<(), Error> {
    check_move(&plain.wallet, psbt, &plain.policy, &target.policy, MIN_HEIR_DELAY, FeeCeiling::DEFAULT)
}

#[test]
fn adding_a_beneficiary_moves_everything_in_one_transaction_and_the_heir_path_then_works() {
    let node = Node::start();
    let (mut plain, mut target) = plain_and_target(&node);
    let mut psbt = prepare(&mut plain, &target);
    assert!(is_move(&psbt) && pays_to(&psbt, &target.policy));
    check(&plain, &target, &psbt).unwrap();

    plain.sign(&mut psbt, &[Holder::Phone, Holder::Helper]);
    let tx = plain.finalize(&psbt).unwrap();
    assert_eq!((tx.input.len(), tx.output.len()), (2, 1), "both coins, one output, one network fee");
    assert_eq!(plain.path_used(&tx), Path::TwoOfThree);
    node.broadcast_and_confirm(&tx);
    let moved_at = node.height();
    plain.sync(&node);
    target.sync(&node);
    assert_eq!(plain.wallet.balance().total(), Amount::ZERO);
    assert_eq!(target.wallet.balance().total(), btc(1.5) - psbt.fee().unwrap());

    // In the new wallet, HEIR alone opens at exactly the heir delay.
    let to = node.new_address();
    target.age_to(&node, moved_at, u32::from(HEIR_DELAY) - 1);
    assert!(matches!(target.build(Path::Heir, &to), Err(Error::TooEarly { has: 19, .. })));
    target.age_to(&node, moved_at, HEIR_DELAY.into());
    let mut heir = target.build(Path::Heir, &to).unwrap();
    target.sign(&mut heir, &[Holder::Heir]);
    assert_eq!(node.accepts(&target.finalize(&heir).unwrap()), Ok(()));
}

#[test]
fn a_move_needs_two_of_three_keys() {
    let node = Node::start();
    let (mut plain, target) = plain_and_target(&node);
    for holder in [Holder::Phone, Holder::Device, Holder::Helper, Holder::Heir] {
        let mut psbt = prepare(&mut plain, &target);
        plain.sign(&mut psbt, &[holder]);
        assert!(matches!(plain.finalize(&psbt), Err(Error::NotEnoughApprovals)), "{holder:?} alone");
    }
}

#[test]
fn a_move_to_a_wallet_with_a_different_heir_key_is_refused() {
    // Whoever prepares the move builds a complete, consistent move to a
    // wallet whose HEIR they hold. The signers check it against their own
    // settings, so it's refused, whether the PSBT owns up to the other HEIR
    // or claims the right one.
    let node = Node::start();
    let (mut plain, target) = plain_and_target(&node);
    let theirs = plain.with_stranger_as_heir(HEIR_DELAY);
    assert_ne!(destination(&theirs.policy), destination(&target.policy));
    let dishonest = prepare(&mut plain, &theirs);
    assert!(check_move(&plain.wallet, &dishonest, &plain.policy, &theirs.policy, MIN_HEIR_DELAY, FeeCeiling::DEFAULT).is_ok());
    assert!(matches!(check(&plain, &target, &dishonest), Err(Error::MoveNotAsSetUp)));

    let mut disguised = dishonest.clone();
    disguised.proprietary = prepare(&mut plain, &target).proprietary;
    assert!(matches!(check(&plain, &target, &disguised), Err(Error::MoveShape)));
}

#[test]
fn a_move_to_a_wallet_with_a_shorter_heir_delay_is_refused() {
    // The same HEIR, but a shorter wait before HEIR alone can spend.
    let node = Node::start();
    let (mut plain, target) = plain_and_target(&node);
    let shorter = plain.with_heir_added(HEIR_DELAY - 5);
    let dishonest = prepare(&mut plain, &shorter);
    assert!(matches!(check(&plain, &target, &dishonest), Err(Error::MoveNotAsSetUp)));

    let mut disguised = dishonest.clone();
    disguised.proprietary = prepare(&mut plain, &target).proprietary;
    assert!(matches!(check(&plain, &target, &disguised), Err(Error::MoveShape)));
}

#[test]
fn a_move_with_one_extra_output_is_refused() {
    let node = Node::start();
    let (mut plain, target) = plain_and_target(&node);
    let good = prepare(&mut plain, &target);

    // To someone else.
    let mut split = good.clone();
    let mut extra = split.unsigned_tx.output[0].clone();
    extra.value = Amount::from_sat(100_000);
    split.unsigned_tx.output[0].value -= extra.value;
    extra.script_pubkey = node.new_address().script_pubkey();
    split.unsigned_tx.output.push(extra.clone());
    assert!(matches!(check(&plain, &target, &split), Err(Error::MoveShape)));

    // Back to the old wallet as change: a move leaves nothing behind.
    let mut change = good.clone();
    extra.script_pubkey = plain.wallet.peek_address(bdk_wallet::KeychainKind::Internal, 0).script_pubkey();
    change.unsigned_tx.output.push(extra);
    assert!(matches!(check(&plain, &target, &change), Err(Error::MoveShape)));

    assert!(check(&plain, &target, &good).is_ok());
}

#[test]
fn a_move_that_pays_anywhere_else_is_refused() {
    let node = Node::start();
    let (mut plain, target) = plain_and_target(&node);
    let good = prepare(&mut plain, &target);

    // The output swapped for someone else's address.
    let mut elsewhere = good.clone();
    elsewhere.unsigned_tx.output[0].script_pubkey = node.new_address().script_pubkey();
    assert!(matches!(check(&plain, &target, &elsewhere), Err(Error::MoveShape)));

    // The marker rewritten to name a different HEIR.
    let stranger = plain.with_stranger_as_heir(HEIR_DELAY);
    let other_heir = stranger.policy.keys().get(Role::Heir).unwrap();
    let mut renamed = good.clone();
    for value in renamed.proprietary.values_mut() {
        *value = format!("{other_heir}\n{HEIR_DELAY}").into_bytes();
    }
    assert!(matches!(check(&plain, &target, &renamed), Err(Error::MoveNotAsSetUp)));

    // Not marked as a move at all.
    let mut unmarked = good.clone();
    unmarked.proprietary.clear();
    assert!(matches!(check(&plain, &target, &unmarked), Err(Error::NotAMove)));

    assert!(check(&plain, &target, &good).is_ok());
}

#[test]
fn an_heir_delay_below_the_minimum_is_refused() {
    let node = Node::start();
    let (mut plain, _) = plain_and_target(&node);
    let heir = plain.with_heir_added(HEIR_DELAY).policy.keys().get(Role::Heir).unwrap().clone();
    let too_short = |delay| Delays::with_heir(delay).unwrap();

    // In the settings the signers rebuild the new wallet from.
    let built = wallet_with_heir(&plain.policy, heir.clone(), too_short(MIN_HEIR_DELAY - 1), MIN_HEIR_DELAY);
    assert!(matches!(built, Err(Error::HeirDelayTooShort { heir: 9, minimum: 10 })), "{:?}", built.err());
    assert!(wallet_with_heir(&plain.policy, heir.clone(), too_short(MIN_HEIR_DELAY), MIN_HEIR_DELAY).is_ok());

    // In building and checking a move to such a wallet.
    let short = plain.with_heir_added(MIN_HEIR_DELAY - 1);
    let built = build_move(&mut plain.wallet, &plain.policy, &short.policy, MIN_HEIR_DELAY, FEE_RATE, FeeCeiling::DEFAULT);
    assert!(matches!(built, Err(Error::HeirDelayTooShort { .. })), "{:?}", built.err());
    let psbt = build_move(&mut plain.wallet, &plain.policy, &short.policy, 1, FEE_RATE, FeeCeiling::DEFAULT).unwrap();
    let checked = check_move(&plain.wallet, &psbt, &plain.policy, &short.policy, MIN_HEIR_DELAY, FeeCeiling::DEFAULT);
    assert!(matches!(checked, Err(Error::HeirDelayTooShort { .. })), "{checked:?}");

    // With the placeholder minimum, a 20-block delay is far too short.
    let built = wallet_with_heir(&plain.policy, heir, too_short(HEIR_DELAY), DEFAULT_MIN_HEIR_DELAY);
    assert!(matches!(built, Err(Error::HeirDelayTooShort { heir: 20, minimum: 26_280 })), "{:?}", built.err());
}

#[test]
fn a_move_must_keep_phone_device_and_helper() {
    // A wallet with a beneficiary but different keys isn't where Savings
    // goes when a beneficiary is added, even if the settings say so.
    let node = Node::start();
    let (mut plain, target) = plain_and_target(&node);
    let someone_else = Savings::new(Delays::with_heir(HEIR_DELAY).unwrap());
    let built = build_move(&mut plain.wallet, &plain.policy, &someone_else.policy, MIN_HEIR_DELAY, FEE_RATE, FeeCeiling::DEFAULT);
    assert!(matches!(built, Err(Error::MoveToWrongWallet)), "{:?}", built.err());
    let good = prepare(&mut plain, &target);
    let checked = check_move(&plain.wallet, &good, &plain.policy, &someone_else.policy, MIN_HEIR_DELAY, FeeCeiling::DEFAULT);
    assert!(matches!(checked, Err(Error::MoveToWrongWallet)), "{checked:?}");
    // And to the very same wallet is no move at all.
    let same = build_move(&mut plain.wallet, &plain.policy, &plain.policy, MIN_HEIR_DELAY, FEE_RATE, FeeCeiling::DEFAULT);
    assert!(same.is_err());
}

#[test]
fn a_move_that_leaves_out_a_confirmed_coin_is_refused() {
    let node = Node::start();
    let (mut plain, target) = plain_and_target(&node);
    let good = prepare(&mut plain, &target);
    let coins: Vec<_> = plain.wallet.list_unspent().map(|u| u.outpoint).collect();
    let mut builder = plain.wallet.build_tx();
    builder
        .add_utxo(coins[0])
        .unwrap()
        .manually_selected_only()
        .drain_to(destination(&target.policy))
        .fee_rate(FEE_RATE)
        .set_condition(Condition { csv: None, timelock: None });
    let mut partial = builder.finish().unwrap();
    partial.proprietary = good.proprietary.clone();
    assert!(matches!(
        check(&plain, &target, &partial),
        Err(Error::MoveMissesCoin { coin }) if coin == coins[1]
    ));
}

#[test]
fn changing_the_beneficiary_is_another_move() {
    // From a wallet with a beneficiary to one with a different heir delay
    // (or HEIR): the same one-transaction move, still 2 of 3.
    let node = Node::start();
    let mut first = Savings::new(Delays::with_heir(HEIR_DELAY).unwrap());
    first.fund(&node, btc(1.0));
    let mut next = first.with_heir_added(HEIR_DELAY + 10);
    let mut psbt = build_move(&mut first.wallet, &first.policy, &next.policy, MIN_HEIR_DELAY, FEE_RATE, FeeCeiling::DEFAULT).unwrap();
    check_move(&first.wallet, &psbt, &first.policy, &next.policy, MIN_HEIR_DELAY, FeeCeiling::DEFAULT).unwrap();
    first.sign(&mut psbt, &[Holder::Device, Holder::Helper]);
    node.broadcast_and_confirm(&first.finalize(&psbt).unwrap());
    next.sync(&node);
    assert!(next.wallet.balance().total() > Amount::ZERO);
}

#[test]
fn nothing_confirmed_means_nothing_to_move() {
    let node = Node::start();
    let mut plain = Savings::new(Delays::none());
    let address = plain.wallet.reveal_next_address(bdk_wallet::KeychainKind::External).address;
    node.send_to(&address, btc(1.0)); // not mined
    plain.sync(&node);
    let target = plain.with_heir_added(HEIR_DELAY);
    let built = build_move(&mut plain.wallet, &plain.policy, &target.policy, MIN_HEIR_DELAY, FEE_RATE, FeeCeiling::DEFAULT);
    assert!(matches!(built, Err(Error::MoveNoCoins)), "{:?}", built.err());
}

#[test]
fn a_move_whose_fee_is_above_the_ceiling_is_refused() {
    let node = Node::start();
    let (mut plain, target) = plain_and_target(&node);
    let typo = FeeRate::from_sat_per_vb_u32(5_000);
    let built = build_move(&mut plain.wallet, &plain.policy, &target.policy, MIN_HEIR_DELAY, typo, FeeCeiling::DEFAULT);
    assert!(matches!(built, Err(Error::FeeTooHigh { .. })), "{:?}", built.err());

    // A good move whose one output was edited down, signed anyway.
    let good = prepare(&mut plain, &target);
    let mut edited = good.clone();
    edited.unsigned_tx.output[0].value = btc(0.1);
    plain.sign(&mut edited, &[Holder::Phone, Holder::Device]);
    let checked = check(&plain, &target, &edited);
    assert!(
        matches!(checked, Err(Error::FeeTooHigh { fee, spent, .. }) if fee == btc(1.4) && spent == btc(1.5)),
        "{checked:?}"
    );
    let generous = FeeCeiling::new(btc(2.0), 100).unwrap();
    assert!(check_move(&plain.wallet, &edited, &plain.policy, &target.policy, MIN_HEIR_DELAY, generous).is_ok());
    assert!(check(&plain, &target, &good).is_ok());
}
