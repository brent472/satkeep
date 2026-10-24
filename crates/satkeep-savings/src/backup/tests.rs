//! The app-side parts: the phone key as a `HolderKey`, and the cloud folder.
//! The format itself (tampering, devices, the exit kit, the test vectors)
//! is tested in the satkeep-backup crate. Keys and devices are made fresh in
//! each test; nothing is printed.

use super::*;

const PIN: &str = "2468";
/// 2026-10-23, as Unix seconds.
const CREATED: u64 = 1_792_713_600;

fn setup() -> WalletSetup {
    WalletSetup {
        network: setup::Network::Regtest,
        pocket: None,
        savings: vec![SavingsWallet { descriptor: "wsh(placeholder)".into(), birthday: 101, heir: None }],
    }
}

fn device() -> SimulatedDevice {
    SimulatedDevice::new(PIN)
}

/// A phone key, its device, and a backup of it locked to that device.
fn backed_up() -> (HolderKey, SimulatedDevice, WalletId, Vec<u8>) {
    let phone = HolderKey::generate();
    let mut device = device();
    let wallet_id = new_wallet_id();
    device.enter_pin(PIN).unwrap();
    let file = make_backup(&phone, &setup(), wallet_id, CREATED, &mut device).unwrap();
    (phone, device, wallet_id, file)
}

fn open(file: &[u8], device: &mut SimulatedDevice) -> Result<Restored, BackupError> {
    device.enter_pin(PIN).unwrap();
    restore(file, device)
}

#[test]
fn restore_gives_back_the_same_holder_key() {
    let (phone, mut device, wallet_id, file) = backed_up();
    let restored = open(&file, &mut device).unwrap();
    // assert!, not assert_eq!: a failure must not print the phrase.
    assert!(restored.phone.to_secret_string() == phone.to_secret_string(), "restored a different phrase");
    assert_eq!(restored.phone.public_key(0), phone.public_key(0));
    assert_eq!(restored.wallet_setup, setup());
    assert_eq!((restored.header.created, restored.header.wallet_id), (CREATED, wallet_id));
}

#[test]
fn restore_kit_gives_back_the_same_holder_key_and_refuses_a_changed_kit() {
    let (phone, mut device, _, file) = backed_up();
    let text = kit::encode(&file, &setup()).unwrap();
    let found = kit::read(text.as_bytes()).unwrap();
    device.enter_pin(PIN).unwrap();
    assert_eq!(restore_kit(&found, &mut device).unwrap().phone.public_key(0), phone.public_key(0));

    let mut changed = setup();
    changed.savings[0].descriptor = "wsh(someone-elses)".into();
    let swapped = kit::read(kit::encode(&file, &changed).unwrap().as_bytes()).unwrap();
    device.enter_pin(PIN).unwrap();
    assert_eq!(restore_kit(&swapped, &mut device).err(), Some(BackupError::KitSetupMismatch));
}

#[test]
fn replacing_the_device_locks_the_backup_to_the_new_one_only() {
    let (phone, mut old, wallet_id, file) = backed_up();
    let mut cloud = MemoryCloud::new();
    save(&mut cloud, file).unwrap();

    let mut new = device();
    new.enter_pin(PIN).unwrap();
    replace_device(&mut cloud, &phone, &setup(), wallet_id, CREATED + 86_400, &mut new).unwrap();

    assert_eq!(cloud.list().len(), 1, "only the new backup is left in view");
    let current = latest(&cloud).unwrap();
    assert_eq!(open(&current, &mut new).unwrap().phone.public_key(0), phone.public_key(0));
    assert_eq!(open(&current, &mut old).err(), Some(BackupError::WrongDevice), "the old device can't open it");
    assert_eq!(read_header(&current).unwrap().created, CREATED + 86_400);
}

/// The limit in docs/phone-key-backup.md section 7.3, pinned so it can't be
/// forgotten: deleting a file doesn't revoke it. Cloud revisions, the trash
/// or an earlier copy keep the old file, and the old device still opens it.
/// That's why a lost or stolen device also replaces the phone key.
#[test]
fn an_old_device_still_opens_an_old_copy_so_a_lost_device_must_also_replace_the_phone_key() {
    let (phone, mut old, wallet_id, file) = backed_up();
    let mut cloud = MemoryCloud::new();
    save(&mut cloud, file).unwrap();
    let mut new = device();
    new.enter_pin(PIN).unwrap();
    replace_device(&mut cloud, &phone, &setup(), wallet_id, CREATED, &mut new).unwrap();

    let copies = cloud.every_copy_ever();
    assert_eq!(copies.len(), 2);
    let reopened: Vec<bool> = copies.iter().map(|copy| open(copy, &mut old).is_ok()).collect();
    assert_eq!(reopened, [true, false], "the old device opens the old copy, not the new one");
}

#[test]
fn latest_finds_nothing_in_an_empty_cloud() {
    assert_eq!(latest(&MemoryCloud::new()).err(), Some(BackupError::NoBackup));
}
