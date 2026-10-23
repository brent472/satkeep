//! Phone key backup files, which come from the owner's cloud, through
//! `backup::read_header` and `backup::restore`: either raw bytes, or a real
//! backup (made inside the target with a simulated device) edited by the
//! input.
//!
//! Besides never panicking: an edited backup must never open, and a backup
//! that opens must give back the original phrase. Nothing is ever printed.

#![no_main]

use libfuzzer_sys::arbitrary::{Result as ArbResult, Unstructured};
use libfuzzer_sys::fuzz_target;
use twofold_savings::HolderKey;
use twofold_savings::backup::{self, SimulatedDevice};

const PIN: &str = "2468";

struct Fixture {
    phrase: String,
    file: Vec<u8>,
}

thread_local! {
    static DEVICE: std::cell::RefCell<SimulatedDevice> = std::cell::RefCell::new(SimulatedDevice::new(PIN));
    static FIXTURE: &'static Fixture = Box::leak(Box::new(DEVICE.with(|d| {
        let phone = HolderKey::generate();
        let mut device = d.borrow_mut();
        device.enter_pin(PIN).expect("right PIN");
        let file = backup::make_backup(&phone, "setup", backup::new_wallet_id(), 1_792_713_600, &mut *device)
            .expect("a backup");
        Fixture { phrase: phone.to_secret_string(), file }
    })));
}

fn edit(file: &[u8], u: &mut Unstructured) -> ArbResult<Vec<u8>> {
    let mut out = file.to_vec();
    for _ in 0..u.int_in_range(1..=4)? {
        match u.int_in_range(0..=3)? {
            0 if !out.is_empty() => {
                let i = u.choose_index(out.len())?;
                out[i] ^= u.int_in_range(1..=255)?;
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
    Ok(out)
}

fuzz_target!(|data: &[u8]| {
    let fixture: &Fixture = FIXTURE.with(|f| *f);
    let mut u = Unstructured::new(data);
    let (file, edited) = if u.arbitrary::<bool>().unwrap_or(false) {
        match edit(&fixture.file, &mut u) {
            Ok(file) => (file, true),
            Err(_) => return,
        }
    } else {
        (u.take_rest().to_vec(), false)
    };

    let _ = backup::read_header(&file);
    DEVICE.with(|d| {
        let mut device = d.borrow_mut();
        device.enter_pin(PIN).expect("right PIN");
        if let Ok(restored) = backup::restore(&file, &mut *device) {
            // assert!, not assert_eq!: a failure must not print a phrase.
            assert!(restored.phone.to_secret_string() == fixture.phrase, "a backup opened to a different key");
            assert!(!edited || file == fixture.file, "an edited backup opened");
        }
    });
});
