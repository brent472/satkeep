//! A holder key from its recovery phrase (the phone key, or HEIR's on the
//! heir's route), and what it does to the Savings descriptors: whether it's
//! in each one (the PHONE check is T5 in docs/restore-tool.md), and the
//! descriptors with its private key in place for Bitcoin Core.
//!
//! The derivation is the one every Satkeep holder key uses (the
//! satkeep-savings README, "Recovery phrases and derivation"): the BIP39
//! seed with no passphrase, the BIP32 test-network master key, then the
//! account at m/87'/1'/0', written as `[fingerprint/87'/1'/0']tpub…/<0;1>/*`.

use std::str::FromStr;

use anyhow::{Context, Result};
use bitcoin::NetworkKind;
use bitcoin::bip32::{DerivationPath, Fingerprint, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use miniscript::ForEachKey;
use miniscript::descriptor::{Descriptor, DescriptorPublicKey, DescriptorSecretKey, DescriptorXKey, KeyMap};
use satkeep_backup::bip39::Mnemonic;
use satkeep_backup::zeroize::Zeroizing;

/// The account path of every Satkeep holder key.
const ACCOUNT: &str = "m/87'/1'/0'";

/// A holder key at its account path. Deliberately not `Debug`: it holds the
/// private key.
pub struct AccountKey {
    fingerprint: Fingerprint,
    path: DerivationPath,
    account: Xpriv,
    xpub: Xpub,
}

/// One Savings descriptor with PHONE's private key in place, for Bitcoin
/// Core: the receive or the change half.
pub struct PrivateDescriptor {
    /// The descriptor with PHONE's `tprv`. Wiped when dropped.
    pub descriptor: Zeroizing<String>,
    /// The change half (`internal` in importdescriptors).
    pub internal: bool,
}

impl AccountKey {
    pub fn from_phrase(phrase: &Mnemonic) -> Self {
        let seed = Zeroizing::new(phrase.to_seed(""));
        let secp = Secp256k1::new();
        let master = Xpriv::new_master(NetworkKind::Test, seed.as_ref()).expect("a 64-byte seed makes a master key");
        let path = DerivationPath::from_str(ACCOUNT).expect("a valid path");
        let account = master.derive_priv(&secp, &path).expect("hardened derivation from a master key");
        let xpub = Xpub::from_priv(&secp, &account);
        Self { fingerprint: master.fingerprint(&secp), path, account, xpub }
    }

    /// The public key as Satkeep writes it in a descriptor, without the
    /// `/<0;1>/*` ending: safe to print.
    pub fn public(&self) -> String {
        format!("[{}/87'/1'/0']{}", self.fingerprint, self.xpub)
    }

    /// Whether `key` is this key: the same extended public key, with
    /// the same fingerprint and account path.
    fn is(&self, key: &DescriptorPublicKey) -> bool {
        let (origin, xkey) = match key {
            DescriptorPublicKey::XPub(k) => (&k.origin, &k.xkey),
            DescriptorPublicKey::MultiXPub(k) => (&k.origin, &k.xkey),
            DescriptorPublicKey::Single(_) => return false,
        };
        *xkey == self.xpub && origin.as_ref() == Some(&(self.fingerprint, self.path.clone()))
    }

    /// Whether this key is one of `descriptor`'s keys.
    pub fn is_in(&self, descriptor: &str) -> Result<bool> {
        let parsed = parse(descriptor)?;
        let mut found = false;
        parsed.for_each_key(|key| {
            found |= self.is(key);
            true
        });
        Ok(found)
    }

    /// `descriptor` split into its receive and change halves, each with this
    /// key's private key in place of its public key. The other keys
    /// stay public.
    pub fn with_private_key(&self, descriptor: &str) -> Result<Vec<PrivateDescriptor>> {
        let halves = parse(descriptor)?.into_single_descriptors().context("splitting a descriptor into receive and change")?;
        let count = halves.len();
        let mut out = Vec::with_capacity(count);
        for (i, half) in halves.into_iter().enumerate() {
            let mut secrets = KeyMap::new();
            half.for_each_key(|key| {
                if let DescriptorPublicKey::XPub(k) = key {
                    if self.is(key) {
                        let secret = DescriptorXKey {
                            origin: k.origin.clone(),
                            xkey: self.account,
                            derivation_path: k.derivation_path.clone(),
                            wildcard: k.wildcard,
                        };
                        secrets.insert(key.clone(), DescriptorSecretKey::XPrv(secret));
                    }
                }
                true
            });
            anyhow::ensure!(!secrets.is_empty(), "the key isn't in this descriptor");
            out.push(PrivateDescriptor {
                descriptor: Zeroizing::new(half.to_string_with_secret(&secrets)),
                internal: count == 2 && i == 1,
            });
        }
        Ok(out)
    }
}

/// A Savings descriptor. The error never repeats it.
fn parse(descriptor: &str) -> Result<Descriptor<DescriptorPublicKey>> {
    Descriptor::from_str(descriptor).map_err(|_| anyhow::anyhow!("a Savings descriptor in the wallet setup doesn't parse"))
}
