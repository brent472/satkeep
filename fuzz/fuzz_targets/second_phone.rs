//! The second phone's unlock request and answer, which people scan or type
//! between the computer and the phone, through the phone's reader
//! (`SimulatedSecondPhone::answer`) and the restore tool's
//! (`SecondPhoneUnlock::enter_answer`): either raw text, or a real request
//! and answer (made inside the target with a simulated phone) edited by the
//! input.
//!
//! Besides never panicking: an answer the tool accepts either opens the
//! backup to the original phrase or opens nothing. Nothing is ever printed.

#![no_main]

use libfuzzer_sys::arbitrary::{Result as ArbResult, Unstructured};
use libfuzzer_sys::fuzz_target;
use satkeep_backup::bip39::{Language, Mnemonic};
use satkeep_backup::setup::{Network, SavingsWallet};
use satkeep_backup::{self as backup, BackupError, SecondPhoneUnlock, SimulatedSecondPhone, WalletSetup};
use satkeep_savings::HolderKey;

const PASSCODE: &str = "2468";

struct Fixture {
    phrase: String,
    file: Vec<u8>,
    request: String,
    answer: String,
}

struct Sides {
    phone: SimulatedSecondPhone,
    tool: SecondPhoneUnlock,
}

thread_local! {
    static SIDES: std::cell::RefCell<Option<Sides>> = const { std::cell::RefCell::new(None) };
    static FIXTURE: &'static Fixture = Box::leak(Box::new({
        let holder = HolderKey::generate();
        let phrase = holder.to_secret_string();
        let mnemonic = Mnemonic::parse_in(Language::English, phrase.as_str()).expect("a phrase");
        let setup = WalletSetup {
            network: Network::Regtest,
            pocket: None,
            savings: vec![SavingsWallet { descriptor: "wsh(setup)".into(), birthday: 0, heir: None }],
        };
        let mut phone = SimulatedSecondPhone::new(PASSCODE);
        let wallet_id = backup::new_wallet_id();
        phone.serve(wallet_id);
        let file = backup::make_backup_for_second_phone(&mnemonic, &setup, wallet_id, 1_792_713_600, &phone.public_key())
            .expect("a backup");
        let tool = SecondPhoneUnlock::for_backup(&backup::read_header(&file).expect("a header")).expect("a second phone");
        let request = tool.request_text().to_string();
        phone.enter_passcode(PASSCODE).expect("right passcode");
        let answer = phone.answer(&request).expect("an answer").to_string();
        SIDES.with(|s| *s.borrow_mut() = Some(Sides { phone, tool }));
        Fixture { phrase: phrase.to_string(), file, request, answer }
    }));
}

fn edit(text: &str, u: &mut Unstructured) -> ArbResult<String> {
    let mut out = text.as_bytes().to_vec();
    for _ in 0..u.int_in_range(1..=4)? {
        match u.int_in_range(0..=3)? {
            0 if !out.is_empty() => {
                let i = u.choose_index(out.len())?;
                out[i] = *u.choose(b"0123456789abcdefABCDEF \t\r\n-x")?;
            }
            1 => out.truncate(u.int_in_range(0..=out.len())?),
            2 => {
                let i = u.int_in_range(0..=out.len())?;
                out.insert(i, u.arbitrary()?);
            }
            _ if !out.is_empty() => {
                let i = u.choose_index(out.len())?;
                out.remove(i);
            }
            _ => {}
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fuzz_target!(|data: &[u8]| {
    let fixture: &Fixture = FIXTURE.with(|f| *f);
    let mut u = Unstructured::new(data);
    let (to_phone, base) = match u.int_in_range(0..=3u8).unwrap_or(0) {
        0 => (true, Some(&fixture.request)),
        1 => (false, Some(&fixture.answer)),
        2 => (true, None),
        _ => (false, None),
    };
    let text = match base {
        Some(base) => match edit(base, &mut u) {
            Ok(text) => text,
            Err(_) => return,
        },
        None => String::from_utf8_lossy(u.take_rest()).into_owned(),
    };

    SIDES.with(|s| {
        let mut sides = s.borrow_mut();
        let sides = sides.as_mut().expect("made with the fixture");
        if to_phone {
            sides.phone.enter_passcode(PASSCODE).expect("right passcode");
            let _ = sides.phone.answer(&text);
            return;
        }
        if sides.tool.enter_answer(&text).is_err() {
            return;
        }
        match backup::restore(&fixture.file, &mut sides.tool) {
            // assert!, not assert_eq!: a failure must not print a phrase.
            Ok(restored) => assert!(restored.phrase.to_string() == fixture.phrase, "an answer opened a different key"),
            Err(BackupError::CannotUnlock) => {}
            Err(other) => panic!("an accepted answer gave an unexpected error: {other}"),
        }
    });
});
