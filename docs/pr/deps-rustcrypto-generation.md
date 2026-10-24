## RustCrypto: one current generation, and Rust 1.87

`satkeep-backup`, the only crate that uses RustCrypto directly, moves to RustCrypto's current generation. This comes before the second phone (D4), which needs `p256` 0.14 from the same generation: `p256` 0.13 pulls in `crypto-bigint` 0.5, which has a timing advisory ([RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387)).

| Crate | Before | After |
| --- | --- | --- |
| `chacha20poly1305` | 0.10.1 | 0.11.0 |
| `hkdf` | 0.12.4 | 0.13.0 |
| `sha2` | 0.10.9 | 0.11.1 |
| `aead` | 0.5.2 | 0.6.1 |
| `hmac` | 0.12.1 | 0.13.0 |
| `digest` | 0.10.7 | 0.11.3 |
| `chacha20` / `poly1305` / `cipher` / `universal-hash` | 0.9.1 / 0.8.0 / 0.4.4 / 0.5.1 | 0.10.2 / 0.9.1 / 0.5.2 / 0.6.1 |
| `crypto-common` / `block-buffer` / `inout` / `cpufeatures` | 0.1.7 / 0.10.4 / 0.1.4 / 0.2.17 | 0.2.2 / 0.12.1 / 0.2.5 / 0.3.1 |

- **Added,** all RustCrypto and MIT or Apache-2.0: `ctutils` 0.4.5 (constant-time helpers), `cmov` 0.5.4, `hybrid-array` 0.4.15, `const-oid` 0.10.2, and `rand_core` 0.10.1.
- **Removed:** `generic-array`, `opaque-debug` and `subtle`. `deny.toml` drops its licence exception for `subtle`, which is no longer in the build.
- **`fuzz/Cargo.lock`** moves to the same versions.

**The minimum Rust version rises from 1.85 to 1.87.** The current generation pulls in `ctutils`, and every `ctutils` without [RUSTSEC-2026-0385](https://rustsec.org/advisories/RUSTSEC-2026-0385) (0.4.4 and later) needs Rust 1.87. With 1.85 the resolver picks 0.4.2, and `cargo deny` fails. CI uses current stable, so nothing else changes. The new floor turns on one clippy lint, fixed here (`is_multiple_of` in the hex reader).

**The code changes are only the new API:**
- **Randomness:** RustCrypto's `Generate` trait on byte arrays (`<[u8; 32]>::generate()`), instead of `generate_key(&mut OsRng)` and `generate_nonce(&mut OsRng)`. It's still the operating system's generator, now through `getrandom` 0.4, which was already in the build through `tempfile`.
- **Nonces and keys:** `XNonce::from(array)` for nonces and `new_from_slice` for the cipher's key, instead of the deprecated `from_slice`. A data key of the wrong length still means "can't unlock", as before.
- **Wiping the cipher's key:** `chacha20poly1305` 0.10 always wiped its key when dropped. In 0.11 that's the `zeroize` feature, so it's turned on here; it also turns on `chacha20`'s wiping, as 0.10 did. Without it, the upgrade would have quietly stopped wiping. `hkdf`, `hmac` and `sha2` didn't wipe their state before, and still don't: 0.13 and 0.11 offer an optional `zeroize`, which could be a small follow-up.
- Nothing else in the format's code changes.

### Checks

- **The format's test vectors pass byte for byte, unchanged:** the KEK, the wrapped key, the 1,720-byte backup and the kit text (`the_spec_test_vectors_decode_and_are_made_exactly`). No vector or spec text changed.
- **The whole workspace's tests pass,** regtest included, on Windows.
- **Clippy** (`-D warnings`) is clean on the workspace and the fuzz crate.
- **`cargo deny check`:** advisories, bans, licences and sources all pass.
- **The exit drill** runs on this PR, because the lock file changed.

### Duplicates that remain

No RustCrypto crate is left in two versions.

| Crate | Versions | Pulled in by |
| --- | --- | --- |
| `rand_core` | 0.6.4, 0.10.1 | 0.6.4: the bitcoin side (`bdk_wallet` 3.2, `rand` 0.8, `secp256k1` 0.29 under `bitcoin` 0.32). 0.10.1: RustCrypto (`crypto-common` 0.2) and `getrandom` 0.4. |
| `getrandom` | 0.2.17, 0.4.3 | 0.2.17: `rand_core` 0.6, on the bitcoin side. 0.4.3: `crypto-common` 0.2 and `tempfile` (already there). |
| `base64` | 0.13.1, 0.21.7, 0.22.1 | `jsonrpc` 0.18, `bitcoin` 0.32, `jsonrpc` 0.19. As before. |
| `hex-conservative` | 0.2.3, 1.3.0 | `bitcoin` 0.32 and `bitcoin_hashes` 0.14; `bitcoin-consensus-encoding`, which is in the lock file but not built. As before. |
| `jsonrpc` | 0.18.0, 0.19.0 | `bitcoincore-rpc` 0.19 (the restore tool); `corepc-client` 0.12 (the test nodes). As before. |
| `syn` | 2.0.119, 3.0.9 | `zeroize_derive`; `clap_derive`, `serde_derive` and `thiserror-impl`. Build-time only. As before. |

`rand_core` 0.6 and `getrandom` 0.2 stay until the bitcoin crates move on: `bitcoin` 0.32 and BDK 3.2 still use `rand` 0.8.

### Security review

SECURITY-REVIEW

🤖 Generated with [Claude Code](https://claude.com/claude-code)
