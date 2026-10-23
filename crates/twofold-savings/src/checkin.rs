//! The check-in: a send-to-self of every Savings coin through the 2-of-3
//! path, which restarts every coin's delay clock.
//!
//! Delays are relative: each coin's clock starts when that coin confirms.
//! A check-in spends every coin into one new Savings coin, so the device
//! and heir delays start again from zero. It must happen before the oldest
//! coin reaches the device delay, or DEVICE alone could spend it.

use bdk_wallet::{KeychainKind, Wallet};

use crate::bitcoin::psbt::{Psbt, raw::ProprietaryKey};
use crate::bitcoin::{Amount, FeeRate, OutPoint};
use crate::spend::{self, Path};
use crate::{Delays, Error};

/// Placeholder check-in interval: about 6 months of blocks. Not final until
/// an outside Bitcoin reviewer signs off.
pub const DEFAULT_CHECKIN_INTERVAL: u16 = 26_280;

/// Placeholder margin the interval must leave before the device delay:
/// about 30 days of blocks, for fees, confirmations and a late check-in.
pub const DEFAULT_CHECKIN_MARGIN: u16 = 4_320;

/// How often check-ins are due, and how much room that leaves before the
/// device delay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckinRule {
    interval: u16,
    margin: u16,
}

impl CheckinRule {
    /// Refuses unless `interval + margin <= device delay`, with both at least
    /// one block. So an interval equal to or longer than the device delay is
    /// always refused.
    pub fn new(interval: u16, margin: u16, delays: Delays) -> Result<Self, Error> {
        if interval == 0 {
            return Err(Error::CheckinIntervalZero);
        }
        if margin == 0 {
            return Err(Error::CheckinMarginZero);
        }
        if u32::from(interval) + u32::from(margin) > u32::from(delays.device()) {
            return Err(Error::CheckinTooLate { interval, margin, device: delays.device() });
        }
        Ok(Self { interval, margin })
    }

    /// Blocks after a coin confirms before its check-in is due.
    pub fn interval(&self) -> u16 {
        self.interval
    }

    /// Blocks the interval leaves before the device delay, at least.
    pub fn margin(&self) -> u16 {
        self.margin
    }

    /// Where a coin with `confirmations` stands. Confirmations are counted
    /// as of the next block, the same way delays are checked.
    pub fn status(&self, delays: Delays, confirmations: u32) -> CoinStatus {
        let left = |blocks: u16| u32::from(blocks).saturating_sub(confirmations);
        CoinStatus {
            confirmations,
            device_opens_in: left(delays.device()),
            heir_opens_in: left(delays.heir()),
            checkin_due: confirmations > 0 && confirmations >= u32::from(self.interval),
            overdue: confirmations > 0 && confirmations >= u32::from(delays.device()),
        }
    }
}

/// One coin's position against the delays and the check-in interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoinStatus {
    /// 0 while unconfirmed.
    pub confirmations: u32,
    /// Blocks until DEVICE alone could spend it; 0 means it already can.
    pub device_opens_in: u32,
    /// Blocks until HEIR alone could spend it; 0 means it already can.
    pub heir_opens_in: u32,
    /// The coin has reached the check-in interval.
    pub checkin_due: bool,
    /// The coin's device path is already open.
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
pub fn coins(wallet: &Wallet, delays: Delays, rule: CheckinRule) -> Vec<Coin> {
    let next_block = wallet.latest_checkpoint().height() + 1;
    wallet
        .list_unspent()
        .map(|utxo| {
            let confirmations =
                utxo.chain_position.confirmation_height_upper_bound().map_or(0, |c| next_block.saturating_sub(c));
            Coin { outpoint: utxo.outpoint, amount: utxo.txout.value, status: rule.status(delays, confirmations) }
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
    /// Some coin's device path is already open: DEVICE alone could spend it.
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
    ProprietaryKey { prefix: b"twofold".to_vec(), subtype: 0, key: b"checkin".to_vec() }
}

/// Whether `psbt` was built by [`build_checkin`].
pub fn is_checkin(psbt: &Psbt) -> bool {
    psbt.proprietary.contains_key(&checkin_marker())
}

/// Builds a check-in: every unspent Savings coin, through the 2-of-3 path,
/// into one new Savings change address.
pub fn build_checkin(wallet: &mut Wallet, delays: Delays, fee_rate: FeeRate) -> Result<Psbt, Error> {
    if wallet.list_unspent().next().is_none() {
        return Err(Error::CheckinNoCoins);
    }
    let to = wallet.reveal_next_address(KeychainKind::Internal).address.script_pubkey();
    let mut psbt = spend::build_spend(wallet, delays, Path::TwoOfThree, |b| {
        b.drain_wallet().drain_to(to.clone()).fee_rate(fee_rate);
    })?;
    psbt.proprietary.insert(checkin_marker(), Vec::new());
    check_checkin(wallet, &psbt, delays)?;
    Ok(psbt)
}

/// Checks a check-in is complete: marked as one, spending every unspent
/// Savings coin and nothing else, through the 2-of-3 path, into exactly one
/// output back to this wallet. Run it again just before signing and
/// broadcasting, since a coin may have arrived since it was built.
pub fn check_checkin(wallet: &Wallet, psbt: &Psbt, delays: Delays) -> Result<(), Error> {
    if !is_checkin(psbt) {
        return Err(Error::NotACheckin);
    }
    if spend::path_of(psbt, delays) != Some(Path::TwoOfThree) {
        return Err(Error::CheckinUsesDelayedPath);
    }
    let tx = &psbt.unsigned_tx;
    if tx.output.len() != 1 || !wallet.is_mine(tx.output[0].script_pubkey.clone()) {
        return Err(Error::CheckinPaysOut);
    }
    for (input, txin) in tx.input.iter().enumerate() {
        if wallet.get_utxo(txin.previous_output).is_none() {
            return Err(Error::UnknownCoin { input });
        }
    }
    for utxo in wallet.list_unspent() {
        if !tx.input.iter().any(|i| i.previous_output == utxo.outpoint) {
            return Err(Error::CheckinMissesCoin { coin: utxo.outpoint });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delays() -> Delays {
        Delays::new(10, 20).unwrap()
    }

    #[test]
    fn default_interval_fits_the_default_device_delay() {
        let rule = CheckinRule::new(DEFAULT_CHECKIN_INTERVAL, DEFAULT_CHECKIN_MARGIN, Delays::default()).unwrap();
        assert_eq!((rule.interval(), rule.margin()), (26_280, 4_320));
    }

    #[test]
    fn refuses_an_interval_at_or_past_the_device_delay() {
        for interval in [10, 11, u16::MAX] {
            assert!(
                matches!(CheckinRule::new(interval, 1, delays()), Err(Error::CheckinTooLate { .. })),
                "interval {interval}"
            );
        }
        assert!(CheckinRule::new(9, 1, delays()).is_ok());
    }

    #[test]
    fn refuses_an_interval_that_leaves_less_than_the_margin() {
        assert!(matches!(CheckinRule::new(8, 3, delays()), Err(Error::CheckinTooLate { .. })));
        assert!(CheckinRule::new(7, 3, delays()).is_ok());
        let d = Delays::default();
        assert!(matches!(CheckinRule::new(35_101, DEFAULT_CHECKIN_MARGIN, d), Err(Error::CheckinTooLate { .. })));
        assert!(CheckinRule::new(35_100, DEFAULT_CHECKIN_MARGIN, d).is_ok());
    }

    #[test]
    fn refuses_zero() {
        assert!(matches!(CheckinRule::new(0, 1, delays()), Err(Error::CheckinIntervalZero)));
        assert!(matches!(CheckinRule::new(5, 0, delays()), Err(Error::CheckinMarginZero)));
    }

    #[test]
    fn due_at_the_interval_and_overdue_at_the_device_delay() {
        let rule = CheckinRule::new(5, 2, delays()).unwrap();
        let s = |c| rule.status(delays(), c);
        assert!(!s(0).checkin_due, "unconfirmed coins aren't due");
        assert!(!s(4).checkin_due);
        assert!(s(5).checkin_due && !s(5).overdue);
        assert!(s(9).checkin_due && !s(9).overdue);
        assert!(s(10).overdue);
        assert_eq!((s(4).device_opens_in, s(4).heir_opens_in), (6, 16));
        assert_eq!((s(10).device_opens_in, s(25).heir_opens_in), (0, 0));
    }
}
