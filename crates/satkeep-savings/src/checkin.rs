//! The check-in: a send-to-self of every Savings coin through the 2-of-3
//! path, which restarts every coin's heir delay.
//!
//! Delays are relative: each coin's clock starts when that coin confirms.
//! A check-in spends every coin into one new Savings coin, so the heir delay
//! starts again from zero. It must happen before the oldest coin reaches the
//! heir delay, or HEIR alone could spend it. Savings with no beneficiary has
//! no delayed path, so it needs no check-in, and this module refuses one.
//!
//! The **payment rule** ([`PaymentRule`]) makes most check-ins unnecessary:
//! every 2-of-3 Savings payment also spends the coins close to needing one,
//! with change back to Savings, so the payment refreshes them.

use bdk_wallet::{KeychainKind, Wallet};

use crate::bitcoin::psbt::{Psbt, raw::ProprietaryKey};
use crate::bitcoin::{Amount, FeeRate, OutPoint};
use crate::spend::{self, FeeCeiling, Path};
use crate::{Delays, Error};

/// Placeholder check-in interval: about 12 months of blocks. Not final until
/// an outside Bitcoin reviewer signs off. A shorter heir delay gets a
/// shorter interval by default ([`default_interval`]).
pub const DEFAULT_CHECKIN_INTERVAL: u16 = 52_560;

/// Placeholder margin the interval must leave before the heir delay: about
/// 2 months of blocks, for fees, confirmations and a late check-in.
pub const DEFAULT_CHECKIN_MARGIN: u16 = 8_760;

/// Placeholder refresh window: about 2 months of blocks. Under the payment
/// rule, a payment also spends every coin within this many blocks of its
/// check-in being due; with the default interval, coins older than about
/// 10 months (43,800 blocks).
pub const DEFAULT_REFRESH_WINDOW: u16 = 8_760;

/// How often check-ins are due, and how much room that leaves before the
/// heir delay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckinRule {
    interval: u16,
    margin: u16,
}

impl CheckinRule {
    /// Refuses unless `interval + margin <= heir delay`, with both at least
    /// one block. So an interval equal to or longer than the heir delay is
    /// always refused. Refuses outright for Savings with no beneficiary.
    pub fn new(interval: u16, margin: u16, delays: Delays) -> Result<Self, Error> {
        let heir = delays.heir().ok_or(Error::NoCheckinWithoutHeir)?;
        if interval == 0 {
            return Err(Error::CheckinIntervalZero);
        }
        if margin == 0 {
            return Err(Error::CheckinMarginZero);
        }
        if u32::from(interval) + u32::from(margin) > u32::from(heir) {
            return Err(Error::CheckinTooLate { interval, margin, heir });
        }
        Ok(Self { interval, margin })
    }

    /// Blocks after a coin confirms before its check-in is due.
    pub fn interval(&self) -> u16 {
        self.interval
    }

    /// Blocks the interval leaves before the heir delay, at least.
    pub fn margin(&self) -> u16 {
        self.margin
    }

    /// Where a coin with `confirmations` stands. Confirmations are counted
    /// as of the next block, the same way delays are checked.
    pub fn status(&self, delays: Delays, confirmations: u32) -> CoinStatus {
        status(delays, Some(*self), confirmations)
    }
}

/// The check-in interval for `delays` when none is chosen (brief 3.27): the
/// smaller of [`DEFAULT_CHECKIN_INTERVAL`] and the heir delay minus
/// `margin`, so the check-ins always fit. The minimum heir delay (26,280
/// blocks) with the default margin gets a check-in every 17,520 blocks,
/// about every 4 months. Refuses Savings with no beneficiary, a zero
/// margin, and an heir delay no longer than the margin, which leaves no
/// room for any check-in.
pub fn default_interval(delays: Delays, margin: u16) -> Result<u16, Error> {
    let heir = delays.heir().ok_or(Error::NoCheckinWithoutHeir)?;
    if margin == 0 {
        return Err(Error::CheckinMarginZero);
    }
    match heir.checked_sub(margin) {
        Some(room) if room > 0 => Ok(room.min(DEFAULT_CHECKIN_INTERVAL)),
        _ => Err(Error::NoRoomForCheckins { margin, heir }),
    }
}

/// What a new wallet's heir delay must allow, from the signer's own
/// settings: at least `min_heir_delay` blocks, and room for check-ins every
/// `checkin_interval` blocks with `checkin_margin` to spare (see
/// [`CheckinRule::new`]). With no interval chosen, it's
/// [`default_interval`], which always fits.
///
/// A minimum alone isn't enough. With a fixed yearly interval, an heir
/// delay between the minimum (26,280) and 61,319 blocks would leave no
/// check-in schedule that runs before the heir path opens, so the wallet
/// would never say a check-in is due, only that it's overdue. Adding a
/// beneficiary checks both ([`crate::migrate::wallet_with_heir`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeirDelayRules {
    pub min_heir_delay: u16,
    /// `None` means [`default_interval`] for the heir delay.
    pub checkin_interval: Option<u16>,
    pub checkin_margin: u16,
}

impl HeirDelayRules {
    /// The placeholder defaults: [`crate::DEFAULT_MIN_HEIR_DELAY`],
    /// [`DEFAULT_CHECKIN_MARGIN`], and the default interval for the heir
    /// delay. Every heir delay from the minimum up gets check-ins that fit.
    pub const DEFAULT: Self = Self {
        min_heir_delay: crate::DEFAULT_MIN_HEIR_DELAY,
        checkin_interval: None,
        checkin_margin: DEFAULT_CHECKIN_MARGIN,
    };

    /// Refuses `delays` unless it has a beneficiary, an heir delay of at
    /// least the minimum, and room for the check-in rule, which it returns.
    pub fn check(&self, delays: Delays) -> Result<CheckinRule, Error> {
        if delays.heir().is_none() {
            return Err(Error::HeirAndDelayMismatch);
        }
        delays.require_heir_at_least(self.min_heir_delay)?;
        let interval = match self.checkin_interval {
            Some(interval) => interval,
            None => default_interval(delays, self.checkin_margin)?,
        };
        CheckinRule::new(interval, self.checkin_margin, delays)
    }
}

/// The payment rule (brief 3.12), for Savings with a beneficiary: every
/// 2-of-3 Savings payment, a pocket refill included, also spends every
/// confirmed coin within `window` blocks of needing a check-in, with change
/// back to Savings. Spending a coin restarts its heir delay, so the payment
/// counts as that coin's check-in, and the yearly check-in is only needed
/// when nothing has been paid for a while.
///
/// It's a wallet setting, on by default. It links the refreshed coins to
/// the payment (anyone watching the chain sees they have one owner), so it
/// can be turned off for privacy, which leaves only check-ins to refresh
/// coins. Including more coins also costs a little more network fee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaymentRule {
    window: u16,
    refresh_age: u16,
}

impl PaymentRule {
    /// Refuses unless `1 <= window < ` the check-in interval, so only coins
    /// at least one block old are ever due.
    pub fn new(window: u16, checkin: CheckinRule) -> Result<Self, Error> {
        if window == 0 {
            return Err(Error::RefreshWindowZero);
        }
        if window >= checkin.interval() {
            return Err(Error::RefreshWindowTooLong { window, interval: checkin.interval() });
        }
        Ok(Self { window, refresh_age: checkin.interval() - window })
    }

    /// Blocks before a coin's check-in is due that a payment refreshes it.
    pub fn window(&self) -> u16 {
        self.window
    }

    /// Confirmations at which a coin is due for a refresh.
    pub fn refresh_age(&self) -> u16 {
        self.refresh_age
    }
}

/// The confirmed coins a payment must also spend under `rule`: those with
/// at least [`PaymentRule::refresh_age`] confirmations, counted as of the
/// next block, as [`coins`] does.
pub fn due_for_refresh(wallet: &Wallet, rule: &PaymentRule) -> Vec<OutPoint> {
    let next_block = wallet.latest_checkpoint().height() + 1;
    wallet
        .list_unspent()
        .filter(|utxo| {
            utxo.chain_position
                .confirmation_height_upper_bound()
                .is_some_and(|c| next_block.saturating_sub(c) >= u32::from(rule.refresh_age))
        })
        .map(|utxo| utxo.outpoint)
        .collect()
}

/// Which coins due for a refresh a payment spends, and which it leaves out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refresh {
    pub refreshed: Vec<OutPoint>,
    pub left_out: Vec<OutPoint>,
}

/// What `psbt` does for the coins due for a refresh under `rule`.
pub fn refresh_of(wallet: &Wallet, psbt: &Psbt, rule: &PaymentRule) -> Refresh {
    let spends = |coin: &OutPoint| psbt.unsigned_tx.input.iter().any(|i| i.previous_output == *coin);
    let (refreshed, left_out) = due_for_refresh(wallet, rule).into_iter().partition(spends);
    Refresh { refreshed, left_out }
}

/// Where a coin with `confirmations` stands. Without a check-in rule (a
/// wallet made before check-ins existed, or one with no beneficiary),
/// nothing is ever "due", but the heir delay and "overdue" don't depend on
/// the rule and are still reported.
pub fn status(delays: Delays, rule: Option<CheckinRule>, confirmations: u32) -> CoinStatus {
    let heir_opens_in = delays.heir().map(|blocks| u32::from(blocks).saturating_sub(confirmations));
    CoinStatus {
        confirmations,
        heir_opens_in,
        checkin_due: rule.is_some_and(|r| confirmations > 0 && confirmations >= u32::from(r.interval)),
        overdue: confirmations > 0 && delays.heir().is_some_and(|blocks| confirmations >= u32::from(blocks)),
    }
}

/// One coin's position against the heir delay and the check-in interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoinStatus {
    /// 0 while unconfirmed.
    pub confirmations: u32,
    /// Blocks until HEIR alone could spend it; 0 means it already can.
    /// `None` with no beneficiary.
    pub heir_opens_in: Option<u32>,
    /// The coin has reached the check-in interval.
    pub checkin_due: bool,
    /// The coin's heir path is already open.
    pub overdue: bool,
}

/// A coin in the wallet and its status.
#[derive(Clone, Copy, Debug)]
pub struct Coin {
    pub outpoint: OutPoint,
    pub amount: Amount,
    pub status: CoinStatus,
}

/// Every unspent Savings coin and its status, as of the wallet's last sync.
/// `rule` is `None` for a wallet without a check-in rule; see [`status`].
pub fn coins(wallet: &Wallet, delays: Delays, rule: Option<CheckinRule>) -> Vec<Coin> {
    let next_block = wallet.latest_checkpoint().height() + 1;
    wallet
        .list_unspent()
        .map(|utxo| {
            let confirmations =
                utxo.chain_position.confirmation_height_upper_bound().map_or(0, |c| next_block.saturating_sub(c));
            Coin { outpoint: utxo.outpoint, amount: utxo.txout.value, status: status(delays, rule, confirmations) }
        })
        .collect()
}

/// What the wallet as a whole needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reminder {
    /// No coin has reached the interval.
    NotDue,
    /// The oldest coin has reached the interval: check in now.
    Due,
    /// Some coin's heir path is already open: HEIR alone could spend it.
    Overdue,
}

pub fn reminder(coins: &[Coin]) -> Reminder {
    if coins.iter().any(|c| c.status.overdue) {
        Reminder::Overdue
    } else if coins.iter().any(|c| c.status.checkin_due) {
        Reminder::Due
    } else {
        Reminder::NotDue
    }
}

/// The PSBT field that marks a check-in. Proprietary fields are ignored by
/// other software.
fn checkin_marker() -> ProprietaryKey {
    ProprietaryKey { prefix: b"satkeep".to_vec(), subtype: 0, key: b"checkin".to_vec() }
}

/// Whether `psbt` was built by [`build_checkin`].
pub fn is_checkin(psbt: &Psbt) -> bool {
    psbt.proprietary.contains_key(&checkin_marker())
}

/// Builds a check-in: every confirmed Savings coin, through the 2-of-3 path,
/// into one new Savings change address.
///
/// Unconfirmed coins are left out. Their delays haven't started, so there's
/// nothing to restart, and including one would let whoever sent it stall or
/// break the check-in by never confirming it (a low fee) or replacing it
/// (RBF), since a transaction can't confirm before the coins it spends.
///
/// Refuses if the fee at `fee_rate` would be above `ceiling`.
pub fn build_checkin(wallet: &mut Wallet, delays: Delays, fee_rate: FeeRate, ceiling: FeeCeiling) -> Result<Psbt, Error> {
    if delays.heir().is_none() {
        return Err(Error::NoCheckinWithoutHeir);
    }
    if !wallet.list_unspent().any(|u| u.chain_position.is_confirmed()) {
        return Err(Error::CheckinNoCoins);
    }
    let to = wallet.reveal_next_address(KeychainKind::Internal).address.script_pubkey();
    let mut psbt = spend::build_spend(wallet, delays, Path::TwoOfThree, |b| {
        b.drain_wallet().exclude_unconfirmed().drain_to(to.clone()).fee_rate(fee_rate);
    })?;
    psbt.proprietary.insert(checkin_marker(), Vec::new());
    check_checkin(wallet, &psbt, delays, ceiling)?;
    Ok(psbt)
}

/// Checks a check-in is complete: marked as one, spending every confirmed
/// Savings coin and nothing else, through the 2-of-3 path, into exactly one
/// output back to this wallet, for a fee no higher than `ceiling`.
/// Unconfirmed coins must not be in it and don't count as missing (see
/// [`build_checkin`]). Run it again just before
/// signing and broadcasting, since a coin may have confirmed since it was
/// built.
pub fn check_checkin(wallet: &Wallet, psbt: &Psbt, delays: Delays, ceiling: FeeCeiling) -> Result<(), Error> {
    if delays.heir().is_none() {
        return Err(Error::NoCheckinWithoutHeir);
    }
    if !is_checkin(psbt) {
        return Err(Error::NotACheckin);
    }
    spend::check_inputs(wallet, psbt)?;
    if spend::path_of(psbt, delays) != Some(Path::TwoOfThree) {
        return Err(Error::CheckinUsesDelayedPath);
    }
    let tx = &psbt.unsigned_tx;
    if tx.output.len() != 1 || !wallet.is_mine(tx.output[0].script_pubkey.clone()) {
        return Err(Error::CheckinPaysOut);
    }
    for (input, txin) in tx.input.iter().enumerate() {
        match wallet.get_utxo(txin.previous_output) {
            None => return Err(Error::UnknownCoin { input }),
            Some(utxo) if !utxo.chain_position.is_confirmed() => {
                return Err(Error::CheckinSpendsUnconfirmed { coin: utxo.outpoint });
            }
            Some(_) => {}
        }
    }
    for utxo in wallet.list_unspent().filter(|u| u.chain_position.is_confirmed()) {
        if !tx.input.iter().any(|i| i.previous_output == utxo.outpoint) {
            return Err(Error::CheckinMissesCoin { coin: utxo.outpoint });
        }
    }
    ceiling.check(psbt)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delays() -> Delays {
        Delays::with_heir(20).unwrap()
    }

    #[test]
    fn default_interval_fits_the_default_heir_delay() {
        let d = Delays::with_heir(crate::DEFAULT_HEIR_DELAY).unwrap();
        let rule = CheckinRule::new(DEFAULT_CHECKIN_INTERVAL, DEFAULT_CHECKIN_MARGIN, d).unwrap();
        assert_eq!((rule.interval(), rule.margin()), (52_560, 8_760));
        assert_eq!(52_560 + 8_760, crate::DEFAULT_HEIR_DELAY, "the defaults use the whole heir delay");
    }

    #[test]
    fn savings_with_no_beneficiary_has_no_check_in() {
        assert!(matches!(CheckinRule::new(5, 2, Delays::none()), Err(Error::NoCheckinWithoutHeir)));
        let s = status(Delays::none(), None, 70_000);
        assert_eq!(s.heir_opens_in, None);
        assert!(!s.overdue && !s.checkin_due, "nothing ever opens with time");
    }

    #[test]
    fn refuses_an_interval_at_or_past_the_heir_delay() {
        for interval in [20, 21, u16::MAX] {
            assert!(
                matches!(CheckinRule::new(interval, 1, delays()), Err(Error::CheckinTooLate { .. })),
                "interval {interval}"
            );
        }
        assert!(CheckinRule::new(19, 1, delays()).is_ok());
    }

    #[test]
    fn refuses_an_interval_that_leaves_less_than_the_margin() {
        assert!(matches!(CheckinRule::new(18, 3, delays()), Err(Error::CheckinTooLate { .. })));
        assert!(CheckinRule::new(17, 3, delays()).is_ok());
        let d = Delays::with_heir(crate::DEFAULT_HEIR_DELAY).unwrap();
        assert!(matches!(
            CheckinRule::new(DEFAULT_CHECKIN_INTERVAL + 1, DEFAULT_CHECKIN_MARGIN, d),
            Err(Error::CheckinTooLate { .. })
        ));
    }

    #[test]
    fn the_default_refresh_window_means_coins_older_than_about_10_months() {
        let d = Delays::with_heir(crate::DEFAULT_HEIR_DELAY).unwrap();
        let checkin = CheckinRule::new(DEFAULT_CHECKIN_INTERVAL, DEFAULT_CHECKIN_MARGIN, d).unwrap();
        let rule = PaymentRule::new(DEFAULT_REFRESH_WINDOW, checkin).unwrap();
        assert_eq!((rule.window(), rule.refresh_age()), (8_760, 43_800));
    }

    #[test]
    fn the_refresh_window_must_fit_inside_the_check_in_interval() {
        let checkin = CheckinRule::new(10, 5, delays()).unwrap();
        assert!(matches!(PaymentRule::new(0, checkin), Err(Error::RefreshWindowZero)));
        for window in [10, 11, u16::MAX] {
            assert!(
                matches!(PaymentRule::new(window, checkin), Err(Error::RefreshWindowTooLong { .. })),
                "window {window}"
            );
        }
        assert_eq!(PaymentRule::new(9, checkin).unwrap().refresh_age(), 1);
    }

    #[test]
    fn the_default_interval_fits_the_heir_delay() {
        let heir = |blocks| Delays::with_heir(blocks).unwrap();
        let interval = |blocks| default_interval(heir(blocks), DEFAULT_CHECKIN_MARGIN).unwrap();
        // The minimum 6-month heir delay: a check-in about every 4 months.
        assert_eq!(interval(crate::DEFAULT_MIN_HEIR_DELAY), 17_520);
        assert_eq!(interval(40_000), 31_240);
        assert_eq!(interval(crate::DEFAULT_HEIR_DELAY - 1), 52_559);
        // From the placeholder heir delay up, about once a year.
        assert_eq!(interval(crate::DEFAULT_HEIR_DELAY), 52_560);
        assert_eq!(interval(u16::MAX), 52_560);
        // Short test delays: the heir delay minus the margin.
        assert_eq!(default_interval(heir(20), 5).unwrap(), 15);
        assert_eq!(default_interval(heir(6), 5).unwrap(), 1);
    }

    #[test]
    fn the_default_interval_needs_room_after_the_margin() {
        let heir = |blocks| Delays::with_heir(blocks).unwrap();
        for blocks in [5, 4, 1] {
            assert!(
                matches!(default_interval(heir(blocks), 5), Err(Error::NoRoomForCheckins { margin: 5, .. })),
                "heir delay {blocks}"
            );
        }
        assert!(matches!(default_interval(heir(20), 0), Err(Error::CheckinMarginZero)));
        assert!(matches!(default_interval(Delays::none(), 5), Err(Error::NoCheckinWithoutHeir)));
    }

    #[test]
    fn with_the_default_rules_every_heir_delay_from_the_minimum_gets_check_ins_that_fit() {
        let rules = HeirDelayRules::DEFAULT;
        let heir = |blocks| Delays::with_heir(blocks).unwrap();
        // Refused before brief 3.27, when the interval was always a year.
        for (blocks, interval) in [(crate::DEFAULT_MIN_HEIR_DELAY, 17_520), (40_000, 31_240), (61_319, 52_559)] {
            let rule = rules.check(heir(blocks)).unwrap();
            assert_eq!((rule.interval(), rule.margin()), (interval, 8_760), "heir delay {blocks}");
        }
        let rule = rules.check(heir(crate::DEFAULT_HEIR_DELAY)).unwrap();
        assert_eq!((rule.interval(), rule.margin()), (52_560, 8_760));
        assert!(matches!(rules.check(heir(crate::DEFAULT_MIN_HEIR_DELAY - 1)), Err(Error::HeirDelayTooShort { .. })));
        assert!(matches!(rules.check(Delays::none()), Err(Error::HeirAndDelayMismatch)));
    }

    #[test]
    fn a_chosen_interval_must_still_fit() {
        let heir = |blocks| Delays::with_heir(blocks).unwrap();
        let rules = HeirDelayRules { checkin_interval: Some(17_000), ..HeirDelayRules::DEFAULT };
        assert!(rules.check(heir(crate::DEFAULT_MIN_HEIR_DELAY)).is_ok());
        assert!(matches!(rules.check(heir(crate::DEFAULT_MIN_HEIR_DELAY - 1)), Err(Error::HeirDelayTooShort { .. })), "the minimum still applies");
        // A yearly check-in chosen for the minimum heir delay doesn't fit.
        for interval in [17_521, DEFAULT_CHECKIN_INTERVAL] {
            assert!(
                matches!(
                    HeirDelayRules { checkin_interval: Some(interval), ..rules }.check(heir(crate::DEFAULT_MIN_HEIR_DELAY)),
                    Err(Error::CheckinTooLate { .. })
                ),
                "interval {interval}"
            );
        }
    }

    #[test]
    fn refuses_zero() {
        assert!(matches!(CheckinRule::new(0, 1, delays()), Err(Error::CheckinIntervalZero)));
        assert!(matches!(CheckinRule::new(5, 0, delays()), Err(Error::CheckinMarginZero)));
    }

    #[test]
    fn without_a_rule_nothing_is_due_but_overdue_still_shows() {
        // Even with a 1-block heir delay, where no rule could fit.
        let d = Delays::with_heir(1).unwrap();
        assert!(!status(d, None, 0).overdue);
        let s = status(d, None, 1);
        assert!(s.overdue && !s.checkin_due);
        assert_eq!(s.heir_opens_in, Some(0));
    }

    #[test]
    fn due_at_the_interval_and_overdue_at_the_heir_delay() {
        let rule = CheckinRule::new(15, 5, delays()).unwrap();
        let s = |c| rule.status(delays(), c);
        assert!(!s(0).checkin_due, "unconfirmed coins aren't due");
        assert!(!s(14).checkin_due);
        assert!(s(15).checkin_due && !s(15).overdue);
        assert!(s(19).checkin_due && !s(19).overdue);
        assert!(s(20).overdue);
        assert_eq!((s(4).heir_opens_in, s(25).heir_opens_in), (Some(16), Some(0)));
    }
}
