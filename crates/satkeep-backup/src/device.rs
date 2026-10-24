//! The security device ("second key") that locks the phone key backup.
//!
//! A device takes part through one call: given a request (a label tied to
//! this wallet), it returns a 32-byte secret that only it can produce, the
//! same every time, and only after its own PIN or fingerprint. That's how
//! Trezor's SLIP-0011 CipherKeyValue works (an AES key derived on the device
//! from a label); see docs/phone-key-backup.md section 3 for which real
//! devices can do it.

use zeroize::Zeroizing;

/// The device's public identity at the backup-unlock path. It's stored in
/// the backup so the app can tell the wrong device apart before asking it to
/// unlock anything.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DeviceId(pub [u8; 32]);

/// How a device produces its unlock secret. Written into each lock entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnlockMethod {
    /// A key derived on the device from a label, after its PIN (SLIP-0011
    /// style). The only method so far.
    CipherKeyValue,
}

impl UnlockMethod {
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::CipherKeyValue => 1,
        }
    }

    pub(crate) fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::CipherKeyValue),
            _ => None,
        }
    }
}

/// Why a device refused. None of these carry anything secret.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("unlock the security device with its PIN first")]
    PinRequired,
    #[error("wrong PIN for the security device ({tries_left} tries left)")]
    WrongPin { tries_left: u8 },
    #[error("the security device is locked after too many wrong PINs")]
    Blocked,
}

/// A security device that can lock and unlock the phone key backup.
pub trait SecurityDevice {
    /// The device's public identity. Readable without the PIN, like a key
    /// fingerprint.
    fn id(&self) -> DeviceId;

    /// How this device produces its secret.
    fn method(&self) -> UnlockMethod;

    /// The device's secret for `request`. The same request always gives the
    /// same secret. Fails unless the device has been unlocked with its own
    /// PIN or fingerprint.
    fn unlock_secret(&mut self, request: &[u8]) -> Result<Zeroizing<[u8; 32]>, DeviceError>;
}

/// Wrong PINs allowed before a [`SimulatedDevice`] blocks for good.
#[cfg(feature = "simulated-device")]
pub const MAX_PIN_TRIES: u8 = 5;

/// A security device simulated in software, for tests and the prototype.
///
/// It holds a random 32-byte secret made when it's created, behaves like a
/// SLIP-0011 device (secret = HKDF of its own secret and the request), needs
/// its PIN entered before each unlock, and blocks after [`MAX_PIN_TRIES`]
/// wrong PINs. Deliberately not `Debug`. Test networks only, and only with
/// the `simulated-device` feature.
#[cfg(feature = "simulated-device")]
pub struct SimulatedDevice {
    secret: Zeroizing<[u8; 32]>,
    pin: Zeroizing<String>,
    unlocked: bool,
    wrong_tries: u8,
    requests: u32,
}

#[cfg(feature = "simulated-device")]
impl SimulatedDevice {
    /// A fresh device with a random secret from the operating system's
    /// random source, and this PIN.
    pub fn new(pin: &str) -> Self {
        use chacha20poly1305::aead::{KeyInit, OsRng};
        let secret = Zeroizing::new(chacha20poly1305::XChaCha20Poly1305::generate_key(&mut OsRng).into());
        Self { secret, pin: Zeroizing::new(pin.to_owned()), unlocked: false, wrong_tries: 0, requests: 0 }
    }

    /// The same simulated device again, from its secret: for tests that
    /// make a backup in one process and unlock it in another (the restore
    /// tool's). Test networks and the `simulated-device` feature only.
    pub fn from_secret(secret: [u8; 32], pin: &str) -> Self {
        Self { secret: Zeroizing::new(secret), pin: Zeroizing::new(pin.to_owned()), unlocked: false, wrong_tries: 0, requests: 0 }
    }

    /// Enters the PIN on the device. A right PIN unlocks it for one
    /// [`SecurityDevice::unlock_secret`] call.
    pub fn enter_pin(&mut self, pin: &str) -> Result<(), DeviceError> {
        if self.wrong_tries >= MAX_PIN_TRIES {
            return Err(DeviceError::Blocked);
        }
        if pin.as_bytes() == self.pin.as_bytes() {
            self.unlocked = true;
            self.wrong_tries = 0;
            Ok(())
        } else {
            self.unlocked = false;
            self.wrong_tries += 1;
            match MAX_PIN_TRIES - self.wrong_tries {
                0 => Err(DeviceError::Blocked),
                tries_left => Err(DeviceError::WrongPin { tries_left }),
            }
        }
    }

    /// How many times anything asked this device for a secret, allowed or
    /// not. Lets tests check a wrong device is never even asked.
    pub fn requests(&self) -> u32 {
        self.requests
    }

    fn derive(&self, info: &[&[u8]]) -> [u8; 32] {
        let mut out = [0u8; 32];
        hkdf::Hkdf::<sha2::Sha256>::new(None, self.secret.as_ref())
            .expand_multi_info(info, &mut out)
            .expect("32 bytes is a valid HKDF-SHA256 length");
        out
    }
}

#[cfg(feature = "simulated-device")]
impl SecurityDevice for SimulatedDevice {
    fn id(&self) -> DeviceId {
        DeviceId(self.derive(&[b"satkeep/simulated-device/v1/id"]))
    }

    fn method(&self) -> UnlockMethod {
        UnlockMethod::CipherKeyValue
    }

    fn unlock_secret(&mut self, request: &[u8]) -> Result<Zeroizing<[u8; 32]>, DeviceError> {
        self.requests += 1;
        if self.wrong_tries >= MAX_PIN_TRIES {
            return Err(DeviceError::Blocked);
        }
        if !self.unlocked {
            return Err(DeviceError::PinRequired);
        }
        // One unlock per PIN entry, like a device that asks every time.
        self.unlocked = false;
        Ok(Zeroizing::new(self.derive(&[b"satkeep/simulated-device/v1/cipher-key-value", request])))
    }
}
