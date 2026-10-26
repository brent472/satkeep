//! Adding or changing a beneficiary: Savings moves to a new wallet with the
//! heir path, in one transaction.
//!
//! Savings starts as plain 2 of 3. Adding a beneficiary can't change an
//! existing wallet's script, so it makes a new wallet with the same PHONE,
//! DEVICE and HELPER keys plus HEIR and the heir delay, and one 2-of-3
//! transaction moves every confirmed coin there, for one network fee.
//! Changing the beneficiary works the same way.
//!
//! Where the move goes is never taken from the PSBT. Each signer rebuilds
//! the new wallet from its own settings: this wallet's PHONE, DEVICE and
//! HELPER keys plus the beneficiary and heir delay set up for the move
//! ([`wallet_with_heir`]). [`check_move`] then refuses, before signing and
//! again before broadcasting, unless everything goes to that wallet. The
//! PSBT also carries the HEIR key and delay it was built for, in a
//! proprietary field ignored by other software; it marks the PSBT as a move
//! and must name the same beneficiary, but it's only ever compared, never
//! trusted.
//!
//! Replacing a key will be a move too, with the replacement key in the same
//! settings; until it's built, a move to a wallet with other keys is
//! refused.

use std::str::FromStr;

use bdk_wallet::Wallet;

use crate::bitcoin::psbt::{Psbt, raw::ProprietaryKey};
use crate::bitcoin::{FeeRate, ScriptBuf};
use crate::miniscript::descriptor::DescriptorPublicKey;
use crate::spend::{self, FeeCeiling, Path};
use crate::{Delays, Error, SavingsPolicy};

fn move_marker() -> ProprietaryKey {
    ProprietaryKey { prefix: b"satkeep".to_vec(), subtype: 0, key: b"move".to_vec() }
}

/// The wallet Savings moves to: the same PHONE, DEVICE and HELPER as `from`,
/// plus `heir` after `heir_delay`, which must be at least `min_heir_delay`
/// blocks (see [`crate::DEFAULT_MIN_HEIR_DELAY`]). Build it from the
/// signer's own settings, never from a PSBT. Refuses if that's `from`
/// itself.
pub fn wallet_with_heir(
    from: &SavingsPolicy,
    heir: DescriptorPublicKey,
    heir_delay: Delays,
    min_heir_delay: u16,
) -> Result<SavingsPolicy, Error> {
    if heir_delay.heir().is_none() {
        return Err(Error::HeirAndDelayMismatch);
    }
    heir_delay.require_heir_at_least(min_heir_delay)?;
    let to = SavingsPolicy::new(from.keys().with_heir(heir)?, heir_delay, from.network())?;
    if to.descriptor() == from.descriptor() {
        return Err(Error::MoveToWrongWallet);
    }
    Ok(to)
}

/// Where a move pays: the new wallet's first receive address.
pub fn destination(to: &SavingsPolicy) -> ScriptBuf {
    let (receive, _) = to.receive_and_change();
    receive.at_derivation_index(0).expect("index 0 is unhardened").script_pubkey()
}

/// Whether `psbt` was built by [`build_move`].
pub fn is_move(psbt: &Psbt) -> bool {
    psbt.proprietary.contains_key(&move_marker())
}

/// Whether any of `psbt`'s outputs pays `to`'s first receive address. A
/// signer with a move set up treats such a PSBT as a move even if it isn't
/// marked as one, so it gets the same checks.
pub fn pays_to(psbt: &Psbt, to: &SavingsPolicy) -> bool {
    let destination = destination(to);
    psbt.unsigned_tx.output.iter().any(|o| o.script_pubkey == destination)
}

/// Checks `to` is a wallet a move from `from` may go to: `from`'s PHONE,
/// DEVICE and HELPER plus a beneficiary, with an heir delay of at least
/// `min_heir_delay`.
fn check_target(from: &SavingsPolicy, to: &SavingsPolicy, min_heir_delay: u16) -> Result<(DescriptorPublicKey, u16), Error> {
    let heir = to.keys().get(crate::Role::Heir).ok_or(Error::MoveToWrongWallet)?.clone();
    let heir_delay = to.delays().heir().ok_or(Error::MoveToWrongWallet)?;
    let expected = match wallet_with_heir(from, heir.clone(), to.delays(), min_heir_delay) {
        Err(e @ Error::HeirDelayTooShort { .. }) => return Err(e),
        Err(_) => return Err(Error::MoveToWrongWallet),
        Ok(expected) => expected,
    };
    if expected.descriptor() != to.descriptor() {
        return Err(Error::MoveToWrongWallet);
    }
    Ok((heir, heir_delay))
}

/// Builds the move: every confirmed coin in `from`'s wallet, through the
/// 2-of-3 path, to `to`'s first receive address. Unconfirmed coins are left
/// out, as in a check-in (see [`crate::checkin::build_checkin`]); they stay
/// spendable by 2 of 3 and can be moved once they confirm. Refuses if the
/// fee at `fee_rate` would be above `ceiling`.
pub fn build_move(
    wallet: &mut Wallet,
    from: &SavingsPolicy,
    to: &SavingsPolicy,
    min_heir_delay: u16,
    fee_rate: FeeRate,
    ceiling: FeeCeiling,
) -> Result<Psbt, Error> {
    let (heir, heir_delay) = check_target(from, to, min_heir_delay)?;
    if !wallet.list_unspent().any(|u| u.chain_position.is_confirmed()) {
        return Err(Error::MoveNoCoins);
    }
    let pay_to = destination(to);
    let mut psbt = spend::build_spend(wallet, from.delays(), Path::TwoOfThree, |b| {
        b.drain_wallet().exclude_unconfirmed().drain_to(pay_to.clone()).fee_rate(fee_rate);
    })?;
    psbt.proprietary.insert(move_marker(), format!("{heir}\n{heir_delay}").into_bytes());
    check_move(wallet, &psbt, from, to, min_heir_delay, ceiling)?;
    Ok(psbt)
}

/// Checks a move is complete and goes where this wallet's settings say:
/// marked as one, naming the same HEIR and delay as `to`, through the 2-of-3
/// path, spending every confirmed coin in `from`'s wallet and nothing else,
/// into exactly one output, `to`'s first receive address, for a fee no
/// higher than `ceiling`.
///
/// `to` must come from the signer's own settings ([`wallet_with_heir`]),
/// not from the PSBT; it's refused unless it has `from`'s PHONE, DEVICE and
/// HELPER, a beneficiary, and an heir delay of at least `min_heir_delay`.
/// Run it again just before signing and broadcasting.
pub fn check_move(
    wallet: &Wallet,
    psbt: &Psbt,
    from: &SavingsPolicy,
    to: &SavingsPolicy,
    min_heir_delay: u16,
    ceiling: FeeCeiling,
) -> Result<(), Error> {
    let (heir, heir_delay) = check_target(from, to, min_heir_delay)?;
    let value = psbt.proprietary.get(&move_marker()).ok_or(Error::NotAMove)?;
    let text = std::str::from_utf8(value).map_err(|_| Error::NotAMove)?;
    let (named_heir, named_delay) = text.split_once('\n').ok_or(Error::NotAMove)?;
    let same_heir = DescriptorPublicKey::from_str(named_heir).is_ok_and(|k| k == heir);
    let same_delay = named_delay.parse::<u16>().is_ok_and(|d| d == heir_delay);
    if !(same_heir && same_delay) {
        return Err(Error::MoveNotAsSetUp);
    }

    if spend::path_of(psbt, from.delays()) != Some(Path::TwoOfThree) {
        return Err(Error::MoveShape);
    }
    spend::check_inputs(wallet, psbt)?;
    let tx = &psbt.unsigned_tx;
    if tx.output.len() != 1 || tx.output[0].script_pubkey != destination(to) {
        return Err(Error::MoveShape);
    }
    for (input, txin) in tx.input.iter().enumerate() {
        match wallet.get_utxo(txin.previous_output) {
            None => return Err(Error::UnknownCoin { input }),
            Some(utxo) if !utxo.chain_position.is_confirmed() => {
                return Err(Error::MoveSpendsUnconfirmed { coin: utxo.outpoint });
            }
            Some(_) => {}
        }
    }
    for utxo in wallet.list_unspent().filter(|u| u.chain_position.is_confirmed()) {
        if !tx.input.iter().any(|i| i.previous_output == utxo.outpoint) {
            return Err(Error::MoveMissesCoin { coin: utxo.outpoint });
        }
    }
    ceiling.check(psbt)?;
    Ok(())
}
