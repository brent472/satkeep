# Restore tool: plan

**Status: plan only, for review. No code yet.** D1–D3 and D5–D7 are approved as recommended; D4 is decided by brief v3.12 (section 8). Test networks only, like everything else in this repo.

The brief promises that if Satkeep disappears, the owner can still get their bitcoin out. They need the exit kit in their own cloud drive, their second key, and Satkeep's open-source restore tool. The brief lists that tool as an open item that must exist and pass the exit drill before launch.

This document covers:
- what the tool does, and what it can't do;
- what the exit kit must hold for it;
- which second keys it can work with;
- how keys leave it safely;
- the order of work;
- the decisions needed before code.

## Contents

1. [What the tool has to do](#1-what-the-tool-has-to-do)
2. [What getting out needs, once Satkeep is gone](#2-what-getting-out-needs-once-satkeep-is-gone)
3. [Proposed design](#3-proposed-design)
4. [The exit kit file](#4-the-exit-kit-file)
5. [Second keys the tool can talk to](#5-second-keys-the-tool-can-talk-to)
6. [Exporting to Liana and Bitcoin Core](#6-exporting-to-liana-and-bitcoin-core)
7. [Threats](#7-threats)
8. [Decisions](#8-decisions)
9. [Order of work](#9-order-of-work)

## 1. What the tool has to do

| # | Requirement | Comes from |
| --- | --- | --- |
| T1 | **Inputs.** Unlocks the phone key backup from the exit kit file and the second key, and needs nothing else: no Satkeep app, account, server or helper, and no app-only cloud storage. | Brief (exit kit, open decisions); `docs/phone-key-backup.md` 6.2 |
| T2 | **Outputs.** Exports what Liana or Bitcoin Core need to spend: Savings in both shapes, and the everyday pocket. | Brief; owner's instruction |
| T3 | **Open source.** Small, reproducible to build, and usable by someone who isn't us. | Brief |
| T4 | **Before unlocking**, shows the backup's date, and which second key locks it. | R5 and R6 in `phone-key-backup.md` |
| T5 | **Checks the result.** The unlocked phone key must be the PHONE key in the wallet setup, so a swapped or mismatched backup is caught. | New |
| T6 | **No key material on screen, in logs or in errors** by default. Keys never go on the command line. | CLAUDE.md |
| T7 | **Passes the exit drill:** the drill restores with the tool, imports into Liana and Bitcoin Core, and spends, with no Satkeep code in the spend. | Brief |
| T8 | **Treats the kit as untrusted input.** The kit is a file anyone with the cloud account can edit. | Threat 7.2 |

## 2. What getting out needs, once Satkeep is gone

When Satkeep is gone, so is the helper. That changes what each part of the wallet needs:

| Part | Who can spend, without Satkeep | What the owner needs |
| --- | --- | --- |
| **Everyday pocket** | PHONE alone | The phone key: the restore tool gives it back |
| **Savings** (either shape) | 2 of 3, but HELPER is gone, so **PHONE + DEVICE** | The phone key (from the tool) **and** the second key, signing in Liana or another wallet |
| **Savings with a beneficiary** | Also HEIR alone, after the heir delay | Unchanged: HEIR's own key in Liana |

**So the second key matters twice.** It unlocks the backup, and it's one of the two keys Savings still needs. A second key that only works inside the Satkeep app fails both jobs once the app is gone. Section 5 looks at which second keys pass. This is the most important finding of this plan. The case that would fail, an old phone or tablet, is settled by **D4** (brief v3.12): that phone's app must work fully offline and be able to export its key.

## 3. Proposed design

A small command-line program, `satkeep-restore`, in this repo as its own crate.

**Steps:**
1. **Read the kit.** Find the locked backup and the wallet setup inside the kit file (section 4), and refuse anything malformed.
2. **Show what was found:** the backup's date, the network, the wallets in the setup (shapes, public keys, birthdays), and which second key locks the backup (its id and unlock method). Nothing is unlocked yet.
3. **Ask the second key.** The device asks for its own PIN or fingerprint. The tool asks it only for this wallet's own label, as `restore` already does.
4. **Unlock.** Derive the KEK, unwrap the data key, decrypt the payload: the phone key's 12 words and the wallet setup. The payload's wallet setup must be the same as the kit's plain copy. The locked copy is authenticated; the plain one isn't (threat 7.2).
5. **Check** that the phone key's public key, at its documented path, is PHONE in every Savings descriptor of the setup (T5).
6. **Export** to Bitcoin Core or Liana (section 6), without putting key material on screen by default.

**Code:**
- **The backup format moves into its own small crate.** It holds the parser and the unlock (today in `satkeep-savings::backup`), and depends only on `chacha20poly1305`, `hkdf`, `sha2`, `bip39` and `zeroize`, all already approved. Both the app-side library and the tool use it, so there's one format implementation. The tool doesn't pull in BDK's whole wallet.
  - *Alternative:* a second, independent implementation, as the exit drill uses Liana's own signer. That's more code to keep in step, for a format we own. See **D5**.
- **A written format spec** (`docs/backup-format.md`) with fixed test vectors. If the tool's own code is ever lost, anyone can rebuild it from the spec. Section 2.1 of `phone-key-backup.md` is its start.
- **Exporting needs descriptors and keys.** That's `miniscript` and `bitcoin` (already dependencies), not `bdk_wallet`.
- **Device access** is behind one trait, like `SecurityDevice` today, with one module per kind of device (section 5).

**The heir's route.** An heir can use the kit too, with their own key (HEIR) and the kit's plain wallet setup, and no second key.
- **That plain setup is unverified.** It's checked against the locked copy only when the second key unlocks the backup. On the heir's route nothing unlocks it, so anyone with the cloud account could have changed it.
- **The tool says so** whenever it shows or exports a setup it hasn't checked.
- **What limits the harm:** the heir's key can only ever sign for wallets that contain it. The tool exports for the heir only the wallets whose descriptors hold HEIR's key. A changed setup can hide a wallet or show wrong details, but it can't get the heir's key to approve anything else.

**What it won't do:**
- **Talk to the network to unlock.** Unlocking is offline.
- **Build or broadcast transactions.** Liana and Bitcoin Core do that.
- **Mainnet.** The tool refuses mainnet until the launch review, like the rest of the repo.

## 4. The exit kit file

The kit's wording belongs to Section 19 of the brief. This section is only about the file's structure, which the tool has to read. Per the brief, the kit holds:
- which apps to use;
- step-by-step instructions;
- the wallet setup;
- a locked copy of the backup.

**What the tool needs inside it:**
- **The locked backup:** the exact bytes `make_backup` produces (format in `phone-key-backup.md` 2.1).
- **The wallet setup, in plain form,** so a viewer shows the balance without the second key, as the brief says.

**The wallet setup today:** a free string (`wallet_setup` in the backup payload). It needs a real, versioned format. It carries:
- the network;
- the everyday pocket's descriptor;
- every Savings descriptor that may still hold coins, with its birthday height and heir delay. That includes earlier wallets after a move, because leftovers stay there (library README, *Adding a beneficiary*);
- the check-in settings.

**Not yet decided:** the everyday pocket's descriptor (brief 3.11). The format needs a place for it now; its exact form comes with the pocket's own design.

**File type (D2).** The brief describes the kit as a page the owner can view and save, as Bitkey's is a PDF. Options:

| Option | For | Against |
| --- | --- | --- |
| **PDF** with the backup and setup as an **embedded attachment**, stored uncompressed between clear markers, plus the same data **printed as armoured text** (like a PGP block) on its last page | Opens on any device; survives "Save a copy". The tool can find the markers by scanning the bytes, with no PDF library. The printed copy survives printing, or a PDF re-saved by other software, by typing or scanning it back. | Two copies to keep identical; printed text is long |
| Plain HTML page with a data block | Simplest to make and read | Less natural to keep or print; some cloud viewers won't render it |
| A guide plus a separate `.tfbk` data file | The tool reads it directly | Two files the owner must keep together; one gets lost |

**Recommendation:** the PDF, with the tool taking any of three inputs:
- the PDF itself;
- the armoured text, pasted or saved;
- the raw backup bytes.

Making the PDF is app work. The plan here only fixes where the data sits, so the tool can find it.

## 5. Second keys the tool can talk to

From the survey in `phone-key-backup.md` 3.2 and decision D1 there, the second key at launch is an **old phone or tablet** or a **Trezor**, and later the **Satkeep card**. Here's how each one works with the tool:

| Second key | Unlocks the backup without Satkeep? | Signs Savings without Satkeep? | Verdict for the exit |
| --- | --- | --- | --- |
| **Trezor** (SLIP-0011 CipherKeyValue) | **Yes.** It's a public protocol, and the tool asks for the same path and label the app used. Over USB from a computer, with the PIN on the device. Code: the `trezor-client` Rust crate, or calling Trezor's own `trezorctl` (`crypto encrypt-keyvalue`). Which one, and its upkeep, is **unconfirmed**. | Liana and Sparrow sign with hardware wallets. Whether Liana v15 supports Trezor is **unconfirmed**; Sparrow does. | **Works**, if the label and path are written down in the format spec |
| **Satkeep card** (FIDO `hmac-secret` / PRF, if built that way) | **Yes in principle.** It's a public standard, so desktop FIDO tools (libfido2's `fido2-assert` with `-h`) can ask for the same secret, given the credential id and salt stored in the lock entry. Untested. | Only if the card also speaks a signing protocol other wallets know. Its design isn't decided. | **Works by design,** if the card is built for it. Add this to the card's requirements. |
| **Old phone or tablet** with the Satkeep app (Secure Enclave or Keystore key, ECDH) | **Through that phone's app, offline (D4).** The unlock key can't leave the phone, by design, and only our app can use it. So the app answers the tool's unlock request with no Satkeep servers: the tool shows the request, the phone's app shows the answer, by QR code or text. Its exact wire format goes in the format spec. If the app is no longer on that phone and can't be installed again, the backup can't be opened. | **Yes, through the export (D4):** the app's Advanced option exports DEVICE's key as 12 words, or straight to Liana or Bitcoin Core. It also signs offline. | **Works while that phone's app is still installed,** with the kit explaining the step. The app recommends, never requires, a hardware second key once Savings is large. |
| **Message-signing devices** (Coldcard, Jade, BitBox02, Ledger; not at launch) | **Yes,** by signing the same fixed message. These have the repeatability caveats in 3.2. | Yes, in Liana and Sparrow | **Works with caveats** |

**The old-phone case** depends on that phone's app still being installed. Brief v3.12 accepts that and makes the app as independent as it can be (D4). For the tool, it means one more kind of device: a **second phone**, reached by an offline request and answer, not over USB.

**Testing:**
- **Every case** runs against the software device that already exists (`SimulatedDevice`, SLIP-0011-like), reachable from the tool only in test builds (a cargo feature that release builds leave out).
- **The second-phone exchange** is tested against a simulated second phone (an ECDH key in software) until the app exists.
- **A real Trezor protocol test** needs the Trezor emulator (`trezor-user-env`) in CI. That's a download, so it waits for your OK (**D6**).

## 6. Exporting to Liana and Bitcoin Core

Liana and Bitcoin Core take keys in different ways, and each way puts the key somewhere.

| Target | What it needs | How the tool gives it |
| --- | --- | --- |
| **Bitcoin Core** | Descriptors with PHONE's private key in place (`importdescriptors`). One for each Savings wallet in the setup, plus the pocket. | **Default: import straight into the owner's own Bitcoin Core** over local RPC, using the cookie file, into a new wallet. The key is never displayed. The exit drill already does this by hand. Option: write a descriptor file the owner names. It must be a new file, private to the user where the OS allows, and the tool refuses to overwrite. |
| **Liana** | The Savings descriptor (public), and a signer for PHONE. Liana's hot signer comes from a 12-word phrase (the exit drill builds it from Liana's own `HotSigner`). | Print the **public** descriptor to paste into Liana. For PHONE: show the **12 words** once, only with an explicit flag (`--show-phrase`), after a warning, for the owner to type into Liana's import. Writing straight into Liana's data folder would mean depending on Liana's file layout, which is internal. |
| **The everyday pocket** | Its descriptor and PHONE's key. **Liana can't hold it:** Liana refuses a policy with no recovery path. | Bitcoin Core (as above), or the 12 words in any BIP39 wallet (Sparrow, for example). |

**Also exported:**
- **Birthday heights,** so a rescan doesn't start at genesis.
- **Each Savings wallet's heir delay,** for the record.

**What the owner still does in Liana or Core:** sign with DEVICE (the second key), then broadcast.

**Rules for anything secret:**
- Nothing secret goes to standard output unless the owner asks, and never to standard error.
- Files are created new, never overwritten.
- Keys and phrases are wiped from memory after use (`zeroize`).
- Errors never repeat input, as today.

## 7. Threats

### 7.1 A fake restore tool

The kit points to the tool, so a fake tool in search results, a look-alike download page or a phishing email is the obvious attack. It would steal the phone key and the second key's unlock.

**Mitigations:**
- **Reproducible release builds,** signed with a published key. The kit carries the key's fingerprint and the expected release digests.
- **Source mirrored** somewhere that survives us (Software Heritage archives public repositories), so it can be built from source.
- **The kit's instructions** say how to check a download. That wording is Section 19's.

**Residual risk:** an owner who skips the check.

### 7.2 A changed kit

Anyone with the cloud account can edit the kit, for example to point the instructions at a fake tool, or to swap the wallet setup so the owner imports a wallet whose coins go elsewhere.

**Mitigations:**
- **The locked backup is authenticated** (its header is AEAD associated data), so it can't be changed undetected. A swapped backup fails the PHONE check (T5).
- **The plain wallet setup in the kit isn't authenticated.** The tool uses the copy inside the locked payload, and refuses if the plain copy differs from it.
- **That check needs the second key.** On the heir's route (HEIR's key plus the plain setup, no second key), the plain setup is unverified. The tool says so, and the heir's key only signs for wallets that contain it (section 3).
- **The plain setup never holds private keys** (`backup-format.md` section 4), so it's safe for the kit to keep it in the cloud.
- **The instructions themselves can't be protected by the tool.** Hence the published fingerprint and digests in 7.1, which the owner can also find outside the kit.

### 7.3 The computer the restore runs on

Once unlocked, the phrase is in that computer's memory, and with the default Core export, in that node's wallet. Malware there gets the phone key: the pocket, and one of the two keys Savings needs. That's the same exposure as `phone-key-backup.md` 7.4, on a desktop instead of a phone.

**Mitigations:**
- **Unlock offline;** the tool never opens a network connection to do it.
- **Guidance** (Section 19) to use a clean computer.
- **Move Savings to new keys afterwards,** since the old ones have been on that computer.

**Savings stays safe on that computer alone:** it still needs DEVICE, whose key never leaves the device.

### 7.4 The second key's unlock asked for something else

A tool, real or fake, could ask the device to derive a key for another label.

**Mitigations:**
- **The device shows the label on screen** (Trezor CipherKeyValue with `ask_on_decrypt`).
- **The label is fixed** in the format spec and names Satkeep and the wallet id.

**Residual risk:** an owner who doesn't read the screen.

### 7.5 A stale kit

A kit from before a phone key or second key change opens to old keys, which may hold nothing. The tool shows the backup's date and the wallets' birthdays before unlocking. After unlocking, it reports which wallets have coins, using the owner's own node, as a separate step after the export.

## 8. Decisions

All approved: D1–D3 and D5–D7 as recommended, and D4 as decided in brief v3.12.

| # | Decision | Options | Outcome |
| --- | --- | --- | --- |
| **D1** | Where the tool lives | A crate in this repo / its own repo | **A crate in this repo** (`crates/satkeep-restore`), with signed reproducible releases and a Software Heritage mirror. Splitting it out later is easy; keeping the format in step across repos isn't. |
| **D2** | The exit kit's file type | PDF with an embedded attachment plus printed armoured text / HTML / guide plus a data file | **PDF**, as in section 4. The tool reads the PDF, the armoured text or the raw bytes. |
| **D3** | How PHONE's key reaches Liana and Core | Core: RPC import / key file. Liana: show the 12 words on request / write into Liana's data folder. | **Core: RPC import by default, file on request. Liana: show the 12 words only with `--show-phrase`, after a warning.** Never Liana's internal files. |
| **D4** | The old-phone second key, which fails once the app is gone (section 5) | (a) Accept it and say so in the kit. (b) The app recommends a hardware second key when the second key is an old phone. (c) An old phone can't be the only second key for Savings above some amount. | **Decided (owner, brief v3.12): an old phone still counts as a second key**, with three requirements: (1) the second-phone app signs and unlocks fully offline, with no Satkeep servers, and has an Advanced option to export its key (12 words, or straight to Liana or Bitcoin Core); (2) the exit kit covers that step for owners whose second key is a phone; (3) the app recommends, never requires, a hardware second key once Savings is large. The wording is the brief's (Section 04 *Inheritance*, Section 19 *Settings*). (1) and (3) are app work; for the tool, (1) means the second-phone request-and-answer above, and (2) means the kit's structure leaves room for that step. |
| **D5** | One backup implementation or two | Shared small crate / an independent second implementation | **Shared crate, plus the written spec and fixed test vectors.** The exit drill still restores through the tool and spends with Liana's and Core's own code, so the parts that matter are checked independently. |
| **D6** | Testing with a real device protocol | Simulated device only / plus the Trezor emulator in CI | **Simulated device first. The Trezor emulator (`trezor-user-env`, a download, on GitHub runners only) when Trezor support is built,** after your OK. |
| **D7** | The wallet setup format | Keep a free string / a versioned format | **A versioned format** (section 4), with room for the pocket's descriptor and for earlier Savings wallets. It's also the kit's plain copy. |

## 9. Order of work

Each item is its own branch and PR, stopping for review after each, as usual.

1. **This plan** (`research/restore-tool`). Stop for your OK.
2. **The format crate and spec.** Move the backup code into `satkeep-backup`, write `docs/backup-format.md` with fixed test vectors, and add the versioned wallet setup (D7) and the kit structure (D2: the markers and the armoured text). No new dependencies are expected. **Done:** [`satkeep-backup`](../crates/satkeep-backup/README.md) and [`backup-format.md`](backup-format.md); no new crates.
   - **Tests:** today's backup tests move with the code. New ones: the kit parser refuses changed, truncated or mismatched kits (plain setup ≠ locked setup); the vectors decode. A fuzz target for the kit parser.
3. **The tool**, with the simulated device: unlock, the PHONE check, Bitcoin Core RPC import and file export, the Liana descriptor and the `--show-phrase` export.
   - **Works / forbidden tests:** the right kit and device restore and spend on regtest. The wrong device, a wrong PIN, a changed kit, a swapped backup, mainnet, and a key on the command line are each refused. No key material appears in the output (the CLI's existing output checks).
   - **The heir's route:** with HEIR's key and the kit's plain setup, the tool marks the setup as unverified, and exports only the wallets that contain HEIR's key.
   - **Dependencies:** reading the kit needs no PDF library (section 4). The Core import can use the RPC client the CLI already has. Anything new gets its reason and a `cargo deny` pass in that PR.
   - **Status:** split into two PRs.
     - **The first** ([`satkeep-restore`](../crates/satkeep-restore/README.md)) is unlock and export: `show`, `core` (RPC import or `--out` file) and `liana` (with `--show-phrase`), with the works and forbidden tests on regtest.
     - **Its dependencies:** `bitcoin`, `miniscript`, `bitcoincore-rpc`, `clap`, `serde_json` and `anyhow`, approved 2026-10-24. All were already in `Cargo.lock`, and there's no `bdk_wallet`.
     - **Mainnet:** the format and the node check both refuse it. There's no mainnet node to test against.
     - **The second** is the heir's route: `satkeep-restore heir`.
       - **Started by** HEIR's 12 words from a key file or stdin, plus the kit. No second key.
       - **Reads only the plain setup,** marked unverified, and exports for Liana only the wallets that hold HEIR's key.
       - **Shows each heir delay,** and that only coins that haven't moved for that long can be moved.
       - **The owner decided this** on 2026-10-24.
4. **The exit drill.** Make a kit with the prototype, restore it with the tool, import into Liana and Bitcoin Core, and spend 2 of 3 with PHONE (from the tool) and DEVICE, in both Savings shapes. Then run the key-material scan.
5. **The second-phone exchange** (D4): the tool's side of the offline request and answer, against a simulated second phone. The app's side comes with the app.
   - **Status:** done. The format is [`backup-format.md`](backup-format.md) section 3.4, with P-256 test vectors. `satkeep-restore` unlocks a backup locked to a second phone in every build, through the request it shows and the answer typed back.
   - **The dependency:** `p256` 0.14, approved 2026-10-24, only for locking a backup to a phone and for the simulated phone. The tool's side is hashing only, so its release builds don't include `p256`.
   - **Before it,** RustCrypto moved to its current generation (PR #25), so `p256` 0.14 sits on one generation.
6. **Trezor support** (D6): CipherKeyValue through the chosen crate or `trezorctl`, tested against the emulator in CI. This needs your OK for the emulator download and any new dependency.

The Satkeep card's unlock (FIDO `hmac-secret`) waits for the card's design. The everyday pocket's export waits for its descriptor. The format leaves room for both.
