Payments count as check-ins: the payment rule from brief 3.12. Test networks only.

## The rule

With a beneficiary (the Taproot shape), every 2-of-3 Savings payment also spends every confirmed coin within a window of needing a check-in. Change comes back to Savings, so the payment restarts those coins' heir delay. A pocket refill is a 2-of-3 Savings payment to the pocket, so the rule covers it once the pocket exists; the pocket itself isn't built yet (brief 3.11).

- **The window:** `checkin::PaymentRule::new(window, checkin_rule)`, where a coin is due at `interval - window` confirmations and `1 <= window < interval`. The placeholder default is 8,760 blocks (about 2 months, `DEFAULT_REFRESH_WINDOW`). With the default interval, that's coins older than 43,800 blocks, about 10 months.
- **Building:** `spend::build_payment` is `build_spend` plus the rule. The due coins become required inputs, whatever coin selection or the caller picks.
  - **Never affected:** the heir path, which mustn't be made to take younger coins; Savings with no beneficiary; check-ins and moves, which already spend everything.
- **Showing it:** `checkin::due_for_refresh` and `refresh_of` say which coins are due, and which a payment refreshes or leaves out. `send` and `sign` show both. A payment that leaves one out isn't refused: a coin can age into the window between building and signing, and that coin still has the check-in.
- **A wallet setting, on by default.**
  - **To set it:** `create` or `add-heir` with `--refresh-window` or `--no-refresh-with-payments`, or `refresh_with_payments` in `savings.json`.
  - **Privacy trade-off:** spending old coins with a payment links them on-chain. Each extra coin also adds to the network fee. Turning the rule off keeps coins apart, and only check-ins then restart the heir delay.
  - **The check-in rules don't change,** so the safety margin is the same either way.

**What doesn't change:** check-in reminders. They already go by each coin's age, so once a payment has refreshed the old coins, nothing is due.

## Tests

`cargo test --workspace` passes on throwaway regtest nodes. Clippy `-D warnings` passes on the workspace and the fuzz crate, and `cargo deny check` passes.

- **Library (`tests/payment_rule.rs`).** Heir delay 20, check-ins every 10 blocks, window 4, so coins with 6 or more confirmations are due.
  - **Rule on:** a payment from one hand-picked young coin also spends the old coin, with change back to Savings. When the old coin would have reached the heir delay, HEIR alone is refused. HEIR opens the change at exactly the heir delay counted from the payment.
  - **Rule off:** the same payment leaves the old coin alone. HEIR is refused one block before its delay and opens it on time.
  - **Without hand-picking,** every due coin goes in.
  - **Untouched:** the heir path and plain Savings.
- **Library (unit):**
  - the window must fit inside the interval;
  - the defaults mean 43,800 blocks.
- **CLI (`tests/payment_rule.rs`):**
  - `send` also spends the old coin and says so, and `sign` shows it before approving. After it confirms, the change has a fresh heir delay and HEIR alone is refused.
  - With `--no-refresh-with-payments`, `create` and `savings.json` say the rule is off, and `send` forces nothing in. Turning it back on in `savings.json` brings it back.
  - A refresh window that doesn't fit is refused, including the default against a 10-block interval. The default fits the placeholder interval, and plain Savings has no rule.
- **Not deterministic, so tested in the library instead:** that `send` *leaves out* the old coin with the rule off. BDK's fallback coin selection is random, so the library tests pick the coin by hand instead.
- **Fuzz:** `config` runs `payment_rule()`, whose refresh age must fall inside the interval. The seed has the new fields.
- **Existing CLI tests** use a 4-block refresh window, because the default doesn't fit their 10-block interval. They pass unchanged otherwise.

## Security review

`/security-review` on the branch diff: **none** at the reporting bar. It found the rule only adds inputs on the 2-of-3 path: no single key gains a path, and `check_inputs`, the delay check, the move and check-in checks and `sign`'s every-input rule are unchanged. Change goes to Savings' internal keychain, the fee shown is pinned to the wallet's amounts, and nothing new prints key material.

It noted two smaller points, both fixed in this PR:
- **`sign` read the setting for every PSBT**, so a misconfigured window would have blocked check-ins and moves too. Now it only reads it for a plain 2-of-3 payment.
- **"Change comes back to Savings" was a fixed sentence.** Now it's shown only when the payment has an output to Savings' change addresses.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
