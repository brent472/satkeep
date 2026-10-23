//! Each property has a test that it holds and a test that the forbidden
//! version fails. Keys and devices are made fresh in each test; nothing is
//! printed.

use super::*;
use crate::contains_recovery_phrase;

const PIN: &str = "2468";
/// 2026-10-23, as Unix seconds.
const CREATED: u64 = 1_792_713_600;
const SETUP: &str = "tr([00000000/87'/1'/0']tpubPlaceholder/<0;1>/*) savings setup for the test";

/// Byte offsets in the file (see format.rs).
const CREATED_AT: usize = 4 + 1 + 16;
const FIRST_ENTRY_KIND: usize = CREATED_AT + 8 + 1;

fn device() -> SimulatedDevice {
    SimulatedDevice::new(PIN)
}

/// A phone key, its device, and a backup of it locked to that device.
fn backed_up() -> (HolderKey, SimulatedDevice, WalletId, Vec<u8>) {
    let phone = HolderKey::generate();
    let mut device = device();
    let wallet_id = new_wallet_id();
    device.enter_pin(PIN).unwrap();
    let file = make_backup(&phone, SETUP, wallet_id, CREATED, &mut device).unwrap();
    (phone, device, wallet_id, file)
}

fn open(file: &[u8], device: &mut SimulatedDevice) -> Result<Restored, BackupError> {
    device.enter_pin(PIN).unwrap();
    restore(file, device)
}

/// No error message may carry key material: no phrase, and no three words
/// in a row from `phrase`.
fn assert_no_secret(message: &str, phrase: &str) {
    assert!(!contains_recovery_phrase(message), "an error looks like it holds a phrase");
    let words: Vec<&str> = phrase.split(' ').collect();
    for three in words.windows(3) {
        assert!(!message.contains(&three.join(" ")), "an error repeats part of the phrase");
    }
}

// ---------------------------------------------------- the backup alone ----

#[test]
fn the_backup_alone_reveals_nothing_but_its_date_and_devices() {
    let (phone, device, wallet_id, file) = backed_up();
    let phrase = phone.to_secret_string();

    // Readable without any device: the date and which device locks it.
    let header = read_header(&file).unwrap();
    assert_eq!((header.created, header.wallet_id), (CREATED, wallet_id));
    assert_eq!(header.entries.len(), 1);
    assert_eq!(header.entries[0].device_id, device.id());

    // Nothing of the phrase or the setup is in the file.
    let text = String::from_utf8_lossy(&file);
    assert!(!contains_recovery_phrase(&text));
    assert!(!text.contains(&phrase));
    for word in phrase.split(' ').filter(|w| w.len() >= 4) {
        assert!(!file.windows(word.len()).any(|w| w == word.as_bytes()), "a phrase word is in the file");
    }
    assert!(!text.contains("savings setup"), "the setup is encrypted too");
    assert!(file.len() < 1024, "a backup is well under 1 MB");
}

#[test]
fn each_backup_uses_a_new_data_key() {
    let phone = HolderKey::generate();
    let mut device = device();
    let wallet_id = new_wallet_id();
    device.enter_pin(PIN).unwrap();
    let first = make_backup(&phone, SETUP, wallet_id, CREATED, &mut device).unwrap();
    device.enter_pin(PIN).unwrap();
    let second = make_backup(&phone, SETUP, wallet_id, CREATED, &mut device).unwrap();
    let (a, b) = (read_header(&first).unwrap(), read_header(&second).unwrap());
    assert_ne!(a.entries[0].wrapped_key, b.entries[0].wrapped_key);
    assert_ne!(first[first.len() - 64..], second[second.len() - 64..]);
}

// ------------------------------------------------- restore with device ----

#[test]
fn restore_with_the_backup_and_its_device_gives_back_the_same_phrase() {
    let (phone, mut device, wallet_id, file) = backed_up();
    let restored = open(&file, &mut device).unwrap();
    // assert!, not assert_eq!: a failure must not print the phrase.
    assert!(restored.phone.to_secret_string() == phone.to_secret_string(), "restored a different phrase");
    assert_eq!(restored.phone.to_secret_string().split(' ').count(), crate::PHRASE_WORDS);
    assert_eq!(restored.phone.public_key(0), phone.public_key(0));
    assert_eq!(restored.phone.public_key(1), phone.public_key(1));
    assert_eq!(restored.wallet_setup, SETUP);
    assert_eq!((restored.header.created, restored.header.wallet_id), (CREATED, wallet_id));
}

#[test]
fn the_wrong_device_is_refused_before_it_is_asked_anything() {
    let (phone, _device, _, file) = backed_up();
    let mut other = device();
    let err = open(&file, &mut other).err().unwrap();
    assert_eq!(err, BackupError::WrongDevice);
    assert_eq!(other.requests(), 0, "a wrong device is never asked for its secret");
    assert_no_secret(&err.to_string(), &phone.to_secret_string());
}

#[test]
fn a_device_that_hasnt_had_its_pin_entered_unlocks_nothing() {
    let (_, mut device, _, file) = backed_up();
    assert_eq!(restore(&file, &mut device).err(), Some(BackupError::Device(DeviceError::PinRequired)));
    // One PIN entry unlocks once.
    device.enter_pin(PIN).unwrap();
    assert!(restore(&file, &mut device).is_ok());
    assert_eq!(restore(&file, &mut device).err(), Some(BackupError::Device(DeviceError::PinRequired)));
}

#[test]
fn wrong_pins_block_the_device_for_good() {
    let (_, mut device, _, file) = backed_up();
    for tries_left in (1..MAX_PIN_TRIES).rev() {
        assert_eq!(device.enter_pin("0000"), Err(DeviceError::WrongPin { tries_left }));
        assert_eq!(restore(&file, &mut device).err(), Some(BackupError::Device(DeviceError::PinRequired)));
    }
    assert_eq!(device.enter_pin("0000"), Err(DeviceError::Blocked));
    assert_eq!(device.enter_pin(PIN), Err(DeviceError::Blocked), "even the right PIN");
    assert_eq!(restore(&file, &mut device).err(), Some(BackupError::Device(DeviceError::Blocked)));
}

// ------------------------------------------------------ replacing it ----

#[test]
fn replacing_the_device_locks_the_backup_to_the_new_one_only() {
    let (phone, mut old, wallet_id, file) = backed_up();
    let mut cloud = MemoryCloud::new();
    save(&mut cloud, file).unwrap();

    let mut new = device();
    new.enter_pin(PIN).unwrap();
    replace_device(&mut cloud, &phone, SETUP, wallet_id, CREATED + 86_400, &mut new).unwrap();

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
    replace_device(&mut cloud, &phone, SETUP, wallet_id, CREATED, &mut new).unwrap();

    let copies = cloud.every_copy_ever();
    assert_eq!(copies.len(), 2);
    let reopened: Vec<bool> = copies.iter().map(|copy| open(copy, &mut old).is_ok()).collect();
    assert_eq!(reopened, [true, false], "the old device opens the old copy, not the new one");
}

#[test]
fn latest_finds_nothing_in_an_empty_cloud() {
    assert_eq!(latest(&MemoryCloud::new()).err(), Some(BackupError::NoBackup));
}

// ------------------------------------------------------- tampering ----

#[test]
fn changing_any_byte_of_the_file_stops_it_opening() {
    let (phone, mut device, _, file) = backed_up();
    let phrase = phone.to_secret_string();
    for i in 0..file.len() {
        let mut changed = file.clone();
        changed[i] ^= 0x01;
        let err = open(&changed, &mut device).err().unwrap_or_else(|| panic!("byte {i} changed and it still opened"));
        assert_no_secret(&err.to_string(), &phrase);
    }
}

#[test]
fn changing_the_date_stops_it_opening() {
    let (_, mut device, _, file) = backed_up();
    let mut changed = file.clone();
    changed[CREATED_AT..CREATED_AT + 8].copy_from_slice(&(CREATED - 1).to_be_bytes());
    assert_eq!(read_header(&changed).unwrap().created, CREATED - 1, "the header still reads");
    assert_eq!(open(&changed, &mut device).err(), Some(BackupError::CannotUnlock));
}

#[test]
fn cut_short_padded_or_garbage_files_are_refused() {
    let (_, mut device, _, file) = backed_up();
    for len in 0..file.len() {
        assert!(read_header(&file[..len]).is_err(), "a {len}-byte prefix parsed");
        assert!(open(&file[..len], &mut device).is_err(), "prefix of {len} bytes opened");
    }
    let mut padded = file.clone();
    padded.push(0);
    assert_eq!(read_header(&padded).err(), Some(BackupError::NotABackup));
    assert_eq!(read_header(b"").err(), Some(BackupError::NotABackup));
    assert_eq!(read_header(&[0u8; 2000]).err(), Some(BackupError::NotABackup));
    let mut newer = file.clone();
    newer[4] = 2;
    assert_eq!(read_header(&newer).err(), Some(BackupError::UnsupportedVersion));
}

#[test]
fn a_changed_device_request_is_refused_before_the_device_is_asked() {
    let (_, mut device, _, file) = backed_up();
    let request_at = FIRST_ENTRY_KIND + 1 + 32 + 1 + 2;
    let mut changed = file.clone();
    changed[request_at] ^= 0x20;
    let asked = device.requests();
    assert_eq!(open(&changed, &mut device).err(), Some(BackupError::CannotUnlock));
    assert_eq!(device.requests(), asked, "a request this wallet didn't make never reaches the device");
}

#[test]
fn a_contact_lock_entry_is_reserved_and_never_opened_by_a_device() {
    let (_, mut device, _, file) = backed_up();
    let mut contact = file.clone();
    contact[FIRST_ENTRY_KIND] = 2;
    assert_eq!(read_header(&contact).unwrap().entries[0].kind, LockKind::Contact);
    let asked = device.requests();
    assert_eq!(open(&contact, &mut device).err(), Some(BackupError::WrongDevice));
    assert_eq!(device.requests(), asked, "the device wasn't asked");
}
