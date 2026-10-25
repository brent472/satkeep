## satkeep-backup: wipe HKDF's working state (hmac's and sha2's zeroize features)

The follow-up to #25's zeroize finding. `satkeep-backup` turns on the optional `zeroize` features of `sha2` and `hmac`, so HKDF's working state is wiped on drop. Before, the README said it wasn't.

### What changes

- **`sha2 = { version = "0.11.1", features = ["zeroize"] }`:** SHA-256's state and block buffer are wiped on drop, everywhere SHA-256 is used here, including inside HMAC.
- **`hmac = { version = "0.13.0", features = ["zeroize"] }`,** a new direct dependency entry, there only to turn its feature on. `hmac` was already in the build, under `hkdf`, at the same version.
- **`hkdf` has no `zeroize` feature.** #25's description said `hkdf` 0.13 offers one; it doesn't. Its only optional feature is `kdf`. HKDF's working state is an HMAC-SHA256, though, so the two features above cover it:
  - the salt-keyed HMAC during extract;
  - the HMAC keyed with the pseudorandom key, which `Hkdf` keeps;
  - the copy `expand` makes for each output block.
- **Under the hood:** both features turn on `digest`'s `zeroize`, which turns on `block-buffer`'s. Nothing else in `hmac`'s code depends on its feature today, so `sha2`'s alone would give the same build. `hmac`'s is listed so the wiping doesn't depend on that staying true.
- **No crate is added or changes version.** The lock files only gain `zeroize` as a dependency of `block-buffer` and `digest` (already in the build), and `hmac` under `satkeep-backup`. `fuzz/Cargo.lock` moves the same way.
- **The release tool gets it too:** `cargo tree -p satkeep-restore -e normal,features` shows both features on.

### What's still not wiped

No feature wipes a few short-lived plain arrays inside the libraries. They're on the stack only during the call:
- in `hkdf`, the pseudorandom key that `Hkdf::new` returns and throws away, and the last output block in `expand`;
- in `hmac`, the key XORed with its pad while keying, and the inner hash while finishing.

Copies the compiler makes when values move aren't wiped either. The README now says all of this, in place of "HKDF's working state isn't wiped".

### Docs

- **`crates/satkeep-backup/README.md`:** the dependency list names `hmac` and why; a new "Wiping inside the libraries" item says what's wiped and what isn't.
- **`crates/satkeep-savings/README.md`:** its line about HKDF's state now points there.
- **`docs/phone-key-backup.md` section 4:** notes which `zeroize` features are on.

### Checks

- **The format's test vectors pass byte for byte, unchanged:** both sets, `the_spec_test_vectors_decode_and_are_made_exactly` and `the_spec_second_phone_vectors_decode_and_are_made_exactly`. Wiping only runs on drop, so no output changes.
- **The whole workspace's tests pass,** regtest included, on Windows.
- **Clippy** (`-D warnings`) is clean on the workspace and the fuzz crate.
- **`cargo deny check`:** advisories, bans, licences and sources all pass.

### Security review

**None.** The review checked:
- **No output can change:** every `zeroize`-gated item in `sha2`, `block-buffer` and `digest` only wipes on drop. `hmac`'s feature only forwards to `digest`'s. The vectors confirm it.
- **Feature unification:** the restore tool and `satkeep-savings` get the same wiping and nothing else. The bitcoin crates hash with `bitcoin_hashes`, which this doesn't touch.
- **Supply chain:** no new package. `hmac` resolves to the 0.13.0 `hkdf` already uses, from crates.io, MIT or Apache-2.0, within `deny.toml`.
- **Key material:** `satkeep-backup`'s source is unchanged, and the features add no `Debug`, logging or I/O.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
