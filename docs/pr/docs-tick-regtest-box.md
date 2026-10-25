## Tick "All tests pass on regtest"

Docs only; no code changes.

- **`docs/milestone-1.md`:** the "All tests pass on regtest" box is ticked. The line notes that the full-length 61,320-block heir delay test, which CI doesn't run, was run locally on 2026-10-24 and passed.
- **`docs/milestone-1-status.md`:** that row now says **Ticked**, with the same note, instead of "Ticking it is your call."

The local run: `the_default_heir_delay_opens_at_exactly_61320_confirmations`, 1 passed, finished in 714.51 s. Every other regtest test passes in CI on every PR.

### Security review

**None.** Two lines of docs; no keys, no private data, no code.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
