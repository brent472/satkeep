The restore tool's step 2 (`docs/restore-tool.md`): the backup format moves into its own crate, `satkeep-backup`, with a written spec and test vectors, the versioned wallet setup (D7) and the exit kit's armoured text (D2). Test networks only. The tool itself is the next step.

## What's new

| Piece | What it does |
| --- | --- |
| [`crates/satkeep-backup`](crates/satkeep-backup/README.md) | Make, read and open the phone key backup; the `SecurityDevice` trait; `SimulatedDevice` behind a `simulated-device` feature. It works on `bip39` phrases, with no BDK, so the restore tool stays small. |
| `setup` (D7) | The wallet setup as versioned text with exactly one form per setup: network, an optional everyday pocket (reserved), and every Savings wallet's descriptor, birthday and heir and check-in settings, oldest first. Mainnet is refused. It's both the copy locked in the backup and the kit's plain copy. |
| `kit` (D2) | The exit kit's armoured text: numbered lines of 32 bytes, each with a 4-byte SHA-256 check, so a typo or missing line is named by its number. The tool reads it from the kit PDF (stored there as an uncompressed attachment), from pasted or retyped text, or as the raw backup. |
| [`docs/backup-format.md`](docs/backup-format.md) | The byte-level spec of all three layers, and test vectors. The tests read the vectors from the page, so the spec and the code can't drift apart. Changing one digit in the spec fails the test (checked). |
| `fuzz/fuzz_targets/kit.rs` | A new fuzz target for kit text, PDF-like bytes and setup text. It's in CI and the long fuzz run. |

`satkeep-savings::backup` is now a thin layer: it re-exports the crate and adds the phone key as a `HolderKey` and the cloud folder (`save`, `latest`, `replace_device`).

## Rules the kit enforces

- **A changed kit is refused** (threat 7.2). `restore_kit` opens the backup, then refuses if the kit's plain setup differs from the one locked inside it. A swapped backup fails as the wrong device.
- **Two different kits in one input are refused.** Blocks that don't decode are skipped if another one decodes, since a PDF may draw the same markers in its page text.
- **The line check is for typos, not a signature.** Anyone can compute it. A miss (about one in 4 billion) is still caught by the backup's authentication and the setup comparison.

## Test vectors

- **Keys:** PHONE is BIP39's published test phrase "abandon … about". DEVICE, HELPER and HEIR are its other published test phrases, so the vector setup holds real descriptors (plain 2 of 3, then Taproot with the placeholder delays). The restore tool's PHONE check can use the same vectors next.
- **Other inputs:** fixed byte runs, including a device that answers with a fixed secret.
- **Outputs:** the unlock request, the KEK, the wrapped key, the 1,720-byte backup and the 100-line kit.

## Tests

Each property has a "works" test and a "forbidden" test:
- the vectors are made and opened exactly;
- every changed byte, cut-short or padded file, changed date and changed device request is refused;
- the wrong device is refused before it's asked anything;
- wrong PINs block the device;
- the setup reads back as itself, and 22 near-misses are refused (Windows line endings, a leading zero, mainnet, fields out of order, and so on);
- a kit reads from a PDF, retyped text and the raw backup;
- a typo on any line is named by that line's number;
- a missing middle line, a missing end, two different kits, a changed plain setup and a swapped backup are each refused;
- the regtest test still shows a restored phone key approving a 2-of-3 spend.

**Results:**
- `cargo test --workspace` passes: 21 in the new crate, the library's 50, and all the regtest suites.
- `cargo clippy --workspace --all-targets -D warnings` is clean, and so is clippy on the fuzz crate.
- `cargo deny check` passes.

## Dependencies

**No new crates.** `satkeep-backup` uses `bip39`, `chacha20poly1305`, `hkdf`, `sha2`, `zeroize` and `thiserror`, all already in the build. `satkeep-savings` no longer needs `chacha20poly1305`, `hkdf` or `sha2` directly.

**One new feature flag:** `chacha20poly1305`'s `getrandom` gives the operating system's generator (`OsRng`) for data keys, nonces and wallet ids. The only lockfile change is the edge `crypto-common` → `rand_core`. `getrandom` 0.2.17 and `rand_core` 0.6.4 were already there through `secp256k1`. It replaces BDK's re-exported `rand`, which the new crate can't use. As in PR #5, no fixed zero buffer is filled for a key or nonce.

## Things to know

- **Prototype backups made before this don't open,** because the payload's setup is now the structured text. They were all test data.
- **The exit drill** now also runs on PRs that touch `crates/satkeep-backup`, because the CLI it builds depends on it.

## Security review

**Two findings, both fixed here:**
1. **Medium: memory use out of proportion to a hostile kit.**
   - **The problem:** `kit::read` split each line into tokens before checking its length. One 64 MiB line of `" a"` would collect about 32 million slices (about 0.5–1 GiB) and could kill the tool on a small machine.
   - **The fix:** lines over 256 bytes are refused first (a written line is 89). A test feeds a 16 MiB line and gets "line 1 is damaged". The spec says so.
2. **Low: the simulated device was always built in.**
   - **The problem:** `satkeep-savings` turned on `simulated-device` unconditionally, so every CLI build linked it. The same was true on `main`, so it isn't a regression.
   - **The fix:** `satkeep-savings` now has its own `simulated-device` feature, turned on only by its tests. `cargo tree` shows `satkeep-cli` building `satkeep-backup` with default features only.

**Checked, nothing found:**
- **Keys and nonces:** a fresh data key and nonces per backup, all from `OsRng`. The fixed-input `seal` is private and used only by tests.
- **Key derivation and associated data:** the HKDF salt, info and labels and the wrap associated data are byte-identical to `main`.
- **Restore order:** the wrong device and a changed request are still refused before the device is asked.
- **Kits:** `restore_kit` compares the plain setup with the authenticated one in canonical form.
- **Parsers:** every parser uses checked arithmetic and bounded slices, and the block scan runs in linear time.
- **Networks:** only regtest and signet are accepted.
- **Key material:** no `Debug` on secrets, no data in errors, and `assert!` rather than `assert_eq!` wherever a failure could print key material.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
