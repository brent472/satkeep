use std::str::FromStr;

use bdk_wallet::SignOptions;
use bdk_wallet::signer::{SignerContext, SignerWrapper, TransactionSigner};
use bip39::{Language, Mnemonic};
use zeroize::Zeroize;

use crate::Error;
use crate::bitcoin::NetworkKind;
use crate::bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use crate::bitcoin::key::Secp256k1;
use crate::bitcoin::psbt::Psbt;
use crate::miniscript::descriptor::{DescriptorPublicKey, DescriptorXKey, Wildcard};

/// Words in a Satkeep recovery phrase. Liana's own wallets use 12 too.
pub const PHRASE_WORDS: usize = 12;

/// The private key of one person or device in a Savings wallet, for test
/// networks only.
///
/// Every key comes from a standard BIP39 recovery phrase: 12 English words,
/// no passphrase. The phrase gives a BIP32 master key on the test network,
/// and public keys are derived at `m/87'/1'/account'` and used as
/// `…/<0;1>/*`. Every holder uses account 0. These paths are test-only
/// placeholders.
///
/// Deliberately not `Debug` or `Display`, so it can't end up in a log or an
/// error message by accident. [`HolderKey::to_secret_string`] is the only way
/// to get the phrase out. The phrase is wiped from memory when the key is
/// dropped.
pub struct HolderKey {
    phrase: Mnemonic,
    master: Xpriv,
}

/// Accounts a holder has a key on: account 0, for everyone.
const ACCOUNTS: [u32; 1] = [0];

impl HolderKey {
    /// A fresh random 12-word phrase from the operating system's random
    /// source.
    pub fn generate() -> Self {
        use crate::bitcoin::secp256k1::rand::{RngCore, thread_rng};
        let mut entropy = [0u8; 16];
        thread_rng().fill_bytes(&mut entropy);
        let phrase = Mnemonic::from_entropy(&entropy).expect("16 bytes make a 12-word phrase");
        entropy.zeroize();
        Self::from_phrase(phrase)
    }

    /// Reads a phrase written by [`HolderKey::to_secret_string`]: 12 English
    /// BIP39 words with a valid checksum. The error never repeats the input.
    pub fn from_secret_str(s: &str) -> Result<Self, Error> {
        // Windows tools (PowerShell 5.1 pipes, Notepad) may put a byte-order
        // mark in front of the text.
        let text = s.trim().trim_start_matches('\u{feff}').trim();
        if text.starts_with("tprv") || text.starts_with("xprv") {
            return Err(Error::OldHolderKey);
        }
        let phrase = Mnemonic::parse_in(Language::English, text).map_err(|_| Error::BadHolderKey)?;
        if phrase.word_count() != PHRASE_WORDS {
            return Err(Error::BadHolderKey);
        }
        Ok(Self::from_phrase(phrase))
    }

    /// A key from a phrase restored from the phone key backup, which has
    /// already checked it's 12 English words.
    pub(crate) fn from_mnemonic(phrase: Mnemonic) -> Result<Self, Error> {
        if phrase.word_count() != PHRASE_WORDS || phrase.language() != Language::English {
            return Err(Error::BadHolderKey);
        }
        Ok(Self::from_phrase(phrase))
    }

    /// The phrase itself, for the phone key backup.
    pub(crate) fn mnemonic(&self) -> &Mnemonic {
        &self.phrase
    }

    fn from_phrase(phrase: Mnemonic) -> Self {
        // BIP39 seed with an empty passphrase, then the BIP32 master key.
        let mut seed = phrase.to_seed("");
        let master = Xpriv::new_master(NetworkKind::Test, &seed).expect("a 64-byte seed makes a valid master key");
        seed.zeroize();
        Self { phrase, master }
    }

    /// The recovery phrase: 12 words separated by single spaces. Anyone with
    /// it can approve as this holder: never print, log or commit it.
    pub fn to_secret_string(&self) -> String {
        self.phrase.to_string()
    }

    /// The public key for `account` as `[fingerprint/87'/1'/account']tpub…/<0;1>/*`.
    pub fn public_key(&self, account: u32) -> DescriptorPublicKey {
        let secp = Secp256k1::new();
        let xpub = Xpub::from_priv(&secp, &self.master.derive_priv(&secp, &account_path(account)).expect("hardened derivation"));
        let fp = self.master.fingerprint(&secp);
        DescriptorPublicKey::from_str(&format!("[{fp}/87'/1'/{account}']{xpub}/<0;1>/*"))
            .expect("a well-formed descriptor key")
    }

    /// Adds this holder's signatures to `psbt` wherever it has a key: every
    /// script leaf of a Taproot Savings coin, or the multisig script of a
    /// P2WSH one (Savings with no beneficiary). Returns how many signatures
    /// were added.
    pub fn sign(&self, psbt: &mut Psbt) -> Result<usize, Error> {
        let before = signature_count(psbt);
        let secp = Secp256k1::new();
        // A Savings PSBT is one shape: P2WSH inputs carry their witness script.
        let context = if psbt.inputs.iter().any(|i| i.witness_script.is_some()) {
            SignerContext::Segwitv0
        } else {
            SignerContext::Tap { is_internal_key: false }
        };
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
                let signer = SignerWrapper::new(key, context);
                signer.sign_transaction(psbt, &SignOptions::default(), &secp)?;
            }
        }
        Ok(signature_count(psbt) - before)
    }
}

/// Whether `text` holds what looks like a recovery phrase: 12 English BIP39
/// words in a row, ignoring case and anything between them that isn't a
/// letter. For refusing a phrase on the command line, and for tests that
/// check nothing prints one.
pub fn contains_recovery_phrase(text: &str) -> bool {
    let mut run = 0;
    for word in text.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()) {
        if Language::English.find_word(&word.to_ascii_lowercase()).is_some() {
            run += 1;
            if run == PHRASE_WORDS {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

fn account_path(account: u32) -> DerivationPath {
    DerivationPath::from_str(&format!("m/87'/1'/{account}'")).expect("valid path")
}

fn signature_count(psbt: &Psbt) -> usize {
    psbt.inputs.iter().map(|i| i.tap_script_sigs.len() + i.partial_sigs.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BIP39's first English test vector (entropy of all zeros), the same one
    /// in the BIP and in every BIP39 library. Public, so never a real key.
    const VECTOR: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[test]
    fn round_trips_through_its_secret_string() {
        let key = HolderKey::generate();
        let phrase = key.to_secret_string();
        assert_eq!(phrase.split(' ').count(), PHRASE_WORDS);
        let again = HolderKey::from_secret_str(&phrase).unwrap();
        assert_eq!(key.public_key(0), again.public_key(0));
        assert_ne!(key.public_key(0), key.public_key(1));
    }

    #[test]
    fn derives_the_standard_master_key() {
        // BIP86's test vectors use this phrase with an empty passphrase and
        // list the public key at m/86'/0'/0' (as mainnet `xpub`). The same
        // key must come out of ours, on the test network.
        let bip86_account = "xpub6BgBgsespWvERF3LHQu6CnqdvfEvtMcQjYrcRzx53QJjSxarj2afYWcLteoGVky7D3UKDP9QyrLprQ3VCECoY49yfdDEHGCtMMj92pReUsQ";
        let mut expected = Xpub::from_str(bip86_account).unwrap();
        expected.network = NetworkKind::Test;
        let key = HolderKey::from_secret_str(VECTOR).unwrap();
        let secp = Secp256k1::new();
        let path = DerivationPath::from_str("m/86'/0'/0'").unwrap();
        let ours = Xpub::from_priv(&secp, &key.master.derive_priv(&secp, &path).unwrap());
        assert_eq!(ours, expected, "the phrase gives BIP86's published account key");
        assert!(key.public_key(0).to_string().starts_with("[73c5da0a/87'/1'/0']tpub"));
    }

    #[test]
    fn reads_a_phrase_with_a_byte_order_mark_crlf_or_extra_spaces() {
        let key = HolderKey::generate();
        let spaced = key.to_secret_string().replace(' ', "  \t");
        let again = HolderKey::from_secret_str(&format!("\u{feff}{spaced}\r\n")).unwrap();
        assert_eq!(key.public_key(0), again.public_key(0));
    }

    #[test]
    fn refuses_a_bad_checksum_or_a_different_length_without_repeating_it() {
        let bad_checksum = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        let words_24 = format!("{} art", ["abandon"; 23].join(" "));
        let words_11 = ["abandon"; 11].join(" ");
        assert!(Mnemonic::parse_in(Language::English, words_24.as_str()).is_ok(), "a valid 24-word phrase");
        for text in [bad_checksum, &words_24, &words_11, "abandon about zzzz"] {
            let err = HolderKey::from_secret_str(text).err().unwrap().to_string();
            assert!(!err.contains("abandon") && !err.contains("zzzz"), "{err}");
        }
    }

    #[test]
    fn refuses_an_old_tprv_key_without_repeating_it() {
        let old = Xpriv::new_master(NetworkKind::Test, &[7u8; 32]).unwrap().to_string();
        let err = HolderKey::from_secret_str(&old).err().unwrap();
        assert!(matches!(err, Error::OldHolderKey));
        assert!(!err.to_string().contains(&old[..20]));
    }

    #[test]
    fn refuses_garbage_without_repeating_it() {
        let err = HolderKey::from_secret_str("not-really-a-key").err().unwrap();
        assert!(!err.to_string().contains("not-really"));
    }

    #[test]
    fn public_keys_are_test_network_multipath_keys() {
        let text = HolderKey::generate().public_key(1).to_string();
        assert!(text.contains("/87'/1'/1']tpub") && text.ends_with("/<0;1>/*"), "{text}");
    }

    #[test]
    fn spots_a_phrase_in_text() {
        let phrase = HolderKey::generate().to_secret_string();
        assert!(contains_recovery_phrase(&phrase));
        assert!(contains_recovery_phrase(&format!("--key-file={}", phrase.replace(' ', ","))));
        assert!(contains_recovery_phrase(&phrase.to_uppercase()));
        let eleven: Vec<&str> = phrase.split(' ').take(11).collect();
        assert!(!contains_recovery_phrase(&eleven.join(" ")));
        assert!(!contains_recovery_phrase(&format!("{} tpub {}", eleven.join(" "), eleven.join(" "))));
        assert!(!contains_recovery_phrase("Anyone with this file can approve as its holder."));
    }
}
