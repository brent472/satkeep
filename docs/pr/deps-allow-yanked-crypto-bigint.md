## cargo deny: allow the yanked crypto-bigint 0.7.7 until 8 November

`cargo deny check` now fails on main. On 2026-10-25, RustCrypto yanked every `crypto-bigint` 0.7 release except 0.7.9, which was published that day. That includes our 0.7.7, which `p256` (the second phone's lock) uses. `deny.toml` refuses yanked crates, so every PR's `cargo deny check` fails until this is fixed.

### Why they were yanked

0.7.9 makes a breaking fix ([crypto-bigint#1373](https://github.com/RustCrypto/crypto-bigint/pull/1373)).
- **The bug:** crypto-bigint promises that a `NonZero` value is never zero. One multiplication rule broke that promise: it said a non-zero number times a non-zero number is non-zero. That's true for whole numbers, but the rule also applied to modular numbers, where two non-zero values can multiply to zero.
- **The fix:** 0.7.9 limits the rule to whole numbers. That breaks the API, so the older 0.7 releases were yanked.
- **It's not a security advisory:** RustSec has none for crypto-bigint 0.7.

### Whether we use it

**No.**
- **Our code:** none of our crates uses `crypto-bigint` directly.
- **p256 0.14.0:** `NonZero` appears only in its `hash2curve` module, which our features (`ecdh` only) don't build, and in that module's tests. Even there it's division and remainder, never `NonZero × NonZero`.
- **elliptic-curve 0.14.2:** its `NonZeroScalar` is its own type, not crypto-bigint's `NonZero`.
- **primefield 0.14.0 and primeorder 0.14.0:** they don't use `NonZero` at all.

### The change

- **`deny.toml`:** an `[advisories] ignore` entry for `crypto-bigint@0.7.7` exactly, with the reason and the link. It allows that one yanked version, nothing else.
- **On 8 November,** when 0.7.9 is 14 days old: move to 0.7.9, check the backup format's test vectors, and remove the entry.

### Checks

- **`cargo deny check`** passes: advisories, bans, licences and sources. Without the entry, advisories fail on the yanked crate.
- **No crate or version changes,** so the build and the tests are as on main.

### Security review

**None.** The change only lets `cargo deny` accept one named, already-reviewed version, and the code it would affect isn't used here. No code changes.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
