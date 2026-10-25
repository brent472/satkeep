# satkeep-backup

The phone key backup and exit kit formats: reading, unlocking and making them. **Test networks only.**

- **Who uses it:** the app side (through [`satkeep-savings`](../satkeep-savings/README.md#phone-key-backup-prototype)) and, next, the restore tool. One implementation keeps the format in step.
- **The spec:** [`docs/backup-format.md`](../../docs/backup-format.md) gives the byte layout and fixed test vectors. The tests read the vectors from that page and check this code makes and opens exactly those bytes, so the spec and the code can't drift apart.
- **The design:** [`docs/phone-key-backup.md`](../../docs/phone-key-backup.md). The restore tool's plan, including the kit, is [`docs/restore-tool.md`](../../docs/restore-tool.md).

```rust
let file = satkeep_backup::make_backup(&phrase, &setup, wallet_id, created, &mut device)?;
let text = kit::encode(&file, &setup)?;                 // the armoured text for the kit PDF
let found = kit::read(&pdf_or_text_or_backup_bytes)?;   // finds the kit, checks every line
let header = satkeep_backup::read_header(&found.backup)?; // date and devices, no unlocking
let restored = kit::restore_kit(&found, &mut device)?;  // the phrase and the locked setup
```

## What's in it

| Module | What it does |
| --- | --- |
| crate root | `make_backup`, `restore`, `read_header`, `new_wallet_id`, and the unlock label the device is asked for |
| `format` | The backup file's layout (`SKBK`, version 1): parse and write, with limits |
| `setup` | The wallet setup: one text form, versioned, with room for the everyday pocket and for earlier Savings wallets (decision D7) |
| `kit` | The exit kit: armoured text with a check on every line. It's read from a PDF, pasted text or a raw backup, and refused if its plain setup differs from the locked one (decision D2) |
| `SecurityDevice` | The one call a second key takes part through, and `SimulatedDevice`, a software device behind the `simulated-device` feature |
| `second_phone` | The second phone (decision D4): `make_backup_for_second_phone` locks a backup to a phone's P-256 key by ECDH, from its public key alone. `SecondPhoneUnlock` is the restore tool's side of the offline request and answer, and acts as the security device once the answer is in. `SimulatedSecondPhone` is the phone's side, in software, behind `simulated-device`. Spec: section 3.4. |

## Dependencies

`bip39`, `chacha20poly1305`, `hkdf`, `hmac`, `sha2`, `zeroize` and `thiserror`, all already in the build, and `p256` for the second phone. `hmac` is listed only to turn on its `zeroize` feature (see Wiping, below). There's no BDK or miniscript, so the restore tool stays small.
- **`p256` 0.14** (RustCrypto, `ecdh` only, approved 2026-10-24) is optional, behind the `second-phone-lock` feature: locking a backup to a second phone, the P-256 test vectors, and the simulated second phone. The restore tool's side of the exchange needs no curve code, so its release builds leave `p256` out. 0.14, not 0.13: 0.13 pulls in `crypto-bigint` 0.5, which has a timing advisory (RUSTSEC-2026-0387). It brings in 15 crates: `p256`, `elliptic-curve`, `primeorder`, `primefield`, `crypto-bigint`, `sec1`, `der`, `base16ct`, `cpubits`, `ff`, `group`, `wnaf`, `subtle`, `num-traits` and `autocfg`. All are MIT or Apache-2.0 except `subtle` (BSD-3-Clause), so `deny.toml` brings back its licence exception for `subtle`.
- **Versions:** RustCrypto's current generation (`chacha20poly1305` 0.11, `hkdf` 0.13, `sha2` 0.11), which needs Rust 1.87 or later. Why: [`docs/phone-key-backup.md`](../../docs/phone-key-backup.md), section 4.
- **Randomness:** `chacha20poly1305`'s `getrandom` feature gives the operating system's generator for data keys, nonces and wallet ids, through RustCrypto's `Generate` trait. It uses `getrandom` 0.4, already in the build through `tempfile`. The bitcoin crates keep their own `getrandom` 0.2 and `rand_core` 0.6.

## Assumptions

- **Not reviewed** by an outside cryptographer yet. The construction is in the spec's section 3.
- **Mainnet is refused** in the wallet setup until the launch review.
- **No private keys.** The wallet setup never holds private keys; a setup with one is refused, so the kit's plain copy is always safe to keep in the cloud. Each descriptor is split on `( ) , [ ] / # *`. A piece starting with `xprv` or `tprv`, or of 51 or 52 base58 characters (a WIF key), is refused when the setup is written and when it's read.
- **The kit's line check** finds typos when the kit is typed back in. It isn't a signature. A changed kit is caught by the backup's own authentication and by comparing the plain setup with the locked one.
- **The wallet setup checks only its form.** Whether a descriptor parses and holds the phone key is the restore tool's check (restore-tool.md, T5).
- **The second phone's curve step is untested on real phones.** The spec's P-256 vectors were checked against an independent computation (plain Python, by hand), not against CryptoKit or Android Keystore. That waits for the app (spec, section 7).
- **The `simulated-device` feature** is for tests, the prototype and fuzzing. It turns on `second-phone-lock`. `satkeep-savings` passes it on only through its own `simulated-device` feature, which its tests turn on. The CLI and the restore tool build without it (`cargo build -p satkeep-cli`, `cargo build -p satkeep-restore`). In tests, `SimulatedDevice::from_secret` rebuilds the same simulated device in another process (the restore tool's).
- **Re-exports `bip39` and `zeroize`**, so the restore tool can name the phrase type and wipe secrets without its own dependency on either.
- **Kit lines over 256 bytes are refused** before they're split, so a hostile file can't make the reader use memory out of proportion to it.
- **Nothing here prints or logs key material**, and no error carries any. `Restored`, `SimulatedDevice`, `SimulatedSecondPhone`, `UnlockRequest` and `SecondPhoneUnlock` aren't `Debug`. The phrase, data key, device secret and key-encryption key are wiped after use.
- **Wiping inside the libraries:**
  - **The cipher** wipes its key on drop (`chacha20poly1305`'s `zeroize` feature).
  - **HKDF's working state** is wiped on drop. `hkdf` 0.13 has no `zeroize` feature of its own, but its state is HMAC-SHA256: `hmac`'s and `sha2`'s `zeroize` features wipe each HMAC (the salt-keyed one, the one keyed with the pseudorandom key, and the copy made for each output block), with SHA-256's state and buffer inside them.
  - **Still not wiped:** a few short-lived plain arrays inside the libraries. In `hkdf`, the pseudorandom key `Hkdf::new` throws away and the last output block in `expand`; in `hmac`, the key XORed with its pad while keying, and the inner hash while finishing. Those are on the stack only during the call, and no feature wipes them. Copies the compiler makes when values move aren't wiped either.

## Tests

`cargo test -p satkeep-backup`. Each property has a "works" test and a "forbidden" test:
- the spec's test vectors are made and opened exactly;
- the backup alone reveals nothing but its date and devices;
- every changed byte, cut-short or padded file, changed date or changed device request is refused;
- a wrong device is refused before it's asked anything;
- wrong PINs block the device;
- the wallet setup has exactly one text form, and 22 near-misses are refused;
- a setup holding a private key (`tprv`, `xprv` or WIF) is refused, written or read, and a `tpub` that happens to contain `prv` isn't;
- the kit reads from a PDF, from retyped text (other line endings, capitals, spaces) or from the raw backup;
- in a kit, a typo or missing line is named by its number, the missing end is reported, two different kits are refused, and a changed plain setup or a swapped backup is refused;
- **the second phone:**
  - the spec's second-phone vectors are made and opened exactly;
  - a backup locked to a phone's public key opens through the request and answer, once per answer, typed loosely or not;
  - every backup gets a new ephemeral key, and every request a different answer;
  - the phone refuses another phone's or another wallet's request before its passcode, answers only after it, and blocks after wrong ones;
  - a typo, an answer to another request or a long paste fails the check, and a typo in the request is caught on the phone;
  - a changed ephemeral key opens nothing even with the phone's honest answer, a key of the wrong shape is refused before any request, and an off-curve one never reaches the phone's key;
  - a backup changed to name another phone opens nothing with that phone's answer;
  - a phone lock needs a real public key, and a backup locked to two phones is refused.

Three fuzz targets cover outside input: `backup_file` (backup bytes), `kit` (kit text, PDFs and setup text) and `second_phone` (the typed request and answer). See [`fuzz/`](../../fuzz/).
