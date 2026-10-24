//! TEST ONLY: makes an exit kit the way the app will, for the exit drill.
//!
//! `make_test_kit PHONE_KEY_FILE SETUP_FILE KIT_OUT DEVICE_OUT`
//!
//! Locks a backup of the phone key (12 words in PHONE_KEY_FILE) and the
//! wallet setup (SETUP_FILE, in its one text form) to a fresh simulated
//! second key, and writes the kit's armoured text to KIT_OUT and the
//! simulated device's state (secret, then PIN 2468) to DEVICE_OUT. Both are
//! new files. Prints nothing secret. Test networks only; built only with
//! `--features simulated-device`.

use satkeep_savings::HolderKey;
use satkeep_savings::backup::{self, SimulatedDevice, WalletSetup, kit};

const PIN: &str = "2468";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [phone_file, setup_file, kit_out, device_out] = args.as_slice() else {
        eprintln!("usage: make_test_kit PHONE_KEY_FILE SETUP_FILE KIT_OUT DEVICE_OUT");
        std::process::exit(2);
    };
    let phone = HolderKey::from_secret_str(&std::fs::read_to_string(phone_file).expect("reading the phone key file"))
        .unwrap_or_else(|_| fail("the phone key file isn't 12 words"));
    let setup = WalletSetup::from_text(&std::fs::read_to_string(setup_file).expect("reading the setup file"))
        .unwrap_or_else(|e| fail(&format!("the setup file: {e}")));

    let mut secret = [0u8; 32];
    secret[..16].copy_from_slice(&backup::new_wallet_id());
    secret[16..].copy_from_slice(&backup::new_wallet_id());
    let mut device = SimulatedDevice::from_secret(secret, PIN);
    device.enter_pin(PIN).expect("the right PIN");
    // 2026-10-23, as the drill's backup date.
    let file = backup::make_backup(&phone, &setup, backup::new_wallet_id(), 1_792_713_600, &mut device)
        .unwrap_or_else(|e| fail(&format!("making the backup: {e}")));
    let text = kit::encode(&file, &setup).unwrap_or_else(|e| fail(&format!("making the kit: {e}")));

    let hex: String = secret.iter().map(|b| format!("{b:02x}")).collect();
    write_new(kit_out, &text);
    write_new(device_out, &format!("{hex}\n{PIN}\n"));
    println!("Wrote a test exit kit and its simulated second key.");
}

fn write_new(path: &str, text: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).expect("creating a new file");
    file.write_all(text.as_bytes()).expect("writing");
}

fn fail(message: &str) -> ! {
    eprintln!("make_test_kit: {message}");
    std::process::exit(1);
}
