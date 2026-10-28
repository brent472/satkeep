## The Ledger signet guide runs from main

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

The steps themselves are unchanged. The commands and the messages they quote were checked against `main`'s code.

### Tests

Docs only: no code changes, so no new tests.

### Security review

**None.** This is a docs-only diff, so I reviewed it myself rather than with a separate agent:
- **No keys or secrets:** it adds no keys, seeds or approval codes, and no command prints a key file or `ledger.json`.
- **"Don't send" is unchanged:** the key files, `ledger.json`, and the Ledger's 24 words and PIN.
- **Test networks only:** every command is `-signet`.
- **Nothing weakened:** the new fee note tells the reader not to override the ceiling.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
