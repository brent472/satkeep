## The Ledger signet guide runs from main, and the Ledger test passed

**The test with a real Ledger passed** on 29 October: a Nano X with Bitcoin Test 2.5.1, on `main`, on signet. Every step worked as written. The results are in [`docs/ledger-signet.md`](../ledger-signet.md) (new "Results" section) and [`docs/milestone-1-status.md`](../milestone-1-status.md) (new "The Ledger on signet: done" section).

[brent472/satkeep#31](https://github.com/brent472/satkeep/pull/31) is merged, and its `twofold-ledger` worktree is removed. [`docs/ledger-signet.md`](../ledger-signet.md) still said to work there, so it now runs from `C:\Users\Brent\Documents\twofold` on `main`.

### What changed

- **Where to run it:** `Documents\twofold`, on `main` (`git switch main`, `git pull`), not the PR's worktree.
- **Starting the node:** the guide said to wait for the signet run (step list A), which finished on 25 October. It now starts the signet node with the run's settings (`bitcoind -signet -prune=550`), then checks it has caught up.
- **USB:** a new step says to plug the Ledger in by USB cable. `satkeep` only talks to a Ledger over USB, so a Nano X's Bluetooth won't work. The "no Ledger found" note now checks the cable too.
- **The `y` answers:** each `sign --key-file` step says to answer `y` at "Approve? (y/N)".
- **The move's fee (step 8):**
  - Note the new wallet's first address, to check against the Ledger's screen in step 9.
  - The move must be within the fee ceiling from [brent472/satkeep#35](https://github.com/brent472/satkeep/pull/35): about 2% here.
  - If it's refused, stop and report it, rather than adding `--allow-high-fee`.

- **Pasting:** a new line says to paste one command at a time, on an empty prompt. During the real run, pasted commands twice ran together onto one line in PowerShell.

### The results

- **Checked against the signet node:** every transaction ID, by finding its block and reading the transaction:
  - the funding coin (output 37, 2,531,619 sats, block 328,592);
  - step 6, the Ledger plus PHONE (block 328,594);
  - step 9, the move (block 328,596);
  - step 11, the Ledger plus HELPER (block 328,597);
  - step 12, the return to the faucet (block 328,598).
- **Each one spends the one before it.** Steps 6 and 11 each paid 1,000 sats to the faucet's address, and step 12 sent it the rest.
- **Step 7** (the Ledger alone) was refused with "not enough keys have approved this payment", and sent nothing.
- **`milestone-1-status.md`** also gets:
  - the new date;
  - the Ledger under "Done along the way";
  - decision 7 brought up to date: `Fuzz second_phone (60 s)` is now a required check, but `Ledger (Speculos)` isn't.

The steps themselves are unchanged. The commands and the messages they quote were checked against `main`'s code.

### Tests

Docs only: no code changes, so no new tests. The real-Ledger run above exercised every command in the guide.

### Security review

**None.** This is a docs-only diff, so I reviewed it myself rather than with a separate agent:
- **No keys or secrets:** it adds no keys, seeds or approval codes, and no command prints a key file or `ledger.json`.
- **"Don't send" is unchanged:** the key files, `ledger.json`, and the Ledger's 24 words and PIN.
- **Test networks only:** every command is `-signet`.
- **Only public data recorded:** transaction IDs, amounts and block heights. No keys, `ledger.json` or wallet files.
- **Nothing weakened:** the new fee note tells the reader not to override the ceiling.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
