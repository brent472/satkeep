Where a move goes now comes from each signer's own settings, never from the PSBT. Test networks only.

## The change

Before this, `check_move` rebuilt the new wallet from the HEIR key and heir delay the PSBT carried. Whoever prepared a move chose both, and approvers could only compare what `sign` showed.

- **Destination from this wallet's settings.** `add-heir` records the beneficiary and heir delay in this wallet's `savings.json` (`moving_to`). `sign` and `broadcast` rebuild the new wallet from that plus this wallet's own PHONE, DEVICE and HELPER. They refuse unless the move has exactly one output, to that wallet's first address.
- **The PSBT's marker is only compared.** It must name the same HEIR key and delay as the settings (`Error::MoveNotAsSetUp`). A PSBT that pays the new wallet is checked as a move even if it isn't marked. A wallet with no move set up refuses any move.
- **Covers every move.** That's adding a beneficiary and changing it. Replacing a key isn't built yet. When it is, the replacement key goes in the same settings. Until then, a move to a wallet with other keys is refused (`Error::MoveToWrongWallet`).
- **`sign` shows the heir delay** in blocks and about how long ("20 blocks, about 3 hours"; "61320 blocks, about 14 months"), for a move and a check-in. `add-heir` and `create` print it too.
- **Minimum heir delay.**
  - **Default:** `DEFAULT_MIN_HEIR_DELAY` is 26,280 blocks, about 6 months, a placeholder like the other timelocks.
  - **Configurable:** set it with `create --min-heir-delay`; `add-heir` keeps it, and the regtest tests use 10.
  - **Enforced:** a wallet with a beneficiary, a move's target, or a move set up below it is refused (`Error::HeirDelayTooShort`). This happens in the library (`Delays::require_heir_at_least`, `migrate`) and in the CLI's `policy()`.
- **Library API:**
  - `wallet_with_heir(from, heir, delay, min_heir_delay)`
  - `build_move(wallet, from, to, min_heir_delay, fee_rate)`
  - `check_move(wallet, psbt, from, to, min_heir_delay)`, which now returns `()`
  - new `pays_to(psbt, to)`

**Existing wallets:** a CLI wallet with an heir delay below 26,280 and no `min_heir_delay` won't open any more. The error says why. These are test wallets only; recreate them with `--min-heir-delay`.

**Known limit:** the check reads the `savings.json` in the folder `sign` uses. A holder signing from their own copy needs the same `moving_to` there, or the move is refused. In the app, that's each device's own record of the beneficiary. Written up in the CLI README.

**Also fixed:** two hand-made fuzz seeds from the last PR (`savings-no-beneficiary.json`, `old-card-only-path.json`) were never committed, because `fuzz/.gitignore` ignores the corpus except listed files. They're listed now, with the new `savings-moving.json`. Separate commit.

## Tests

`cargo test --workspace` passes on throwaway regtest nodes. Clippy `-D warnings` passes on the workspace and the fuzz crate, and `cargo deny check` passes.

- **Library (`tests/migrate.rs`), checked against the wallet the settings say.** A move is refused if it:
  - goes to a wallet with a **different HEIR key**, whether its marker names that HEIR or claims the right one;
  - goes to a wallet with a **shorter heir delay**, either way;
  - has **one extra output**, to someone else or back as change;
  - pays elsewhere, names another HEIR, isn't marked, has other keys, or leaves out a coin.

  An heir delay below the minimum is refused in the settings, in building and in checking. With the placeholder minimum, 20 blocks is refused. The honest move, and changing the beneficiary, still work end to end.
- **CLI (`tests/beneficiary.rs`).**
  - **At `sign`**, which saves nothing, these are refused: a move to a different HEIR key (as built, and with the marker rewritten); a shorter delay; an extra output (marked, and unmarked but paying the new wallet); and any move where none is set up.
  - **At `broadcast`:** a shorter-delay move signed by two holders whose settings were changed to match, and an extra output added after two approvals.
  - **Minimum heir delay:** `create` refuses 26,279 blocks and accepts 26,280; `add-heir` refuses below the wallet's minimum, and `sign` refuses a set-up move once the minimum is raised above it.
- **Unit:** `about()` for 1 block up to the 65,535 cap; the minimum's default and its check.
- **Fuzz:**
  - `psbt` checks moves against the settings' wallet, and has a second template moving to a different HEIR. Any accepted move must be one output to the settings' destination.
  - `config` runs `move_target`, which must give the same three keys plus a beneficiary, at or above the minimum.

## Security review

`/security-review` on the branch diff: **none**.

The reviewer checked these and found they hold:
- **No PSBT input reaches the destination.** The marker is only compared.
- **An unmarked PSBT paying the target plus anything else is refused.** One that pays elsewhere is shown as an ordinary payment, with its addresses, and not as a move.
- **The check-in branch can't skip the move check.** At `sign`, a check-in can't pay the new wallet. At `broadcast`, the move check runs anyway.
- **No error mapping fails open.** One key still can't move Savings without a delay.
- **The new settings fail closed** for old wallets.
- **No key material** appears in the new output or errors.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
