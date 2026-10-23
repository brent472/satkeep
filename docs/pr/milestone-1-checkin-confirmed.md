A small follow-up to the check-in (#3): a check-in now spends only confirmed Savings coins. Plus a fix to `balance` for wallets without a check-in rule.

## 1. A check-in spends only confirmed coins

Before, a check-in had to include every unspent coin, unconfirmed ones too. Anyone can send to a Savings address, and a transaction can't confirm before the coins it spends. So a stranger could:
- **stall a check-in**, by sending a coin with a fee too low to confirm, or
- **break it**, by replacing their coin (RBF), which invalidates the check-in.

Now:
- **`build_checkin`** drains only confirmed coins (BDK's `exclude_unconfirmed`). It refuses with "no confirmed Savings coins" if there are none.
- **`check_checkin`** requires every *confirmed* coin. Unconfirmed coins don't count as missing, because their delays haven't started. A check-in that *spends* an unconfirmed coin is refused with the new `Error::CheckinSpendsUnconfirmed`.
- **`twofold checkin`** says when it left coins out: "Left out 1 unconfirmed coin: its delays haven't started yet."
- **Once such a coin confirms,** its clock starts, and the next check-in must include it. A check-in prepared before the coin confirmed is refused at `broadcast`, as before.

The assumption is written in the savings README (Assumptions → Delays).

## 2. `balance` without a check-in rule

`print_balance` used a stand-in rule, `CheckinRule::new(1, device - 1, …)`, for wallets without a check-in rule. That fails when the device delay is 1 (the margin would be 0), and then `balance` itself errored. Now no stand-in is built:
- **Library:** `checkin::coins` and the new `checkin::status` take an `Option<CheckinRule>`. With `None`, nothing is ever "due". The delays and "overdue" don't depend on the rule, so they're still reported.
- **CLI:** `balance` says check-ins aren't set up, still lists each coin's delays, and **still warns when a coin is overdue**. Before, it skipped every warning for such wallets.

## Tests

- **Library `tests/checkin.rs`:**
  - `an_unconfirmed_incoming_coin_neither_blocks_nor_joins_a_check_in`: with a coin still in the mempool, the check-in builds from the one confirmed coin, passes `check_checkin`, and the node accepts it. A hand-made check-in that spends the unconfirmed coin is refused.
  - `a_confirmed_coin_left_out_is_still_refused`: a check-in that leaves out one of two confirmed coins is refused, naming that coin.
  - `no_confirmed_coins_means_nothing_to_check_in`
  - To make these possible, the test harness's `sync` now reads the mempool as well as blocks.
- **Library unit test:** `without_a_rule_nothing_is_due_but_overdue_still_shows`, with a 1-block device delay.
- **CLI `tests/checkin.rs`:**
  - `an_unconfirmed_incoming_coin_doesnt_hold_up_a_check_in`: the check-in takes 1 coin, says it left 1 out, and is sent.
  - `balance_works_for_a_wallet_without_a_check_in_rule`: a wallet whose `savings.json` has no interval still shows each coin's delays, never says "due", and warns when overdue.
- **Mutation check:** with the old behaviour (spend and require unconfirmed coins) back in place, both unconfirmed-coin tests fail.

The CLI can't create a wallet with a 1-block device delay, because no check-in rule can fit one. So the device-delay-1 case is covered by the library unit test, and the CLI test covers a wallet with no rule.

Last run, on Windows with Bitcoin Core v31.1.0: **98 passed, 0 failed** (1 ignored: the full-length 61,320-block test, which this change doesn't touch).

```powershell
$env:BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe"; cargo test
```

🤖 Generated with [Claude Code](https://claude.com/claude-code)
