//! Phone key backup: the phone key, encrypted on the phone, kept in the
//! owner's own cloud, and unlocked only with their security device.
//!
//! **Prototype, test networks only.** The design, its sources and its
//! limits are in docs/phone-key-backup.md. In short:
//!
//! - A fresh random **data key** encrypts the payload: the phone key's
//!   recovery phrase and the wallet's public setup. XChaCha20-Poly1305, with
//!   the whole header as associated data, so changing any of it (the date, a
//!   device id) makes the backup refuse to open.
//! - Each **lock entry** wraps the data key under a key derived (HKDF-SHA256)
//!   from one security device's secret, bound to this wallet, this device
//!   and this format.
//! - The helper never sees the file or any part of what unlocks it.
//!
//! Nothing here prints, logs, or puts key material in an error.

mod cloud;
mod device;
mod format;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

pub use cloud::{CloudStore, MemoryCloud};
pub use device::{DeviceError, DeviceId, MAX_PIN_TRIES, SecurityDevice, SimulatedDevice, UnlockMethod};
pub use format::{Header, LockEntry, LockKind};

use crate::HolderKey;
use crate::bitcoin::hex::DisplayHex;
use format::{NONCE_LEN, WRAPPED_KEY_LEN};

/// Identifies one wallet's backups. Random, made with the wallet.
pub type WalletId = [u8; 16];

/// Why a backup couldn't be made, found or opened. No variant carries the
/// file's contents or anything secret.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum BackupError {
    #[error("that isn't a Twofold phone key backup")]
    NotABackup,
    #[error("this backup was made by a newer version of Twofold")]
    UnsupportedVersion,
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
}

/// A backup opened with the right device. Deliberately not `Debug`: it holds
/// the phone key.
pub struct Restored {
    pub phone: HolderKey,
    /// The wallet's public setup (for Savings, its descriptor and delays).
    pub wallet_setup: String,
    pub header: Header,
}

/// A fresh random wallet id.
pub fn new_wallet_id() -> WalletId {
    random()
}

/// Makes a backup of `phone` and `wallet_setup`, locked to `device`. The
/// device must have been unlocked with its own PIN. Each call uses a new data
/// key. `created` is the backup's date, in Unix seconds.
pub fn make_backup(
    phone: &HolderKey,
    wallet_setup: &str,
    wallet_id: WalletId,
    created: u64,
    device: &mut dyn SecurityDevice,
) -> Result<Vec<u8>, BackupError> {
    let data_key: Zeroizing<[u8; 32]> = Zeroizing::new(random());
    let device_id = device.id();
    let method = device.method();
    let request = unlock_request(&wallet_id);
    let device_secret = device.unlock_secret(&request)?;
    let kek = key_encryption_key(&device_secret, &wallet_id, device_id, method);
    drop(device_secret);

    let wrap_nonce: [u8; NONCE_LEN] = random();
    let aad = wrap_aad(&wallet_id, LockKind::Device, device_id, method, &request);
    let wrapped = aead(&kek)
        .encrypt(XNonce::from_slice(&wrap_nonce), Payload { msg: data_key.as_ref(), aad: &aad })
        .expect("XChaCha20-Poly1305 encryption of 32 bytes can't fail");
    let wrapped_key: [u8; WRAPPED_KEY_LEN] = wrapped.try_into().expect("32 bytes plus a 16-byte tag");

    let entry = LockEntry { kind: LockKind::Device, device_id, method, request, wrap_nonce, wrapped_key };
    let header = Header { wallet_id, created, entries: vec![entry] };
    let header_bytes = format::encode_header(&header);

    let phrase = Zeroizing::new(phone.to_secret_string());
    let plaintext = encode_payload(&phrase, wallet_setup);
    let body_nonce: [u8; NONCE_LEN] = random();
    let body = aead(&data_key)
        .encrypt(XNonce::from_slice(&body_nonce), Payload { msg: &plaintext, aad: &header_bytes })
        .expect("XChaCha20-Poly1305 encryption can't fail for a small payload");
    let file = format::encode_body(&header_bytes, &body_nonce, &body);
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
    let (phrase, wallet_setup) = decode_payload(&plaintext).ok_or(BackupError::CannotUnlock)?;
    let phone = HolderKey::from_secret_str(phrase).map_err(|_| BackupError::CannotUnlock)?;
    Ok(Restored { phone, wallet_setup: wallet_setup.to_owned(), header: parsed.header })
}

/// Saves `file` as the wallet's only backup: writes it as a new file, deletes
/// every other backup the app can see, and checks only the new one is left.
///
/// This removes old copies from the app's view only. Old versions can
/// survive in cloud revisions, the trash or someone's earlier copy, and the
/// old device still opens those; see docs/phone-key-backup.md section 7.3.
pub fn save(cloud: &mut dyn CloudStore, file: Vec<u8>) -> Result<(), BackupError> {
    let new = cloud.write_new(file);
    for id in cloud.list().into_iter().filter(|id| *id != new) {
        cloud.delete(id);
    }
    if cloud.list() != [new] {
        return Err(BackupError::OldCopyRemains);
    }
    Ok(())
}

/// The newest backup in the cloud folder.
pub fn latest(cloud: &dyn CloudStore) -> Result<Vec<u8>, BackupError> {
    let id = cloud.list().into_iter().max().ok_or(BackupError::NoBackup)?;
    cloud.read(id).ok_or(BackupError::NoBackup)
}

/// Replaces the security device: a new backup of the same phone key, under
/// a new data key, locked to `new_device`, saved as the only visible copy.
///
/// If the old device was lost or stolen, the phone key itself should be
/// replaced too (docs/phone-key-backup.md, decision D3); this function only
/// re-locks.
pub fn replace_device(
    cloud: &mut dyn CloudStore,
    phone: &HolderKey,
    wallet_setup: &str,
    wallet_id: WalletId,
    created: u64,
    new_device: &mut dyn SecurityDevice,
) -> Result<(), BackupError> {
    let file = make_backup(phone, wallet_setup, wallet_id, created, new_device)?;
    save(cloud, file)
}

/// What the app sends the device: a label naming the purpose and this
/// wallet, which a device with a screen would show.
fn unlock_request(wallet_id: &WalletId) -> Vec<u8> {
    format!("Twofold phone key backup unlock v1, wallet {}", wallet_id.as_hex()).into_bytes()
}

fn key_encryption_key(
    device_secret: &[u8; 32],
    wallet_id: &WalletId,
    device_id: DeviceId,
    method: UnlockMethod,
) -> Zeroizing<[u8; 32]> {
    let mut kek = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(wallet_id), device_secret)
        .expand_multi_info(&[b"twofold/phone-key-backup/v1/kek", &[method.code()], &device_id.0], kek.as_mut())
        .expect("32 bytes is a valid HKDF-SHA256 length");
    kek
}

fn wrap_aad(wallet_id: &WalletId, kind: LockKind, device_id: DeviceId, method: UnlockMethod, request: &[u8]) -> Vec<u8> {
    let mut aad = b"twofold/phone-key-backup/v1/wrap".to_vec();
    aad.extend_from_slice(wallet_id);
    aad.push(match kind {
        LockKind::Device => 1,
        LockKind::Contact => 2,
    });
    aad.extend_from_slice(&device_id.0);
    aad.push(method.code());
    aad.extend_from_slice(request);
    aad
}

/// Phrase (u16 length + UTF-8) then setup (u32 length + UTF-8).
fn encode_payload(phrase: &str, wallet_setup: &str) -> Zeroizing<Vec<u8>> {
    let mut out = Zeroizing::new(Vec::with_capacity(phrase.len() + wallet_setup.len() + 6));
    out.extend_from_slice(&u16::try_from(phrase.len()).expect("a 12-word phrase").to_be_bytes());
    out.extend_from_slice(phrase.as_bytes());
    out.extend_from_slice(&u32::try_from(wallet_setup.len()).expect("a small setup").to_be_bytes());
    out.extend_from_slice(wallet_setup.as_bytes());
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

fn random<const N: usize>() -> [u8; N] {
    use crate::bitcoin::secp256k1::rand::{RngCore, thread_rng};
    let mut out = [0u8; N];
    thread_rng().fill_bytes(&mut out);
    out
}

fn aead(key: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(Key::from_slice(key))
}

#[cfg(test)]
mod tests;
