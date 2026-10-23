Milestone 1, part 3 of 3: the check-in. A send-to-self of every Savings coin through the 2-of-3 path, which restarts every coin's delays, plus the interval rule and the due/overdue warnings.

## What's here

**Library (`twofold-savings`, new `checkin` module)**
- **`CheckinRule::new(interval, margin, delays)`** requires `interval + margin <= device delay`, with both at least 1 block, so an interval at or past the device delay is always refused. Defaults are 26,280 (interval) and 4,320 (margin) blocks, against the 39,420-block device delay. All are placeholders until the outside review.
- **`checkin::coins` / `CoinStatus`.** For each coin: blocks until its device and heir paths open, **due** (reached the interval) and **overdue** (device path open). `checkin::reminder` sums them up for the wallet.
- **`build_checkin`.** Drains every coin, through the 2-of-3 path, into one new Savings address. It marks the PSBT with a proprietary field (`twofold`/`checkin`) that other software ignores.
- **`check_checkin`.** Refuses a check-in that leaves out any unspent coin, pays anywhere but one Savings address, or uses a delayed path.

**CLI (`twofold`)**
- **`twofold checkin --out FILE.psbt`** uses the same `sign` → `broadcast` flow as payments, with the same approval prompt. `sign` heads the summary "Check-in to approve" and re-checks it is complete. `broadcast` syncs and checks again, so a coin that arrived after the check-in was prepared gets it refused.
- **`create`** takes `--checkin-interval` and `--checkin-margin`. The library refuses settings that don't fit.
- **`balance`** shows, for each coin, how many blocks until its device and heir paths open, and marks it `CHECK-IN DUE` or `OVERDUE`. It ends with when the next check-in is due, or with a due or overdue warning.
- **Older wallets** without an interval in `savings.json` use the defaults if they fit. If not, `balance` says check-ins aren't set up.

## ⚠️ Second finding for the reviewer: BDK's OR also works the other way

Part 1 found that BDK's finalizer will accept a delayed leaf when the sequence matches the delay, however young the coin. That happens because miniscript ORs BDK's height check with the PSBT's sequence check. Testing the check-in found the reverse: **once a coin is old enough, BDK will finalize through a delayed leaf even when the input's sequence carries no delay.** DEVICE alone could get a check-in (whose sequence has no delay) finalized after its device delay had passed. The network rejects that transaction, because `OP_CHECKSEQUENCEVERIFY` fails, so funds were never at risk. But Twofold would have produced it.

**Fix:** `spend::finalize` no longer uses BDK's finalizer. It runs `check_delays` (the sequence against the coin's age, from part 1) and then miniscript's `PsbtExt::finalize_mut`, whose only satisfier is the PSBT (the leaf against the sequence). Together they match the network's rule. A new test pins BDK's behaviour, `bdk_alone_would_finalize_a_delayed_leaf_whose_sequence_has_no_delay`, alongside the part 1 one. The library README's finding section now covers both directions.

## Tests

All run on throwaway regtest nodes with short delays (device 10, heir 20).

- **Library `tests/checkin.rs`:**
  - After a check-in of two coins (the older one block from its device delay), the single new coin's device path opens at exactly 10 confirmations and its heir path at exactly 20. Before that, the library refuses and the node rejects (`non-BIP68-final`).
  - A check-in that leaves out a coin is refused, whether the coin arrived after the check-in was built or the "check-in" was hand-picked from fewer coins.
  - A check-in that pays out, or that isn't marked as one, is refused.
  - A check-in through a delayed path is refused. With both delayed paths open, DEVICE or HEIR alone can't finalize the real check-in, and the node rejects a hand-built attempt on the script.
  - An immature coinbase coin holds up the check-in until it can be spent.
- **Library unit tests:** the interval is refused at or past the device delay, when it leaves less than the margin, and at zero. Due starts at exactly the interval and overdue at exactly the device delay.
- **CLI `tests/checkin.rs`:**
  - `create` refuses an interval of 10, 11 or 65,535, or one that leaves less than the margin, or zero (device delay 10), and creates nothing. The placeholder defaults are refused for a 10-block device delay and accepted for the default one.
  - The `balance` wording changes exactly on time. With an interval of 5 and a device delay of 10:

    | Confirmations | `balance` shows |
    | --- | --- |
    | 1 | "due in 4 blocks" |
    | 4 | "due in 1 block", no warning |
    | 5 | `CHECK-IN DUE` and the due warning |
    | 9 | still due, not overdue, "device path opens in 1 block" |
    | 10 | `OVERDUE` and "DEVICE alone can already spend 1 coin" |
  - A check-in of an overdue wallet restarts both delays (DEVICE and HEIR alone refused at 1 confirmation) and clears the warning.
  - A prepared and approved check-in is refused at `broadcast` once a new coin has arrived, and then at `sign` too. A fresh check-in takes both coins.
  - DEVICE, HEIR or HELPER alone can't send a check-in, even with both delayed paths open.
- **Mutation check:** with the broadcast-time check switched off, the left-out-coin test fails.

```powershell
$env:BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe"; cargo test -- --include-ignored
```

Last run, on Windows with Bitcoin Core v31.1.0: **93 passed, 0 failed.**
- Library: 34 unit, 5 allowed, 5 check-in, 13 forbidden, 1 export, 2 harness, and the full-length 61,320-block test (about 10.5 minutes on its own, rerun because `finalize` changed)
- CLI: 6 approve, 6 check-in, 6 keys, 7 payments, 7 wallet

The CLI README's demo, now including a check-in step, was also run as written on a throwaway regtest node, with only the node arguments changed and the y/n answers piped in. Every step matched its comment.

## For the reviewer to judge

- **The margin.** 4,320 blocks (about 30 days) is the minimum room the interval must leave before the device delay. The defaults leave 13,140 blocks.
- **Due and overdue thresholds.** "Due" starts at the interval and "overdue" at the device delay, both counted from each coin's own confirmation.
- **Unconfirmed coins are included.** A check-in takes unconfirmed incoming coins too. Their clocks haven't started, so it costs only fees, and it keeps "every coin" simple.
- **Fees.** A check-in pays a network fee from Savings each time, at a fixed sat/vB rate here. Choosing a good fee and timing comes later.

## Not in this PR

- The signet demo
- The Liana/Sparrow exit drill
- The outside review

🤖 Generated with [Claude Code](https://claude.com/claude-code)
