## A fee ceiling on check-ins and moves, and questions for the reviewer

From the pre-review of `satkeep-savings` (finding 2). A check-in and a move both sweep the whole wallet into one output back to Savings, so the network fee is the only thing that leaves, and nothing limited it. The CLI only had a minimum fee rate. `check_checkin` and `check_move` checked the inputs and the single destination, but not the output amount. So a typo in the fee rate, or a PSBT whose one output was edited down, passed every check.

**How far it reached.** Each approver sees the fee at `sign` before approving, and Bitcoin Core's `sendrawtransaction` refuses fee rates above 0.10 BTC/kvB by default. But the helper will approve without a person looking, and the app won't always send through Core, so Satkeep needs its own limit.

### What changes

- **`spend::FeeCeiling`** is new. A check-in's or move's fee must be at most `max_fee` **and** at most `max_percent` of what it spends.
  - **Placeholders:** 0.005 BTC (`DEFAULT_MAX_FEE`) and 5% (`DEFAULT_MAX_FEE_PERCENT`), configurable with `FeeCeiling::new`. A zero amount, or a percentage outside 1 to 100, is refused.
  - **The amounts are the wallet's own:** it runs after `check_inputs`, so a PSBT can't understate its inputs to get under it. Sums are done in `u128`, so hostile output values can't overflow them.
- **Where it runs:**
  - `check_checkin` and `check_move` take the ceiling and refuse above it (`Error::FeeTooHigh`). So it runs at `sign` and again at `broadcast`.
  - `build_checkin` and `build_move` refuse to build one above it.
  - The helper must run the same check before approving; the crate README says so.
- **CLI:**
  - `savings.json` gains `max_fee_sat` and `max_fee_percent`, which are optional and not written while they're the defaults.
  - `add-heir` copies them to the new wallet.
  - A bad value is refused with the field names.
- **Not covered:** ordinary payments have no ceiling yet; `sign` shows their fee. Whether they need one is part of question 3 below.
- **Questions for the reviewer:** a new section in `crates/satkeep-savings/README.md`, in plain words, so they travel with the code the reviewer reads:
  1. What must the helper check before approving a Savings payment, given that a stolen, unlocked phone plus the helper is 2 of 3?
  2. Should the payment rule be on by default, given that it links coins on-chain?
  3. How should a check-in with many coins pay its fee, and should one check-in sweep at most so many coins?

### Tests

- **Unit tests (`src/spend.rs`):**
  - Exactly the ceiling passes and 1 sat more is refused, by amount and by percentage.
  - An output edited down to 1 sat is refused.
  - Outputs above the inputs pay no fee, even at `u64::MAX`, without overflowing.
  - A missing input amount is refused.
  - A ceiling of zero, 0%, 101% or 255% is refused.
- **`tests/checkin.rs`, `a_check_in_whose_fee_is_above_the_ceiling_is_refused`:**
  - A 5,000 sat/vB typo is refused when building.
  - A signed check-in whose output was edited from 1.5 BTC down to 0.1 BTC is refused with a fee of 1.4 BTC. A raised ceiling lets it through.
  - 1,000 sat/vB on a 0.01 BTC wallet is refused by the 5% rule, though it's under 0.005 BTC.
- **`tests/migrate.rs`, `a_move_whose_fee_is_above_the_ceiling_is_refused`:** the same for a move.
- **`satkeep-cli` tests:**
  - `checkin --fee-rate 5000` and `add-heir --fee-rate 5000` are refused and write nothing.
  - An edited-down check-in or move is refused at `sign` and, after honest approval, at `broadcast`.
  - `max_fee_percent: 0` is refused with the field names.
  - `max_fee_sat: 2000000` lets the 5,000 sat/vB check-in through.
- **The PSBT fuzz target** now also asserts that anything `check_checkin` or `check_move` accepts is within the default ceiling.

### Merging with #34

[brent472/satkeep#34](https://github.com/brent472/satkeep/pull/34) also changes `build_move` and `check_move`: it replaces the minimum heir delay with `HeirDelayRules`. Both PRs branch from `main`, so whichever merges second will conflict on those signatures and their test calls. I'll merge `main` into it and fix them before it merges.

### Security review

**None.** The change only adds refusals: every check-in or move accepted after it was accepted before.
- **No new trust in the PSBT:** the fee is computed from input amounts that `check_inputs` has already matched against the wallet's own records, and the ceiling comes from the signer's own settings.
- **No overflow:** sums are done in `u128`, so a hostile PSBT can't wrap them, and outputs above the inputs pay no fee and pass. The network refuses such a transaction anyway.
- **Known limit (from the review):** a check-in is recognised by its marker, so one with the marker stripped is treated as an ordinary payment, and ordinary payments have no ceiling yet. `sign` still shows its fee. A move can't slip through the same way: a PSBT that pays the new wallet is checked as a move whether it's marked or not. Closing this belongs with question 3 (a ceiling for every Savings payment).
- **No change to scripts, descriptors or addresses.**
- **Nothing secret is printed or logged:** errors give amounts only.
- **No new dependency.**

🤖 Generated with [Claude Code](https://claude.com/claude-code)
