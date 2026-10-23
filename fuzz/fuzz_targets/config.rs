//! `savings.json` text through `Config`, `policy()`, `checkin_rule()` and
//! `move_target()`.
//! Valid files are in the seed corpus (public test keys from Liana's own
//! tests). Nothing is ever printed.

#![no_main]

use libfuzzer_sys::fuzz_target;
use twofold_cli::wallet_dir::Config;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let Ok(config) = serde_json::from_str::<Config>(text) else { return };
    let _ = config.checkin_rule();
    let Ok(policy) = config.policy() else { return };
    // A move set up in the settings goes to the same three keys plus a
    // beneficiary, with at least the minimum heir delay.
    if let Ok(Some(to)) = config.move_target(&policy) {
        assert!(to.keys().same_two_of_three(policy.keys()) && to.has_heir(), "move_target: wrong wallet");
        assert!(to.delays().heir() >= Some(config.min_heir_delay()), "move_target: below the minimum");
    }
});
