//! Building and finalizing Savings spends, with Twofold's own delay check.
//!
//! When BDK finalizes, miniscript combines BDK's height check with the
//! PSBT's own sequence check using OR, which goes wrong both ways:
//!
//! - A sequence that matches a leaf's delay is enough, however young the
//!   coin (the network rejects it: `non-BIP68-final`).
//! - The coin's age is enough, even when the sequence carries no delay
//!   (the network rejects it: CSV fails).
//!
//! Funds are never at risk, because the network enforces both, but Twofold
//! would hand the user a transaction that can't confirm. So [`build_spend`]
//! and [`finalize`] run [`check_delays`] (sequence against coin age), and
//! [`finalize`] uses miniscript's PSBT-only finalizer (leaf against
//! sequence). Together they match the network's rule.

use bdk_wallet::coin_selection::DefaultCoinSelectionAlgorithm;
use bdk_wallet::descriptor::policy::Condition;
use bdk_wallet::{TxBuilder, Wallet};

use crate::bitcoin::psbt::Psbt;
use crate::miniscript::psbt::{InputError, PsbtExt};
use crate::bitcoin::{Sequence, Transaction, relative};
use crate::{Delays, Error};

/// Which Savings spending path a payment uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    /// Any 2 of PHONE, DEVICE and HELPER. No delay.
    TwoOfThree,
    /// HEIR alone, once each coin has waited the heir delay. Only in Savings
    /// with a beneficiary.
    Heir,
}

impl Path {
    /// The input sequence this path needs, or `None` for no delay (and for
    /// the heir path in Savings with no beneficiary, which has no such path;
    /// [`build_spend`] refuses it).
    pub fn sequence(self, delays: Delays) -> Option<Sequence> {
        match self {
            Path::TwoOfThree => None,
            Path::Heir => delays.heir().map(Sequence::from_height),
        }
    }
}

/// The path a PSBT's inputs ask for, read from their sequences: a sequence
/// equal to the heir delay means the heir path, anything else the 2-of-3
/// path. `None` if the inputs disagree, which [`build_spend`] never
/// produces.
pub fn path_of(psbt: &Psbt, delays: Delays) -> Option<Path> {
    let path_of_input = |sequence: Sequence| {
        if Path::Heir.sequence(delays) == Some(sequence) { Path::Heir } else { Path::TwoOfThree }
    };
    let mut paths = psbt.unsigned_tx.input.iter().map(|i| path_of_input(i.sequence));
    let first = paths.next()?;
    paths.all(|p| p == first).then_some(first)
}

/// Builds an unsigned spend through `path`. `configure` adds the recipients
/// and fee rate. Refuses if any coin chosen is too young for the path's
/// delay, and refuses the heir path in Savings with no beneficiary.
pub fn build_spend(
    wallet: &mut Wallet,
    delays: Delays,
    path: Path,
    configure: impl FnOnce(&mut TxBuilder<'_, DefaultCoinSelectionAlgorithm>),
) -> Result<Psbt, Error> {
    if path == Path::Heir && delays.heir().is_none() {
        return Err(Error::NoHeirPath);
    }
    let mut builder = wallet.build_tx();
    configure(&mut builder);
    builder.set_condition(Condition { csv: path.sequence(delays), timelock: None });
    let psbt = builder.finish()?;
    check_delays(wallet, &psbt)?;
    Ok(psbt)
}

/// Checks every input of `psbt` is old enough for the delay its sequence
/// asks for, as of the next block. Inputs without a relative delay pass.
pub fn check_delays(wallet: &Wallet, psbt: &Psbt) -> Result<(), Error> {
    let next_block = wallet.latest_checkpoint().height() + 1;
    for (input, txin) in psbt.unsigned_tx.input.iter().enumerate() {
        let needed = match txin.sequence.to_relative_lock_time() {
            None => continue,
            Some(relative::LockTime::Blocks(h)) => h.value(),
            Some(relative::LockTime::Time(_)) => return Err(Error::TimeBasedDelay { input }),
        };
        // If BDK only knows an upper bound for the confirmation height, the
        // age below can only come out too young, never too old.
        let confirmed_at = wallet
            .get_tx(txin.previous_output.txid)
            .ok_or(Error::UnknownCoin { input })?
            .chain_position
            .confirmation_height_upper_bound();
        // A coin confirmed at height c has (next_block - c) confirmations by
        // the time a spend can be mined in next_block, which is what BIP 68
        // compares against the sequence.
        let has = confirmed_at.map_or(0, |c| next_block.saturating_sub(c));
        if has < u32::from(needed) {
            return Err(Error::TooEarly { input, needed, has });
        }
    }
    Ok(())
}

/// Finalizes a signed Savings PSBT into a transaction ready to broadcast.
///
/// Refuses if any coin is too young for its delay ([`check_delays`]) or if
/// the signatures present don't satisfy any spending path.
///
/// Uses miniscript's PSBT finalizer rather than BDK's: its only satisfier
/// is the PSBT itself, so a delayed leaf is used only when the input's
/// sequence commits to that delay, exactly as the network requires (CSV).
/// BDK's finalizer also accepts a delayed leaf on the coin's age alone,
/// even when the sequence carries no delay, and would produce a transaction
/// the network rejects.
pub fn finalize(wallet: &Wallet, psbt: &Psbt) -> Result<Transaction, Error> {
    check_delays(wallet, psbt)?;
    let mut psbt = psbt.clone();
    let secp = crate::bitcoin::key::Secp256k1::verification_only();
    if let Err(errors) = psbt.finalize_mut(&secp) {
        // Taproot and P2WSH inputs report "not enough signatures" differently.
        let unsatisfied = errors.iter().all(|e| {
            matches!(
                e,
                crate::miniscript::psbt::Error::InputError(InputError::CouldNotSatisfyTr, _)
                    | crate::miniscript::psbt::Error::InputError(
                        InputError::MiniscriptError(crate::miniscript::Error::CouldNotSatisfy),
                        _
                    )
            )
        });
        if unsatisfied {
            return Err(Error::NotEnoughApprovals);
        }
        let reasons: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        return Err(Error::CannotFinalize(reasons.join("; ")));
    }
    Ok(psbt.extract_tx()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitcoin::{OutPoint, Transaction, TxIn, absolute, transaction};

    fn psbt_with(sequences: &[Sequence]) -> Psbt {
        let input = sequences.iter().map(|&sequence| TxIn { previous_output: OutPoint::null(), sequence, ..Default::default() });
        let tx = Transaction {
            version: transaction::Version::TWO,
            lock_time: absolute::LockTime::ZERO,
            input: input.collect(),
            output: vec![],
        };
        Psbt::from_unsigned_tx(tx).unwrap()
    }

    #[test]
    fn reads_the_path_from_the_input_sequences() {
        let d = Delays::with_heir(20).unwrap();
        let (other, heir) = (Sequence::from_height(10), Sequence::from_height(20));
        assert_eq!(path_of(&psbt_with(&[Sequence::ENABLE_RBF_NO_LOCKTIME]), d), Some(Path::TwoOfThree));
        assert_eq!(path_of(&psbt_with(&[other]), d), Some(Path::TwoOfThree), "no other delayed path");
        assert_eq!(path_of(&psbt_with(&[heir, heir]), d), Some(Path::Heir));
        assert_eq!(path_of(&psbt_with(&[other, heir]), d), None, "inputs disagree");
        assert_eq!(path_of(&psbt_with(&[]), d), None);
    }

    #[test]
    fn without_a_beneficiary_every_sequence_reads_as_2_of_3() {
        let none = Delays::none();
        assert_eq!(Path::Heir.sequence(none), None);
        for seq in [Sequence::ENABLE_RBF_NO_LOCKTIME, Sequence::from_height(20), Sequence::from_height(u16::MAX)] {
            assert_eq!(path_of(&psbt_with(&[seq]), none), Some(Path::TwoOfThree));
        }
    }
}
