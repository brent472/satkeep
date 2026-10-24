Every command the exit drill runs now has a 60-second timeout. A hang fails in about a minute, naming the command, instead of sitting until GitHub's 45-minute limit. On 4c5589d (PR #14), the first attempt hung in "Run the drill" for over 20 minutes, just after "Liana's first 5 receive and change addresses match Bitcoin Core's", so during Liana's 2-of-3 spend. The re-run passed.

## What changes

- **`run()` in `contrib/exit-drill/drill.py`:** every command goes through it, and it now gives each one 60 seconds (`DRILL_COMMAND_TIMEOUT` overrides it). That covers `bitcoind`, every `bitcoin-cli` call, every `liana-cli` call (Liana's RPC), Liana's signer and `satkeep`.
- **On a timeout:** the command is killed and the drill fails with, for example, `ERROR: liana-cli createspend didn't finish within 60 s`.
- **What the message names:** each caller passes a name that is only the program and its subcommand (the `bitcoin-cli` method, the `liana-cli` method, the `satkeep` subcommand), or "Liana's signer".
- **What it never includes:** the other arguments, the input or the output, any of which could hold key material or a key file's path. The `TimeoutExpired` that holds them is suppressed (`from None`), and the drill prints only the message, never a traceback.
- **The wait loops** for the node to start and for Liana to catch up now have a 60-second deadline. If Bitcoin Core never answers `getblockcount` by then, the drill raises "bitcoind didn't answer within 60 s" instead of carrying on (added in review). Before, `wait_for_liana_sync` retried 120 times, so a `liana-cli` that hung on every try could have run for about two hours. A timed-out `liana-cli` there now fails the drill instead of being retried.
- **Stopping `lianad`:** if it ignores the stop signal for 30 seconds, it's killed instead of raising.
- **Stopping `bitcoind`** in cleanup: a timeout prints a warning without hiding the drill's own result.
- **The workflow's "Run the drill" step** has a 15-minute backstop (`timeout-minutes`) for anything outside a command. A good run takes about 4 minutes.
- **`docs/exit-drill.md`** says all this.

## Checks

- **Locally:** with dummy paths and `DRILL_COMMAND_TIMEOUT=2`, a 30-second command whose argument and input hold a fake phrase and `tprv` fails after 2.0 s with "Liana's signer didn't finish within 2 s". The message and the exception carry none of the argument or input, there's no cause attached, and the context is suppressed. A normal command still runs. With `run()` stubbed so `bitcoind` starts but never answers, `main()` raises "bitcoind didn't answer within 2 s" after 2.0 s.
- **On the PR:** the drill runs on its own here, because the drill and its workflow changed.
- No Rust changes and no new dependencies.

## Security review

None. Reviewed:
- **Error text:** every name passed to `run()` holds no argument beyond the subcommand. Key imports still go through stdin, and stdin is never printed.
- **Partial output:** the partial output a timeout captures is dropped.
- **Unchanged:** the key-material scan of the drill's log, and the existing messages for keygen and key imports, which already give no detail.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
