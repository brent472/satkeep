# Phone key backup: research and proposed design

**Status: research for review. No code yet.** The prototype described at the end starts only after this is approved.

The requirements come from the master brief v3.6 (private repo `brent472/twofold-private`):
- Section 04, *Phone key backup* and *Threats to address*
- Section 13, the two open decisions on the backup
- the Section 19 screens: *Onboarding: back up your phone key*, *Restore a wallet*, *Protection*, *Recover my account* and *Settings*

This document doesn't reproduce the brief's text. Where it says a screen "promises" or "shows" something, it paraphrases what the copy requires the design to do.

Everything here is on test networks only, like the rest of Twofold. Sources are linked inline. Anything marked **unconfirmed** wasn't verified against a primary source; treat it as a lead, not a fact.

## Contents

1. [What the design has to do](#1-what-the-design-has-to-do)
2. [Proposed design](#2-proposed-design)
3. [Which security devices can unlock a backup](#3-which-security-devices-can-unlock-a-backup)
4. [Encryption library](#4-encryption-library)
5. [The recovery contact: helping unlock without reading](#5-the-recovery-contact-helping-unlock-without-reading)
6. [Where the backup lives](#6-where-the-backup-lives)
7. [Threats](#7-threats)
8. [Decisions for you](#8-decisions-for-you)
9. [Prototype plan (after approval)](#9-prototype-plan-after-approval)

## 1. What the design has to do

From the brief, and from what the screen copy commits us to:

| # | Requirement | Comes from |
| --- | --- | --- |
| R1 | The phone key (its 12-word BIP39 phrase) is encrypted on the phone, and the ciphertext is stored in the person's personal cloud account (iCloud on iPhone, Google Drive on Android). There's nothing to write down in the main flow. | Section 04; backup onboarding screen |
| R2 | The cloud copy alone reveals nothing. Unlocking it needs the security device (the "second key"). | Section 04; backup onboarding screen |
| R3 | Twofold's servers (the helper) never hold the backup, and can't restore anyone's phone key. | Section 04; backup onboarding screen |
| R4 | Restore on a new phone needs only the cloud login used before plus the second key, with **no delay**, and it brings back both wallets. | *Restore a wallet*; *Recover my account*, lost phone |
| R5 | Before unlocking, the restore screen shows the date of the backup it found. | *Restore a wallet* |
| R6 | The wrong second key gets its own clear error. | *Restore a wallet* |
| R7 | The old phone is alerted when the phone key is restored somewhere else. | *Restore a wallet* |
| R8 | When the second key is replaced, the backup is locked to the replacement instead, and the retired device stops opening it. | Section 04; *Settings* |
| R9 | Lost phone **and** second key: the recovery contact assists in opening the backup but can never read the key themselves. A waiting period applies, with notifications to everyone involved. | Section 04; *Recover my account*, lost both; Section 13 |
| R10 | No second key yet: no backup, so the app asks for a second key first. | Backup onboarding screen; Section 13 |
| R11 | The backup is under 1 MB. | Backup onboarding screen, the "no space" error |
| R12 | An advanced, opt-in export shows the 12 words, from Settings only. | Section 04; *Settings*, Recovery kit |
| R13 | Unlocking the backup or signing must require the security device's own PIN or fingerprint, since a taken-over cloud account plus a stolen device would otherwise add up to 2 of 3. | Section 04, *Threats* |
| R14 | It must cover the four threats asked for: cloud account takeover, a stolen device, malware on the phone during restore, and a stale backup after the device is replaced. | Work queue |

R11 is easy: the backup is a few hundred bytes.

R1, R4, R5, R6 and R12 are covered by the design in section 2.

Four requirements need decisions from you:
- **R3 with R9:** a waiting period that's enforced, not just promised, needs something other than the contact to hold part of the unlock. See section 5.
- **R7:** a thief who has the cloud account and the second key can unlock without contacting us. So the alert is only reliable if unlocking needs a step that goes through the helper. See sections 5 and 7.
- **R8:** a retired device that stops opening the backup can't be guaranteed by deleting files. Old copies of the encrypted backup survive in cloud version history, trash and provider backups, or in an attacker's hands, and the old device still opens them. See section 7.3.
- **R2/R9 vs the copy:** the backup onboarding screen promises that only the second key can unlock the cloud copy. Any contact-assisted unlock (R9) is a second way in, so either the copy or R9 has to give. See section 8.

## 2. Proposed design

### 2.1 What's in the backup

One file, under 1 KB:

```text
header (plain, but authenticated):
  format version            1
  wallet id                 random 16 bytes, made when the wallet is created
  created                   date (shown on the restore screen, R5)
  lock entries, one per unlock path:
    kind                    "device" (later: "contact-split")
    device id               hash of the device's public identity at the unlock path (R6)
    unlock method           e.g. slip11-cipherkeyvalue, ecdh, msg-signature, hmac-secret
    method parameters       path, label or salt; ephemeral public key for ECDH
    wrapped data key        XChaCha20-Poly1305(KEK, data key), with its nonce
body:
  XChaCha20-Poly1305(data key, payload), with its nonce; header as associated data
payload:
  the phone key's 12 words
  the wallet's public setup: the Savings descriptor and delays (so Savings comes back too, R4)
```

**How the pieces fit:**
- **Data key:** a fresh random 32-byte key for every backup. It encrypts the payload, and it's what gets "locked" with each unlock path.
- **KEK:** each lock entry wraps the data key under a KEK. The KEK is `HKDF-SHA256(secret from the device, salt = wallet id, info = "twofold/phone-key-backup/v1" ‖ device id ‖ method)`.
  - This binds every wrap to this wallet, this device and this format, which Bitkey's own construction doesn't do ([`ssb.rs`](https://raw.githubusercontent.com/proto-at-block/bitkey/main/core/crypto/src/ssb.rs) uses an empty info and no associated data).
- **Associated data:** the whole header is authenticated, so changing the date, the device id or a lock entry makes decryption fail.

### 2.2 The four flows

- **Back up** (needs a second key, R10). The phone:
  1. makes a data key and encrypts the payload with it;
  2. asks the second key for its unlock secret (or, for an ECDH device, does the ECDH itself);
  3. wraps the data key and writes the file to the cloud.
  - Wiping memory: the phone wipes the data key and the secret. The phrase stays in the phone's own protected storage.
- **Restore** (R4–R6).
  1. Sign in to the cloud, then read the header.
  2. Show the backup's date.
  3. Check that the connected device's id matches a lock entry, so a wrong device gets the "wrong key" error before any unwrap.
  4. The device unlocks (PIN or fingerprint on the device), the KEK is derived, and the data key is unwrapped.
  5. The payload decrypts into the app's protected storage. It's never shown, never copied, never logged.
- **Replace the second key** (R8).
  1. Make a **new** data key and re-encrypt under it for the new device.
  2. Write the new file, delete the old one, and check that no old copy is left where the API lets us look.
  3. The old device's wrap is useless against the new file. Old copies are another matter (section 7.3).
- **Check-in** (every 6 months).
  - The check-in already needs two keys. When the second key is one of them, the app also unlocks the backup's header test value, which proves the backup still opens.
  - That catches a device whose firmware changed how it answers (section 3.2) before it's needed.

### 2.3 How this maps to the screens

| Screen | What the design does |
| --- | --- |
| Backup onboarding | Back up flow. With no second key, the app stops and asks for one (R10). Cloud not signed in and no space are reported before anything is written. |
| *Restore a wallet* | Finds the file, shows its date from the header, and checks the device id first, so "wrong key" is its own error. |
| *Restore a wallet*, alert to the old phone | Restoring registers the new phone with the helper (it has to, to co-sign Savings), and the helper pushes the alert to the old phone. A thief who skips the app skips the alert; see section 7.1. |
| *Protection* row for the backup | The file's date, and which device locks it. |
| *Recover my account*, lost phone | Restore flow, with no delay. |
| *Recover my account*, lost both | Section 5. |
| *Settings*, recovery kit | The opt-in written backup shows the 12 words from the phone's own storage, never from the cloud. |
| *Settings*, new second key | Replace flow. See section 7.3 for what the promise about the old key can honestly mean. |

## 3. Which security devices can unlock a backup

### 3.1 What a device must be able to do

The device has to give the phone a secret that:
- only it can produce,
- comes out the same every time, and
- comes only after its own PIN or fingerprint (R13).

There are four ways a device can do this:

| Mechanism | How it gives a secret | Notes |
| --- | --- | --- |
| **(a) Device-side encrypt/decrypt** | e.g. Trezor's [SLIP-0011 CipherKeyValue](https://github.com/satoshilabs/slips/blob/master/slip-0011.md): an AES key derived on the device from a path and a label | Clean: the output isn't something that's ever published |
| **(b) ECDH** | The device multiplies a host-supplied public key by its private key ([SLIP-0017](https://github.com/satoshilabs/slips/blob/master/slip-0017.md)) | Clean; the device confirms on screen |
| **(c) A deterministic message signature fed into HKDF** | Sign a fixed, Twofold-specific message with RFC 6979 ECDSA | Works widely but has real caveats (3.2) |
| **(d) Wrapping keys on a custom card** | Seal/unseal (Bitkey), or a FIDO `hmac-secret`-style secret | What the Twofold card should do (3.3) |

### 3.2 The survey

Verdicts are from vendor docs, specs and firmware source. **No hardware was tested.**

| Device | Mechanism | On-device check | Phone transport | Verdict |
| --- | --- | --- | --- | --- |
| **Old phone or tablet with the Twofold app** | (b) or (d): a non-exportable P-256 key in the Secure Enclave or Android Keystore, needing biometrics, used for ECDH ([Apple](https://developer.apple.com/documentation/security/protecting-keys-with-the-secure-enclave), [Android](https://developer.android.com/privacy-and-security/keystore)) | Its own biometric or passcode | Network, between our own apps | **Works.** Our code at both ends. |
| **Future Twofold card** | (d) Design it for this: FIDO CTAP2 [`hmac-secret`](https://fidoalliance.org/specs/fido-v2.1-ps-20210615/fido-client-to-authenticator-protocol-v2.1-ps-errata-20220621.html#sctn-hmac-secret-extension) / PRF, or HPKE ([RFC 9180](https://www.rfc-editor.org/rfc/rfc9180)) decapsulation, behind the fingerprint | Fingerprint, with retry limits | NFC | **Works by design** (3.3) |
| **Trezor Safe 7** | (a) CipherKeyValue with `ask_on_encrypt`/`ask_on_decrypt` set, on a dedicated path and label ([source](https://github.com/trezor/trezor-firmware/blob/main/core/src/apps/misc/cipher_key_value.py)); (b) as a fallback | PIN; confirms the label on screen | Bluetooth on iOS and Android ([Trezor](https://trezor.io/guides/trezor-devices/trezor-fundamentals/trezor-bluetooth-connections)) | **Works with caveats:** whether a third-party app can pair over Trezor's Bluetooth protocol and make this call is **unconfirmed** |
| **Trezor Model One, Model T, Safe 3, Safe 5** | (a), as above ([Model One](https://github.com/trezor/trezor-firmware/blob/main/legacy/firmware/fsm_msg_crypto.h)) | PIN | USB only: works on Android, not on iPhone | **Works with caveats:** Android only |
| **Blockstream Jade / Jade Plus** | (c) `sign_message` without anti-exfil (RFC 6979); or BIP-85 entropy encrypted to the phone ([Jade API](https://github.com/Blockstream/Jade/blob/master/docs/index.rst)) | PIN; confirms | Bluetooth on iOS and Android; USB on Android | **Works with caveats:** unlocking a Jade goes through its blind PIN server (Blockstream's by default), so restore needs the network. Its ECDH call is P-256 for ssh/gpg identities only, so it doesn't fit. |
| **Coldcard Mk4 / Q** | (c) message signing, which is deterministic ([`msgsign.py`](https://github.com/Coldcard/firmware/blob/master/shared/msgsign.py) → [libngu](https://github.com/switck/libngu/blob/master/ngu/k1.c) → libsecp256k1's RFC 6979 default) | PIN; confirms | NFC, where the user starts NFC Tools on the device ([docs](https://coldcard.com/docs/nfc-tools/)); QR on the Q; USB on Android | **Works with caveats:** clunky, user-driven flow, and the iPhone NFC round-trip is **unconfirmed**. HSM mode must be off, since it can allow signing without approval ([docs](https://coldcard.com/docs/hsm/rules/)). The duress ("delta") PIN gives invalid signatures, which fails safely. |
| **BitBox02 (Nova for iPhone)** | (c) message signing without anti-klepto ([`signmsg.rs`](https://github.com/BitBoxSwiss/bitbox02-firmware/blob/master/src/rust/bitbox02-rust/src/hww/api/bitcoin/signmsg.rs)) | PIN; shows the message | USB-C on Android; Bluetooth on iPhone (Nova only) | **Works with caveats:** whether its signatures repeat is **unconfirmed** (its code notes it "differs from plain RFC6979"). Only standard address paths. Avoid its BIP-85 Lightning export, which is the same secret as the BitBoxApp's Lightning wallet. |
| **Ledger (Nano X, Stax, Flex)** | (c) message signing in the Bitcoin app, RFC 6979 ([`crypto.c`](https://github.com/LedgerHQ/app-bitcoin-new/blob/develop/src/crypto.c)) | PIN; confirms | Bluetooth; USB on Android | **Works with caveats:** closed OS, so a future change could break repeatability. Nano S Plus: Android only. |
| **TAPSIGNER** | Only `derive` at a dedicated hardened path, using the returned chain code as the secret ([protocol](https://github.com/coinkite/coinkite-tap-proto/blob/master/docs/protocol.md)) | 6-digit code typed **on the phone**; slows down after wrong guesses but never wipes | NFC | **Works with caveats, weak:** the secret is only as private as an xpub (any app that reads that path learns it), and `derive` moves the card's signing path, so it has to be moved back. Signing can't be used: the card's signatures are deliberately random. |
| **SATSCARD** | none | none (bearer card, no PIN) | NFC | **Can't** |
| **Bitkey hardware** | Its own seal/unseal (AES-GCM under a secure-element key) | Fingerprint | NFC | **Can't for us:** no third-party API (**unconfirmed**). It's the reference pattern ([firmware](https://github.com/proto-at-block/bitkey/blob/main/firmware/lib/wallet/src/wallet.c)). |

**Mechanism (c), a signature used as a key, carries risks that (a), (b) and (d) don't:**
- **It's built to be published.** A Bitcoin signed message is meant for proving ownership, so anyone who ever sees that one signature can open the backup.
  - Use a path that's used for nothing else.
  - Use an unmistakable message, e.g. "Twofold phone key backup unlock v1, wallet ‹id›", and never sign it for any other reason.
- **Malware with the unlocked device can ask for it.** On-device confirmation is the only defence, and the message text has to make the request obvious.
- **Repeatability is today's firmware behaviour, not a promise.** A firmware update that randomises signatures would lock the backup for good. Mitigations:
  - At setup: sign twice and compare; verify the signature against the known public key and normalise it to low-S compact form before HKDF.
  - Stay on legacy ECDSA, not Taproot/BIP-340, which adds randomness.
  - At every check-in: unlock-test the backup (2.2).
- A signed message can't be turned into a transaction signature, because of the message prefix.

**What we do for devices that can't** (SATSCARD, Bitkey, and in practice TAPSIGNER if we judge it too weak):
- The device can still be a Savings key. It just can't lock the backup.
- At setup, the app says so and offers the options that can lock it: an old phone or tablet, or the Twofold card.
- If the person has neither, the backup isn't made, as with no second key (R10).
- ⚠️ **Section 19 has no copy for "your second key can't lock a backup."** It needs a line in the brief before the app can say it; I haven't invented one.

### 3.3 The Twofold card

The survey points to two good options:
- **FIDO CTAP2 `hmac-secret` (WebAuthn PRF).** The card keeps a per-credential secret and returns HMAC-SHA-256(secret, salt) only after user verification. It's repeatable, and it's a published standard with phone support ([Yubico on PRF](https://developers.yubico.com/WebAuthn/Concepts/PRF_Extension/); whether iOS exposes it for NFC keys is **unconfirmed**).
- **HPKE decapsulation** with a key that can't leave the card. This has one advantage: backing up needs only the card's public key, so it works without the card present.

Either way the card needs:
- attestation that the key lives on the card;
- a secure channel over NFC;
- limited fingerprint and PIN retries;
- no fingerprint enrolment without an existing fingerprint or PIN.

## 4. Encryption library

**Recommendation:** RustCrypto [`chacha20poly1305`](https://docs.rs/chacha20poly1305), using `XChaCha20Poly1305`, with [`hkdf`](https://docs.rs/hkdf) and [`sha2`](https://docs.rs/sha2) for key derivation. No hand-rolled crypto: the only construction of our own is "derive a key with HKDF, encrypt with an AEAD", with every input labelled.

| Option | Audit | Fit |
| --- | --- | --- |
| **`chacha20poly1305` (XChaCha20-Poly1305)** | [NCC Group, 2020](https://www.nccgroup.com/research/public-report-rustcrypto-aesgcm-and-chacha20pluspoly1305-implementation-review/): no significant findings. Short (5 days) and on older versions, so treat it as thin. | Random 24-byte nonces remove the risk of reusing a nonce. Bitkey uses the same primitive in production ([`ssb.rs`](https://raw.githubusercontent.com/proto-at-block/bitkey/main/core/crypto/src/ssb.rs)). |
| `age` / rage | No audit found ([spec](https://github.com/C2SP/C2SP/blob/main/age.md)) | Built around X25519 and password recipients, which our hardware unlocks don't fit. In a multi-recipient file, any recipient can forge the contents for the others ([Filippo Valsorda](https://words.filippo.io/age-authentication/)). |
| `hpke` | "Nobody has performed a paid audit" ([docs](https://docs.rs/crate/hpke/latest)) | Only needed for an HPKE card; revisit then. |

All three would be new dependencies, so the prototype PR will ask for them with reasons, and `cargo deny` must pass.

## 5. The recovery contact: helping unlock without reading

**The problem.** If the phone and the second key are both lost, the brief wants the recovery contact to help unlock the backup:
- without ever being able to read it,
- after a wait, and
- with everyone notified.

The hard part is the wait. Bitkey's version doesn't carry over:
- **Bitkey:** any one trusted contact can decrypt the backup **immediately** ([recovery design](https://bitkey.world/blog/sharing-our-recovery-design); [`RelationshipsCrypto.kt`](https://raw.githubusercontent.com/proto-at-block/bitkey/main/app/domain/relationships/public/src/commonMain/kotlin/build/wallet/relationships/RelationshipsCrypto.kt)). Its delay applies only to replacing the hardware key. That's safe for Bitkey, because its app key alone can't spend.
- **Twofold:** our phone key alone *is* the Spending wallet. So the delay has to come **before** the backup opens.

**The options:**

| | A. Contact share + helper share released after the wait (Apple-style) | B. Shamir 2-of-3 {device, contact, password} | C. No backup unlock; recover through Savings |
| --- | --- | --- | --- |
| **How** | The data key is wrapped under a key split in two. The contact's app holds one half. The helper holds the other and releases it only after the wait, the notifications, and a code exchanged between the contact and the owner. This is how [Apple's Account Recovery Contact](https://support.apple.com/guide/security/secafa525057/web) works. | The data key is split 2-of-3 between the device, the contact, and a password only the owner knows. Contact + password unlocks. | Lost both recovers Savings through a policy path that includes the recovery contact. Spending is lost unless it was emptied into Savings earlier. |
| **Contact alone** | Nothing | Nothing | Nothing |
| **Contact + cloud thief** | Nothing until the wait ends, and the owner is alerted and can stop it | Nothing without the password | n/a |
| **Helper alone** | Nothing: never has the backup or the contact's half | n/a | n/a |
| **Is the wait enforced?** | **Yes:** the helper holds back its half | **No:** only the contact's app delays, which is a promise, not a guarantee | Yes, on-chain (timelock) |
| **Replacing the contact** | New split; the helper deletes its old half, which kills the old contact's half even if old files survive | Re-split and re-encrypt; old copies still open with old shares | New Savings wallet |
| **Fits R3 (no unlock key on our servers)?** | Only if that means "never enough to unlock". The helper holds half a key, alone useless. | Yes | Yes |
| **Fits the lost-both screen?** | Yes | Partly: the password is new to the owner, and nothing enforces the wait | Partly: Savings only |
| **Complexity** | Highest: an invite ceremony and a short-code channel. Bitkey's [August 2026 enrolment bug](https://bitkey.world/blog/relationship-enrollment-bug-in-bitkey) shows the code must come from a secure random source and be single-use. | Medium. No audited Rust Shamir crate found: [`sharks`](https://rustsec.org/advisories/RUSTSEC-2024-0398) has an unfixed advisory, and [`vsss-rs`](https://github.com/mikelodder7/vsss-rs) publishes no report. | Policy work, not backup work |

**Recommendation: A.** It's the only one where the wait is enforced, and where the contact plus someone who took over the owner's cloud account still can't open the backup early. It needs your call on the "servers never hold the unlock key" rule (decision D2).

Two points either way:
- **It doesn't block the prototype.** Section 9 builds the device path only, and leaves a lock entry kind for this.
- **The contact isn't in the Savings policy yet.** The brief's role table says the recovery contact must be in the policy from the start. Milestone 1's Savings policy has no contact key, so that's separate policy work for the inheritance design.

## 6. Where the backup lives

| | iPhone | Android |
| --- | --- | --- |
| **Proposed** | CloudKit private database, the backup in an `encryptedValues` field ([Apple](https://developer.apple.com/documentation/cloudkit/ckrecord/encryptedvalues)). It survives deleting the app, and isn't a visible file. | Google Drive's [`appDataFolder`](https://developers.google.com/workspace/drive/api/guides/appdata): hidden, can't be trashed or shared, and needs only the `drive.appdata` permission. |
| **What others do** | Bitkey: the iCloud key-value store, moving to CloudKit ([Bitkey README](https://raw.githubusercontent.com/proto-at-block/bitkey/main/app/domain/cloud-backup/README.md)) | Bitkey: app-specific Drive storage (from the search snippet of [Bitkey's recovery paper](https://bitkey.build/content/files/2025/06/Bitkey-Recovery-Features.pdf), **unconfirmed**) |
| **Encryption by the provider** | End-to-end only with Advanced Data Protection on ([Apple](https://support.apple.com/en-us/102651)). We don't rely on it: our own encryption is the barrier. | Drive files aren't end-to-end encrypted. Same: we don't rely on it. |
| **Not used** | The iCloud key-value store ("don't store … sensitive information", [Apple](https://developer.apple.com/documentation/foundation/nsubiquitouskeyvaluestore)). iCloud Keychain sync: end-to-end, but it ties the backup to the device's own sync, with no published size limit (**unconfirmed**). | Block Store: 4 KB per entry ([Android](https://developer.android.com/identity/block-store)), which is enough for a wrapped key but not a clean fit. Auto Backup: device-wide, and keeps only the latest copy ([Android](https://developer.android.com/identity/data/autobackup)). |

Two rules for the phone's own storage:
- **The phrase stays out of device backups.** On Android, exclude it with `dataExtractionRules` ([Android](https://developer.android.com/identity/data/autobackup)). On iOS, keep it in the keychain with a "this device only" setting.
- **The cloud backup is the only copy that leaves the phone.**

## 7. Threats

### 7.1 Cloud account takeover

**How it happens:** SIM swaps and phishing take over cloud accounts ([FBI](https://www.ic3.gov/PSA/2022/PSA220208)). In the [Retool breach](https://www.bleepingcomputer.com/news/security/retool-blames-breach-on-google-authenticator-mfa-cloud-sync-feature/), secrets synced to a cloud account turned a phishing text into a theft.

**Takeover alone:**
- The attacker gets only ciphertext (R2).
- Our encryption is the only barrier, so the data key and every KEK are full-strength random or device-derived secrets. Nothing is derived from a guessable password.

**Takeover + stolen second key:** this is the brief's named threat. Together they open the backup: the phone key plus the device makes 2 of 3 in Savings, and Spending in full.

| Defence | What it does |
| --- | --- |
| **The device's own PIN or fingerprint (R13)** | **Required.** Section 3 marks devices without one as "can't" (SATSCARD). TAPSIGNER's code is typed on the phone and never wipes, which is why it's marked weak. |
| **An alert to the old phone (R7)** | Only fires if the attacker uses our app. A thief who reads the cloud file with their own code and unlocks the device directly sends no alert. Making the alert unskippable means putting a helper-gated step in the unlock itself, the same mechanism as option A, applied to every restore. That would add a wait to restores, which the copy rules out ("no waiting"). |
| **Savings is still protected by the helper** | After the attacker has 2 of 3, the helper co-signs according to its limits and holds. But phone + device don't need the helper at all. |

**Conclusion:** for this threat, the device PIN is the real defence, and the alert is best-effort. I recommend we say so plainly in the threat model rather than claim more.

### 7.2 Stolen security device

- **The device alone:** it can't open anything without the cloud file. It can't sign or unlock without its own PIN or fingerprint (R13).
- **Duress:** a Coldcard's duress PIN gives invalid signatures, so a forced unlock fails (3.2).
- **What we require:** for message-signing devices, HSM or auto-approve modes must be off.

### 7.3 Stale backup after device replacement (R8)

**The problem.** Replacing the second key writes a new file under a new data key. But the old device still opens **any old copy of the old file**, and old copies survive:
- **Google Drive** keeps old revisions of binary files for about 30 days ([Drive](https://developers.google.com/workspace/drive/api/guides/manage-revisions)). An app can delete some of them through the API ([revisions.delete](https://developers.google.com/workspace/drive/api/reference/rest/v3/revisions/delete)); whether `appDataFolder` files keep revisions at all is **unconfirmed**.
- **Google** keeps deleted data in its backups for up to about 6 months ([Google](https://policies.google.com/technologies/retention)).
- **iCloud Drive** has a 30-day Recently Deleted and data recovery ([Apple](https://support.apple.com/guide/icloud/recover-files-mmae56ea1ca5/icloud)). Retention for CloudKit records is **unconfirmed**.
- **Anyone who copied the file before** keeps their copy.

**What we can do:**
- Always write a new file, then delete the old one.
- Delete revisions where the API allows, then list what's left and check.
- Still, the promise that the old key stops working only holds for files we control.

**When that matters:**

| Why the device is replaced | Risk | Proposed handling |
| --- | --- | --- |
| Upgrade, old device in hand | Low: the old device is reset | Re-encrypt and delete, as above |
| **Lost or stolen device** | Old device + any old file copy = the phone key | **Re-encrypt and also rotate the phone key** (see below) |

**How the rotation works:**
- *Recover my account* already moves Savings to a new wallet when the card is lost, with one network fee.
- Rotating the phone key means a new Spending wallet and a new PHONE key in that new Savings wallet.
- Both sweeps can go in **the same transaction**: Spending's coins (phone key) and Savings' coins (phone + helper) as inputs, the new wallets as outputs. So it's still one network fee.

**Alternatives:**
- **A helper-held revocable share** (like option A) would make revocation real without rotating. But it adds a helper step, and a wait, to every restore.
- **Accept the gap and say so.**

Decision D3.

### 7.4 Malware on the phone during restore

**What can't be avoided:**
- The phone's hardware key stores can't hold the bitcoin key itself. The Secure Enclave works only with P-256 ([Apple](https://developer.apple.com/documentation/security/protecting-keys-with-the-secure-enclave)), and StrongBox lists no secp256k1 ([Android](https://developer.android.com/privacy-and-security/keystore)).
- So the phrase is in the app's memory when it signs, and malware with that level of control on the new phone gets the key.
- Android banking trojans already read recovery phrases off the screen through accessibility services ([SC Media](https://www.scworld.com/news/android-trojan-crocodilus-hijacks-accessibility-settings-for-control)).

**What we do:**
- Restore happens entirely inside the app, decrypting straight into protected storage. The phrase is never shown in the main flow and never goes on the clipboard.
- **Android:**
  - `FLAG_SECURE` and `HIDE_OVERLAY_WINDOWS` on these screens ([Android](https://developer.android.com/security/fraud-prevention/activities)).
  - Marking views as sensitive to accessibility (API 34).
  - Play Integrity's app-access risk verdict, to warn when apps that capture the screen or misuse accessibility are running ([Android](https://developer.android.com/google/play/integrity/overview)).
- **iOS:** App Attest, so the helper knows it's talking to a real copy of the app. Its keys don't survive reinstall or restore, which also signals a new install for the alert ([Apple](https://developer.apple.com/documentation/devicecheck/establishing-your-app-s-integrity)).
- **Any device that shows its request on its own screen** (Trezor, Jade, Coldcard, BitBox02, Ledger) stops malware from quietly using it for something else. The message must say plainly that it's a Twofold backup unlock.

**What this can't do:** none of it stops malware that already controls the new phone. The written backup in Settings is the same risk: the copy already warns against typing or photographing the words.

## 8. Decisions for you

| # | Decision | Options | My recommendation |
| --- | --- | --- | --- |
| **D1** | Which devices to support at launch for locking the backup | Old phone/tablet and the Twofold card (works); Trezor and Ledger (works with caveats); Coldcard, Jade, BitBox02 (works with caveats, mechanism (c)); TAPSIGNER (weak) | Start with **old phone/tablet** and **Trezor (CipherKeyValue)**, the cleanest mechanisms. Then add message-signing devices with the setup and check-in tests in 3.2. Leave TAPSIGNER out of backup locking, and let the card be designed for it. |
| **D2** | The rule that our servers hold neither the backup nor its unlock key (R3): may the helper hold **half** of a key, alone useless, for the lost-both path? | Yes, option A / no, option B or C | **Yes, option A.** It's the only enforced wait. The helper still never holds the backup or anything that unlocks it alone. |
| **D3** | What replacing a lost or stolen second key does | Re-encrypt only / re-encrypt + rotate the phone key in the same Savings move / helper-held revocable share | **Re-encrypt + rotate**, one transaction, one fee. Re-encrypt only for an upgrade with the old device in hand. |
| **D4** | Copy: the backup onboarding screen says only the second key unlocks the cloud copy | Keep R9 (contact-assisted unlock) and adjust the copy / keep the copy and drop R9 for the backup (option C) | Keep R9 and adjust the copy. That's your edit in Section 19; I won't change product wording. |
| **D5** | Copy for a second key that can't lock a backup (3.2) | New line in Section 19 | Needed before the app ships that case |
| **D6** | Section 13's open decision on Spending-only users | Second key first (current copy) / password-locked (weaker) / recovery contact | **Keep "second key first."** An old phone counts as a second key, so nobody has to buy anything. |
| **D7** | The written backup's restore path | Settings only, as the copy says | Agree; nothing to change |

## 9. Prototype plan (after approval)

A `backup` module, with the security device simulated in software, on test networks only.

**Simulated device:**
- It holds a random secret, generated in each test.
- It answers only after a simulated PIN, and refuses after too many wrong tries.
- It implements the **(a) device-side encrypt/decrypt** method, the cleanest one.
- A second method can be added later behind the same trait.

**Contents:**
- The file format in 2.1 (header with associated data, data key, per-device lock entry). The contact-split entry is reserved but not implemented.
- Backup, restore and replace-device functions, with a fake "cloud" folder that keeps old versions, like the real ones do.

**Tests, each with the working case and the forbidden one:**

| Case | Works | Forbidden version fails |
| --- | --- | --- |
| The backup alone reveals nothing | The file decrypts with the device | Without the device: no plaintext, and no phrase words anywhere in the file |
| Restore needs backup + device | Right device + PIN → the same phone key | Wrong device → "wrong key"; no device → refused; wrong PIN → refused, then locked |
| Replacing the device re-encrypts | The new device opens the new file | The old device can't open the new file. The test also shows the old device **still opens an old copy**, as an explicit test of the limit in 7.3, so the limit is pinned rather than hidden. |
| Tampering | Unchanged file opens | Any change to the header (date, device id) or body → refused |
| The restored key signs | The restored phone key co-signs a regtest 2-of-3 Savings spend with the helper's key | A key from a different backup can't |
| The backup holds the BIP39 phrase | The restored phrase is the same 12 words as `HolderKey` | n/a |
| No key material in logs, errors or test output | n/a | Errors never repeat input, and tests check output for phrase words, as the CLI tests do |

**New dependencies, to approve then:** `chacha20poly1305`, `hkdf`, `sha2`. Each will have its reason and a `cargo deny` pass in the PR.
