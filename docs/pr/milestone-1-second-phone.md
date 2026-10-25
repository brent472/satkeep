## Second phone (D4): lock a backup to an old phone, unlock it offline

An old phone or tablet with the Satkeep app can now be the second key that locks the phone key backup (decision D4, [`docs/restore-tool.md`](../restore-tool.md) section 5). This is the restore plan's step 5: the tool's side, against a simulated second phone. The app's side comes with the app.

### The format: method 2 ([`docs/backup-format.md`](../backup-format.md) section 3.4)

- **Locking:** ECDH with the phone's P-256 key, the only curve the Secure Enclave and Android Keystore offer.
  - The owner's phone needs only the second phone's public key, kept since pairing, so the second phone needn't be there.
  - The lock entry's request is the backup's ephemeral key.
  - The unlock secret is HKDF of the shared x-coordinate, bound to the wallet, the ephemeral key and the phone's key.
- **Unlocking** is an offline exchange of text, with no servers and no cable:
  1. The tool shows an **unlock request**: the wallet, the phone's id, the ephemeral key, a fresh session key and a check, as 244 hex digits.
  2. The phone refuses a request for another phone or another wallet before it asks for its face, fingerprint or passcode.
  3. It shows a **warning**, required app behaviour, before every answer: "Only do this if you're restoring your own Satkeep on a computer in front of you right now. Satkeep and your helper will never ask you to read this answer to anyone, by phone, message or email."
  4. It **answers** with the unlock secret masked by the session key, plus a check bound to the whole request: 72 hex digits.
  5. The person types the answer into the tool.
- **Why the mask:** the phone's screen never shows the secret itself, and an answer to one request fails another's check.
- **No curve code in the tool:** its side is hashing only, so release builds of `satkeep-restore` don't include `p256`.
- **Test vectors** for every step (6.1). The P-256 values were checked against an independent computation in plain Python, by hand.

### The code

- **`satkeep-backup`:**
  - `make_backup_for_second_phone`;
  - `SecondPhoneUnlock`, the tool's side, which acts as the security device once the answer is in;
  - `SimulatedSecondPhone`, the phone's side, behind `simulated-device`.
  - `make_backup` now refuses a device that isn't a label-deriving one, so it can't make an entry nothing opens.
- **`satkeep-restore`,** in every build:
  - a backup locked to a second phone shows the request, then reads the typed answer on standard input: 3 tries, at most 4 KiB, never printed or put in an error;
  - `show` names the second phone;
  - a backup locked to more than one second phone is refused, since the tool can't tell which one is at hand;
  - a backup locked only to a security device still needs a test build until Trezor support (step 6).
- **A `second_phone` fuzz target** for the typed request and answer. It's added to CI's fuzz matrix.

### The new dependency: `p256` 0.14.0 (approved 2026-10-24)

- **Why:** the second phone's key is P-256. `p256` is optional, behind a new `second-phone-lock` feature, which `simulated-device` turns on. It's used only to lock a backup to a phone, for the vectors and for the simulated phone.
- **Why 0.14:** 0.13 pulls in `crypto-bigint` 0.5, which has RUSTSEC-2026-0387. 0.14 sits on the RustCrypto generation from #25, with `crypto-bigint` 0.7.7, and adds no duplicate.
- **What it adds:** 15 crates, `p256`, `elliptic-curve`, `primeorder`, `primefield`, `crypto-bigint`, `sec1`, `der`, `base16ct`, `cpubits`, `ff`, `group`, `wnaf`, `subtle`, `num-traits` and `autocfg`. All are MIT or Apache-2.0 except `subtle` (BSD-3-Clause), so `deny.toml` brings back the licence exception #25 removed, with the new reason.
- **`cargo deny check`:** advisories, bans, licences and sources all pass.
- **The release tool:** `cargo tree -p satkeep-restore -e normal -i p256` finds nothing.

### Tests

- **`satkeep-backup`, 31 tests, 8 of them new:**
  - **Works:**
    - both vector sets are made and opened exactly;
    - a backup opens through the request and answer, once per answer, typed loosely or not;
    - every backup gets a new ephemeral key, and every request a different answer.
  - **Forbidden:**
    - another phone's or another wallet's request is refused before the passcode;
    - the phone answers only after its passcode, and blocks after wrong ones;
    - a typo, an answer to another request or a long paste fails the check;
    - a changed ephemeral key opens nothing, even with the phone's honest answer;
    - an off-curve key never reaches the phone's key;
    - a backup changed to name another phone opens nothing with that phone's answer;
    - a phone lock needs a real public key, and a backup locked to two phones is refused.
- **`satkeep-restore`, 20 regtest tests, 4 of them new.** These drive the real binary through pipes: read the request it prints, answer with the simulated phone, type the answer back.
  - **Works:**
    - `show` names the phone;
    - `liana` prints the descriptors;
    - `core` imports both shapes, and Core then signs as PHONE;
    - a typo can be typed again.
  - **Forbidden:**
    - three wrong answers, no answer, and an answer to an earlier request unlock nothing;
    - the answer is never printed.
- **The whole workspace's tests and clippy** (`-D warnings`, also on the fuzz crate) are clean.
- **The exit drill** runs on this PR. Its kits use the simulated security device, so it doesn't exercise the second phone; the tool's regtest tests cover that.
- **Not tested:** real phones. Whether CryptoKit and Android Keystore give the raw x-coordinate is a to-check for the app (spec section 7).

### Docs

- The spec's new sections 3.4 and 6.1, and its assumptions.
- Both crate READMEs.
- `restore-tool.md`: the status for step 5, and a new threat, 7.6:
  - **The threat:** the phone can't tell who made a request, so someone with a stolen backup file could try to talk the owner into answering it, which reveals the phone key.
  - **The mitigations:** the warning, and a new backup if the owner reports it. A new backup doesn't take back a phone key that's out, so 7.6 says moving to new keys is the full fix.
- The draft exit guide: the old-phone unlock, step by step, with the warning and "never read the answer out", and two new troubleshooting lines. These are plain words for a family member; the warning is the only app copy, quoted as the owner gave it.

### Security review

**No findings.** The review checked:
- **A changed kit or request:**
  - the phone refuses a request for another phone or wallet before its passcode;
  - the unlock secret is bound to the wallet, so a relabelled backup fails to unwrap;
  - a request someone builds only yields the secret for their own ephemeral key.
- **The method-2 request check in `restore()`:**
  - entries are matched by method, so a security device never sees a method-2 entry;
  - the request is bound into the wrap's associated data;
  - two second-phone entries are refused.
- **ECDH:**
  - `p256` checks every point is on the curve;
  - the identity point can't be written in 33 bytes, and P-256's cofactor is 1;
  - the phone only gives out a hash of the shared value.
- **The mask:** a fresh 32-byte session key per request makes it a one-time pad. The 4-byte check only catches typos: a forged answer just fails to unwrap.
- **Randomness:** the session key and the ephemeral key come from the operating system's generator. Each backup gets a new ephemeral key, and each answer's secret is used once.
- **Output:**
  - only the request is printed, as the spec says;
  - the answer never is;
  - errors carry nothing secret;
  - the request and tool types aren't `Debug`.
- **Wiping:** the session key, the secret, the mask, the decoded bytes and the typed line are `Zeroizing`, and `p256`'s keys wipe themselves.
- **Release builds:** the simulated phone sits behind `simulated-device`, which only dev-dependencies turn on.

**Below the bar, noted:**
- **Terminal scrollback** shows the request and the typed answer, which together give the unlock secret. Someone who can read that screen can already see the restored result. The README says to close the terminal.
- **A seen request and answer keep opening that backup,** because its ephemeral key doesn't change. That's the design, and the spec now says so (3.4): after such an exposure, the app should make a new backup.
- **The standard-input buffer** may keep the typed answer, unwiped.
- **Someone else's request** (added after review): the phone can't tell who made a request, so a person with a stolen backup file could try to talk the owner into answering. That's social engineering, not a code flaw; the spec now requires a warning before every answer (3.4) and restore-tool.md threat 7.6 covers it.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
