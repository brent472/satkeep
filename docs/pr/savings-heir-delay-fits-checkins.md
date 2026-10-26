## A new wallet's heir delay must leave room for its check-ins

From the pre-review of `satkeep-savings` (finding 1). Adding a beneficiary only checked the heir delay against the minimum (26,280 blocks). With the default check-in rule (every 52,560 blocks, plus an 8,760-block margin), any heir delay from 26,280 to 61,319 passed that check but left no check-in schedule that runs before the heir path opens. Such a wallet would never say a check-in is due, only that it's overdue once HEIR alone can already spend.

**How far it reached.** The CLI already refused this case: `create` and `add-heir` fill in the check-in interval and margin and refuse a wallet whose check-in rule doesn't fit. The gap was in the library, which the app and the helper will call directly, and in `sign` and `broadcast`, which checked the move set up in `savings.json` against the minimum only.

**The default interval now fits the heir delay (brief 3.27).** Refusing was safe but unhelpful: the minimum 6-month heir delay could only be used with a hand-picked interval. Now the default interval is the smaller of 52,560 blocks and the heir delay minus the margin (8,760). A 26,280-block heir delay gets a check-in every 17,520 blocks, about every 4 months, instead of being refused. A chosen interval that doesn't fit is still refused, as the brief says.

### What changes

- **`checkin::default_interval(delays, margin)`** is new: the smaller of 52,560 blocks and the heir delay minus the margin. It refuses an heir delay no longer than the margin (`Error::NoRoomForCheckins`), since that leaves no room for any check-in.
- **`checkin::HeirDelayRules`** is new: the minimum heir delay plus the new wallet's check-in interval and margin.
  - `check(delays)` refuses an heir delay below the minimum (`Error::HeirDelayTooShort`) or too short for the check-ins (`Error::CheckinTooLate`), and returns the `CheckinRule`.
  - An interval of `None` means the default for the heir delay.
  - `HeirDelayRules::DEFAULT` holds the placeholders, and accepts every heir delay from the minimum up.
- **`migrate::wallet_with_heir`, `build_move` and `check_move`** take `HeirDelayRules` in place of the bare minimum. So every signer that rebuilds the new wallet from its own settings checks both.
- **CLI:**
  - `create` and `add-heir` without `--checkin-interval` save the interval that fits the heir delay. With the placeholder heir delay that's still 52,560.
  - `add-heir` now saves the new wallet's check-in interval and margin in `moving_to`.
  - A wallet or move made before check-ins existed, with no interval saved, gets the interval that fits its heir delay.
  - `sign` and `broadcast` refuse the move if those check-ins don't fit its heir delay.
  - A move set up by an earlier version has no check-ins saved, so it gets the default margin of 8,760 blocks. On a test wallet whose heir delay is shorter than that, nothing fits, so the move is refused; set it up again with `add-heir`.
- **READMEs:** the assumption is written down in `satkeep-savings`'s Delays section and in the CLI's beneficiary section.

### Tests

- **Unit tests (`src/checkin.rs`):**
  - **The default interval:** 17,520 for the minimum 26,280, 31,240 for 40,000, 52,559 for 61,319, and 52,560 for 61,320 and for 65,535.
  - **Refused:** an heir delay no longer than the margin, a zero margin, and no beneficiary.
  - **The default rules:** each of those heir delays gets check-ins that fit; below the minimum is still refused.
  - **A chosen interval:** 17,000 lets 26,280 through; 17,521 and 52,560 don't fit.
- **`tests/migrate.rs`:**
  - **`an_heir_delay_the_check_ins_cant_beat_is_refused`:** with check-ins chosen every 15 blocks and a 5-block margin, a 19-block heir delay is refused when the settings are built, when the move is built and when it's checked; 20 is accepted. A yearly interval chosen with a 40,000-block delay is refused, and the default lets the same delay through.
  - **`a_move_to_the_minimum_heir_delay_gets_check_ins_that_fit`:** with the default rules, a move to a 26,280-block heir delay is built and checked with check-ins every 17,520 blocks. A yearly interval chosen for it is refused.
- **`satkeep-cli` tests:**
  - **`tests/checkin.rs`, `the_default_interval_fits_a_shorter_heir_delay`:** `create --heir-delay 26280` succeeds with `Check in every 17520 blocks` and saves 17,520; 40,000 gets 31,240. `--checkin-interval 52560` with 26,280 is refused, and nothing is created.
  - **`tests/checkin.rs`:** a 20-block heir delay with the default margin is refused because no interval fits.
  - **`tests/beneficiary.rs`, `a_move_whose_check_ins_cant_beat_the_heir_delay_is_refused`:**
    - `add-heir` refuses check-ins every 16 blocks against a 20-block heir delay, and writes nothing.
    - `sign` refuses once the saved check-ins are edited to come too late.
    - `sign` refuses a move set up without saved check-ins, whose default margin doesn't fit.
    - `broadcast` refuses a move signed under good settings when the settings no longer fit.
- **Existing tests and the PSBT fuzz target** now pass rules (minimum 10, check-ins chosen every 6 blocks with a 3-block margin), which every delay they use fits.

### Security review

**None.**
- **What's now accepted that wasn't:** a wallet or move whose heir delay is between the minimum and 61,319 blocks, with no interval chosen. It now gets check-ins that fit, and its heir delay still meets the minimum.
- **Everything else only adds refusals.**
- **No new way to spend:** no script, descriptor or address changes.
- **No new input is trusted:** the check-in settings come from the signer's own `savings.json`, never from the PSBT, the same as the beneficiary and heir delay.
- **Nothing new is printed or logged:** errors give block counts only.
- **No new dependency.**

🤖 Generated with [Claude Code](https://claude.com/claude-code)
