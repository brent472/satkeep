//! Building and finalizing Savings spends, with Twofold's own delay check.
//!
//! BDK alone does not stop a delayed-path spend before the delay is met.
//! When it finalizes, miniscript combines BDK's height check with the
//! PSBT's own sequence check using OR, so a sequence that matches the
//! leaf's delay is enough, however young the coin. The network would then
//! reject the transaction (BIP 68), so funds are never at risk, but Twofold
//! would hand the user a spend that cannot confirm. [`build_spend`] and
//! [`finalize`] both run [`check_delays`] so Twofold refuses first, with a
//! clear reason.

use bdk_wallet::coin_selection::DefaultCoinSelectionAlgorithm;
use bdk_wallet::descriptor::policy::Condition;
use bdk_wallet::{SignOptions, TxBuilder, Wallet};

use crate::bitcoin::psbt::Psbt;
use crate::bitcoin::{Sequence, Transaction, relative};
use crate::{Delays, Error};

/// Which Savings spending path a payment uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    /// Any 2 of PHONE, DEVICE and HELPER. No delay.
    TwoOfThree,
    /// DEVICE alone, once each coin has waited the device delay.
    Device,
    /// HEIR alone, once each coin has waited the heir delay.
    Heir,
}

impl Path {
    /// The input sequence this path needs, or `None` for no delay.
    pub fn sequence(self, delays: Delays) -> Option<Sequence> {
        match self {
            Path::TwoOfThree => None,
            Path::Device => Some(Sequence::from_height(delays.device())),
            Path::Heir => Some(Sequence::from_height(delays.heir())),
        }
    }
}

/// The path a PSBT's inputs ask for, read from their sequences: a sequence
/// equal to the device or heir delay means that delayed path, anything else
/// the 2-of-3 path. `None` if the inputs disagree, which [`build_spend`]
/// never produces.
pub fn path_of(psbt: &Psbt, delays: Delays) -> Option<Path> {
    let path_of_input = |sequence: Sequence| {
        [Path::Device, Path::Heir].into_iter().find(|p| p.sequence(delays) == Some(sequence)).unwrap_or(Path::TwoOfThree)
    };
    let mut paths = psbt.unsigned_tx.input.iter().map(|i| path_of_input(i.sequence));
    let first = paths.next()?;
    paths.all(|p| p == first).then_some(first)
}

/// Builds an unsigned spend through `path`. `configure` adds the recipients
/// and fee rate. Refuses if any coin chosen is too young for the path's
/// delay.
pub fn build_spend(
    wallet: &mut Wallet,
    delays: Delays,
    path: Path,
    configure: impl FnOnce(&mut TxBuilder<'_, DefaultCoinSelectionAlgorithm>),
) -> Result<Psbt, Error> {
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
pub fn finalize(wallet: &Wallet, psbt: &Psbt) -> Result<Transaction, Error> {
    check_delays(wallet, psbt)?;
    let mut psbt = psbt.clone();
    let options = SignOptions {
        // Relative delays are judged against the block the spend would go in.
        assume_height: Some(wallet.latest_checkpoint().height() + 1),
        ..Default::default()
    };
    if !wallet.finalize_psbt(&mut psbt, options)? {
        return Err(Error::NotEnoughApprovals);
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
        let d = Delays::new(10, 20).unwrap();
        let (device, heir) = (Sequence::from_height(10), Sequence::from_height(20));
        assert_eq!(path_of(&psbt_with(&[Sequence::ENABLE_RBF_NO_LOCKTIME]), d), Some(Path::TwoOfThree));
        assert_eq!(path_of(&psbt_with(&[device, device]), d), Some(Path::Device));
        assert_eq!(path_of(&psbt_with(&[heir]), d), Some(Path::Heir));
        assert_eq!(path_of(&psbt_with(&[device, heir]), d), None, "inputs disagree");
        assert_eq!(path_of(&psbt_with(&[]), d), None);
    }
}
