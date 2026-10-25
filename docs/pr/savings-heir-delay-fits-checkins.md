## A new wallet's heir delay must leave room for its check-ins

From the pre-review of `satkeep-savings` (finding 1). Adding a beneficiary only checked the heir delay against the minimum (26,280 blocks). With the default check-in rule (every 52,560 blocks, plus an 8,760-block margin), any heir delay from 26,280 to 61,319 passed that check but left no check-in schedule that runs before the heir path opens. Such a wallet would never say a check-in is due, only that it's overdue once HEIR alone can already spend.

**How far it reached.** The CLI already refused this case: `create` and `add-heir` fill in the check-in interval and margin and refuse a wallet whose check-in rule doesn't fit. The gap was in the library, which the app and the helper will call directly, and in `sign` and `broadcast`, which checked the move set up in `savings.json` against the minimum only.

### What changes

- **`checkin::HeirDelayRules`** is new: the minimum heir delay plus the new wallet's check-in interval and margin. `check(delays)` refuses an heir delay below the minimum (`Error::HeirDelayTooShort`) or too short for the check-ins (`Error::CheckinTooLate`), and returns the `CheckinRule`. `HeirDelayRules::DEFAULT` holds the placeholders, which together accept only heir delays of 61,320 blocks or more. A shorter heir delay needs a shorter interval set with it.
- **`migrate::wallet_with_heir`, `build_move` and `check_move`** take `HeirDelayRules` in place of the bare minimum. So every signer that rebuilds the new wallet from its own settings checks both.
- **CLI:**
  - `add-heir` now saves the new wallet's check-in interval and margin in `moving_to`.
  - `sign` and `broadcast` refuse the move if those check-ins don't fit its heir delay.
  - A move set up by an earlier version has no check-ins saved, so it gets the placeholder ones. On a test wallet with a short heir delay those don't fit, so the move is refused; set it up again with `add-heir`.
- **READMEs:** the assumption is written down in `satkeep-savings`'s Delays section and in the CLI's beneficiary section.

### Tests

- **Unit tests (`src/checkin.rs`):**
  - With the default rules, heir delays of 26,280, 40,000 and 61,319 are refused, and 61,320 is accepted.
  - An interval of 17,000 lets 26,280 through; 17,521 doesn't fit.
  - No beneficiary is refused.
- **`tests/migrate.rs`, `an_heir_delay_the_check_ins_cant_beat_is_refused`:**
  - With check-ins every 15 blocks and a 5-block margin, a 19-block heir delay is refused when the settings are built, when the move is built and when it's checked; 20 is accepted.
  - With the placeholder defaults, a 40,000-block delay is refused.
- **`satkeep-cli` `tests/beneficiary.rs`, `a_move_whose_check_ins_cant_beat_the_heir_delay_is_refused`:**
  - `add-heir` refuses check-ins every 16 blocks against a 20-block heir delay, and writes nothing.
  - `sign` refuses once the saved check-ins are edited to come too late.
  - `sign` refuses a move set up without saved check-ins, since the defaults don't fit.
  - `broadcast` refuses a move signed under good settings when the settings no longer fit.
- **Existing tests and the PSBT fuzz target** now pass rules (minimum 10, check-ins every 6 blocks with a 3-block margin), which every delay they use fits.

### Security review

**None.** The change only adds refusals: every move or wallet accepted after it was accepted before.
- **No new way to spend:** no script, descriptor or address changes.
- **No new input is trusted:** the check-in settings come from the signer's own `savings.json`, never from the PSBT, the same as the beneficiary and heir delay.
- **Nothing new is printed or logged:** errors give block counts only.
- **No new dependency.**

🤖 Generated with [Claude Code](https://claude.com/claude-code)
