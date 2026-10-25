//! A Ledger as DEVICE, the second key of a Satkeep Savings wallet. Test
//! networks only: the Ledger must run the "Bitcoin Test" app.
//!
//! Three steps, each confirmed on the Ledger's own screen:
//!
//! 1. [`Ledger::public_key`]: the Ledger shows its public key at
//!    `m/87'/1'/0'` and the owner approves it. That key becomes DEVICE in
//!    `satkeep create`. The Bitcoin app gives out a key at this path only
//!    after showing it, because it isn't one of the app's standard paths.
//! 2. [`Ledger::register`]: the Ledger shows the Savings wallet's rules and
//!    keys, and the owner approves them. The Ledger answers with an approval
//!    code (an HMAC over the wallet's name and rules, under a key only the
//!    Ledger knows) that proves the registration later. The code isn't
//!    secret, but Satkeep never prints it.
//! 3. [`Ledger::sign`]: the Ledger shows a Savings payment, the owner
//!    approves it, and it signs as DEVICE: the 2-of-3 leaf of Savings with a
//!    beneficiary, or the multisig script of Savings with none.
//!
//! The Ledger signs only for a wallet registered on it, and only with the
//! approval code it gave for that wallet. Its signature alone moves
//! nothing: Savings needs a second key, or HEIR alone after the heir delay.
//!
//! Built on Liana's `async-hwi`, Ledger only.

use std::fmt;
use std::str::FromStr;

use async_hwi::HWI;
use async_hwi::ledger::{Ledger as HwiLedger, Transport, TransportHID};
use satkeep_savings::bitcoin::NetworkKind;
use satkeep_savings::bitcoin::bip32::{DerivationPath, Fingerprint, Xpub};
use satkeep_savings::bitcoin::psbt::Psbt;
use satkeep_savings::miniscript::descriptor::DescriptorPublicKey;
use satkeep_savings::{Role, SavingsPolicy};
use tokio::runtime::Runtime;

/// The path of DEVICE's key on the Ledger: the same test-only placeholder
/// every Satkeep key uses (BIP 87 multisig, test network, account 0).
pub const KEY_PATH: &str = "m/87'/1'/0'";

/// The name the Ledger shows for a Savings wallet, at registration and when
/// signing. The same for both shapes: the Ledger also shows the rules and
/// keys, which tell them apart.
pub const WALLET_NAME: &str = "Satkeep Savings";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no Ledger found: plug it in, unlock it and open the Bitcoin Test app")]
    NotFound,
    #[error("the Ledger refused: {0}")]
    Refused(String),
    #[error("the Ledger gave a mainnet key; open the Bitcoin Test app instead")]
    Mainnet,
    #[error("this Savings wallet's DEVICE key isn't from this Ledger (fingerprint {0}); is it the right Ledger?")]
    NotThisLedger(Fingerprint),
    #[error("the Ledger signed nothing in this payment")]
    SignedNothing,
    #[error("the Ledger couldn't sign every coin in this payment, so nothing was kept; the payment may have been altered")]
    SignedSome,
    #[error("couldn't start the Ledger connection: {0}")]
    Runtime(String),
}

/// Proof that the owner registered a Savings wallet on their Ledger: the
/// wallet's name and descriptor, and the approval code the Ledger gave for
/// them. Not secret, but never printed: `Debug` leaves the code out.
#[derive(Clone, PartialEq, Eq)]
pub struct Registration {
    pub name: String,
    pub descriptor: String,
    pub hmac: [u8; 32],
}

impl fmt::Debug for Registration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registration")
            .field("name", &self.name)
            .field("descriptor", &self.descriptor)
            .finish_non_exhaustive()
    }
}

impl Registration {
    /// Whether this registration is for `policy`.
    pub fn is_for(&self, policy: &SavingsPolicy) -> bool {
        self.descriptor == policy.export_descriptor()
    }
}

enum Device {
    Usb(HwiLedger<TransportHID>),
    #[cfg(feature = "speculos")]
    Speculos(async_hwi::ledger::LedgerSimulator),
}

impl From<HwiLedger<TransportHID>> for Device {
    fn from(d: HwiLedger<TransportHID>) -> Self {
        Device::Usb(d)
    }
}

#[cfg(feature = "speculos")]
impl From<async_hwi::ledger::LedgerSimulator> for Device {
    fn from(d: async_hwi::ledger::LedgerSimulator) -> Self {
        Device::Speculos(d)
    }
}

/// A connected Ledger running the Bitcoin Test app.
pub struct Ledger {
    // Taken out and put back by calls that configure it first; `None` only
    // if one of those lost it, and then every call asks to reconnect.
    device: Option<Device>,
    runtime: Runtime,
}

impl Ledger {
    /// The first Ledger on USB.
    pub fn connect() -> Result<Self, Error> {
        let device = HwiLedger::<TransportHID>::try_connect_hid().map_err(|_| Error::NotFound)?;
        Ok(Self { device: Some(Device::Usb(device)), runtime: runtime()? })
    }

    /// Ledger's Speculos emulator on 127.0.0.1:9999. For tests.
    #[cfg(feature = "speculos")]
    pub fn connect_speculos() -> Result<Self, Error> {
        let runtime = runtime()?;
        let device =
            runtime.block_on(async_hwi::ledger::LedgerSimulator::try_connect()).map_err(|_| Error::NotFound)?;
        Ok(Self { device: Some(Device::Speculos(device)), runtime })
    }

    /// The Ledger's master key fingerprint. Needs no approval.
    pub fn fingerprint(&mut self) -> Result<Fingerprint, Error> {
        let result = match self.device.as_ref().ok_or(Error::NotFound)? {
            Device::Usb(d) => self.runtime.block_on(d.get_master_fingerprint()),
            #[cfg(feature = "speculos")]
            Device::Speculos(d) => self.runtime.block_on(d.get_master_fingerprint()),
        };
        result.map_err(refused)
    }

    /// The Ledger's public key for Savings, as
    /// `[fingerprint/87'/1'/0']tpub…/<0;1>/*`: DEVICE's key. The Ledger shows
    /// it, and gives it only once the owner approves.
    pub fn public_key(&mut self) -> Result<DescriptorPublicKey, Error> {
        let fingerprint = self.fingerprint()?;
        let path = DerivationPath::from_str(KEY_PATH).expect("valid path");
        let xpub = self.with_device(|runtime, device| match device {
            Device::Usb(d) => key_shown(runtime, d, &path),
            #[cfg(feature = "speculos")]
            Device::Speculos(d) => key_shown(runtime, d, &path),
        })?;
        if xpub.network == NetworkKind::Main {
            return Err(Error::Mainnet);
        }
        Ok(DescriptorPublicKey::from_str(&format!("[{fingerprint}/87'/1'/0']{xpub}/<0;1>/*"))
            .expect("a well-formed descriptor key"))
    }

    /// Registers `policy` on the Ledger. The Ledger shows the wallet's name,
    /// rules and every key, and registers it only once the owner approves.
    /// Refuses a policy whose DEVICE key isn't this Ledger's.
    pub fn register(&mut self, policy: &SavingsPolicy) -> Result<Registration, Error> {
        self.check_is_device(policy)?;
        let descriptor = policy.export_descriptor();
        let result = match self.device.as_ref().ok_or(Error::NotFound)? {
            Device::Usb(d) => self.runtime.block_on(d.register_wallet(WALLET_NAME, &descriptor)),
            #[cfg(feature = "speculos")]
            Device::Speculos(d) => self.runtime.block_on(d.register_wallet(WALLET_NAME, &descriptor)),
        };
        let hmac = result.map_err(refused)?.ok_or_else(|| Error::Refused("no approval code came back".into()))?;
        Ok(Registration { name: WALLET_NAME.into(), descriptor, hmac })
    }

    /// Has the Ledger show `psbt` and, once the owner approves, sign it as
    /// DEVICE for the wallet in `registration`. Returns how many signatures
    /// it added.
    ///
    /// Leaves `psbt` as it was if the Ledger signs nothing, or doesn't sign
    /// every input: a payment that hides a coin from DEVICE has been altered.
    pub fn sign(&mut self, registration: &Registration, psbt: &mut Psbt) -> Result<usize, Error> {
        let mut signed = psbt.clone();
        let before = signatures_per_input(&signed);
        self.with_device(|runtime, device| match device {
            Device::Usb(d) => sign_with(runtime, d, registration, &mut signed),
            #[cfg(feature = "speculos")]
            Device::Speculos(d) => sign_with(runtime, d, registration, &mut signed),
        })?;
        let after = signatures_per_input(&signed);
        let added: usize = after.iter().zip(&before).map(|(a, b)| a.saturating_sub(*b)).sum();
        if added == 0 {
            return Err(Error::SignedNothing);
        }
        if before.iter().zip(&after).any(|(b, a)| a <= b) {
            return Err(Error::SignedSome);
        }
        *psbt = signed;
        Ok(added)
    }

    /// Checks DEVICE's key in `policy` is from this Ledger, by its
    /// fingerprint. The Ledger itself checks its keys again when it
    /// registers.
    fn check_is_device(&mut self, policy: &SavingsPolicy) -> Result<(), Error> {
        let ours = self.fingerprint()?;
        let device = policy.keys().get(Role::Device).expect("DEVICE is always there");
        if device.master_fingerprint() != ours {
            return Err(Error::NotThisLedger(ours));
        }
        Ok(())
    }

    /// Runs `f` with the device taken out (some of async-hwi's options
    /// consume it) and puts back whatever `f` returns.
    fn with_device<T>(
        &mut self,
        f: impl FnOnce(&Runtime, Device) -> (Option<Device>, Result<T, Error>),
    ) -> Result<T, Error> {
        let device = self.device.take().ok_or(Error::NotFound)?;
        let (device, result) = f(&self.runtime, device);
        self.device = device;
        result
    }
}

/// The key at `path`, shown on the Ledger and approved there.
fn key_shown<T: Transport + Sync + Send>(
    runtime: &Runtime,
    device: HwiLedger<T>,
    path: &DerivationPath,
) -> (Option<Device>, Result<Xpub, Error>)
where
    Device: From<HwiLedger<T>>,
{
    // Setting the option can't fail; it returns a Result only for its API.
    let Ok(device) = device.display_xpub(true) else { return (None, Err(Error::NotFound)) };
    let result = runtime.block_on(device.get_extended_pubkey(path)).map_err(refused);
    let Ok(device) = device.display_xpub(false) else { return (None, result) };
    (Some(Device::from(device)), result)
}

fn sign_with<T: Transport + Sync + Send>(
    runtime: &Runtime,
    device: HwiLedger<T>,
    registration: &Registration,
    psbt: &mut Psbt,
) -> (Option<Device>, Result<(), Error>)
where
    Device: From<HwiLedger<T>>,
{
    // Fails only on a descriptor it can't read; the device is then gone, and
    // the next call asks to reconnect.
    let device = match device.with_wallet(registration.name.clone(), &registration.descriptor, Some(registration.hmac)) {
        Ok(device) => device,
        Err(_) => return (None, Err(Error::Refused("the registered wallet couldn't be read".into()))),
    };
    let result = runtime.block_on(device.sign_tx(psbt)).map_err(refused);
    (Some(Device::from(device)), result)
}

fn runtime() -> Result<Runtime, Error> {
    tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| Error::Runtime(e.to_string()))
}

fn refused(e: async_hwi::Error) -> Error {
    match e {
        async_hwi::Error::DeviceNotFound | async_hwi::Error::DeviceDisconnected => Error::NotFound,
        other => Error::Refused(other.to_string()),
    }
}

fn signatures_per_input(psbt: &Psbt) -> Vec<usize> {
    psbt.inputs
        .iter()
        .map(|i| i.tap_script_sigs.len() + i.partial_sigs.len() + usize::from(i.tap_key_sig.is_some()))
        .collect()
}
