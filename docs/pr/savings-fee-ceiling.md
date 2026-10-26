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
- **A person can approve a higher fee, knowingly: `--allow-high-fee`** on `checkin`, `add-heir`, `sign` and `broadcast`.
  - **Why:** on a small Savings with many coins, 5% can block a check-in when fees are high, and a blocked check-in lets the heir path open.
  - **What it does:** above the ceiling, it prints the fee and its percentage of what's spent, and asks "Approve this network fee anyway? (y/N)".
    - On yes, the check runs again with a ceiling of exactly that fee, so only the fee the person saw is approved.
    - Anything else refuses, and nothing is written or signed.
  - **Without the flag,** the ceiling refuses as before, and the error now says the flag exists.
  - **Every approver must use it,** and `broadcast` too. On `sign` the fee question comes before "Approve? (y/N)", and `--yes` can't be combined with it.
  - **The helper never gets this override;** the crate README says so. For it the ceiling is absolute.
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
  - **`tests/checkin.rs`, `a_check_in_above_the_percentage_ceiling_needs_allow_high_fee_and_a_second_yes`** (1,000 sat/vB on 0.01 BTC, over 5% but under 0.005 BTC):
    - Refused without the flag, with the error naming the flag, and nothing written.
    - With the flag and "n": refused, nothing written. With "y": the fee and its percentage are shown and the check-in is prepared.
    - `sign` without the flag is refused; with it, PHONE and HELPER each answer the fee question and then "Approve?".
    - `--yes` with `--allow-high-fee` is refused.
    - `broadcast` without the flag is refused; with it and "y", the check-in is sent.
- **The PSBT fuzz target** now also asserts that anything `check_checkin` or `check_move` accepts is within the default ceiling.

### Merged with #34

[brent472/satkeep#34](https://github.com/brent472/satkeep/pull/34) changed `build_move` and `check_move` too: it replaced the minimum heir delay with `HeirDelayRules`. `main` is merged into this branch, and the conflicts are fixed: both take `rules` and then the ceiling, `build_move(wallet, from, to, rules, fee_rate, ceiling)` and `check_move(wallet, psbt, from, to, rules, ceiling)`. Every test from both PRs is kept and passes.

### Security review

**None.**
- **The ceiling only adds refusals.** Without `--allow-high-fee`, every check-in or move accepted after it was accepted before.
- **The override is narrow:**
  - It's opt-in per command and needs a second yes on standard input.
  - It approves exactly the fee shown, so a PSBT edited afterwards to pay more is still refused.
  - It can't be combined with `sign --yes`.
  - It changes nothing in the library, so the helper, built on the library, has no way to use it.
- **No new trust in the PSBT:** the fee is computed from input amounts that `check_inputs` has already matched against the wallet's own records, and the ceiling comes from the signer's own settings.
- **No overflow:** sums are done in `u128`, so a hostile PSBT can't wrap them, and outputs above the inputs pay no fee and pass. The network refuses such a transaction anyway.
- **Known limit (from the review):** a check-in is recognised by its marker, so one with the marker stripped is treated as an ordinary payment, and ordinary payments have no ceiling yet. `sign` still shows its fee. A move can't slip through the same way: a PSBT that pays the new wallet is checked as a move whether it's marked or not. Closing this belongs with question 3 (a ceiling for every Savings payment).
- **No change to scripts, descriptors or addresses.**
- **Nothing secret is printed or logged:** errors give amounts only.
- **No new dependency.**

🤖 Generated with [Claude Code](https://claude.com/claude-code)
