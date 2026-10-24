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

## Dependencies

`bip39`, `chacha20poly1305`, `hkdf`, `sha2`, `zeroize` and `thiserror`, all already in the build. There's no BDK or miniscript, so the restore tool stays small.
- **Randomness:** `chacha20poly1305`'s `getrandom` feature gives the operating system's generator (`OsRng`) for data keys, nonces and wallet ids. It adds no crate to the build: `getrandom` 0.2 and `rand_core` 0.6 were already there.

## Assumptions

- **Not reviewed** by an outside cryptographer yet. The construction is in the spec's section 3.
- **Mainnet is refused** in the wallet setup until the launch review.
- **The kit's line check** finds typos when the kit is typed back in. It isn't a signature. A changed kit is caught by the backup's own authentication and by comparing the plain setup with the locked one.
- **The wallet setup checks only its form.** Whether a descriptor parses and holds the phone key is the restore tool's check (restore-tool.md, T5).
- **The `simulated-device` feature** is for tests, the prototype and fuzzing. `satkeep-savings` passes it on only through its own `simulated-device` feature, which its tests turn on. The CLI (`cargo build -p satkeep-cli`), and the restore tool to come, build without it.
- **Kit lines over 256 bytes are refused** before they're split, so a hostile file can't make the reader use memory out of proportion to it.
- **Nothing here prints or logs key material**, and no error carries any. `Restored` and `SimulatedDevice` aren't `Debug`. The phrase, data key, device secret and key-encryption key are wiped after use, except HKDF's working state (the `hkdf` crate doesn't support wiping it).

## Tests

`cargo test -p satkeep-backup`. Each property has a "works" test and a "forbidden" test:
- the spec's test vectors are made and opened exactly;
- the backup alone reveals nothing but its date and devices;
- every changed byte, cut-short or padded file, changed date or changed device request is refused;
- a wrong device is refused before it's asked anything;
- wrong PINs block the device;
- the wallet setup has exactly one text form, and 22 near-misses are refused;
- the kit reads from a PDF, from retyped text (other line endings, capitals, spaces) or from the raw backup;
- in a kit, a typo or missing line is named by its number, the missing end is reported, two different kits are refused, and a changed plain setup or a swapped backup is refused.

Two fuzz targets cover outside input: `backup_file` (backup bytes) and `kit` (kit text, PDFs and setup text). See [`fuzz/`](../../fuzz/).
