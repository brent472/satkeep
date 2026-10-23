Phone key backup: the research document, approved with D1–D7 and updated for master brief v3.8, plus the prototype it planned, with the security device simulated in software. Test networks only. No real devices or cloud storage yet.

## 1. Research (`f91cfa3`, `25cfe78`)

[`docs/phone-key-backup.md`](../phone-key-backup.md) contains:
- the requirements, taken from the brief's backup section and Section 19 screens without reproducing their text
- the proposed format and flows
- a sourced device survey (works / works with caveats / can't)
- the encryption library choice
- the recovery-contact options
- where the file lives
- the four threats
- decisions D1–D7 and the prototype plan

**Updated for v3.8:**
- **D2 is superseded.** The recovery contact unlocks right away (Bitkey's model), and the helper holds no part of any unlock key. After a restore on a new phone, large Savings payments wait in a helper hold, with alerts.
- **Section 5 is rewritten** to match, with notes for when the contact path is built:
  - a single-use, securely random short code
  - the contact's key bound to the owner's keys at enrolment
  - the contact's app sending its own alerts
- **New section 7.5** covers collusion and coercion around the contact:
  - **Contact fooled or pressured + cloud takeover:** Spending is exposed, and Savings is protected by the hold.
  - **Helper + contact:** they still need the owner's cloud file.
  - **A compromised helper alone** can't unlock anything, and can't hide a restore.
- **D4 and D5 are copy changes**, made in the brief's Section 19. No product wording is written here.

**Added after review: the exit kit (section 6.2, requirement R15).**
- **The problem:** CloudKit's private database and Drive's `appDataFolder` can be read only by the Twofold app, so if Twofold disappeared no outside tool could fetch the backup.
- **The fix:** like Bitkey's Emergency Exit Kit, a normal file in the owner's visible iCloud Drive / Google Drive holds a copy of the locked backup itself, with the same bytes and locks. The restore tool then needs only that file and the second key.
- **Section 6.2 covers** security (the same ciphertext, just easier to find), keeping the kit current, the owner deleting it, and old copies. The kit's file name and instructions are product copy for Section 19.
- **The prototype is unchanged:** the kit carries the bytes `make_backup` produces.

## 2. Prototype (`56843da`, `c186a04`)

`twofold_savings::backup`:

| Piece | What it does |
| --- | --- |
| `make_backup` | A fresh random data key per backup encrypts the phrase and the wallet's public setup with XChaCha20-Poly1305, with the whole header as associated data. The data key is wrapped under an HKDF-SHA256 key derived from the device's secret, bound to the wallet id, device id, unlock method and format version. |
| `read_header` | The date and the device ids, without unlocking. |
| `restore` | Finds the lock entry for this device, so a wrong device is refused before it's asked anything, then unlocks with the device's PIN. |
| `save` / `latest` / `replace_device` | Write a new file, delete every other visible copy, and check only the new one is left. Replacing the device re-locks under a new data key. |
| `SecurityDevice`, `SimulatedDevice` | A SLIP-0011-style secret per request, only after the PIN, blocked after 5 wrong tries |
| `MemoryCloud` | Keeps every old copy, like real cloud storage |
| The contact lock entry | Reserved in the format, never opened by a device |

**Nothing secret is printed or put in an error:**
- No error variant carries data.
- `Restored` and `SimulatedDevice` aren't `Debug`.
- The phrase, data key, device secret and key-encryption key are wiped from memory after use.

### Tests (every property with its forbidden case)

| Property | Works | Forbidden version fails |
| --- | --- | --- |
| The backup alone reveals nothing | The header shows the date and device ids | No phrase word (4+ letters) and no setup text in the file |
| Restore needs backup + device | Right device + PIN → the same 12 words, keys and setup | Wrong device → refused, and never asked; no PIN → refused; wrong PINs → blocked for good |
| Replacing the device re-encrypts | The new device opens the new file; only one copy is left in view | The old device can't open the new file. **It still opens an old copy**: pinned as a test, the limit that makes a lost device also replace the phone key (D3). |
| Tampering | — | Every single-bit change anywhere in the file refuses, the date included. A changed device request is refused before the device is asked. Cut-short, padded, garbage and newer-version files are refused. |
| The restored key signs (regtest) | Restored PHONE + HELPER spends Savings 2-of-3 | A key restored from another wallet's backup adds no signature, and the spend can't be finalized |
| It holds the BIP39 phrase | The restored phrase is the same 12 words | — |
| No key material in errors | — | Errors are checked for phrase runs |

**Fuzzing:** a new target, `backup_file`, because the file comes back from the cloud as outside input. It takes raw bytes, or a real backup edited by the input. An edited backup must never open, and one that opens must give back the original phrase. It's added to the CI and long-fuzz matrices.

### New dependencies (approved)

| Crate | Why |
| --- | --- |
| `chacha20poly1305` 0.10.1 (`alloc` only) | XChaCha20-Poly1305. RustCrypto's AEAD is the only candidate with a published audit (NCC, 2020), and random 24-byte nonces remove nonce bookkeeping. |
| `hkdf` 0.12.4 | Deriving each lock entry's key, with every input labelled |
| `sha2` 0.10.9 | SHA-256 for HKDF |

- **Brought in with them:** `aead`, `chacha20`, `cipher`, `poly1305`, `universal-hash`, `crypto-common`, `inout`, `cpufeatures`, `opaque-debug`, `digest`, `block-buffer` and `hmac` (all RustCrypto), plus `generic-array`, `typenum` and `subtle`.
- **Why not the newest line** (0.11 / 0.13 / 0.11):
  - `cargo deny` failed on it. It pulls in `ctutils` 0.4.2, which has [RUSTSEC-2026-0385](https://rustsec.org/advisories/RUSTSEC-2026-0385).
  - The fix, `ctutils` 0.4.4, was released today and needs Rust 1.87, above our 1.85 floor.
  - The 0.10 line doesn't use `ctutils`.
- **`subtle` is BSD-3-Clause.** I allowed it in `deny.toml` for that crate only, like `hex_lit`, rather than widening the global licence list.
- `cargo deny check` passes.

## Checks

- **`cargo test --workspace`** passes with throwaway regtest nodes: 56 library unit tests (14 new for the backup) and every integration suite, including the 2 new regtest backup tests.
- **`cargo clippy -D warnings`** passes for the workspace and the fuzz crate.
- **`cargo deny check`** passes.

## Security review

**Findings: none.** `/security-review` was run on the full diff before opening this PR.

- **What it checked:**
  - nonce and key reuse
  - domain separation in the HKDF labels and associated data
  - header authentication, and downgrade through the version, method and kind fields
  - wrong, absent and blocked devices
  - the parser on untrusted bytes: bounds, overflow, panics
  - leaks through errors and `Debug`
  - wiping memory and randomness
  - rollback to an old cloud copy (the documented limit in section 7.3)
  - the CI changes and the `subtle` licence exception
- **Fixed from its lower-severity notes** (`0547822`):
  - **Phrases in failure output.** Two checks (a unit test and the fuzz target) compared phrases with `assert_eq!`, which would print both phrases if it ever failed, into the CI log in the fuzz job's case. They now use `assert!` with a fixed message.
  - **The device request.** `restore` now refuses a lock entry whose device request isn't this wallet's own label, before asking the device. Without that, a changed cloud file could make a device derive a key for some other label. A test covers it.
- **Noted, not changed:** `chacha20poly1305` 0.10 wipes its own key copy on drop, but HKDF's working state isn't wiped, because the `hkdf` crate doesn't support it. That's written in the library README.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
