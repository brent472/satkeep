Closes the two CodeQL alerts that appeared on `main` after PR #4 merged. Both are in the backup prototype.

**Why they weren't caught on PR #4:** that PR's summary `CodeQL` check finished *neutral* ("1 configuration not found"), so it didn't compare alerts. The three Analyze jobs passed, and GitHub counts neutral as passing.

| Alert | Where | What CodeQL saw | Fix |
| --- | --- | --- | --- |
| #4 `rust/hard-coded-cryptographic-value` (critical) | `backup/mod.rs`, `random()` | `random()` made a `[0u8; N]` buffer, then filled it from `OsRng`. CodeQL treats the zero literal as a hard-coded nonce: it doesn't model the fill. The nonces were random in fact. | `random()` now returns `OsRng.gen()` directly, so no fixed buffer exists in our code. It still serves exactly the data key, the nonces and the wallet id, still straight from the operating system's generator. No new dependency. |
| #2 `rust/cleartext-logging` (high) | `backup/tests.rs:67` | A failure message printed the backup file's **length**. CodeQL traces the file back to the device's unlock secret. | The message no longer includes the length. |

Neither was an actual leak or a fixed nonce, but both are real patterns to keep out of key-handling code.

## Checks

- `cargo clippy -D warnings` passes.
- The backup unit tests pass (14).

## Security review

**None.** I reviewed the diff by hand: it changes only how `random()` gets its bytes (the same generator) and one test message. No secret reaches any output.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
