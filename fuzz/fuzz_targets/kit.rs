//! Exit kits, which come from the owner's cloud drive, a printout typed back
//! in, or anywhere else, through `kit::read`, `kit::restore_kit` and the
//! wallet setup parser: either raw bytes, or a real kit (made inside the
//! target with a simulated device) edited by the input.
//!
//! Besides never panicking:
//! - a setup text that parses writes back as exactly the same text;
//! - a kit that reads back with a different backup never opens with the
//!   device, and one with a different plain setup is refused as changed;
//! - a kit that opens gives back the original phrase.
//!
//! Nothing is ever printed.

#![no_main]

use libfuzzer_sys::arbitrary::{Result as ArbResult, Unstructured};
use libfuzzer_sys::fuzz_target;
use satkeep_backup::setup::{HeirSettings, Network, SavingsWallet};
use satkeep_backup::{BackupError, SimulatedDevice, WalletSetup, kit};
use satkeep_savings::HolderKey;

const PIN: &str = "2468";

struct Fixture {
    phrase: String,
    backup: Vec<u8>,
    setup: WalletSetup,
    text: String,
}

thread_local! {
    static DEVICE: std::cell::RefCell<SimulatedDevice> = std::cell::RefCell::new(SimulatedDevice::new(PIN));
    static FIXTURE: &'static Fixture = Box::leak(Box::new(DEVICE.with(|d| {
        let phone = HolderKey::generate();
        let setup = WalletSetup {
            network: Network::Regtest,
            pocket: None,
            savings: vec![
                SavingsWallet { descriptor: "wsh(plain)".into(), birthday: 101, heir: None },
                SavingsWallet {
                    descriptor: "tr(heir)".into(),
                    birthday: 150,
                    heir: Some(HeirSettings { heir_delay: 20, checkin_interval: 10, checkin_margin: 5, refresh_window: Some(2) }),
                },
            ],
        };
        let mut device = d.borrow_mut();
        device.enter_pin(PIN).expect("right PIN");
        let backup = satkeep_savings::backup::make_backup(&phone, &setup, satkeep_backup::new_wallet_id(), 1_792_713_600, &mut *device)
            .expect("a backup");
        let text = kit::encode(&backup, &setup).expect("a kit");
        Fixture { phrase: phone.to_secret_string(), backup, setup, text }
    })));
}

fn edit(text: &str, u: &mut Unstructured) -> ArbResult<Vec<u8>> {
    let mut out = text.as_bytes().to_vec();
    for _ in 0..u.int_in_range(1..=4)? {
        match u.int_in_range(0..=4)? {
            0 if !out.is_empty() => {
                let i = u.choose_index(out.len())?;
                out[i] = *u.choose(b"0123456789abcdefABCDEF \r\n-x")?;
            }
            1 => out.truncate(u.int_in_range(0..=out.len())?),
            2 => {
                let i = u.int_in_range(0..=out.len())?;
                out.insert(i, u.arbitrary()?);
            }
            3 => {
                // Drop or repeat a line.
                let lines: Vec<&[u8]> = out.split(|b| *b == b'\n').collect();
                let i = u.choose_index(lines.len())?;
                let mut rebuilt: Vec<&[u8]> = lines.clone();
                if u.arbitrary()? { rebuilt.remove(i); } else { rebuilt.insert(i, lines[i]); }
                out = rebuilt.join(&b'\n');
            }
            _ if !out.is_empty() => {
                let i = u.choose_index(out.len())?;
                out.remove(i);
            }
            _ => {}
        }
    }
    Ok(out)
}

fuzz_target!(|data: &[u8]| {
    let fixture: &Fixture = FIXTURE.with(|f| *f);
    let mut u = Unstructured::new(data);
    let input = match u.int_in_range(0..=2u8).unwrap_or(0) {
        0 => match edit(&fixture.text, &mut u) {
            Ok(input) => input,
            Err(_) => return,
        },
        _ => u.take_rest().to_vec(),
    };

    if let Ok(text) = std::str::from_utf8(&input)
        && let Ok(setup) = WalletSetup::from_text(text)
    {
        assert!(setup.to_text().as_deref() == Ok(text), "a setup has two text forms");
    }

    let Ok(found) = kit::read(&input) else { return };
    DEVICE.with(|d| {
        let mut device = d.borrow_mut();
        device.enter_pin(PIN).expect("right PIN");
        match kit::restore_kit(&found, &mut *device) {
            Ok(restored) => {
                // assert!, not assert_eq!: a failure must not print a phrase.
                assert!(restored.phrase.to_string() == fixture.phrase, "a kit opened to a different key");
                assert!(found.backup == fixture.backup, "a changed backup opened");
                assert!(found.plain_setup.as_ref().is_none_or(|s| *s == fixture.setup), "a changed plain setup was accepted");
            }
            Err(BackupError::KitSetupMismatch) => {
                assert!(found.plain_setup.as_ref() != Some(&fixture.setup), "the right setup was refused as changed");
            }
            Err(_) => {}
        }
    });
});
