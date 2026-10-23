Closes the two CodeQL alerts that appeared on `main` after PR #4 merged. Both are in the backup prototype.

**Why they weren't caught on PR #4:** that PR's summary `CodeQL` check finished *neutral* ("1 configuration not found"), so it didn't compare alerts. The three Analyze jobs passed, and GitHub counts neutral as passing.

| Alert | Where | What CodeQL saw | Fix |
| --- | --- | --- | --- |
| #4 `rust/hard-coded-cryptographic-value` (critical) | `backup/mod.rs`, `random()` | `random()` made a `[0u8; N]` buffer, then filled it from `OsRng`. CodeQL treats the zero literal as a hard-coded nonce: it doesn't model the fill. The nonces were random in fact. | `random()` now returns `OsRng.gen()` directly, so no fixed buffer exists in our code. It still serves exactly the data key, the nonces and the wallet id, still straight from the operating system's generator. No new dependency. |
| #2 `rust/cleartext-logging` (high) | `backup/tests.rs:67` | A failure message printed the backup file's **length**. CodeQL traces the file back to the device's unlock secret. | The message no longer includes the length. |

Neither was an actual leak or a fixed nonce, but both are real patterns to keep out of key-handling code.

**Also fixed: a flaky test from PR #4**, found when this PR's regtest job failed. `the_backup_alone_reveals_nothing_but_its_date_and_devices` checked that no phrase word appears anywhere in the backup file. But the readable header holds the device's request label, "Twofold phone key backup unlock v1, wallet …", and "phone" and "wallet" are both BIP39 words. So about 1 run in 85 failed whenever the random phrase contained either word.
- The test now checks for no two phrase words in a row anywhere in the file, and no single phrase word in the **encrypted body**.
- 300 runs in a row pass.

## Checks

- `cargo clippy -D warnings` passes.
- The backup unit tests pass (14), and the fixed test passes 300 runs in a row.

## Security review

**None.** I reviewed the diff by hand: it changes only how `random()` gets its bytes (the same generator) and one test message. No secret reaches any output.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
