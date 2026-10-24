//! Phone key backup: the phone key, encrypted on the phone, kept in the
//! owner's own cloud, and unlocked only with their security device.
//!
//! **Prototype, test networks only.** The file format, the device trait and
//! the exit kit are in the `satkeep-backup` crate, specified in
//! docs/backup-format.md, and re-exported here. This module adds what only
//! the app side needs: the phone key as a [`HolderKey`], and the cloud
//! folder. The design and its limits are in docs/phone-key-backup.md.
//!
//! Nothing here prints, logs, or puts key material in an error.

mod cloud;

pub use cloud::{CloudStore, MemoryCloud};
pub use satkeep_backup::kit::{self, Kit};
pub use satkeep_backup::setup::{self, HeirSettings, SavingsWallet, WalletSetup};
pub use satkeep_backup::{
    BackupError, DeviceError, DeviceId, Header, LockEntry, LockKind, MAX_PIN_TRIES, SecurityDevice, SimulatedDevice,
    UnlockMethod, WalletId, new_wallet_id, read_header,
};

use crate::HolderKey;

/// A backup opened with the right device. Deliberately not `Debug`: it holds
/// the phone key.
pub struct Restored {
    pub phone: HolderKey,
    /// The wallet setup from inside the locked payload.
    pub wallet_setup: WalletSetup,
    pub header: Header,
}

/// Makes a backup of `phone` and `wallet_setup`, locked to `device`. The
/// device must have been unlocked with its own PIN. Each call uses a new data
/// key. `created` is the backup's date, in Unix seconds.
pub fn make_backup(
    phone: &HolderKey,
    wallet_setup: &WalletSetup,
    wallet_id: WalletId,
    created: u64,
    device: &mut dyn SecurityDevice,
) -> Result<Vec<u8>, BackupError> {
    satkeep_backup::make_backup(phone.mnemonic(), wallet_setup, wallet_id, created, device)
}

/// Opens a backup with `device`, which must have been unlocked with its own
/// PIN. A device the backup isn't locked to is refused before it's asked for
/// anything.
pub fn restore(file: &[u8], device: &mut dyn SecurityDevice) -> Result<Restored, BackupError> {
    into_holder(satkeep_backup::restore(file, device)?)
}

/// Opens an exit kit's backup with `device`, refusing a kit whose plain
/// wallet setup differs from the locked one.
pub fn restore_kit(kit: &Kit, device: &mut dyn SecurityDevice) -> Result<Restored, BackupError> {
    into_holder(satkeep_backup::kit::restore_kit(kit, device)?)
}

fn into_holder(restored: satkeep_backup::Restored) -> Result<Restored, BackupError> {
    let satkeep_backup::Restored { phrase, wallet_setup, header } = restored;
    let phone = HolderKey::from_mnemonic(phrase).map_err(|_| BackupError::CannotUnlock)?;
    Ok(Restored { phone, wallet_setup, header })
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
    wallet_setup: &WalletSetup,
    wallet_id: WalletId,
    created: u64,
    new_device: &mut dyn SecurityDevice,
) -> Result<(), BackupError> {
    let file = make_backup(phone, wallet_setup, wallet_id, created, new_device)?;
    save(cloud, file)
}

#[cfg(test)]
mod tests;
