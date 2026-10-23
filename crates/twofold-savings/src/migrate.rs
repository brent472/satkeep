//! Adding or changing a beneficiary: Savings moves to a new wallet with the
//! heir path, in one transaction.
//!
//! Savings starts as plain 2 of 3. Adding a beneficiary can't change an
//! existing wallet's script, so it makes a new wallet with the same PHONE,
//! DEVICE and HELPER keys plus HEIR and the heir delay, and one 2-of-3
//! transaction moves every confirmed coin there, for one network fee.
//! Changing the beneficiary works the same way.
//!
//! The PSBT carries the new HEIR key and heir delay in a proprietary field
//! (ignored by other software), so [`check_move`] can rebuild the new wallet
//! from the old one and check, before signing and again before
//! broadcasting, that everything goes to that wallet and nowhere else.

use std::str::FromStr;

use bdk_wallet::Wallet;

use crate::bitcoin::psbt::{Psbt, raw::ProprietaryKey};
use crate::bitcoin::{FeeRate, ScriptBuf};
use crate::miniscript::descriptor::DescriptorPublicKey;
use crate::spend::{self, Path};
use crate::{Delays, Error, SavingsPolicy};

fn move_marker() -> ProprietaryKey {
    ProprietaryKey { prefix: b"twofold".to_vec(), subtype: 0, key: b"move".to_vec() }
}

/// The wallet Savings moves to: the same PHONE, DEVICE and HELPER as `from`,
/// plus `heir` after `heir_delay`. Refuses if that's `from` itself.
pub fn wallet_with_heir(from: &SavingsPolicy, heir: DescriptorPublicKey, heir_delay: Delays) -> Result<SavingsPolicy, Error> {
    if heir_delay.heir().is_none() {
        return Err(Error::HeirAndDelayMismatch);
    }
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

/// Builds the move: every confirmed coin in `from`'s wallet, through the
/// 2-of-3 path, to `to`'s first receive address. Unconfirmed coins are left
/// out, as in a check-in (see [`crate::checkin::build_checkin`]); they stay
/// spendable by 2 of 3 and can be moved once they confirm.
pub fn build_move(wallet: &mut Wallet, from: &SavingsPolicy, to: &SavingsPolicy, fee_rate: FeeRate) -> Result<Psbt, Error> {
    let heir = to.keys().get(crate::Role::Heir).ok_or(Error::MoveToWrongWallet)?.clone();
    let expected = wallet_with_heir(from, heir.clone(), to.delays())?;
    if expected.descriptor() != to.descriptor() {
        return Err(Error::MoveToWrongWallet);
    }
    if !wallet.list_unspent().any(|u| u.chain_position.is_confirmed()) {
        return Err(Error::MoveNoCoins);
    }
    let pay_to = destination(to);
    let mut psbt = spend::build_spend(wallet, from.delays(), Path::TwoOfThree, |b| {
        b.drain_wallet().exclude_unconfirmed().drain_to(pay_to.clone()).fee_rate(fee_rate);
    })?;
    let heir_delay = to.delays().heir().expect("checked above");
    psbt.proprietary.insert(move_marker(), format!("{heir}\n{heir_delay}").into_bytes());
    check_move(wallet, &psbt, from)?;
    Ok(psbt)
}

/// Checks a move is complete and goes where it says: marked as one, through
/// the 2-of-3 path, spending every confirmed coin in `from`'s wallet and
/// nothing else, into exactly one output, the first receive address of the
/// wallet rebuilt from `from` and the HEIR key and delay the PSBT carries.
/// Returns that wallet. Run it again just before signing and broadcasting.
pub fn check_move(wallet: &Wallet, psbt: &Psbt, from: &SavingsPolicy) -> Result<SavingsPolicy, Error> {
    let value = psbt.proprietary.get(&move_marker()).ok_or(Error::NotAMove)?;
    let text = std::str::from_utf8(value).map_err(|_| Error::NotAMove)?;
    let (heir, delay) = text.split_once('\n').ok_or(Error::NotAMove)?;
    let heir = DescriptorPublicKey::from_str(heir).map_err(|_| Error::MoveToWrongWallet)?;
    let delay = delay.parse::<u16>().map_err(|_| Error::MoveToWrongWallet)?;
    let to = wallet_with_heir(from, heir, Delays::with_heir(delay)?).map_err(|_| Error::MoveToWrongWallet)?;

    if spend::path_of(psbt, from.delays()) != Some(Path::TwoOfThree) {
        return Err(Error::MoveShape);
    }
    spend::check_inputs(wallet, psbt)?;
    let tx = &psbt.unsigned_tx;
    if tx.output.len() != 1 || tx.output[0].script_pubkey != destination(&to) {
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
    Ok(to)
}
