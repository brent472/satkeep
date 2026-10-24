//! The payment rule on regtest: with a beneficiary, a 2-of-3 Savings
//! payment also spends the coins close to needing a check-in, with change
//! back to Savings, so their heir delay restarts. With the setting off, it
//! leaves them alone.
//!
//! Heir delay 20, check-ins every 10 blocks with a 5-block margin, refresh
//! window 4: coins with 6 or more confirmations are due for a refresh.
//! Each payment here is from one hand-picked young coin, so it's the rule,
//! not coin selection, that decides whether the old coin goes in.

mod common;

use common::{FEE_RATE, Holder, Node, Path, Savings};
use satkeep_savings::bitcoin::psbt::Psbt;
use satkeep_savings::bitcoin::{Address, Amount, OutPoint};
use satkeep_savings::checkin::{self, CheckinRule, PaymentRule, Refresh};
use satkeep_savings::{Delays, Error, spend};

const HEIR_DELAY: u16 = 20;

fn btc(n: f64) -> Amount {
    Amount::from_btc(n).unwrap()
}

fn rule() -> PaymentRule {
    let delays = Delays::with_heir(HEIR_DELAY).unwrap();
    let rule = PaymentRule::new(4, CheckinRule::new(10, 5, delays).unwrap()).unwrap();
    assert_eq!(rule.refresh_age(), 6);
    rule
}

/// The only unspent coin confirmed at `height`.
fn coin_at(savings: &Savings, height: u32) -> OutPoint {
    let mut coins = savings
        .wallet
        .list_unspent()
        .filter(|u| u.chain_position.confirmation_height_upper_bound() == Some(height))
        .map(|u| u.outpoint);
    let coin = coins.next().expect("a coin at that height");
    assert!(coins.next().is_none());
    coin
}

/// Savings with an old coin (8 confirmations, due for a refresh) and a
/// young one (1 confirmation). Returns the wallet, the old coin and its
/// height, and the young coin.
fn old_and_young(node: &Node) -> (Savings, OutPoint, u32, OutPoint) {
    let mut savings = Savings::new(Delays::with_heir(HEIR_DELAY).unwrap());
    let old_at = savings.fund(node, btc(0.3));
    savings.age_to(node, old_at, 7);
    let young_at = savings.fund(node, btc(1.0));
    let (old, young) = (coin_at(&savings, old_at), coin_at(&savings, young_at));
    assert_eq!(checkin::due_for_refresh(&savings.wallet, &rule()), vec![old], "8 confirmations: due; 1: not yet");
    (savings, old, old_at, young)
}

/// A 0.5 BTC payment from `young` only, under `rule`.
fn pay_from(savings: &mut Savings, young: OutPoint, to: &Address, rule: Option<&PaymentRule>) -> Psbt {
    let delays = savings.delays();
    spend::build_payment(&mut savings.wallet, delays, Path::TwoOfThree, rule, |b| {
        b.add_utxo(young).unwrap().manually_selected_only().add_recipient(to.script_pubkey(), btc(0.5)).fee_rate(FEE_RATE);
    })
    .unwrap()
}

fn send(savings: &mut Savings, node: &Node, mut psbt: Psbt) -> u32 {
    savings.sign(&mut psbt, &[Holder::Phone, Holder::Helper]);
    node.broadcast_and_confirm(&savings.finalize(&psbt).unwrap());
    savings.sync(node);
    node.height()
}

fn spends(psbt: &Psbt, coin: OutPoint) -> bool {
    psbt.unsigned_tx.input.iter().any(|i| i.previous_output == coin)
}

/// HEIR alone, spending just `coin`.
fn heir_spends(savings: &mut Savings, node: &Node, coin: OutPoint) -> Result<(), Error> {
    let delays = savings.delays();
    let to = node.new_address().script_pubkey();
    let mut psbt = spend::build_spend(&mut savings.wallet, delays, Path::Heir, |b| {
        b.add_utxo(coin).unwrap().manually_selected_only().drain_to(to).fee_rate(FEE_RATE);
    })?;
    savings.sign(&mut psbt, &[Holder::Heir]);
    assert_eq!(node.accepts(&savings.finalize(&psbt)?), Ok(()));
    Ok(())
}

#[test]
fn a_payment_refreshes_old_coins_and_their_heir_delay_restarts() {
    let node = Node::start();
    let (mut savings, old, old_at, young) = old_and_young(&node);
    let to = node.new_address();
    let psbt = pay_from(&mut savings, young, &to, Some(&rule()));
    assert!(spends(&psbt, old) && spends(&psbt, young), "the old coin went in too");
    assert_eq!(checkin::refresh_of(&savings.wallet, &psbt, &rule()), Refresh { refreshed: vec![old], left_out: vec![] });
    let change: Vec<_> = psbt.unsigned_tx.output.iter().filter(|o| o.script_pubkey != to.script_pubkey()).collect();
    assert_eq!(change.len(), 1);
    assert!(savings.wallet.is_mine(change[0].script_pubkey.clone()), "the change comes back to Savings");

    let fee = psbt.fee().unwrap();
    let paid_at = send(&mut savings, &node, psbt);
    let left: Vec<_> = savings.wallet.list_unspent().collect();
    assert_eq!(left.len(), 1, "only the change is left");
    assert_eq!(left[0].txout.value, btc(1.3) - btc(0.5) - fee, "both coins, less the payment and its fee");
    let change = left[0].outpoint;

    // When the old coin would have reached the heir delay, HEIR alone still
    // can't touch Savings: the change started its own clock at the payment.
    savings.age_to(&node, old_at, HEIR_DELAY.into());
    let refused = heir_spends(&mut savings, &node, change);
    assert!(matches!(refused, Err(Error::TooEarly { .. })), "{refused:?}");
    assert!(checkin::coins(&savings.wallet, savings.delays(), None).iter().all(|c| !c.status.overdue));
    // It opens at exactly the heir delay from the payment.
    savings.age_to(&node, paid_at, HEIR_DELAY.into());
    heir_spends(&mut savings, &node, change).unwrap();
}

#[test]
fn with_the_setting_off_a_payment_leaves_old_coins_alone() {
    let node = Node::start();
    let (mut savings, old, old_at, young) = old_and_young(&node);
    let psbt = pay_from(&mut savings, young, &node.new_address(), None);
    assert!(!spends(&psbt, old) && spends(&psbt, young));
    assert_eq!(checkin::refresh_of(&savings.wallet, &psbt, &rule()), Refresh { refreshed: vec![], left_out: vec![old] });
    send(&mut savings, &node, psbt);
    assert!(savings.wallet.get_utxo(old).is_some(), "the old coin is still there");

    // Its heir delay kept counting: HEIR alone opens it on time.
    savings.age_to(&node, old_at, u32::from(HEIR_DELAY) - 1);
    let refused = heir_spends(&mut savings, &node, old);
    assert!(matches!(refused, Err(Error::TooEarly { .. })), "{refused:?}");
    savings.age_to(&node, old_at, HEIR_DELAY.into());
    heir_spends(&mut savings, &node, old).unwrap();
}

#[test]
fn any_payment_takes_every_coin_due_whatever_coin_selection_picks() {
    // Without hand-picking: the old coins are required inputs, so they go in
    // even when one young coin would cover the payment.
    let node = Node::start();
    let (mut savings, old, _, _) = old_and_young(&node);
    let second_old = {
        let at = savings.fund(&node, btc(0.2));
        savings.age_to(&node, at, 7);
        coin_at(&savings, at)
    };
    let delays = savings.delays();
    let to = node.new_address().script_pubkey();
    let psbt = spend::build_payment(&mut savings.wallet, delays, Path::TwoOfThree, Some(&rule()), |b| {
        b.add_recipient(to, btc(0.01)).fee_rate(FEE_RATE);
    })
    .unwrap();
    assert!(spends(&psbt, old) && spends(&psbt, second_old));
    assert!(checkin::refresh_of(&savings.wallet, &psbt, &rule()).left_out.is_empty());
}

#[test]
fn the_rule_leaves_the_heir_path_and_plain_savings_alone() {
    let node = Node::start();

    // HEIR spending one open coin isn't made to take younger ones, which
    // would make the heir payment too early to send.
    let mut savings = Savings::new(Delays::with_heir(HEIR_DELAY).unwrap());
    let open_at = savings.fund(&node, btc(0.3));
    let due_at = savings.fund(&node, btc(0.3));
    savings.age_to(&node, open_at, HEIR_DELAY.into());
    let (open, due) = (coin_at(&savings, open_at), coin_at(&savings, due_at));
    assert_eq!(checkin::due_for_refresh(&savings.wallet, &rule()).len(), 2);
    let delays = savings.delays();
    let to = node.new_address().script_pubkey();
    let mut heir = spend::build_payment(&mut savings.wallet, delays, Path::Heir, Some(&rule()), |b| {
        b.add_utxo(open).unwrap().manually_selected_only().drain_to(to).fee_rate(FEE_RATE);
    })
    .unwrap();
    assert!(spends(&heir, open) && !spends(&heir, due));
    savings.sign(&mut heir, &[Holder::Heir]);
    assert_eq!(node.accepts(&savings.finalize(&heir).unwrap()), Ok(()));

    // Savings with no beneficiary has nothing to refresh.
    let mut plain = Savings::new(Delays::none());
    let at = plain.fund(&node, btc(0.3));
    plain.age_to(&node, at, 8);
    let young_at = plain.fund(&node, btc(1.0));
    let (old, young) = (coin_at(&plain, at), coin_at(&plain, young_at));
    let psbt = pay_from(&mut plain, young, &node.new_address(), Some(&rule()));
    assert!(!spends(&psbt, old));
}
