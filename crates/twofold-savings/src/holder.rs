use std::str::FromStr;

use bdk_wallet::SignOptions;
use bdk_wallet::signer::{SignerContext, SignerWrapper, TransactionSigner};

use crate::Error;
use crate::bitcoin::NetworkKind;
use crate::bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use crate::bitcoin::key::Secp256k1;
use crate::bitcoin::psbt::Psbt;
use crate::miniscript::descriptor::{DescriptorPublicKey, DescriptorXKey, Wildcard};

/// The private key of one person or device in a Savings wallet, for test
/// networks only.
///
/// Public keys are derived at `m/87'/1'/account'` and used as
/// `…/<0;1>/*`. Every holder uses account 0; DEVICE also uses account 1 for
/// its delayed-path key (DEVICE_R). These paths are test-only placeholders.
///
/// Deliberately not `Debug` or `Display`, so it can't end up in a log or an
/// error message by accident. [`HolderKey::to_secret_string`] is the only way
/// to get the private key out.
pub struct HolderKey {
    master: Xpriv,
}

/// Accounts a holder may have a key on: 0 for everyone, 1 for DEVICE_R.
const ACCOUNTS: [u32; 2] = [0, 1];

impl HolderKey {
    /// A fresh random test-network key from the operating system's random
    /// source.
    pub fn generate() -> Self {
        use crate::bitcoin::secp256k1::rand::{RngCore, thread_rng};
        let mut seed = [0u8; 32];
        thread_rng().fill_bytes(&mut seed);
        Self { master: Xpriv::new_master(NetworkKind::Test, &seed).expect("32 random bytes make a valid seed") }
    }

    /// Reads a key written by [`HolderKey::to_secret_string`]. Refuses
    /// mainnet keys. The error never repeats the input.
    pub fn from_secret_str(s: &str) -> Result<Self, Error> {
        // Windows tools (PowerShell 5.1 pipes, Notepad) may put a byte-order
        // mark in front of the text.
        let text = s.trim().trim_start_matches('\u{feff}');
        let master = Xpriv::from_str(text).map_err(|_| Error::BadHolderKey)?;
        if master.network == NetworkKind::Main {
            return Err(Error::BadHolderKey);
        }
        if master.depth != 0 {
            return Err(Error::BadHolderKey);
        }
        Ok(Self { master })
    }

    /// The private key as text (`tprv…`). Anyone with it can approve as this
    /// holder: never print, log or commit it.
    pub fn to_secret_string(&self) -> String {
        self.master.to_string()
    }

    /// The public key for `account` as `[fingerprint/87'/1'/account']tpub…/<0;1>/*`.
    pub fn public_key(&self, account: u32) -> DescriptorPublicKey {
        let secp = Secp256k1::new();
        let xpub = Xpub::from_priv(&secp, &self.master.derive_priv(&secp, &account_path(account)).expect("hardened derivation"));
        let fp = self.master.fingerprint(&secp);
        DescriptorPublicKey::from_str(&format!("[{fp}/87'/1'/{account}']{xpub}/<0;1>/*"))
            .expect("a well-formed descriptor key")
    }

    /// Adds this holder's signatures to every script leaf of `psbt` it has a
    /// key in. Returns how many signatures were added.
    pub fn sign(&self, psbt: &mut Psbt) -> Result<usize, Error> {
        let before = signature_count(psbt);
        let secp = Secp256k1::new();
        let fp = self.master.fingerprint(&secp);
        for account in ACCOUNTS {
            let origin = account_path(account);
            let xkey = self.master.derive_priv(&secp, &origin).expect("hardened derivation");
            for branch in ["m/0", "m/1"] {
                // BDK only signs with a key whose origin and path match the
                // PSBT's exactly, so each branch needs its own signer.
                let key = DescriptorXKey {
                    origin: Some((fp, origin.clone())),
                    xkey,
                    derivation_path: DerivationPath::from_str(branch).expect("valid path"),
                    wildcard: Wildcard::Unhardened,
                };
                let signer = SignerWrapper::new(key, SignerContext::Tap { is_internal_key: false });
                signer.sign_transaction(psbt, &SignOptions::default(), &secp)?;
            }
        }
        Ok(signature_count(psbt) - before)
    }
}

fn account_path(account: u32) -> DerivationPath {
    DerivationPath::from_str(&format!("m/87'/1'/{account}'")).expect("valid path")
}

fn signature_count(psbt: &Psbt) -> usize {
    psbt.inputs.iter().map(|i| i.tap_script_sigs.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_its_secret_string() {
        let key = HolderKey::generate();
        let again = HolderKey::from_secret_str(&key.to_secret_string()).unwrap();
        assert_eq!(key.public_key(0), again.public_key(0));
        assert_ne!(key.public_key(0), key.public_key(1));
    }

    #[test]
    fn reads_a_key_with_a_byte_order_mark_or_crlf() {
        let key = HolderKey::generate();
        let again = HolderKey::from_secret_str(&format!("\u{feff}{}\r\n", key.to_secret_string())).unwrap();
        assert_eq!(key.public_key(0), again.public_key(0));
    }

    #[test]
    fn refuses_a_mainnet_key_without_repeating_it() {
        let main = Xpriv::new_master(NetworkKind::Main, &[7u8; 32]).unwrap().to_string();
        let err = HolderKey::from_secret_str(&main).err().unwrap();
        assert!(!err.to_string().contains(&main[..20]));
    }

    #[test]
    fn refuses_garbage_without_repeating_it() {
        let err = HolderKey::from_secret_str("tprv-not-really-a-key").err().unwrap();
        assert!(!err.to_string().contains("not-really"));
    }

    #[test]
    fn public_keys_are_test_network_multipath_keys() {
        let text = HolderKey::generate().public_key(1).to_string();
        assert!(text.contains("/87'/1'/1']tpub") && text.ends_with("/<0;1>/*"), "{text}");
    }
}
