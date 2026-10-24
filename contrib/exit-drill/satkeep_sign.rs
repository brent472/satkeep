//! Signs a PSBT with Liana's own hot signer (`liana::signer::HotSigner`), for
//! Satkeep's exit drill.
//!
//! The drill copies this file into Liana's v15.0 source tree, after checking
//! the tag's signature, and builds it there as an example of the `liana`
//! crate. So the signing is Liana's code, built with Liana's own Cargo.lock.
//!
//! Usage: `satkeep_sign KEY_FILE < PSBT > SIGNED_PSBT`, with the PSBT in
//! base64. The key file holds a 12-word recovery phrase, which HotSigner reads
//! the same way the Liana app does (BIP39, no passphrase). Nothing secret is
//! ever printed: errors name only what failed.

use std::io::Read;
use std::process::ExitCode;
use std::str::FromStr;

use liana::miniscript::bitcoin::{psbt::Psbt, secp256k1, Network};
use liana::signer::HotSigner;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("satkeep_sign: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or("usage: satkeep_sign KEY_FILE < PSBT")?;
    let phrase = std::fs::read_to_string(path).map_err(|_| "can't read the key file")?;
    let signer = HotSigner::from_str(Network::Regtest, phrase.trim())
        .map_err(|_| "the key file doesn't hold a valid recovery phrase")?;
    drop(phrase);

    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).map_err(|_| "can't read the PSBT")?;
    let psbt = Psbt::from_str(text.trim()).map_err(|_| "that isn't a valid base64 PSBT")?;
    let secp = secp256k1::Secp256k1::new();
    let signed = signer.sign_psbt(psbt, &secp).map_err(|_| "Liana's signer refused the PSBT")?;
    println!("{signed}");
    Ok(())
}
