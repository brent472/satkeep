//! Adding a beneficiary on regtest: plain 2-of-3 Savings moves, in one
//! 2-of-3 transaction, to a new wallet with the same PHONE, DEVICE and
//! HELPER plus HEIR, and the heir path then works there. Every way of
//! bending the move is refused.

mod common;

use common::{FEE_RATE, Holder, Node, Path, Savings};
use bdk_wallet::descriptor::policy::Condition;
use twofold_savings::bitcoin::Amount;
use twofold_savings::bitcoin::psbt::Psbt;
use twofold_savings::migrate::{build_move, check_move, destination, is_move, wallet_with_heir};
use twofold_savings::{Delays, Error};

const HEIR_DELAY: u16 = 20;

fn btc(n: f64) -> Amount {
    Amount::from_btc(n).unwrap()
}

/// Plain Savings with two confirmed coins, and the wallet that adding the
/// beneficiary moves it to.
fn plain_and_target(node: &Node) -> (Savings, Savings) {
    let mut plain = Savings::new(Delays::none());
    plain.fund(node, btc(1.0));
    plain.fund(node, btc(0.5));
    let target = plain.with_heir_added(HEIR_DELAY);
    (plain, target)
}

fn prepare(plain: &mut Savings, target: &Savings) -> Psbt {
    build_move(&mut plain.wallet, &plain.policy, &target.policy, FEE_RATE).expect("build the move")
}

#[test]
fn adding_a_beneficiary_moves_everything_in_one_transaction_and_the_heir_path_then_works() {
    let node = Node::start();
    let (mut plain, mut target) = plain_and_target(&node);
    let mut psbt = prepare(&mut plain, &target);
    assert!(is_move(&psbt));
    let checked = check_move(&plain.wallet, &psbt, &plain.policy).unwrap();
    assert_eq!(checked.descriptor(), target.policy.descriptor(), "the PSBT names the new wallet");

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
fn a_move_that_pays_anywhere_else_is_refused() {
    let node = Node::start();
    let (mut plain, target) = plain_and_target(&node);
    let good = prepare(&mut plain, &target);

    // The output swapped for someone else's address.
    let mut elsewhere = good.clone();
    elsewhere.unsigned_tx.output[0].script_pubkey = node.new_address().script_pubkey();
    assert!(matches!(check_move(&plain.wallet, &elsewhere, &plain.policy), Err(Error::MoveShape)));

    // The marker rewritten to name a different HEIR: the output no longer
    // matches the wallet it names.
    let stranger = Savings::new(Delays::with_heir(HEIR_DELAY).unwrap());
    let other_heir = stranger.policy.keys().get(twofold_savings::Role::Heir).unwrap().clone();
    let other = wallet_with_heir(&plain.policy, other_heir, Delays::with_heir(HEIR_DELAY).unwrap()).unwrap();
    let mut renamed = good.clone();
    for value in renamed.proprietary.values_mut() {
        *value = format!("{}\n{HEIR_DELAY}", other.keys().get(twofold_savings::Role::Heir).unwrap()).into_bytes();
    }
    assert_ne!(destination(&other), destination(&target.policy));
    assert!(matches!(check_move(&plain.wallet, &renamed, &plain.policy), Err(Error::MoveShape)));

    // Not marked as a move at all.
    let mut unmarked = good.clone();
    unmarked.proprietary.clear();
    assert!(matches!(check_move(&plain.wallet, &unmarked, &plain.policy), Err(Error::NotAMove)));

    // An extra output.
    let mut split = good.clone();
    let mut extra = split.unsigned_tx.output[0].clone();
    extra.script_pubkey = node.new_address().script_pubkey();
    split.unsigned_tx.output.push(extra);
    assert!(matches!(check_move(&plain.wallet, &split, &plain.policy), Err(Error::MoveShape)));

    assert!(check_move(&plain.wallet, &good, &plain.policy).is_ok());
}

#[test]
fn a_move_must_keep_phone_device_and_helper() {
    // A wallet with a beneficiary but different keys isn't where Savings
    // goes when a beneficiary is added.
    let node = Node::start();
    let (mut plain, _) = plain_and_target(&node);
    let someone_else = Savings::new(Delays::with_heir(HEIR_DELAY).unwrap());
    let built = build_move(&mut plain.wallet, &plain.policy, &someone_else.policy, FEE_RATE);
    assert!(matches!(built, Err(Error::MoveToWrongWallet)), "{:?}", built.err());
    // And to the very same wallet is no move at all.
    let same = build_move(&mut plain.wallet, &plain.policy, &plain.policy, FEE_RATE);
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
        check_move(&plain.wallet, &partial, &plain.policy),
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
    let mut psbt = build_move(&mut first.wallet, &first.policy, &next.policy, FEE_RATE).unwrap();
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
    let built = build_move(&mut plain.wallet, &plain.policy, &target.policy, FEE_RATE);
    assert!(matches!(built, Err(Error::MoveNoCoins)), "{:?}", built.err());
}
