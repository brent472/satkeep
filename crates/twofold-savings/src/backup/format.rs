//! The backup file's byte layout. Every integer is big-endian.
//!
//! ```text
//! header (authenticated as associated data of the body):
//!   magic          "TFBK"
//!   version        u8 = 1
//!   wallet id      16 bytes
//!   created        u64, Unix seconds
//!   entry count    u8, 1..=MAX_ENTRIES
//!   each entry:
//!     kind         u8 (1 = device, 2 = contact, reserved)
//!     device id    32 bytes
//!     method       u8
//!     request      u16 length + bytes, at most MAX_REQUEST
//!     wrap nonce   24 bytes
//!     wrapped key  48 bytes (the 32-byte data key + a 16-byte tag)
//! body:
//!   nonce          24 bytes
//!   ciphertext     u32 length + bytes, at least the 16-byte tag
//! ```
//!
//! Nothing may follow the body. Parsing never panics and never repeats its
//! input in an error.

use super::{BackupError, DeviceId, UnlockMethod};

pub const MAGIC: &[u8; 4] = b"TFBK";
pub const VERSION: u8 = 1;
pub const MAX_ENTRIES: usize = 8;
pub const MAX_REQUEST: usize = 256;
/// Far above any real backup (a few hundred bytes), far below the 1 MB the
/// screen copy promises.
pub const MAX_FILE: usize = 64 * 1024;
pub const NONCE_LEN: usize = 24;
pub const WRAPPED_KEY_LEN: usize = 32 + 16;
const TAG_LEN: usize = 16;

/// What kind of unlock path a lock entry is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockKind {
    /// The owner's security device.
    Device,
    /// The recovery contact. Reserved: the format has room for it, but
    /// nothing makes or opens one yet (docs/phone-key-backup.md section 5).
    Contact,
}

impl LockKind {
    fn code(self) -> u8 {
        match self {
            Self::Device => 1,
            Self::Contact => 2,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Device),
            2 => Some(Self::Contact),
            _ => None,
        }
    }
}

/// One way to unlock the backup: the data key, wrapped under a key only that
/// path can derive.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LockEntry {
    pub kind: LockKind,
    pub device_id: DeviceId,
    pub method: UnlockMethod,
    /// What the app sends the device to get its secret.
    pub request: Vec<u8>,
    pub wrap_nonce: [u8; NONCE_LEN],
    pub wrapped_key: [u8; WRAPPED_KEY_LEN],
}

/// The readable part of a backup: enough to show its date and pick the
/// right device, nothing that helps decrypt it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Header {
    pub wallet_id: [u8; 16],
    /// When the backup was made, in Unix seconds.
    pub created: u64,
    pub entries: Vec<LockEntry>,
}

pub struct ParsedFile<'a> {
    pub header: Header,
    /// The header's exact bytes, which the body authenticates.
    pub header_bytes: &'a [u8],
    pub body_nonce: [u8; NONCE_LEN],
    pub body: &'a [u8],
}

pub fn encode_header(header: &Header) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + header.entries.len() * 128);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&header.wallet_id);
    out.extend_from_slice(&header.created.to_be_bytes());
    out.push(u8::try_from(header.entries.len()).expect("at most MAX_ENTRIES entries"));
    for entry in &header.entries {
        out.push(entry.kind.code());
        out.extend_from_slice(&entry.device_id.0);
        out.push(entry.method.code());
        out.extend_from_slice(&u16::try_from(entry.request.len()).expect("at most MAX_REQUEST bytes").to_be_bytes());
        out.extend_from_slice(&entry.request);
        out.extend_from_slice(&entry.wrap_nonce);
        out.extend_from_slice(&entry.wrapped_key);
    }
    out
}

pub fn encode_body(header_bytes: &[u8], body_nonce: &[u8; NONCE_LEN], body: &[u8]) -> Vec<u8> {
    let mut out = header_bytes.to_vec();
    out.extend_from_slice(body_nonce);
    out.extend_from_slice(&u32::try_from(body.len()).expect("a small body").to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// Reads a backup file's structure without decrypting anything.
pub fn parse(file: &[u8]) -> Result<ParsedFile<'_>, BackupError> {
    if file.len() > MAX_FILE {
        return Err(BackupError::NotABackup);
    }
    let mut r = Reader { data: file, pos: 0 };
    if r.take(4)? != MAGIC {
        return Err(BackupError::NotABackup);
    }
    let version = r.u8()?;
    if version != VERSION {
        return Err(BackupError::UnsupportedVersion);
    }
    let wallet_id = r.array::<16>()?;
    let created = u64::from_be_bytes(r.array::<8>()?);
    let count = usize::from(r.u8()?);
    if count == 0 || count > MAX_ENTRIES {
        return Err(BackupError::NotABackup);
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let kind = LockKind::from_code(r.u8()?).ok_or(BackupError::NotABackup)?;
        let device_id = DeviceId(r.array::<32>()?);
        let method = UnlockMethod::from_code(r.u8()?).ok_or(BackupError::NotABackup)?;
        let request_len = usize::from(u16::from_be_bytes(r.array::<2>()?));
        if request_len > MAX_REQUEST {
            return Err(BackupError::NotABackup);
        }
        let request = r.take(request_len)?.to_vec();
        let wrap_nonce = r.array::<NONCE_LEN>()?;
        let wrapped_key = r.array::<WRAPPED_KEY_LEN>()?;
        entries.push(LockEntry { kind, device_id, method, request, wrap_nonce, wrapped_key });
    }
    let header_bytes = &file[..r.pos];
    let body_nonce = r.array::<NONCE_LEN>()?;
    let body_len = usize::try_from(u32::from_be_bytes(r.array::<4>()?)).map_err(|_| BackupError::NotABackup)?;
    if body_len < TAG_LEN {
        return Err(BackupError::NotABackup);
    }
    let body = r.take(body_len)?;
    if r.pos != file.len() {
        return Err(BackupError::NotABackup);
    }
    Ok(ParsedFile { header: Header { wallet_id, created, entries }, header_bytes, body_nonce, body })
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], BackupError> {
        let end = self.pos.checked_add(n).filter(|end| *end <= self.data.len()).ok_or(BackupError::NotABackup)?;
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, BackupError> {
        Ok(self.take(1)?[0])
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], BackupError> {
        Ok(self.take(N)?.try_into().expect("take returned N bytes"))
    }
}
