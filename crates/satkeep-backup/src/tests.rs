//! Each property has a test that it holds and a test that the forbidden
//! version fails. Phrases and devices are made fresh in each test, except
//! the published test vectors in docs/backup-format.md; nothing is printed.

use super::*;
use crate::kit::{self, Kit};
use crate::setup::{HeirSettings, Network, SavingsWallet};

const PIN: &str = "2468";
/// 2026-10-23, as Unix seconds.
const CREATED: u64 = 1_792_713_600;

/// Byte offsets in the file (see format.rs).
const CREATED_AT: usize = 4 + 1 + 16;
const FIRST_ENTRY_KIND: usize = CREATED_AT + 8 + 1;

fn phrase() -> Mnemonic {
    let entropy: Zeroizing<[u8; 32]> = Zeroizing::new(XChaCha20Poly1305::generate_key(&mut OsRng).into());
    Mnemonic::from_entropy(&entropy[..16]).expect("16 bytes make a 12-word phrase")
}

fn setup() -> WalletSetup {
    WalletSetup {
        network: Network::Regtest,
        pocket: None,
        savings: vec![
            SavingsWallet { descriptor: "wsh(sortedmulti(2,a,b,c))#00000000".into(), birthday: 101, heir: None },
            SavingsWallet {
                descriptor: "tr(n,{multi_a(2,a,b,c),and_v(v:pk(d),older(20))})#11111111".into(),
                birthday: 150,
                heir: Some(HeirSettings { heir_delay: 20, checkin_interval: 10, checkin_margin: 5, refresh_window: Some(2) }),
            },
        ],
    }
}

fn device() -> SimulatedDevice {
    SimulatedDevice::new(PIN)
}

/// A phrase, its device, and a backup of it locked to that device.
fn backed_up() -> (Mnemonic, SimulatedDevice, WalletId, Vec<u8>) {
    let phrase = phrase();
    let mut device = device();
    let wallet_id = new_wallet_id();
    device.enter_pin(PIN).unwrap();
    let file = make_backup(&phrase, &setup(), wallet_id, CREATED, &mut device).unwrap();
    (phrase, device, wallet_id, file)
}

fn open(file: &[u8], device: &mut SimulatedDevice) -> Result<Restored, BackupError> {
    device.enter_pin(PIN).unwrap();
    restore(file, device)
}

/// 12 or more BIP39 words in a row anywhere in `text`.
fn contains_a_phrase(text: &str) -> bool {
    let list = Language::English.word_list();
    let mut run = 0;
    for word in text.split(|c: char| !c.is_ascii_lowercase()) {
        run = if list.contains(&word) { run + 1 } else { 0 };
        if run >= PHRASE_WORDS {
            return true;
        }
    }
    false
}

/// No error message may carry key material: no phrase, and no three words
/// in a row from `phrase`.
fn assert_no_secret(message: &str, phrase: &Mnemonic) {
    assert!(!contains_a_phrase(message), "an error looks like it holds a phrase");
    let words: Vec<&str> = phrase.words().collect();
    for three in words.windows(3) {
        assert!(!message.contains(&three.join(" ")), "an error repeats part of the phrase");
    }
}

// ---------------------------------------------------- the backup alone ----

#[test]
fn the_backup_alone_reveals_nothing_but_its_date_and_devices() {
    let (phrase, device, wallet_id, file) = backed_up();

    // Readable without any device: the date and which device locks it.
    let header = read_header(&file).unwrap();
    assert_eq!((header.created, header.wallet_id), (CREATED, wallet_id));
    assert_eq!(header.entries.len(), 1);
    assert_eq!(header.entries[0].device_id, device.id());

    // Nothing of the phrase or the setup is in the file: no run of its
    // words anywhere, and no single word in the encrypted body. (Single
    // words aren't checked in the readable header, whose device label says
    // "phone" and "wallet", which are BIP39 words too.)
    let text = String::from_utf8_lossy(&file);
    assert!(!contains_a_phrase(&text));
    let words: Vec<&str> = phrase.words().collect();
    for pair in words.windows(2) {
        assert!(!text.contains(&pair.join(" ")), "two phrase words in a row are in the file");
    }
    let body = &file[format::encode_header(&header).len()..];
    for word in words.iter().filter(|w| w.len() >= 4) {
        assert!(!body.windows(word.len()).any(|w| w == word.as_bytes()), "a phrase word is in the encrypted body");
    }
    assert!(!text.contains("sortedmulti"), "the setup is encrypted too");
    assert!(file.len() < 1024, "a backup is well under 1 MB");
}

#[test]
fn each_backup_uses_a_new_data_key() {
    let phrase = phrase();
    let mut device = device();
    let wallet_id = new_wallet_id();
    device.enter_pin(PIN).unwrap();
    let first = make_backup(&phrase, &setup(), wallet_id, CREATED, &mut device).unwrap();
    device.enter_pin(PIN).unwrap();
    let second = make_backup(&phrase, &setup(), wallet_id, CREATED, &mut device).unwrap();
    let (a, b) = (read_header(&first).unwrap(), read_header(&second).unwrap());
    assert_ne!(a.entries[0].wrapped_key, b.entries[0].wrapped_key);
    assert_ne!(first[first.len() - 64..], second[second.len() - 64..]);
}

#[test]
fn only_a_12_word_english_phrase_is_backed_up() {
    let mut device = device();
    let words_24 = Mnemonic::from_entropy(&XChaCha20Poly1305::generate_key(&mut OsRng)).unwrap();
    device.enter_pin(PIN).unwrap();
    assert_eq!(make_backup(&words_24, &setup(), new_wallet_id(), CREATED, &mut device).err(), Some(BackupError::BadPhrase));
    assert_eq!(device.requests(), 0, "the device isn't asked for a backup that won't be made");
}

// ------------------------------------------------- restore with device ----

#[test]
fn restore_with_the_backup_and_its_device_gives_back_the_same_phrase_and_setup() {
    let (phrase, mut device, wallet_id, file) = backed_up();
    let restored = open(&file, &mut device).unwrap();
    // assert!, not assert_eq!: a failure must not print the phrase.
    assert!(restored.phrase == phrase, "restored a different phrase");
    assert_eq!(restored.wallet_setup, setup());
    assert_eq!((restored.header.created, restored.header.wallet_id), (CREATED, wallet_id));
}

#[test]
fn the_wrong_device_is_refused_before_it_is_asked_anything() {
    let (phrase, _device, _, file) = backed_up();
    let mut other = device();
    let err = open(&file, &mut other).err().unwrap();
    assert_eq!(err, BackupError::WrongDevice);
    assert_eq!(other.requests(), 0, "a wrong device is never asked for its secret");
    assert_no_secret(&err.to_string(), &phrase);
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

// ------------------------------------------------------- tampering ----

#[test]
fn changing_any_byte_of_the_file_stops_it_opening() {
    let (phrase, mut device, _, file) = backed_up();
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

// --------------------------------------------------- the wallet setup ----

#[test]
fn a_setup_reads_back_as_itself_and_has_one_text_form() {
    for setup in [setup(), WalletSetup { pocket: Some(setup::PocketWallet { descriptor: "wpkh(p)".into(), birthday: 0 }), ..setup() }] {
        let text = setup.to_text().unwrap();
        assert_eq!(WalletSetup::from_text(&text), Ok(setup));
        assert!(text.ends_with('\n'));
    }
    let text = setup().to_text().unwrap();
    assert_eq!(
        text,
        "satkeep wallet setup 1\nnetwork regtest\n\
         savings\ndescriptor wsh(sortedmulti(2,a,b,c))#00000000\nbirthday 101\n\
         savings\ndescriptor tr(n,{multi_a(2,a,b,c),and_v(v:pk(d),older(20))})#11111111\nbirthday 150\n\
         heir-delay 20\ncheckin-interval 10\ncheckin-margin 5\nrefresh-window 2\n"
    );
}

#[test]
fn a_setup_in_any_other_form_is_refused() {
    let text = setup().to_text().unwrap();
    let variants: Vec<(String, &str)> = vec![
        (text.replace('\n', "\r\n"), "Windows line endings"),
        (text.trim_end().to_owned(), "no final newline"),
        (format!("{text}\n"), "a blank line at the end"),
        (text.replace("birthday 101", "birthday 0101"), "a leading zero"),
        (text.replace("birthday 101", "birthday +101"), "a sign"),
        (text.replace("birthday 101", "birthday  101"), "two spaces"),
        (text.replace("birthday 101", "birthday 101 "), "a trailing space"),
        (text.replace("birthday 101", "birthday 4294967296"), "a birthday past u32"),
        (text.replace("heir-delay 20", "heir-delay 0"), "a zero delay"),
        (text.replace("heir-delay 20", "heir-delay 65536"), "a delay past 65,535 blocks"),
        (text.replace("network regtest", "network bitcoin"), "mainnet"),
        (text.replace("network regtest", "network testnet"), "an unknown network"),
        (text.replace("setup 1", "setup 2"), "a newer version"),
        (text.replace("heir-delay 20\ncheckin-interval 10\n", "checkin-interval 10\nheir-delay 20\n"), "fields out of order"),
        (text.replace("checkin-margin 5\n", ""), "heir settings missing a field"),
        (text.replace("birthday 101\n", "birthday 101\nrefresh-window 2\n"), "a refresh window without a beneficiary"),
        (text.replace("#00000000", "# 00000000"), "a space in a descriptor"),
        (text.replace("#00000000", "#0000\u{e9}000"), "a non-ASCII descriptor"),
        (format!("{text}savings\n"), "a Savings wallet with nothing in it"),
        (format!("{text}pocket\ndescriptor p\nbirthday 1\n"), "the pocket after Savings"),
        ("satkeep wallet setup 1\nnetwork regtest\n".to_owned(), "no Savings wallet"),
        (String::new(), "nothing"),
    ];
    for (variant, what) in variants {
        assert_eq!(WalletSetup::from_text(&variant), Err(BackupError::BadSetup), "{what} was accepted");
    }
    let many = format!("{text}{}", "savings\ndescriptor x\nbirthday 1\n".repeat(setup::MAX_WALLETS - 1));
    assert_eq!(WalletSetup::from_text(&many), Err(BackupError::BadSetup), "too many wallets");
}

#[test]
fn a_setup_that_wouldnt_read_back_the_same_is_never_written() {
    let mut bad = setup();
    bad.savings[0].descriptor = "wsh(a)\nnetwork signet".into();
    assert_eq!(bad.to_text(), Err(BackupError::BadSetup), "a newline in a descriptor");
    let mut empty = setup();
    empty.savings.clear();
    assert_eq!(empty.to_text(), Err(BackupError::BadSetup), "no Savings wallet");
    let mut device = device();
    device.enter_pin(PIN).unwrap();
    assert_eq!(make_backup(&phrase(), &empty, new_wallet_id(), CREATED, &mut device).err(), Some(BackupError::BadSetup));
}

// ------------------------------------------------------ the exit kit ----

fn kit_text() -> (Mnemonic, SimulatedDevice, Vec<u8>, String) {
    let (phrase, device, _, file) = backed_up();
    let text = kit::encode(&file, &setup()).unwrap();
    (phrase, device, file, text)
}

#[test]
fn a_kit_reads_back_from_its_text_inside_a_pdf_or_as_the_raw_backup() {
    let (_, mut device, file, text) = kit_text();
    let expected = Kit { backup: file.clone(), plain_setup: Some(setup()) };
    assert!(kit::read(text.as_bytes()) == Ok(expected.clone()), "the kit text");

    // Inside other bytes, as in a PDF with the text as an uncompressed
    // attachment, and the same block twice.
    let mut pdf = b"%PDF-1.7\n\xe2\xe3\xcf\xd3\n1 0 obj << /Type /EmbeddedFile >> stream\n".to_vec();
    pdf.extend_from_slice(text.as_bytes());
    pdf.extend_from_slice(b"endstream\n(page text: ) Tj\n");
    pdf.extend_from_slice(text.as_bytes());
    pdf.extend_from_slice(b"%%EOF\n");
    assert!(kit::read(&pdf) == Ok(expected.clone()), "inside a PDF");

    // Typed or copied back in: other line endings, capitals, extra spaces
    // and blank lines, surrounding text.
    let lines: String = text
        .lines()
        .map(|l| if l.starts_with("-----") { format!("{l}\r\n") } else { format!("  {}\r\n\r\n", l.to_uppercase().replace(' ', "   ")) })
        .collect();
    let retyped = format!("Here it is:\r\n{lines}Thanks");
    assert!(kit::read(retyped.as_bytes()) == Ok(expected), "typed back in");

    // The raw backup: no plain setup, and it still restores.
    assert!(kit::read(&file) == Ok(Kit { backup: file.clone(), plain_setup: None }), "the raw backup");
    device.enter_pin(PIN).unwrap();
    assert!(kit::restore_kit(&kit::read(&file).unwrap(), &mut device).is_ok());
}

#[test]
fn a_typo_or_missing_line_names_the_line() {
    let (_, _, _, text) = kit_text();
    let lines: Vec<&str> = text.lines().collect();
    let data_lines = lines.len() - 2;
    assert!(data_lines >= 4, "the test needs a few lines");
    for n in 1..=data_lines {
        // One hex digit changed.
        let mut changed: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
        let line = &mut changed[n];
        let at = line.find(' ').unwrap() + 1;
        let digit = if &line[at..at + 1] == "0" { "1" } else { "0" };
        line.replace_range(at..at + 1, digit);
        let line_no = u32::try_from(n).unwrap();
        assert_eq!(kit::read(changed.join("\n").as_bytes()), Err(BackupError::KitLine { line: line_no }), "a typo on line {n}");
    }
    let mut without_2: Vec<&str> = lines.clone();
    without_2.remove(2);
    assert_eq!(kit::read(without_2.join("\n").as_bytes()), Err(BackupError::KitLine { line: 2 }), "line 2 missing");
    let mut without_last: Vec<&str> = lines.clone();
    without_last.remove(data_lines);
    assert_eq!(kit::read(without_last.join("\n").as_bytes()), Err(BackupError::KitIncomplete), "the last line missing");
    let no_end = text.replace(kit::END, "");
    assert_eq!(kit::read(no_end.as_bytes()), Err(BackupError::KitIncomplete), "no END marker");
    assert_eq!(kit::read(b"no kit here"), Err(BackupError::NotAKit));
    assert_eq!(kit::read(b""), Err(BackupError::NotAKit));
}

#[test]
fn two_different_kits_are_refused_and_a_bad_block_beside_a_good_one_is_skipped() {
    let (_, _, _, a) = kit_text();
    let (_, _, _, b) = kit_text();
    assert_eq!(kit::read(format!("{a}{b}").as_bytes()), Err(BackupError::TwoDifferentKits));
    let garbage = format!("{}\n(0001 not hex) Tj\n{}\n", kit::BEGIN, kit::END);
    assert!(kit::read(format!("{garbage}{a}").as_bytes()).is_ok(), "a PDF's page text between markers");
    assert_eq!(kit::read(garbage.as_bytes()), Err(BackupError::KitLine { line: 1 }));
}

#[test]
fn a_kit_whose_plain_setup_was_changed_is_refused_after_unlocking() {
    let (phrase, mut device, file, _) = kit_text();
    let mut other = setup();
    other.savings[1].heir = Some(HeirSettings { heir_delay: 10, checkin_interval: 5, checkin_margin: 2, refresh_window: None });
    let changed = kit::read(kit::encode(&file, &other).unwrap().as_bytes()).unwrap();
    device.enter_pin(PIN).unwrap();
    let err = kit::restore_kit(&changed, &mut device).err().unwrap();
    assert_eq!(err, BackupError::KitSetupMismatch);
    assert_no_secret(&err.to_string(), &phrase);

    // A swapped backup: another wallet's, with this kit's setup.
    let (_, _, _, theirs) = backed_up();
    let swapped = kit::read(kit::encode(&theirs, &setup()).unwrap().as_bytes()).unwrap();
    device.enter_pin(PIN).unwrap();
    assert_eq!(kit::restore_kit(&swapped, &mut device).err(), Some(BackupError::WrongDevice));
}

#[test]
fn a_kit_is_only_made_from_a_real_backup() {
    assert_eq!(kit::encode(b"SKBK", &setup()), Err(BackupError::NotABackup));
    let (_, _, _, file) = backed_up();
    let mut empty = setup();
    empty.savings.clear();
    assert_eq!(kit::encode(&file, &empty), Err(BackupError::BadSetup));
}

// ---------------------------------------------------- the test vectors ----

/// A device that answers with a fixed secret, for the test vectors.
struct FixedDevice {
    id: [u8; 32],
    secret: [u8; 32],
    expected_request: Vec<u8>,
}

impl SecurityDevice for FixedDevice {
    fn id(&self) -> DeviceId {
        DeviceId(self.id)
    }
    fn method(&self) -> UnlockMethod {
        UnlockMethod::CipherKeyValue
    }
    fn unlock_secret(&mut self, request: &[u8]) -> Result<Zeroizing<[u8; 32]>, DeviceError> {
        assert_eq!(request, self.expected_request, "the device was asked for the spec's label");
        Ok(Zeroizing::new(self.secret))
    }
}

/// The text of the code block after `<!-- vector: name -->` in the spec.
fn vector(spec: &str, name: &str) -> String {
    let marker = format!("<!-- vector: {name} -->");
    let after = &spec[spec.find(&marker).unwrap_or_else(|| panic!("no vector {name}")) + marker.len()..];
    let start = after.find("```").unwrap();
    let body = &after[start..];
    let body = &body[body.find('\n').unwrap() + 1..];
    body[..body.find("```").unwrap()].to_owned()
}

fn vector_hex(spec: &str, name: &str) -> Vec<u8> {
    unhex(&vector(spec, name).split_whitespace().collect::<String>()).unwrap()
}

fn input_text(spec: &str, name: &str) -> String {
    let inputs = vector(spec, "inputs");
    let line = inputs.lines().find(|l| l.starts_with(&format!("{name} "))).unwrap_or_else(|| panic!("no input {name}"));
    line[name.len()..].trim().to_owned()
}

fn input<const N: usize>(spec: &str, name: &str) -> [u8; N] {
    unhex(&input_text(spec, name)).unwrap().try_into().unwrap()
}

#[test]
fn the_spec_test_vectors_decode_and_are_made_exactly() {
    let spec = include_str!("../../../docs/backup-format.md");
    let wallet_id: WalletId = input(spec, "wallet-id");
    let created: u64 = input_text(spec, "created").parse().unwrap();
    let request = vector(spec, "unlock-request").trim_end_matches('\n').as_bytes().to_vec();
    assert_eq!(request, unlock_request(&wallet_id));
    let mut device = FixedDevice { id: input(spec, "device-id"), secret: input(spec, "device-secret"), expected_request: request };
    let phrase_line = vector(spec, "phrase");
    let phrase = Mnemonic::parse_in(Language::English, phrase_line.trim_end()).unwrap();
    let setup = WalletSetup::from_text(&vector(spec, "setup")).unwrap();

    // The intermediate values. assert!, not assert_eq!, here and below: a
    // failure must not print key material, even a test vector's.
    let kek = key_encryption_key(&device.secret, &wallet_id, device.id(), device.method());
    assert!(kek.to_vec() == vector_hex(spec, "kek"), "the KEK isn't the spec's");

    // Made from the inputs: exactly the spec's bytes.
    let file = seal(
        &phrase,
        &setup,
        wallet_id,
        created,
        &mut device,
        &input(spec, "data-key"),
        &input(spec, "wrap-nonce"),
        &input(spec, "body-nonce"),
    )
    .unwrap();
    assert!(file == vector_hex(spec, "backup"), "the backup isn't the spec's bytes");
    assert!(read_header(&file).unwrap().entries[0].wrapped_key.to_vec() == vector_hex(spec, "wrapped-key"), "the wrapped key isn't the spec's");
    let kit_text = kit::encode(&file, &setup).unwrap();
    assert!(kit_text == vector(spec, "kit"), "the kit text isn't the spec's");

    // Decoded from the spec's bytes: the inputs again.
    let restored = restore(&vector_hex(spec, "backup"), &mut device).unwrap();
    assert!(restored.phrase == phrase, "the vector restores to its phrase");
    assert_eq!(restored.wallet_setup, setup);
    let found = kit::read(vector(spec, "kit").as_bytes()).unwrap();
    assert!(found == Kit { backup: file, plain_setup: Some(setup) }, "the spec's kit reads back differently");
    assert!(kit::restore_kit(&found, &mut device).is_ok());
}
