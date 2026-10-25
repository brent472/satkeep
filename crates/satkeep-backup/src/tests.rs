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
    let entropy: Zeroizing<[u8; 32]> = Zeroizing::new(<[u8; 32]>::generate());
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
    let words_24 = Mnemonic::from_entropy(&<[u8; 32]>::generate()).unwrap();
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

// ------------------------------------------- no private keys, ever ----

/// Made-up keys in the shape of private keys, never real ones.
const FAKE_TPRV: &str = "tprv8ZgxMBicQKsPdNotARealKeyJustTheShapeOfOneForTheTestNotARealKeyJustTheShapeOfOneForTheTestNotARealKeyJustT";
const FAKE_XPRV: &str = "xprv9s21ZrQH143KNotARealKeyJustTheShapeOfOneForTheTestNotARealKeyJustTheShapeOfOneForTheTestNotARealKeyJustT";

fn fake_wif(len: usize) -> String {
    format!("c{}", "N".repeat(len - 1))
}

fn with_descriptor(descriptor: &str) -> WalletSetup {
    let mut s = setup();
    s.savings[0].descriptor = descriptor.to_owned();
    s
}

#[test]
fn public_descriptors_are_accepted_even_when_a_tpub_happens_to_contain_prv() {
    // The spec's real descriptors (also checked by the vector test).
    let spec = &spec();
    let vectors = WalletSetup::from_text(&vector(spec, "setup")).unwrap();
    assert!(vectors.to_text().is_ok());
    // "prv" inside a public key's piece, and 51 or 52 characters that aren't
    // all base58 (here a 0, and a _), are fine.
    let tpub_with_prv = "tpubDCChhoz5Qdrkn7Z7KXawq6Ad6r3A4MUkCoVTqeWxfTkA6bHNJ3CHUEtALQdkNeixNz4446PcAmw4WKcj3mV2vb29H7sgprvPzbyCU1y2merw";
    for descriptor in [
        format!("wsh(sortedmulti(2,[73c5da0a/87'/1'/0']{tpub_with_prv}/<0;1>/*,a,b))#2pv7mn6z"),
        format!("wsh(pk({}0))", "N".repeat(51)),
        format!("wsh(multi_a{})", "N".repeat(44)),
    ] {
        let s = with_descriptor(&descriptor);
        assert_eq!(WalletSetup::from_text(&s.to_text().unwrap()), Ok(s), "{descriptor}");
    }
}

#[test]
fn a_setup_holding_a_private_key_is_refused_when_written_and_when_read() {
    let keyed = [
        format!("wsh(sortedmulti(2,[73c5da0a/87'/1'/0']{FAKE_TPRV}/<0;1>/*,a,b))#00000000"),
        format!("tr({FAKE_XPRV}/<0;1>/*)"),
        format!("wsh(pk({}))", fake_wif(52)),
        format!("sh(wpkh({}))", fake_wif(51)),
        format!("wsh(multi(2,a,{}))", fake_wif(52)),
    ];
    let mut device = device();
    for descriptor in &keyed {
        let s = with_descriptor(descriptor);
        assert!(s.to_text() == Err(BackupError::PrivateKeyInSetup), "written: a private key was accepted");
        let pocket = WalletSetup { pocket: Some(setup::PocketWallet { descriptor: descriptor.clone(), birthday: 0 }), ..setup() };
        assert!(pocket.to_text() == Err(BackupError::PrivateKeyInSetup), "written: a private key in the pocket");
        let text = setup().to_text().unwrap().replace("wsh(sortedmulti(2,a,b,c))#00000000", descriptor);
        assert!(WalletSetup::from_text(&text) == Err(BackupError::PrivateKeyInSetup), "read: a private key was accepted");
        device.enter_pin(PIN).unwrap();
        assert!(make_backup(&phrase(), &s, new_wallet_id(), CREATED, &mut device).err() == Some(BackupError::PrivateKeyInSetup));
    }
    // A kit whose plain copy holds one, made by hand, is refused on reading.
    let (_, _, _, file) = backed_up();
    let text = setup().to_text().unwrap().replace("wsh(sortedmulti(2,a,b,c))#00000000", &keyed[0]);
    let mut data = kit::MAGIC.to_vec();
    data.push(kit::VERSION);
    data.extend_from_slice(&u32::try_from(file.len()).unwrap().to_be_bytes());
    data.extend_from_slice(&file);
    data.extend_from_slice(&u32::try_from(text.len()).unwrap().to_be_bytes());
    data.extend_from_slice(text.as_bytes());
    assert!(kit::read(kit::armour(&data).as_bytes()) == Err(BackupError::PrivateKeyInSetup), "a kit with a private key");
    // And the error never repeats the key.
    assert!(!BackupError::PrivateKeyInSetup.to_string().contains("prv"));
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
    let mut too_long = lines.clone();
    let padded = format!("{}{}", lines[1], " ".repeat(kit::MAX_LINE));
    too_long[1] = &padded;
    assert_eq!(kit::read(too_long.join("
").as_bytes()), Err(BackupError::KitLine { line: 1 }), "an over-long line");
    let huge = format!("{}
0001{}
{}
", kit::BEGIN, " a".repeat(8 * 1024 * 1024), kit::END);
    assert_eq!(kit::read(huge.as_bytes()), Err(BackupError::KitLine { line: 1 }), "a huge line is refused before it's split");
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

// ---------------------------------------------------- the second phone ----

/// A second phone that's the second key of a fresh wallet, and a backup
/// locked to it by its public key alone.
fn phone_backup() -> (Mnemonic, SimulatedSecondPhone, WalletId, Vec<u8>) {
    let phrase = phrase();
    let mut phone = SimulatedSecondPhone::new(PIN);
    let wallet_id = new_wallet_id();
    phone.serve(wallet_id);
    let file = make_backup_for_second_phone(&phrase, &setup(), wallet_id, CREATED, &phone.public_key()).unwrap();
    (phrase, phone, wallet_id, file)
}

/// The tool's side of the exchange for `file`, with a fresh request.
fn tool_for(file: &[u8]) -> SecondPhoneUnlock {
    SecondPhoneUnlock::for_backup(&read_header(file).unwrap()).unwrap()
}

/// The whole exchange: the tool's request, the phone's answer after its
/// passcode, typed into the tool.
fn exchange(file: &[u8], phone: &mut SimulatedSecondPhone) -> SecondPhoneUnlock {
    let mut tool = tool_for(file);
    phone.enter_passcode(PIN).unwrap();
    let answer = phone.answer(&tool.request_text()).unwrap();
    tool.enter_answer(&answer).unwrap();
    tool
}

/// The 72 hex digits of an answer, spaces removed.
fn digits(answer: &str) -> String {
    answer.split_whitespace().collect()
}

#[test]
fn a_second_phone_unlocks_its_backup_through_a_request_and_an_answer() {
    let (phrase, mut phone, _, file) = phone_backup();
    let entry = &read_header(&file).unwrap().entries[0];
    assert_eq!((entry.method, entry.device_id), (UnlockMethod::SecondPhone, phone.id()));

    let mut tool = exchange(&file, &mut phone);
    let restored = restore(&file, &mut tool).unwrap();
    assert!(restored.phrase == phrase, "the phrase comes back");
    assert_eq!(restored.wallet_setup, setup());
    // One answer opens it once: the tool doesn't keep the secret.
    assert_eq!(restore(&file, &mut tool).err(), Some(BackupError::Device(DeviceError::AnswerRequired)));

    // Typed loosely: upper case, no spaces, a line break.
    let mut tool = tool_for(&file);
    phone.enter_passcode(PIN).unwrap();
    let answer = digits(&phone.answer(&tool.request_text()).unwrap()).to_uppercase();
    tool.enter_answer(&format!("{}\n{}", &answer[..30], &answer[30..])).unwrap();
    assert!(restore(&file, &mut tool).is_ok());
}

#[test]
fn every_backup_for_a_second_phone_has_its_own_ephemeral_key_and_every_request_its_own_answer() {
    let (phrase, mut phone, wallet_id, file) = phone_backup();
    let again = make_backup_for_second_phone(&phrase, &setup(), wallet_id, CREATED, &phone.public_key()).unwrap();
    assert_ne!(read_header(&file).unwrap().entries[0].request, read_header(&again).unwrap().entries[0].request);

    // Two requests for the same backup get different answers (a new
    // session key each time), and each opens it.
    let (first, second) = (tool_for(&file), tool_for(&file));
    phone.enter_passcode(PIN).unwrap();
    let a = phone.answer(&first.request_text()).unwrap();
    phone.enter_passcode(PIN).unwrap();
    let b = phone.answer(&second.request_text()).unwrap();
    assert_ne!(digits(&a)[..64], digits(&b)[..64], "the screen shows the secret masked, differently each time");
    for (mut tool, answer) in [(first, a), (second, b)] {
        tool.enter_answer(&answer).unwrap();
        assert!(restore(&file, &mut tool).is_ok());
    }
}

#[test]
fn the_phone_refuses_another_phones_or_another_wallets_request_before_its_passcode() {
    let (_, mut phone, _, file) = phone_backup();
    let (_, mut other, _, others_file) = phone_backup();
    phone.enter_passcode(PIN).unwrap();

    // Another phone's backup: refused without using the passcode just
    // entered, and without reaching the key.
    assert_eq!(phone.answer(&tool_for(&others_file).request_text()).err(), Some(SecondPhoneError::NotThisPhone));
    // Locked to this phone's key, but for a wallet it isn't the second key
    // of.
    let strangers_file = make_backup_for_second_phone(&phrase(), &setup(), new_wallet_id(), CREATED, &phone.public_key()).unwrap();
    assert_eq!(phone.answer(&tool_for(&strangers_file).request_text()).err(), Some(SecondPhoneError::NotThisWallet));
    assert_eq!(phone.answers(), 0, "a refused request never reaches the phone's key");

    // The passcode entered before still allows this backup's answer.
    let mut tool = tool_for(&file);
    let answer = phone.answer(&tool.request_text()).unwrap();
    tool.enter_answer(&answer).unwrap();
    assert!(restore(&file, &mut tool).is_ok());

    // The backup changed to name another phone, which then answers: its
    // answer doesn't open it.
    let wallet_id = read_header(&file).unwrap().wallet_id;
    let mut changed = file.clone();
    changed[FIRST_ENTRY_KIND + 1..FIRST_ENTRY_KIND + 33].copy_from_slice(&other.id().0);
    other.serve(wallet_id);
    let mut tool = exchange(&changed, &mut other);
    assert_eq!(restore(&changed, &mut tool).err(), Some(BackupError::CannotUnlock));
}

#[test]
fn the_phone_answers_only_after_its_passcode_and_blocks_after_wrong_ones() {
    let (_, mut phone, _, file) = phone_backup();
    let tool = tool_for(&file);
    let request = tool.request_text();
    assert_eq!(phone.answer(&request).err(), Some(SecondPhoneError::Device(DeviceError::PinRequired)));
    phone.enter_passcode(PIN).unwrap();
    phone.answer(&request).unwrap();
    assert_eq!(phone.answer(&request).err(), Some(SecondPhoneError::Device(DeviceError::PinRequired)), "one answer per passcode");
    for tries_left in (1..MAX_PIN_TRIES).rev() {
        assert_eq!(phone.enter_passcode("0000"), Err(DeviceError::WrongPin { tries_left }));
    }
    assert_eq!(phone.enter_passcode("0000"), Err(DeviceError::Blocked));
    assert_eq!(phone.enter_passcode(PIN), Err(DeviceError::Blocked), "blocked for good, even with the right passcode");
    assert_eq!(phone.answer(&request).err(), Some(SecondPhoneError::Device(DeviceError::Blocked)));
    assert_eq!(phone.answers(), 1);
}

#[test]
fn a_typo_or_an_answer_to_another_request_fails_the_check() {
    let (_, mut phone, _, file) = phone_backup();
    let mut tool = tool_for(&file);
    phone.enter_passcode(PIN).unwrap();
    let answer = digits(&phone.answer(&tool.request_text()).unwrap());

    let mut typo = answer.clone().into_bytes();
    typo[40] = if typo[40] == b'0' { b'1' } else { b'0' };
    let typo = String::from_utf8(typo).unwrap();
    for wrong in [typo.as_str(), &answer[..70], &format!("{answer}00"), "", "not an answer", &answer.replace('a', "g")] {
        let refused = tool.enter_answer(wrong).unwrap_err();
        assert_eq!(refused, SecondPhoneError::AnswerTypo);
        assert!(!refused.to_string().contains(&answer[..8]), "an error never repeats the answer");
    }
    // The right answer, to an earlier request for this same backup, fails
    // too: it's bound to its own request.
    let mut later = tool_for(&file);
    assert_eq!(later.enter_answer(&answer), Err(SecondPhoneError::AnswerTypo));
    // A long paste is refused without being read through.
    assert_eq!(tool.enter_answer(&" ".repeat(5000)), Err(SecondPhoneError::AnswerTypo));
    // After the typos, the right one still works.
    tool.enter_answer(&answer).unwrap();
    assert!(restore(&file, &mut tool).is_ok());

    // A typo in the request is caught on the phone.
    let request = tool_for(&file).request_text();
    let mut changed = request.as_bytes().to_vec();
    changed[10] = if changed[10] == b'0' { b'1' } else { b'0' };
    phone.enter_passcode(PIN).unwrap();
    assert_eq!(phone.answer(std::str::from_utf8(&changed).unwrap()).err(), Some(SecondPhoneError::RequestTypo));
    assert_eq!(phone.answer(&request[..200]).err(), Some(SecondPhoneError::RequestTypo));
}

/// Where a single-entry backup's request starts (see format.rs).
const REQUEST_AT: usize = FIRST_ENTRY_KIND + 1 + 32 + 1 + 2;

#[test]
fn a_changed_ephemeral_key_opens_nothing_even_with_the_phones_honest_answer() {
    let (_, mut phone, _, file) = phone_backup();
    // Another valid point in its place: the phone answers for it, and the
    // answer doesn't unwrap the data key.
    let mut changed = file.clone();
    changed[REQUEST_AT..REQUEST_AT + 33].copy_from_slice(&SimulatedSecondPhone::new(PIN).public_key());
    let mut tool = exchange(&changed, &mut phone);
    assert_eq!(restore(&changed, &mut tool).err(), Some(BackupError::CannotUnlock));

    // Not shaped like a key: refused before any request is shown.
    let mut changed = file.clone();
    changed[REQUEST_AT] = 0x04;
    assert_eq!(SecondPhoneUnlock::for_backup(&read_header(&changed).unwrap()).err(), Some(BackupError::CannotUnlock));
    // Shaped like one but off the curve: the phone refuses it without
    // working out an answer.
    let mut changed = file.clone();
    changed[REQUEST_AT + 1..REQUEST_AT + 33].fill(0xff);
    let tool = tool_for(&changed);
    phone.enter_passcode(PIN).unwrap();
    let before = phone.answers();
    assert_eq!(phone.answer(&tool.request_text()).err(), Some(SecondPhoneError::BadRequest));
    assert_eq!(phone.answers(), before);
}

#[test]
fn a_second_phone_lock_is_made_only_from_a_real_public_key_and_only_one_is_chosen() {
    let mut not_a_key = [0xffu8; 33];
    not_a_key[0] = 0x02;
    assert_eq!(
        make_backup_for_second_phone(&phrase(), &setup(), new_wallet_id(), CREATED, &not_a_key).err(),
        Some(BackupError::NotASecondPhoneKey)
    );
    // The tool's side isn't a device that derives a label's secret, so it
    // can't lock a backup.
    let (_, _, _, file) = phone_backup();
    assert_eq!(make_backup(&phrase(), &setup(), new_wallet_id(), CREATED, &mut tool_for(&file)).err(), Some(BackupError::WrongDevice));

    // Locked to a device and not a phone: there's nothing to ask a phone.
    let (_, _, _, device_file) = backed_up();
    assert_eq!(SecondPhoneUnlock::for_backup(&read_header(&device_file).unwrap()).err(), Some(BackupError::WrongDevice));
    // Two second phones: this version won't guess which one is at hand.
    let mut header = read_header(&file).unwrap();
    header.entries.push(header.entries[0].clone());
    assert_eq!(SecondPhoneUnlock::for_backup(&header).err(), Some(BackupError::SeveralSecondPhones));
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

/// The spec, with LF line endings even where git checked it out with CRLF
/// (core.autocrlf on Windows): the vectors' text uses LF.
fn spec() -> String {
    include_str!("../../../docs/backup-format.md").replace("\r\n", "\n")
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
    block_input(spec, "inputs", name)
}

fn block_input(spec: &str, block: &str, name: &str) -> String {
    let inputs = vector(spec, block);
    let line = inputs.lines().find(|l| l.starts_with(&format!("{name} "))).unwrap_or_else(|| panic!("no input {name}"));
    line[name.len()..].trim().to_owned()
}

fn input<const N: usize>(spec: &str, name: &str) -> [u8; N] {
    unhex(&input_text(spec, name)).unwrap().try_into().unwrap()
}

#[test]
fn the_spec_test_vectors_decode_and_are_made_exactly() {
    let spec = &spec();
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
    let lock = device_lock(&wallet_id, &mut device).unwrap();
    let file = seal(
        &phrase,
        &setup.to_text().unwrap(),
        wallet_id,
        created,
        lock,
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

fn phone_input<const N: usize>(spec: &str, name: &str) -> [u8; N] {
    unhex(&block_input(spec, "second-phone-inputs", name)).unwrap().try_into().unwrap()
}

#[test]
fn the_spec_second_phone_vectors_decode_and_are_made_exactly() {
    let spec = &spec();
    let wallet_id: WalletId = input(spec, "wallet-id");
    let created: u64 = input_text(spec, "created").parse().unwrap();
    let phrase = Mnemonic::parse_in(Language::English, vector(spec, "phrase").trim_end()).unwrap();
    let setup = WalletSetup::from_text(&vector(spec, "setup")).unwrap();
    let mut phone = SimulatedSecondPhone::from_secret(phone_input(spec, "phone-secret"), PIN).unwrap();
    phone.serve(wallet_id);
    let phone_key = phone.public_key();
    // assert!, not assert_eq!: a failure must not print key material, even
    // a test vector's.
    assert!(phone_key.to_vec() == vector_hex(spec, "phone-public-key"), "Q isn't the spec's");
    assert!(phone.id().0.to_vec() == vector_hex(spec, "phone-device-id"), "D isn't the spec's");

    // Locking, from the inputs: each intermediate value, then exactly the
    // spec's bytes.
    let ephemeral = p256::SecretKey::from_bytes(&phone_input::<32>(spec, "ephemeral-secret").into()).unwrap();
    let shared = p256::ecdh::diffie_hellman(
        ephemeral.to_nonzero_scalar(),
        p256::PublicKey::from_sec1_bytes(&phone_key).unwrap().as_affine(),
    );
    assert!(shared.raw_secret_bytes().to_vec() == vector_hex(spec, "shared-x"), "Z isn't the spec's");
    let lock = second_phone::lock(&phone_key, &wallet_id, &ephemeral).unwrap();
    assert!(lock.request == vector_hex(spec, "ephemeral-key"), "E isn't the spec's");
    assert!(lock.secret.to_vec() == vector_hex(spec, "phone-unlock-secret"), "S isn't the spec's");
    let kek = key_encryption_key(&lock.secret, &wallet_id, lock.device_id, lock.method);
    assert!(kek.to_vec() == vector_hex(spec, "phone-kek"), "the KEK isn't the spec's");
    let file = seal(
        &phrase,
        &setup.to_text().unwrap(),
        wallet_id,
        created,
        lock,
        &input(spec, "data-key"),
        &input(spec, "wrap-nonce"),
        &input(spec, "body-nonce"),
    )
    .unwrap();
    let entry = read_header(&file).unwrap().entries.remove(0);
    assert!(entry.wrapped_key.to_vec() == vector_hex(spec, "phone-wrapped-key"), "the wrapped key isn't the spec's");
    assert!(file == vector_hex(spec, "phone-backup"), "the backup isn't the spec's bytes");

    // The exchange, with the spec's session key: the tool's request, the
    // phone's answer, and the backup opened with it.
    let ephemeral_key = entry.request.as_slice().try_into().unwrap();
    let request = second_phone::UnlockRequest::with_session_key(wallet_id, entry.device_id, ephemeral_key, phone_input(spec, "session-key"));
    assert!(request.to_text().as_str() == vector(spec, "phone-request").trim_end(), "the request isn't the spec's");
    phone.enter_passcode(PIN).unwrap();
    let answer = phone.answer(&vector(spec, "phone-request")).unwrap();
    assert!(answer.as_str() == vector(spec, "phone-answer").trim_end(), "the answer isn't the spec's");
    assert!(digits(&answer)[..64] != hex(&vector_hex(spec, "phone-unlock-secret")), "the answer shows S masked");
    let mut tool = SecondPhoneUnlock::from_request(request);
    tool.enter_answer(&vector(spec, "phone-answer")).unwrap();
    let restored = restore(&vector_hex(spec, "phone-backup"), &mut tool).unwrap();
    assert!(restored.phrase == phrase, "the vector restores to its phrase");
    assert_eq!(restored.wallet_setup, setup);
}
