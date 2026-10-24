//! The Satkeep phone key backup and exit kit formats.
//!
//! **Test networks only.** The format is specified in
//! `docs/backup-format.md`, with fixed test vectors that this crate's tests
//! check. Both the app-side library (`satkeep-savings`) and the restore tool
//! use this one implementation. In short:
//!
//! - A fresh random **data key** encrypts the payload: the phone key's
//!   12-word recovery phrase and the wallet setup ([`setup`]).
//!   XChaCha20-Poly1305, with the whole header as associated data, so
//!   changing any of it (the date, a device id) makes the backup refuse to
//!   open.
//! - Each **lock entry** wraps the data key under a key derived
//!   (HKDF-SHA256) from one security device's secret, bound to this wallet,
//!   this device and this format.
//! - The **exit kit** ([`kit`]) carries the locked backup and a plain copy of
//!   the wallet setup as armoured text, which the restore tool finds in a
//!   PDF, in pasted text, or reads as the raw backup.
//!
//! Nothing here prints, logs, or puts key material in an error.

mod device;
pub mod format;
pub mod kit;
pub mod setup;

use bip39::{Language, Mnemonic};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

#[cfg(feature = "simulated-device")]
pub use device::{MAX_PIN_TRIES, SimulatedDevice};
pub use device::{DeviceError, DeviceId, SecurityDevice, UnlockMethod};
pub use format::{Header, LockEntry, LockKind};
pub use setup::WalletSetup;
// The phrase type in `Restored`, and the wiping wrapper, for users of this
// crate (the restore tool) without their own dependency on either.
pub use bip39;
pub use zeroize;

use format::{NONCE_LEN, WRAPPED_KEY_LEN};

/// Words in the phone key's recovery phrase.
pub const PHRASE_WORDS: usize = 12;

/// Identifies one wallet's backups. Random, made with the wallet.
pub type WalletId = [u8; 16];

/// Why a backup or kit couldn't be made, found or opened. No variant carries
/// the file's contents or anything secret.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum BackupError {
    #[error("that isn't a Satkeep phone key backup")]
    NotABackup,
    #[error("this backup was made by a newer version of Satkeep")]
    UnsupportedVersion,
    #[error("the phone key must be a 12-word recovery phrase")]
    BadPhrase,
    #[error("that key can't unlock this backup")]
    WrongDevice,
    #[error(transparent)]
    Device(#[from] DeviceError),
    #[error("the backup couldn't be unlocked: it's damaged or has been changed")]
    CannotUnlock,
    #[error("no phone key backup found")]
    NoBackup,
    #[error("the old backup couldn't be removed from the cloud")]
    OldCopyRemains,
    #[error("that wallet setup isn't in the Satkeep wallet setup format")]
    BadSetup,
    #[error("a wallet setup must never hold a private key, and this one does")]
    PrivateKeyInSetup,
    #[error("no Satkeep exit kit found")]
    NotAKit,
    #[error("line {line} of the exit kit is missing or has a typo")]
    KitLine { line: u32 },
    #[error("the exit kit is incomplete: lines are missing from its end")]
    KitIncomplete,
    #[error("the input holds two different exit kits")]
    TwoDifferentKits,
    #[error("the exit kit's wallet setup doesn't match the one locked in its backup, so the kit has been changed")]
    KitSetupMismatch,
}

/// A backup opened with the right device. Deliberately not `Debug`: it holds
/// the phone key's phrase, which is wiped from memory when it's dropped.
pub struct Restored {
    pub phrase: Mnemonic,
    /// The wallet setup from inside the locked payload: the authenticated
    /// copy.
    pub wallet_setup: WalletSetup,
    pub header: Header,
}

/// A fresh random wallet id, from the operating system's generator.
pub fn new_wallet_id() -> WalletId {
    let random = XChaCha20Poly1305::generate_key(&mut OsRng);
    random[..16].try_into().expect("a 32-byte key has 16 bytes")
}

/// Makes a backup of `phrase` and `setup`, locked to `device`. The device
/// must have been unlocked with its own PIN. Each call uses a new data key
/// and new nonces. `created` is the backup's date, in Unix seconds.
pub fn make_backup(
    phrase: &Mnemonic,
    setup: &WalletSetup,
    wallet_id: WalletId,
    created: u64,
    device: &mut dyn SecurityDevice,
) -> Result<Vec<u8>, BackupError> {
    let data_key: Zeroizing<[u8; 32]> = Zeroizing::new(XChaCha20Poly1305::generate_key(&mut OsRng).into());
    let wrap_nonce: [u8; NONCE_LEN] = XChaCha20Poly1305::generate_nonce(&mut OsRng).into();
    let body_nonce: [u8; NONCE_LEN] = XChaCha20Poly1305::generate_nonce(&mut OsRng).into();
    seal(phrase, setup, wallet_id, created, device, &data_key, &wrap_nonce, &body_nonce)
}

/// [`make_backup`] with its random parts given, for the test vectors.
#[allow(clippy::too_many_arguments)]
fn seal(
    phrase: &Mnemonic,
    setup: &WalletSetup,
    wallet_id: WalletId,
    created: u64,
    device: &mut dyn SecurityDevice,
    data_key: &[u8; 32],
    wrap_nonce: &[u8; NONCE_LEN],
    body_nonce: &[u8; NONCE_LEN],
) -> Result<Vec<u8>, BackupError> {
    if phrase.word_count() != PHRASE_WORDS || phrase.language() != Language::English {
        return Err(BackupError::BadPhrase);
    }
    let setup_text = setup.to_text()?;
    let device_id = device.id();
    let method = device.method();
    let request = unlock_request(&wallet_id);
    let device_secret = device.unlock_secret(&request)?;
    let kek = key_encryption_key(&device_secret, &wallet_id, device_id, method);
    drop(device_secret);

    let aad = wrap_aad(&wallet_id, LockKind::Device, device_id, method, &request);
    let wrapped = aead(&kek)
        .encrypt(XNonce::from_slice(wrap_nonce), Payload { msg: data_key, aad: &aad })
        .expect("XChaCha20-Poly1305 encryption of 32 bytes can't fail");
    let wrapped_key: [u8; WRAPPED_KEY_LEN] = wrapped.try_into().expect("32 bytes plus a 16-byte tag");

    let entry = LockEntry {
        kind: LockKind::Device,
        device_id,
        method,
        request,
        wrap_nonce: *wrap_nonce,
        wrapped_key,
    };
    let header = Header { wallet_id, created, entries: vec![entry] };
    let header_bytes = format::encode_header(&header);

    let phrase_text = Zeroizing::new(phrase.to_string());
    let plaintext = encode_payload(&phrase_text, &setup_text);
    let body = aead(data_key)
        .encrypt(XNonce::from_slice(body_nonce), Payload { msg: &plaintext, aad: &header_bytes })
        .expect("XChaCha20-Poly1305 encryption can't fail for a small payload");
    let file = format::encode_body(&header_bytes, body_nonce, &body);
    if file.len() > format::MAX_FILE {
        return Err(BackupError::NotABackup);
    }
    Ok(file)
}

/// Reads the backup's header without unlocking it: its date and which
/// devices can open it.
pub fn read_header(file: &[u8]) -> Result<Header, BackupError> {
    Ok(format::parse(file)?.header)
}

/// Opens a backup with `device`, which must have been unlocked with its own
/// PIN. A device the backup isn't locked to is refused before it's asked for
/// anything.
pub fn restore(file: &[u8], device: &mut dyn SecurityDevice) -> Result<Restored, BackupError> {
    let parsed = format::parse(file)?;
    let device_id = device.id();
    let method = device.method();
    let entry = parsed
        .header
        .entries
        .iter()
        .find(|e| e.kind == LockKind::Device && e.device_id == device_id && e.method == method)
        .ok_or(BackupError::WrongDevice)?;
    let wallet_id = parsed.header.wallet_id;
    // Only ever ask the device for this wallet's own label, so a changed
    // file can't get it to derive a key for anything else.
    if entry.request != unlock_request(&wallet_id) {
        return Err(BackupError::CannotUnlock);
    }

    let device_secret = device.unlock_secret(&entry.request)?;
    let kek = key_encryption_key(&device_secret, &wallet_id, device_id, method);
    drop(device_secret);
    let aad = wrap_aad(&wallet_id, entry.kind, device_id, method, &entry.request);
    let data_key = Zeroizing::new(
        aead(&kek)
            .decrypt(XNonce::from_slice(&entry.wrap_nonce), Payload { msg: &entry.wrapped_key, aad: &aad })
            .map_err(|_| BackupError::CannotUnlock)?,
    );
    if data_key.len() != 32 {
        return Err(BackupError::CannotUnlock);
    }
    let plaintext = Zeroizing::new(
        XChaCha20Poly1305::new(Key::from_slice(&data_key))
            .decrypt(XNonce::from_slice(&parsed.body_nonce), Payload { msg: parsed.body, aad: parsed.header_bytes })
            .map_err(|_| BackupError::CannotUnlock)?,
    );
    let (phrase_text, setup_text) = decode_payload(&plaintext).ok_or(BackupError::CannotUnlock)?;
    let phrase = Mnemonic::parse_in(Language::English, phrase_text).map_err(|_| BackupError::CannotUnlock)?;
    // Only the canonical form: 12 lowercase words, single spaces.
    if phrase.word_count() != PHRASE_WORDS || Zeroizing::new(phrase.to_string()).as_str() != phrase_text {
        return Err(BackupError::CannotUnlock);
    }
    let wallet_setup = WalletSetup::from_text(setup_text).map_err(|_| BackupError::CannotUnlock)?;
    Ok(Restored { phrase, wallet_setup, header: parsed.header })
}

/// What the app sends the device: a label naming the purpose and this
/// wallet, which a device with a screen would show. Fixed by the format
/// spec.
pub fn unlock_request(wallet_id: &WalletId) -> Vec<u8> {
    let mut label = b"Satkeep phone key backup unlock v1, wallet ".to_vec();
    label.extend_from_slice(hex(wallet_id).as_bytes());
    label
}

fn key_encryption_key(
    device_secret: &[u8; 32],
    wallet_id: &WalletId,
    device_id: DeviceId,
    method: UnlockMethod,
) -> Zeroizing<[u8; 32]> {
    let mut kek = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(wallet_id), device_secret)
        .expand_multi_info(&[b"satkeep/phone-key-backup/v1/kek", &[method.code()], &device_id.0], kek.as_mut())
        .expect("32 bytes is a valid HKDF-SHA256 length");
    kek
}

fn wrap_aad(wallet_id: &WalletId, kind: LockKind, device_id: DeviceId, method: UnlockMethod, request: &[u8]) -> Vec<u8> {
    let mut aad = b"satkeep/phone-key-backup/v1/wrap".to_vec();
    aad.extend_from_slice(wallet_id);
    aad.push(kind.code());
    aad.extend_from_slice(&device_id.0);
    aad.push(method.code());
    aad.extend_from_slice(request);
    aad
}

/// Phrase (u16 length + UTF-8) then setup (u32 length + UTF-8).
fn encode_payload(phrase: &str, setup: &str) -> Zeroizing<Vec<u8>> {
    let mut out = Zeroizing::new(Vec::with_capacity(phrase.len() + setup.len() + 6));
    out.extend_from_slice(&u16::try_from(phrase.len()).expect("a 12-word phrase").to_be_bytes());
    out.extend_from_slice(phrase.as_bytes());
    out.extend_from_slice(&u32::try_from(setup.len()).expect("at most MAX_SETUP bytes").to_be_bytes());
    out.extend_from_slice(setup.as_bytes());
    out
}

fn decode_payload(plaintext: &[u8]) -> Option<(&str, &str)> {
    let phrase_len = usize::from(u16::from_be_bytes(plaintext.get(..2)?.try_into().ok()?));
    let phrase = plaintext.get(2..2 + phrase_len)?;
    let rest = &plaintext[2 + phrase_len..];
    let setup_len = usize::try_from(u32::from_be_bytes(rest.get(..4)?.try_into().ok()?)).ok()?;
    let setup = rest.get(4..4 + setup_len)?;
    if rest.len() != 4 + setup_len {
        return None;
    }
    Some((std::str::from_utf8(phrase).ok()?, std::str::from_utf8(setup).ok()?))
}

fn aead(key: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(Key::from_slice(key))
}

/// Lowercase hex, for the unlock label and the kit's lines.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

/// Reads hex in either case. `None` for an odd length or any other
/// character.
fn unhex(text: &str) -> Option<Vec<u8>> {
    fn digit(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
    let bytes = text.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    bytes.chunks(2).map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?)).collect()
}

#[cfg(test)]
mod tests;
