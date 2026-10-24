//! The second phone: an old phone or tablet with the Satkeep app, as the
//! second key (decision D4; docs/backup-format.md section 3.4).
//!
//! The phone holds a P-256 key that never leaves its Secure Enclave or
//! Android Keystore. A backup is locked to it by ECDH with that key's public
//! half, so the second phone needn't be there when a backup is made. It's
//! unlocked through an offline exchange, as text, with no Satkeep servers:
//!
//! 1. the restore tool shows an **unlock request**: the wallet, the phone's
//!    id, the backup's ephemeral key, and a fresh session key;
//! 2. the phone's app checks it, asks for the phone's own face, fingerprint
//!    or passcode, and shows an **answer**: the unlock secret, masked with
//!    the session key, and a check;
//! 3. the person types the answer into the tool.
//!
//! The tool's side ([`SecondPhoneUnlock`]) is hashing only. It never does
//! ECDH, so it needs no elliptic-curve code. Locking a backup to a phone and
//! the simulated phone need P-256, behind the `second-phone-lock` feature.

use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{BackupError, DeviceError, DeviceId, Header, LockKind, SecurityDevice, UnlockMethod, WalletId, hex, unhex};

/// A P-256 public key in SEC1 compressed form.
pub const PUBLIC_KEY_LEN: usize = 33;
const REQUEST_MAGIC: &[u8; 4] = b"SKRQ";
const REQUEST_VERSION: u8 = 1;
const CHECK_LEN: usize = 4;
/// Magic, version, wallet id, device id, ephemeral key, session key, check.
pub const REQUEST_LEN: usize = 4 + 1 + 16 + 32 + PUBLIC_KEY_LEN + 32 + CHECK_LEN;
/// The masked unlock secret, then a check.
pub const ANSWER_LEN: usize = 32 + CHECK_LEN;
/// The longest text either reader looks at: far above the hex and its
/// spaces, so a pasted file can't make it work hard.
const MAX_TEXT: usize = 4096;

/// Why a request or an answer was refused. None of these carry anything
/// secret, or repeat what was typed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum SecondPhoneError {
    #[error("that unlock request is incomplete or has a typo")]
    RequestTypo,
    #[error("that answer has a typo, or it answers a different unlock request")]
    AnswerTypo,
    #[error("this backup isn't locked to this phone")]
    NotThisPhone,
    #[error("this phone isn't the second key of that wallet")]
    NotThisWallet,
    #[error("that unlock request isn't valid")]
    BadRequest,
    #[error(transparent)]
    Device(#[from] DeviceError),
}

/// The device id of the second phone whose public key is `public_key`
/// (SEC1 compressed): SHA-256 of a fixed label and the key.
pub fn device_id(public_key: &[u8; PUBLIC_KEY_LEN]) -> DeviceId {
    DeviceId(Sha256::new().chain_update(b"satkeep/second-phone/v1/id").chain_update(public_key).finalize().into())
}

/// Whether a lock entry's request has the shape of a second phone's
/// ephemeral key: 33 bytes, starting like a compressed point. Only the
/// phone checks it's on the curve.
pub(crate) fn is_ephemeral_key(request: &[u8]) -> bool {
    request.len() == PUBLIC_KEY_LEN && matches!(request[0], 2 | 3)
}

/// An unlock request: what the tool shows and the phone reads. Deliberately
/// not `Debug`: its session key, with the phone's answer, gives the unlock
/// secret.
pub struct UnlockRequest {
    pub wallet_id: WalletId,
    pub device_id: DeviceId,
    pub ephemeral_key: [u8; PUBLIC_KEY_LEN],
    session_key: Zeroizing<[u8; 32]>,
}

impl UnlockRequest {
    /// A request with a fresh session key from the operating system's
    /// generator.
    fn new(wallet_id: WalletId, device_id: DeviceId, ephemeral_key: [u8; PUBLIC_KEY_LEN]) -> Self {
        use chacha20poly1305::aead::Generate;
        let session_key = Zeroizing::new(<[u8; 32]>::generate());
        Self { wallet_id, device_id, ephemeral_key, session_key }
    }

    #[cfg(test)]
    pub(crate) fn with_session_key(
        wallet_id: WalletId,
        device_id: DeviceId,
        ephemeral_key: [u8; PUBLIC_KEY_LEN],
        session_key: [u8; 32],
    ) -> Self {
        Self { wallet_id, device_id, ephemeral_key, session_key: Zeroizing::new(session_key) }
    }

    fn encode(&self) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(REQUEST_LEN));
        out.extend_from_slice(REQUEST_MAGIC);
        out.push(REQUEST_VERSION);
        out.extend_from_slice(&self.wallet_id);
        out.extend_from_slice(&self.device_id.0);
        out.extend_from_slice(&self.ephemeral_key);
        out.extend_from_slice(self.session_key.as_ref());
        let check = short_hash(&[&out]);
        out.extend_from_slice(&check);
        out
    }

    /// The request as text: hex, in groups of 4 digits, 8 groups a line.
    pub fn to_text(&self) -> Zeroizing<String> {
        grouped(&self.encode(), 8)
    }

    /// Reads a request's text, in either case, ignoring spaces and line
    /// breaks. Anything else, a wrong length or a failed check is a typo.
    pub fn from_text(text: &str) -> Result<Self, SecondPhoneError> {
        let bytes = read_hex(text, REQUEST_LEN).ok_or(SecondPhoneError::RequestTypo)?;
        let (body, check) = bytes.split_at(REQUEST_LEN - CHECK_LEN);
        if check != short_hash(&[body]) || &body[..4] != REQUEST_MAGIC {
            return Err(SecondPhoneError::RequestTypo);
        }
        if body[4] != REQUEST_VERSION {
            return Err(SecondPhoneError::BadRequest);
        }
        let field = |from: usize, len: usize| &body[from..from + len];
        Ok(Self {
            wallet_id: field(5, 16).try_into().expect("16 bytes"),
            device_id: DeviceId(field(21, 32).try_into().expect("32 bytes")),
            ephemeral_key: field(53, PUBLIC_KEY_LEN).try_into().expect("33 bytes"),
            session_key: Zeroizing::new(field(86, 32).try_into().expect("32 bytes")),
        })
    }

    /// What the unlock secret is masked with: HKDF-SHA256 of the session key.
    fn mask(&self) -> Zeroizing<[u8; 32]> {
        let mut mask = Zeroizing::new([0u8; 32]);
        Hkdf::<Sha256>::new(None, self.session_key.as_ref())
            .expand(b"satkeep/second-phone/v1/answer-mask", mask.as_mut())
            .expect("32 bytes is a valid HKDF-SHA256 length");
        mask
    }

    /// The answer's check, bound to this whole request, so an answer to
    /// another request fails it like a typo.
    fn answer_check(&self, masked: &[u8]) -> [u8; CHECK_LEN] {
        short_hash(&[b"satkeep/second-phone/v1/answer", &self.encode(), masked])
    }
}

/// The restore tool's side of the exchange, for one backup: it makes the
/// request, takes the typed answer, and then acts as the security device
/// for [`crate::restore`]. Deliberately not `Debug`.
pub struct SecondPhoneUnlock {
    request: UnlockRequest,
    secret: Option<Zeroizing<[u8; 32]>>,
}

impl SecondPhoneUnlock {
    /// For the backup with `header`: its second phone's lock entry, and a
    /// new request with a fresh session key. Refused if the backup isn't
    /// locked to a second phone, if it's locked to more than one (this
    /// version can't choose yet), or if the entry's request isn't shaped
    /// like a key.
    pub fn for_backup(header: &Header) -> Result<Self, BackupError> {
        let mut phones =
            header.entries.iter().filter(|e| e.kind == LockKind::Device && e.method == UnlockMethod::SecondPhone);
        let entry = phones.next().ok_or(BackupError::WrongDevice)?;
        if phones.next().is_some() {
            return Err(BackupError::SeveralSecondPhones);
        }
        if !is_ephemeral_key(&entry.request) {
            return Err(BackupError::CannotUnlock);
        }
        let ephemeral_key = entry.request.as_slice().try_into().expect("checked: 33 bytes");
        Ok(Self::from_request(UnlockRequest::new(header.wallet_id, entry.device_id, ephemeral_key)))
    }

    pub(crate) fn from_request(request: UnlockRequest) -> Self {
        Self { request, secret: None }
    }

    /// The request to give the phone, as text.
    pub fn request_text(&self) -> Zeroizing<String> {
        self.request.to_text()
    }

    /// Takes the answer the phone showed, as typed: either case, spaces and
    /// line breaks ignored. A typo, or an answer to another request, fails
    /// its check and can be typed again.
    pub fn enter_answer(&mut self, text: &str) -> Result<(), SecondPhoneError> {
        let bytes = read_hex(text, ANSWER_LEN).ok_or(SecondPhoneError::AnswerTypo)?;
        let (masked, check) = bytes.split_at(32);
        if check != self.request.answer_check(masked) {
            return Err(SecondPhoneError::AnswerTypo);
        }
        let mask = self.request.mask();
        let mut secret = Zeroizing::new([0u8; 32]);
        for (i, byte) in secret.iter_mut().enumerate() {
            *byte = masked[i] ^ mask[i];
        }
        self.secret = Some(secret);
        Ok(())
    }
}

impl SecurityDevice for SecondPhoneUnlock {
    /// The id from the backup's own entry: the phone checks it's its own
    /// before it answers.
    fn id(&self) -> DeviceId {
        self.request.device_id
    }

    fn method(&self) -> UnlockMethod {
        UnlockMethod::SecondPhone
    }

    /// The unlock secret from the answer, once.
    fn unlock_secret(&mut self, request: &[u8]) -> Result<Zeroizing<[u8; 32]>, DeviceError> {
        if request != self.request.ephemeral_key {
            return Err(DeviceError::AnswerRequired);
        }
        self.secret.take().ok_or(DeviceError::AnswerRequired)
    }
}

/// The first 4 bytes of SHA-256 over `parts`.
fn short_hash(parts: &[&[u8]]) -> [u8; CHECK_LEN] {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part);
    }
    hash.finalize()[..CHECK_LEN].try_into().expect("SHA-256 has 32 bytes")
}

/// Hex in groups of 4 digits, `per_line` groups a line.
fn grouped(bytes: &[u8], per_line: usize) -> Zeroizing<String> {
    let digits = Zeroizing::new(hex(bytes));
    let mut out = Zeroizing::new(String::with_capacity(digits.len() * 5 / 4 + 1));
    for (i, group) in digits.as_bytes().chunks(4).enumerate() {
        if i > 0 {
            out.push(if i % per_line == 0 { '\n' } else { ' ' });
        }
        out.push_str(std::str::from_utf8(group).expect("hex is ASCII"));
    }
    out
}

/// Exactly `len` bytes of hex, in either case, with spaces and line breaks
/// ignored. `None` for anything else.
fn read_hex(text: &str, len: usize) -> Option<Zeroizing<Vec<u8>>> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let digits: Zeroizing<String> = Zeroizing::new(text.chars().filter(|c| !c.is_ascii_whitespace()).collect());
    let bytes = Zeroizing::new(unhex(&digits)?);
    (bytes.len() == len).then_some(bytes)
}

// ------------------------------------------ the phone's side: P-256 ----

/// The unlock secret, from the ECDH shared x-coordinate: HKDF-SHA256 bound
/// to the wallet, the ephemeral key and the phone's key.
#[cfg(feature = "second-phone-lock")]
fn unlock_secret(
    shared_x: &[u8],
    wallet_id: &WalletId,
    ephemeral_key: &[u8; PUBLIC_KEY_LEN],
    phone_key: &[u8; PUBLIC_KEY_LEN],
) -> Zeroizing<[u8; 32]> {
    let mut secret = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(wallet_id), shared_x)
        .expand_multi_info(&[b"satkeep/second-phone/v1/unlock", ephemeral_key, phone_key], secret.as_mut())
        .expect("32 bytes is a valid HKDF-SHA256 length");
    secret
}

/// A fresh random P-256 key, from the operating system's generator: 32
/// random bytes, drawn again in the rare case they aren't a valid key.
#[cfg(feature = "second-phone-lock")]
pub(crate) fn random_key() -> p256::SecretKey {
    use chacha20poly1305::aead::Generate;
    loop {
        let bytes = Zeroizing::new(<[u8; 32]>::generate());
        if let Ok(key) = p256::SecretKey::from_bytes(&(*bytes).into()) {
            return key;
        }
    }
}

#[cfg(feature = "second-phone-lock")]
fn compressed(key: &p256::PublicKey) -> [u8; PUBLIC_KEY_LEN] {
    use p256::elliptic_curve::sec1::ToSec1Point;
    key.to_sec1_point(true).as_bytes().try_into().expect("a compressed P-256 point is 33 bytes")
}

/// The lock entry's parts for the phone with `phone_key`, made on the
/// owner's phone with the ephemeral key `ephemeral`: ECDH with the second
/// phone's public key, which is all the owner's phone needs of it.
#[cfg(feature = "second-phone-lock")]
pub(crate) fn lock(
    phone_key: &[u8; PUBLIC_KEY_LEN],
    wallet_id: &WalletId,
    ephemeral: &p256::SecretKey,
) -> Result<crate::Lock, BackupError> {
    let phone = p256::PublicKey::from_sec1_bytes(phone_key).map_err(|_| BackupError::NotASecondPhoneKey)?;
    let ephemeral_key = compressed(&ephemeral.public_key());
    let shared = p256::ecdh::diffie_hellman(ephemeral.to_nonzero_scalar(), phone.as_affine());
    let secret = unlock_secret(shared.raw_secret_bytes(), wallet_id, &ephemeral_key, phone_key);
    Ok(crate::Lock { device_id: device_id(phone_key), method: UnlockMethod::SecondPhone, request: ephemeral_key.to_vec(), secret })
}

/// A second phone simulated in software, for tests and the prototype: the
/// app's side of the exchange, with a P-256 key standing in for the Secure
/// Enclave's. It answers only requests for its own id and for wallets it's
/// the second key of, and only after its passcode, once per entry; it
/// blocks after [`crate::MAX_PIN_TRIES`] wrong passcodes. Deliberately not
/// `Debug`. Test networks only, and only with the `simulated-device`
/// feature.
#[cfg(feature = "simulated-device")]
pub struct SimulatedSecondPhone {
    key: p256::SecretKey,
    passcode: Zeroizing<String>,
    unlocked: bool,
    wrong_tries: u8,
    wallets: Vec<WalletId>,
    answers: u32,
}

#[cfg(feature = "simulated-device")]
impl SimulatedSecondPhone {
    /// A fresh phone with a random key and this passcode.
    pub fn new(passcode: &str) -> Self {
        Self::with_key(random_key(), passcode)
    }

    /// The same simulated phone again, from its key's 32 bytes, for tests
    /// that make a backup in one process and unlock it in another. `None` if
    /// the bytes aren't a valid P-256 key.
    pub fn from_secret(secret: [u8; 32], passcode: &str) -> Option<Self> {
        let secret = Zeroizing::new(secret);
        let key = p256::SecretKey::from_bytes(&(*secret).into()).ok()?;
        Some(Self::with_key(key, passcode))
    }

    fn with_key(key: p256::SecretKey, passcode: &str) -> Self {
        Self { key, passcode: Zeroizing::new(passcode.to_owned()), unlocked: false, wrong_tries: 0, wallets: Vec::new(), answers: 0 }
    }

    /// Its public key, SEC1 compressed: what the owner's phone locks
    /// backups to.
    pub fn public_key(&self) -> [u8; PUBLIC_KEY_LEN] {
        compressed(&self.key.public_key())
    }

    pub fn id(&self) -> DeviceId {
        device_id(&self.public_key())
    }

    /// Makes it the second key of `wallet_id`, as pairing does in the app.
    pub fn serve(&mut self, wallet_id: WalletId) {
        self.wallets.push(wallet_id);
    }

    /// Enters the passcode. The right one allows one answer.
    pub fn enter_passcode(&mut self, passcode: &str) -> Result<(), DeviceError> {
        if self.wrong_tries >= crate::MAX_PIN_TRIES {
            return Err(DeviceError::Blocked);
        }
        if passcode.as_bytes() == self.passcode.as_bytes() {
            self.unlocked = true;
            self.wrong_tries = 0;
            Ok(())
        } else {
            self.unlocked = false;
            self.wrong_tries += 1;
            match crate::MAX_PIN_TRIES - self.wrong_tries {
                0 => Err(DeviceError::Blocked),
                tries_left => Err(DeviceError::WrongPin { tries_left }),
            }
        }
    }

    /// How many answers it has worked out. Lets tests check a refused
    /// request never reached its key.
    pub fn answers(&self) -> u32 {
        self.answers
    }

    /// The answer to a request's text, as the phone would show it: 72 hex
    /// digits in groups of 4. Checks the request is for this phone and one
    /// of its wallets before it asks for the passcode.
    pub fn answer(&mut self, request_text: &str) -> Result<Zeroizing<String>, SecondPhoneError> {
        let request = UnlockRequest::from_text(request_text)?;
        if request.device_id != self.id() {
            return Err(SecondPhoneError::NotThisPhone);
        }
        if !self.wallets.contains(&request.wallet_id) {
            return Err(SecondPhoneError::NotThisWallet);
        }
        if self.wrong_tries >= crate::MAX_PIN_TRIES {
            return Err(DeviceError::Blocked.into());
        }
        if !self.unlocked {
            return Err(DeviceError::PinRequired.into());
        }
        self.unlocked = false;
        let ephemeral = p256::PublicKey::from_sec1_bytes(&request.ephemeral_key).map_err(|_| SecondPhoneError::BadRequest)?;
        self.answers += 1;
        let shared = p256::ecdh::diffie_hellman(self.key.to_nonzero_scalar(), ephemeral.as_affine());
        let secret = unlock_secret(shared.raw_secret_bytes(), &request.wallet_id, &request.ephemeral_key, &self.public_key());
        Ok(answer_text(&request, &secret))
    }
}

/// The phone's answer to `request`: `secret` masked with the session key,
/// then the check.
#[cfg(feature = "simulated-device")]
fn answer_text(request: &UnlockRequest, secret: &[u8; 32]) -> Zeroizing<String> {
    let mask = request.mask();
    let mut bytes = Zeroizing::new(Vec::with_capacity(ANSWER_LEN));
    bytes.extend(secret.iter().zip(mask.iter()).map(|(s, m)| s ^ m));
    let check = request.answer_check(&bytes);
    bytes.extend_from_slice(&check);
    grouped(&bytes, ANSWER_LEN * 2 / 4)
}
