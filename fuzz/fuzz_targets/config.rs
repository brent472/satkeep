//! `savings.json` text through `Config`, `policy()` and `checkin_rule()`.
//! A valid file is in the seed corpus (public test keys from Liana's own
//! tests). Nothing is ever printed.

#![no_main]

use libfuzzer_sys::fuzz_target;
use twofold_cli::wallet_dir::Config;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let Ok(config) = serde_json::from_str::<Config>(text) else { return };
    let _ = config.policy();
    let _ = config.checkin_rule();
});
