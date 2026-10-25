# Backup and exit kit format, version 1

This is the byte-level spec of the phone key backup, the wallet setup it carries, and the exit kit that holds both. If the restore tool's code is ever lost, anyone can rebuild it from this page. The fixed test vectors at the end show they got it right. The code is the [`satkeep-backup`](../crates/satkeep-backup/README.md) crate, and its tests check the vectors on this page, so the two can't drift apart.

**Status:** test networks only, and not yet reviewed by an outside cryptographer. The design and its reasons are in [`phone-key-backup.md`](phone-key-backup.md) (section 2) and [`restore-tool.md`](restore-tool.md) (sections 4 and 7). Anything here can change before launch; a change that breaks old files gets a new version number.

## 1. The three layers

| Layer | What it is | Marker |
| --- | --- | --- |
| **Backup file** | The phone key's 12 words and the wallet setup, encrypted, with a readable header naming the date and the devices that can unlock it | `SKBK` |
| **Wallet setup** | Text: the network and every wallet's public descriptor, birthday and delays | `satkeep wallet setup 1` |
| **Exit kit data** | The backup file plus a plain copy of the wallet setup, as armoured text with a check on every line | `SKKT`, inside `-----BEGIN SATKEEP EXIT KIT-----` |

## 2. Conventions

- **Integers** are unsigned and big-endian. `u16 length + bytes` means a 2-byte length, then that many bytes.
- **Hex** is written lowercase and read in either case.
- **Primitives:**
  - HKDF-SHA256, as in [RFC 5869](https://www.rfc-editor.org/rfc/rfc5869);
  - XChaCha20-Poly1305 with a 24-byte nonce and a 16-byte tag (the [draft-irtf-cfrg-xchacha](https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha) construction, as in libsodium and RustCrypto's `chacha20poly1305`);
  - SHA-256;
  - English BIP39 phrases.
- **Random values:** the data key, both nonces and the wallet id come from the operating system's generator. A new backup always gets a new data key and new nonces, even for the same phrase and device.
- **Readers refuse** anything that doesn't fit exactly, and never repeat their input in an error.

## 3. The backup file

```text
header (authenticated as the body's associated data):
  magic          "SKBK"
  version        u8 = 1
  wallet id      16 bytes
  created        u64, Unix seconds
  entry count    u8, 1 to 8
  each entry:
    kind         u8: 1 = device; 2 = recovery contact (reserved)
    device id    32 bytes
    method       u8: 1 = CipherKeyValue; 2 = second phone
    request      u16 length + bytes, at most 256
    wrap nonce   24 bytes
    wrapped key  48 bytes (the 32-byte data key + a 16-byte tag)
body:
  nonce          24 bytes
  ciphertext     u32 length + bytes, at least 16 (the tag)
```

- **Nothing may follow the body.** A file over 64 KiB is refused.
- **Unknown values are refused:** a kind or method other than those above, or a version other than 1 (reported as "made by a newer version").
- **The header is readable without unlocking.** It gives the date and which devices can unlock the backup, and nothing that helps decrypt it.

### 3.1 Making a backup

For one device, with the wallet id `W`, the device id `D` and the method `M`:

1. `request` = the ASCII text `Satkeep phone key backup unlock v1, wallet ` followed by `W` in lowercase hex. A device with a screen shows it.
2. Ask the device for its 32-byte **unlock secret** `S` for `request`, after its own PIN or fingerprint.
3. `KEK` = HKDF-SHA256 with:
   - input key material `S`;
   - salt `W`;
   - info = `satkeep/phone-key-backup/v1/kek` ‖ `M` (1 byte) ‖ `D`;
   - length 32.
4. `wrap AAD` = `satkeep/phone-key-backup/v1/wrap` ‖ `W` ‖ kind (1 byte) ‖ `D` ‖ `M` (1 byte) ‖ `request`.
5. `wrapped key` = XChaCha20-Poly1305(key `KEK`, wrap nonce, plaintext = the data key, associated data = wrap AAD).
6. `payload` = the phrase (u16 length + UTF-8), then the wallet setup text (u32 length + UTF-8).
   - **The phrase** is 12 lowercase English BIP39 words with a valid checksum, separated by single spaces.
   - **The setup** is in its one text form (section 4).
7. `ciphertext` = XChaCha20-Poly1305(key = the data key, body nonce, plaintext = payload, associated data = the header's exact bytes).

### 3.2 Opening a backup

1. **Parse** the file as in section 3.
2. **Find the entry** whose kind is 1 (device) and whose device id and method match the connected device. If none does, stop with "wrong key" **before asking the device anything**.
   - **A second phone** isn't connected, so its entry is the backup's one method-2 entry, and the phone itself checks the id (3.4).
3. **Check the request.** Otherwise stop, without asking the device:
   - **Method 1:** it must equal the request rebuilt from `W` in step 1 of 3.1. A changed file must never get a device to derive a key for another label.
   - **Method 2:** it must be 33 bytes starting with `02` or `03`, the shape of a compressed point (3.4).
4. **Unlock:** ask the device for `S`, derive the KEK, and open the wrapped key. Then open the body with the header's exact bytes as associated data. Any failure means the backup is damaged or changed.
5. **Check the payload:**
   - it must split exactly into the two fields;
   - the phrase must be in its canonical form (12 words, as above);
   - the setup must read in its one text form.
   - Otherwise it's refused, as damaged.

**Wiping memory:** the data key, `S`, the KEK, the payload and the phrase are wiped as soon as they're no longer needed.

### 3.3 Devices

- **The device id** is 32 bytes that identify the device at its unlock path, readable without its PIN. How each kind of device computes it is fixed when that device is supported (Trezor: [`restore-tool.md`](restore-tool.md) section 5).
- **Method 1 (CipherKeyValue):** the device returns the same 32-byte secret for the same request, only after its own PIN, the way Trezor's [SLIP-0011](https://github.com/satoshilabs/slips/blob/master/slip-0011.md) CipherKeyValue works.
- **Method 2 (second phone):** ECDH with a phone's P-256 key, unlocked through an offline request and answer (3.4).
- **More methods later:** a message signature, FIDO `hmac-secret` for the card. Each gets its own method code when it's built.
- **Kind 2 (recovery contact)** is reserved. Readers parse it and never open it with a device.
- **The software device in the tests** (`SimulatedDevice`) isn't part of the format: it's a stand-in with a random secret. So is the simulated second phone (`SimulatedSecondPhone`), with a P-256 key in software standing in for the Secure Enclave's.

### 3.4 Method 2: the second phone

A second phone is an old phone or tablet with the Satkeep app (decision D4 in [`restore-tool.md`](restore-tool.md)). Its key is a P-256 key pair `(d, Q)` made in its Secure Enclave or Android Keystore, which never leaves the phone and works only after its own face, fingerprint or passcode. `Q` is written in SEC1 compressed form, 33 bytes.

**The device id:** `D` = SHA-256(`satkeep/second-phone/v1/id` ‖ `Q`).

**Locking,** on the owner's phone. It needs only `Q`, kept since pairing, so the second phone needn't be there:
1. Make a fresh ephemeral P-256 key pair `(e, E)`.
2. `Z` = the x-coordinate of `e·Q`, 32 bytes.
3. The **unlock secret** `S` = HKDF-SHA256 with:
   - input key material `Z`;
   - salt `W`;
   - info = `satkeep/second-phone/v1/unlock` ‖ `E` ‖ `Q`;
   - length 32.
4. Go on from step 3 of 3.1 with `S`, method `M` = 2, and `request` = `E`.
- `e`, `Z` and `S` are wiped once the wrap is made. Every backup gets a new `e`.

**Unlocking** is an offline exchange of text between the computer and the phone, with no servers:
1. **The request.** The tool makes a fresh random 32-byte session key `K` and shows these 122 bytes as 244 hex digits, in groups of 4, 8 groups a line:
   ```text
   magic        "SKRQ"
   version      u8 = 1
   wallet id    16 bytes (W)
   device id    32 bytes (D, from the entry)
   ephemeral    33 bytes (E, the entry's request)
   session key  32 bytes (K)
   check        the first 4 bytes of SHA-256 of all of the above
   ```
2. **The phone** reads it, scanned or typed; case, spaces and line breaks don't matter. Before asking for its face, fingerprint or passcode, it refuses, in this order:
   - a failed check: a typo;
   - a version other than 1;
   - a `D` that isn't its own id;
   - a wallet `W` it isn't the second key of.
3. **After** its face, fingerprint or passcode, the phone computes `Z` = the x-coordinate of `d·E`, refusing an `E` that isn't on the curve, then `S` as in locking.
4. **The warning,** required app behaviour. Before it shows the answer, the phone must show this warning, word for word, and show the answer only once the person has seen it:
   > Only do this if you're restoring your own Satkeep on a computer in front of you right now. Satkeep and your helper will never ask you to read this answer to anyone, by phone, message or email.
5. **The answer** is 36 bytes, shown as 72 hex digits in groups of 4:
   - `S` XOR `mask`, where `mask` = HKDF-SHA256 with input key material `K`, no salt, info `satkeep/second-phone/v1/answer-mask`, length 32;
   - then a check: the first 4 bytes of SHA-256(`satkeep/second-phone/v1/answer` ‖ the request's 122 bytes ‖ the masked secret).
6. **The tool** reads the typed answer the same way. A failed check means a typo, or an answer to another request, and it can be typed again. Then `S` = the masked secret XOR `mask`, and opening goes on from step 4 of 3.2.

**Why it's built this way:**
- **The mask:** the phone's screen never shows `S` itself. A photo of the answer is useless without that request's session key, which only the tool and the computer's screen have, and which is new for every request.
- **The answer's check covers the request,** so an old answer, or one to another request, is caught like a typo.
- **No elliptic-curve code in the tool.** It only hashes, and checks `E` has a compressed point's shape. Only the phone checks `E` is on the curve.
- **`S` and the backup file together open the backup,** so the tool wipes `K` and `S` once used and never prints the answer.
- **`S` stays the same for the life of a backup,** because `E` does. Anyone who saw both a request and its answer, on the computer's screen or in its scrollback, can work out `S` and keep opening that backup. A new backup gets a new `e`, so after any such exposure the app should make a new backup.
- **The phone can't tell who made a request.** Anyone with a copy of the backup file can make a valid one, and an answer to it opens the backup: the phone key, one of the three keys. Hence the warning in step 4, which no format check can replace. If the owner reports answering a request they didn't make, the app should make a new backup. If the backup was opened, the phone key itself is out, and a new backup doesn't change that ([`restore-tool.md`](restore-tool.md) threat 7.6).

## 4. The wallet setup

UTF-8 text with a `\n` after every line, including the last, and no other whitespace except the single space between a key and its value:

```text
satkeep wallet setup 1
network <regtest | signet>
[pocket                              the everyday pocket, at most one; reserved
 descriptor <descriptor>
 birthday <height>]
savings                              one or more, oldest first; the last is the current one
descriptor <descriptor>
birthday <height>
[heir-delay <blocks>                 all three, once a beneficiary is added
 checkin-interval <blocks>
 checkin-margin <blocks>
 [refresh-window <blocks>]]          the payment rule's window; absent when the rule is off
```

(The brackets and the notes on the right are only for this page; a real setup has no indentation and no comments.)

- **Each setup has one text form:** two setups are the same exactly when their texts are byte for byte.
- **Numbers** are decimal, with no sign and no leading zero.
  - A height is 0 to 4,294,967,295.
  - A delay in blocks is 1 to 65,535 (Bitcoin's relative-timelock limit).
- **Descriptors** are 1 to 4,096 bytes of printable ASCII with no spaces. Satkeep writes them as `satkeep export` does, with the checksum.
- **Limits:** at most 16 Savings wallets, and 16 KiB in all.
- **Networks:** only `regtest` and `signet`. Mainnet is refused until the launch review. Adding it later needs no new version, and older readers will refuse it, which is the safe way round.
- **No private keys.** The wallet setup never holds private keys; a setup with one is refused, so the kit's plain copy is always safe to keep in the cloud.
  - **How it's checked,** both when a setup is written and when it's read: each descriptor is split on `( ) , [ ] / # *`, and any piece that starts with `xprv` or `tprv`, or is 51 or 52 base58 characters (a WIF key), is refused.
  - **Why pieces:** a real `tpub` can contain the letters `prv` by chance, so the whole string isn't searched.
  - **No false alarms:** no public part of a descriptor is 51 or 52 base58 characters. Extended public keys are 111, and hex keys and hashes are 40, 64 or 66.
- **Otherwise only the form is checked here.** Whether a descriptor parses, holds the phone key, or has delays the Savings rules accept is checked by whatever uses it. The restore tool checks the phone key (restore-tool.md, T5), and the library builds the policy.
- **Earlier Savings wallets stay listed after a move,** because coins sent to them later stay there.
- **The pocket's descriptor isn't designed yet** (brief 3.11). The format has its place, and nothing makes one yet.

## 5. The exit kit

### 5.1 The kit data

```text
magic          "SKKT"
version        u8 = 1
backup         u32 length + bytes: the backup file (section 3), at most 64 KiB
setup          u32 length + bytes: the wallet setup text (section 4), at most 16 KiB
```

Nothing may follow. **The setup here is the plain copy:** it lets a viewer show the wallets without the second key. It isn't authenticated, so it must match the copy inside the backup (5.4).

### 5.2 The armoured text

```text
-----BEGIN SATKEEP EXIT KIT-----
0001 534b4b54 01000006 b8534b42 … (8 groups of 8 hex digits) <check>
0002 …
-----END SATKEEP EXIT KIT-----
```

- **Each line** is:
  - its number, from 1, written with at least 4 digits;
  - the kit data's next 32 bytes in hex, in groups of 4 bytes (the last line may be shorter);
  - an 8-digit check.
  - The parts are separated by spaces.
- **The check** is the first 4 bytes of SHA-256(`satkeep/exit-kit/v1/line` ‖ the line number as u32 ‖ the line's bytes).
  - It catches a typo or a missing line when the text is typed or scanned back in, and names the line.
  - **It's not a signature:** anyone can compute it.
- **When writing,** lines end with `\n`, and so does the END line.

### 5.3 Where it goes, and reading it back

- **Where the app puts it:** in two places in the kit PDF (restore-tool.md, D2):
  - an embedded attachment, **stored uncompressed**, so its bytes appear as they are in the PDF file;
  - printed on the PDF's last page.
- **What a reader accepts:**
  1. **A raw backup file** (it starts with `SKBK`). There's no plain setup then.
  2. **Any bytes holding the armoured block:** the PDF, a text file, or pasted text.
     - Every BEGIN…END pair is decoded.
     - **Tolerated:** surrounding text, `\r\n` line endings, extra spaces and blank lines, capital hex.
     - **Pairs that don't decode are skipped** if another one decodes. A PDF may draw the same markers as page text.
     - **If none decodes,** the first pair's error is reported: the number of the first line that's missing or fails its check, or "incomplete" if lines are missing from the end.
     - **Two pairs that decode to different kit data are refused.**
- **Input size:** a reader looks through at most 64 MiB. Inside a block, a line over 256 bytes is refused as damaged before it's split up; a written line is 89 bytes.

### 5.4 Restoring from a kit

1. **Open** the kit's backup as in 3.2.
2. **Compare** the setup from inside the backup with the kit's plain copy, if there is one. If they differ, refuse: the kit has been changed. Use only the copy from inside the backup.
3. **A swapped backup** (someone else's, behind this kit's setup) fails at step 2 of 3.2, or later at the restore tool's phone key check.

## 6. Test vectors

- **The phrase** is BIP39's published test phrase "abandon … about". It has never held funds, and must never be used for any.
- **The other fixed inputs** are simple byte runs.
- **The wallet setup** has two Savings wallets made from BIP39's other published test phrases:
  - PHONE: "abandon … about", fingerprint `73c5da0a`;
  - DEVICE: "legal winner … yellow", `b8688df1`;
  - HELPER: "letter advice … above", `28645006`;
  - HEIR: "zoo … wrong", `3f635a63`.
  - They are first the plain 2-of-3 wallet, then the Taproot wallet after adding a beneficiary, with the placeholder delays.
- **The device** answers the unlock request with `device-secret` directly.

**Inputs:**

<!-- vector: inputs -->
```text
wallet-id      000102030405060708090a0b0c0d0e0f
created        1792713600
device-id      a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf
device-secret  c0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedf
data-key       505152535455565758595a5b5c5d5e5f606162636465666768696a6b6c6d6e6f
wrap-nonce     101112131415161718191a1b1c1d1e1f2021222324252627
body-nonce     303132333435363738393a3b3c3d3e3f4041424344454647
```

`created` is 2026-10-23 00:00 UTC. The method is 1 (CipherKeyValue).

**The phrase:**

<!-- vector: phrase -->
```text
abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about
```

**The wallet setup** (the descriptors are long single lines):

<!-- vector: setup -->
```text
satkeep wallet setup 1
network regtest
savings
descriptor wsh(sortedmulti(2,[73c5da0a/87'/1'/0']tpubDCChhoz5Qdrkn7Z7KXawq6Ad6r3A4MUkCoVTqeWxfTkA6bHNJ3CHUEtALQdkNeixNz4446PcAmw4WKcj3mV2vb29H7sg9EPzbyCU1y2merw/<0;1>/*,[b8688df1/87'/1'/0']tpubDC44ZFebbFm5ZXnG5Xt8k9PpdSZXUQ4HyuU8e8u5zfutYuu8EW4MLKS54EFTfGYgeqt7P2jEEd1iCF5kSPi7Xrj6EZcsStCcAjPy97DSiLb/<0;1>/*,[28645006/87'/1'/0']tpubDCs6kWyAq2CsViTz5YR3tKpCQ4kq2xwDmERmF5cGet45CsHAqR8wbQGRin6fzLJgCdrKfYts4ddyTJ7Zo4ocmG1zHK3vw71mh32wj25WmsT/<0;1>/*))#2pv7mn6z
birthday 101
savings
descriptor tr(tpubD6NzVbkrYhZ4XJV7Yde1kE44iTPaVs9MQY3bovjpJKZMGVLAdtqBRsjQ2EVJqWUuF9uVAV5JBVH8XwZs8ofy5BGTvQTzrNRz6AVJMC1sNuN/<0;1>/*,{multi_a(2,[73c5da0a/87'/1'/0']tpubDCChhoz5Qdrkn7Z7KXawq6Ad6r3A4MUkCoVTqeWxfTkA6bHNJ3CHUEtALQdkNeixNz4446PcAmw4WKcj3mV2vb29H7sg9EPzbyCU1y2merw/<0;1>/*,[b8688df1/87'/1'/0']tpubDC44ZFebbFm5ZXnG5Xt8k9PpdSZXUQ4HyuU8e8u5zfutYuu8EW4MLKS54EFTfGYgeqt7P2jEEd1iCF5kSPi7Xrj6EZcsStCcAjPy97DSiLb/<0;1>/*,[28645006/87'/1'/0']tpubDCs6kWyAq2CsViTz5YR3tKpCQ4kq2xwDmERmF5cGet45CsHAqR8wbQGRin6fzLJgCdrKfYts4ddyTJ7Zo4ocmG1zHK3vw71mh32wj25WmsT/<0;1>/*),and_v(v:pk([3f635a63/87'/1'/0']tpubDC2ji5AZnYmXs3g3QeA5UEuN5na7z1z96GRRrRhtwKyxUGrtjgoHXNvzsjZ4AMNs91x92jQN4qhFUwZWNfvteyHMuXsma8D6dFmoe7im9kN/<0;1>/*),older(61320))})#4u5ny3ff
birthday 150
heir-delay 61320
checkin-interval 52560
checkin-margin 8760
refresh-window 8760
```

**Outputs.** The unlock request (3.1, step 1):

<!-- vector: unlock-request -->
```text
Satkeep phone key backup unlock v1, wallet 000102030405060708090a0b0c0d0e0f
```

The KEK (step 3):

<!-- vector: kek -->
```text
0961a48a7be169c03d24b795f5ee5dde6f18235bd8021940446084718d9385ff
```

The wrapped key (step 5):

<!-- vector: wrapped-key -->
```text
da1140e9f6e4779dbcfa32e538ccbc0cd97073d15f3ea4ddae29198d1a46a3b9
db477dccaea1862b3047c730e3493fd8
```

The backup file, 1,720 bytes, in hex:

<!-- vector: backup -->
```text
534b424b01000102030405060708090a0b0c0d0e0f000000006adaa3800101a0
a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf01
004b5361746b6565702070686f6e65206b6579206261636b757020756e6c6f63
6b2076312c2077616c6c65742030303031303230333034303530363037303830
39306130623063306430653066101112131415161718191a1b1c1d1e1f202122
2324252627da1140e9f6e4779dbcfa32e538ccbc0cd97073d15f3ea4ddae2919
8d1a46a3b9db477dccaea1862b3047c730e3493fd8303132333435363738393a
3b3c3d3e3f4041424344454647000005c770069d0fde1ff4170c23a115e73257
990e2e5922367d2c384ddb6eda9f032ae759527792a4707e57e6c1cff6f30b0e
5f5252271cf96677a03e0510ff95b6387ee1038364e0ecb62b5cbb411bfaf46e
6cad80d4ffeb5cdc008641465d61e0afcd27b3c79131292981dc8d8de5d0e9d1
5614a0f62ed77d7708fe61512ba2cb5606b6f4cdd2de36017b8be26771f2f2f3
2edf5e7dc332a37c1c812e5fa5a63c62ff73d6ba6b93c1d714f6ad939f433649
ed54aecc397a715321f561bc5da174ce10183c4bb0f65592baba3e1f9dd33992
f4138ede5f9e1bc19d6ee64aa5c079a79f97870b4647ed1ae4ee8ce720a6e9ae
eb7a85e5cd252b8d2d0556446020b302b2816a456804204e9b1c3ee422a4a43a
9e9b32371d9a78737f00a52b9b624d209a248b7f48e438262a960673608ec674
a980179f586f425b7b91e9b2ec35e16a6ece581d8c2e244c5cf116eccb72daa5
d401273805ba8d36a7c676e0fa54db2c9d1071fda2f5862adf9302677a93d13d
7e6c70ad67eed247713deb50d6ef560bccbd69915a75a9b7b5ae975002b2c5a0
8d82d2b352923a9e8a929dd054a5ad7eecc5b564f872cad9ae5fee773fdc09a7
779ddea075de33eb2bcd6306123c984da023dd13c69a6bcc42f248b6c008b845
a0bee031168117497a72ae493a890f5fb4de84820e150525afa82a7adabb9258
9488775fd8b4c556b6d9809001bc56177052cd5717c5065d3ac6827bed64091d
8f06b327fad887b56f9cce97b77c5817c596002641e62f543527569ea5ba9d0d
1588a6e2103d66a1e0e0fdc694396d65c5a6db7233ba8ef96d183a28b5f9922e
42bad9c452b4bc190ba2475cb135c3d4ca4ddc34c5da7d12f1ee768af9545cc2
03314f910102a4be0ad0ae2c2a703a0f41c54dc26dd4cb292ef40376865666e9
888316eedba5b84e1f6567192c7d83e76a37c928103aa53edbc719dfa0cdddb5
d728b5d7c6416b7a044ad8283fe6f716e7938d9d20a0fa7d0d6fded13ea07f5c
bcc0895828b39b687d2e20289ff5d45216df85f5c00ebc55600f247f8a4eb758
d183ebba6ed17f060ee61635614a9777a8a4adf005f0093f4b8a19732ef22a3a
19e2145805e21af1b2400465a6bb6b5de3191551168fdbf232a5f9572ef1e009
46535761a87db54eac08bbab078eaaa02813b0cea2d5279e45f74d218086849b
ad93f7e0ba6dd8f191eceb5ce6c6f86ae8dfeae8b60b922260358400ca695ce5
a1431dddbc7272faa106a9085f8d52994defea9ffa7fbbf42d065584ce64834e
1b7ff919d3b4f474fe33aaa844ed01264d062fd0fed1dada97a2c4b4795033e2
42babc1bac4301c1a905be2c5772e8fe98fdc94b0e6d45c1522496355419eb6a
31c2f88d57d01cac7fdd2ad12855aca7aeee45826358279f6ceb7ecadf663295
279d31a86762271ff218e68a1fead052b1238ed117ee48f814a90c580bfb4951
394756df45a89dc92c515eef53ea345fe3df32eab251bac8e9b85cf8972d297f
21f4d53761054c718be9f283b3f97d78ad42f90984d906956d352d23304cdca6
873bdf2979b10e9d77d13861aa6544114abd8080786a43c9cac255768838eab9
2328e80e9a5fa6cdc8ec2b2e859535719a1492487f647abc6cf9156a45260ccf
99e1fb9db9b07d66e062a758b30971faad2ddb8d0967af97a5a12a7fbcb6ce42
54fbda0ec14a3a100423bc0e572810764542f77bdd7cb584480cd507cd267073
53409117bcbdb2d27aab60d2f9732f0f2e67be6c4ed869ea0e0961ebbb96a0f3
eb12e40abeadbb446d74b74b69b107dc234e3a854c3970eea776ae2e0df3dfd9
a280f7da3183591e86eac5d0a8001a5bb3a32f2bdc2bd68a80211cc4aa5ee4a9
ab727d69d86a690a46e60ca94809a02379f88c11ff5a4eb1722f78a5123333e0
68a6a918b539f67554a7509e5c24b797ec4584da841109a32c83b600edf4e2d0
46de3827d905dc3d11b775f4fb2b502f2973233389b0ca0f86b2e86d06f5783e
dfa7c6463f45a2b4b9a11786576739c7d6dd2863ba15ad3403da7807e7304160
a20e1dc9c2b423a518ff7ff1248c00a40b0763d8eb69799b
```

The exit kit's armoured text:

<!-- vector: kit -->
```text
-----BEGIN SATKEEP EXIT KIT-----
0001 534b4b54 01000006 b8534b42 4b010001 02030405 06070809 0a0b0c0d 0e0f0000 c79352e4
0002 00006ada a3800101 a0a1a2a3 a4a5a6a7 a8a9aaab acadaeaf b0b1b2b3 b4b5b6b7 6223341d
0003 b8b9babb bcbdbebf 01004b53 61746b65 65702070 686f6e65 206b6579 20626163 aa1a9ef7
0004 6b757020 756e6c6f 636b2076 312c2077 616c6c65 74203030 30313032 30333034 4f045b2c
0005 30353036 30373038 30393061 30623063 30643065 30661011 12131415 16171819 5d080e4b
0006 1a1b1c1d 1e1f2021 22232425 2627da11 40e9f6e4 779dbcfa 32e538cc bc0cd970 a5218baf
0007 73d15f3e a4ddae29 198d1a46 a3b9db47 7dccaea1 862b3047 c730e349 3fd83031 42731644
0008 32333435 36373839 3a3b3c3d 3e3f4041 42434445 46470000 05c77006 9d0fde1f 7a4a5638
0009 f4170c23 a115e732 57990e2e 5922367d 2c384ddb 6eda9f03 2ae75952 7792a470 488674c9
0010 7e57e6c1 cff6f30b 0e5f5252 271cf966 77a03e05 10ff95b6 387ee103 8364e0ec 91ca1dbd
0011 b62b5cbb 411bfaf4 6e6cad80 d4ffeb5c dc008641 465d61e0 afcd27b3 c7913129 d5984837
0012 2981dc8d 8de5d0e9 d15614a0 f62ed77d 7708fe61 512ba2cb 5606b6f4 cdd2de36 f59d2b0f
0013 017b8be2 6771f2f2 f32edf5e 7dc332a3 7c1c812e 5fa5a63c 62ff73d6 ba6b93c1 3bad6fa6
0014 d714f6ad 939f4336 49ed54ae cc397a71 5321f561 bc5da174 ce10183c 4bb0f655 89ef3532
0015 92baba3e 1f9dd339 92f4138e de5f9e1b c19d6ee6 4aa5c079 a79f9787 0b4647ed cde74d26
0016 1ae4ee8c e720a6e9 aeeb7a85 e5cd252b 8d2d0556 446020b3 02b2816a 45680420 439f24e7
0017 4e9b1c3e e422a4a4 3a9e9b32 371d9a78 737f00a5 2b9b624d 209a248b 7f48e438 8b445c9a
0018 262a9606 73608ec6 74a98017 9f586f42 5b7b91e9 b2ec35e1 6a6ece58 1d8c2e24 53f97ec2
0019 4c5cf116 eccb72da a5d40127 3805ba8d 36a7c676 e0fa54db 2c9d1071 fda2f586 ce6bdc9b
0020 2adf9302 677a93d1 3d7e6c70 ad67eed2 47713deb 50d6ef56 0bccbd69 915a75a9 725093ff
0021 b7b5ae97 5002b2c5 a08d82d2 b352923a 9e8a929d d054a5ad 7eecc5b5 64f872ca 774ecb5c
0022 d9ae5fee 773fdc09 a7779dde a075de33 eb2bcd63 06123c98 4da023dd 13c69a6b c9e81ae4
0023 cc42f248 b6c008b8 45a0bee0 31168117 497a72ae 493a890f 5fb4de84 820e1505 5da3fb98
0024 25afa82a 7adabb92 58948877 5fd8b4c5 56b6d980 9001bc56 177052cd 5717c506 2ea9ae36
0025 5d3ac682 7bed6409 1d8f06b3 27fad887 b56f9cce 97b77c58 17c59600 2641e62f b2f93237
0026 54352756 9ea5ba9d 0d1588a6 e2103d66 a1e0e0fd c694396d 65c5a6db 7233ba8e 2c235739
0027 f96d183a 28b5f992 2e42bad9 c452b4bc 190ba247 5cb135c3 d4ca4ddc 34c5da7d 343441eb
0028 12f1ee76 8af9545c c203314f 910102a4 be0ad0ae 2c2a703a 0f41c54d c26dd4cb 2d9b81fd
0029 292ef403 76865666 e9888316 eedba5b8 4e1f6567 192c7d83 e76a37c9 28103aa5 72b020a6
0030 3edbc719 dfa0cddd b5d728b5 d7c6416b 7a044ad8 283fe6f7 16e7938d 9d20a0fa 8cc691bc
0031 7d0d6fde d13ea07f 5cbcc089 5828b39b 687d2e20 289ff5d4 5216df85 f5c00ebc d8a7773b
0032 55600f24 7f8a4eb7 58d183eb ba6ed17f 060ee616 35614a97 77a8a4ad f005f009 3d268824
0033 3f4b8a19 732ef22a 3a19e214 5805e21a f1b24004 65a6bb6b 5de31915 51168fdb cc957972
0034 f232a5f9 572ef1e0 09465357 61a87db5 4eac08bb ab078eaa a02813b0 cea2d527 fb8cb488
0035 9e45f74d 21808684 9bad93f7 e0ba6dd8 f191eceb 5ce6c6f8 6ae8dfea e8b60b92 01ce0a73
0036 22603584 00ca695c e5a1431d ddbc7272 faa106a9 085f8d52 994defea 9ffa7fbb ba558613
0037 f42d0655 84ce6483 4e1b7ff9 19d3b4f4 74fe33aa a844ed01 264d062f d0fed1da 3fd5a758
0038 da97a2c4 b4795033 e242babc 1bac4301 c1a905be 2c5772e8 fe98fdc9 4b0e6d45 4bd78fff
0039 c1522496 355419eb 6a31c2f8 8d57d01c ac7fdd2a d12855ac a7aeee45 82635827 5eac8468
0040 9f6ceb7e cadf6632 95279d31 a8676227 1ff218e6 8a1fead0 52b1238e d117ee48 03638e35
0041 f814a90c 580bfb49 51394756 df45a89d c92c515e ef53ea34 5fe3df32 eab251ba fc26085a
0042 c8e9b85c f8972d29 7f21f4d5 3761054c 718be9f2 83b3f97d 78ad42f9 0984d906 41254de4
0043 956d352d 23304cdc a6873bdf 2979b10e 9d77d138 61aa6544 114abd80 80786a43 7e38d274
0044 c9cac255 768838ea b92328e8 0e9a5fa6 cdc8ec2b 2e859535 719a1492 487f647a 7b040048
0045 bc6cf915 6a45260c cf99e1fb 9db9b07d 66e062a7 58b30971 faad2ddb 8d0967af f20a8178
0046 97a5a12a 7fbcb6ce 4254fbda 0ec14a3a 100423bc 0e572810 764542f7 7bdd7cb5 f19f01fc
0047 84480cd5 07cd2670 73534091 17bcbdb2 d27aab60 d2f9732f 0f2e67be 6c4ed869 b30346ba
0048 ea0e0961 ebbb96a0 f3eb12e4 0abeadbb 446d74b7 4b69b107 dc234e3a 854c3970 cdc88036
0049 eea776ae 2e0df3df d9a280f7 da318359 1e86eac5 d0a8001a 5bb3a32f 2bdc2bd6 6edba4a3
0050 8a80211c c4aa5ee4 a9ab727d 69d86a69 0a46e60c a94809a0 2379f88c 11ff5a4e 2f50c244
0051 b1722f78 a5123333 e068a6a9 18b539f6 7554a750 9e5c24b7 97ec4584 da841109 710c2004
0052 a32c83b6 00edf4e2 d046de38 27d905dc 3d11b775 f4fb2b50 2f297323 3389b0ca 4df36f47
0053 0f86b2e8 6d06f578 3edfa7c6 463f45a2 b4b9a117 86576739 c7d6dd28 63ba15ad 8acc78aa
0054 3403da78 07e73041 60a20e1d c9c2b423 a518ff7f f1248c00 a40b0763 d8eb6979 f0a1281b
0055 9b000005 54736174 6b656570 2077616c 6c657420 73657475 7020310a 6e657477 cc91eada
0056 6f726b20 72656774 6573740a 73617669 6e67730a 64657363 72697074 6f722077 2585be2e
0057 73682873 6f727465 646d756c 74692832 2c5b3733 63356461 30612f38 37272f31 04988034
0058 272f3027 5d747075 62444343 68686f7a 35516472 6b6e375a 374b5861 77713641 0879b44c
0059 64367233 41344d55 6b436f56 54716557 7866546b 41366248 4e4a3343 48554574 d9d3452a
0060 414c5164 6b4e6569 784e7a34 34343650 63416d77 34574b63 6a336d56 32766232 8244e712
0061 39483773 67394550 7a627943 55317932 6d657277 2f3c303b 313e2f2a 2c5b6238 bc20bf4c
0062 36383864 66312f38 37272f31 272f3027 5d747075 62444334 345a4665 6262466d db17b11c
0063 355a586e 47355874 386b3950 7064535a 58555134 48797555 38653875 357a6675 0bb30721
0064 74597575 38455734 4d4c4b53 35344546 54664759 67657174 3750326a 45456431 99d09e3c
0065 69434635 6b535069 3758726a 36455a63 73537443 63416a50 79393744 53694c62 79c73a37
0066 2f3c303b 313e2f2a 2c5b3238 36343530 30362f38 37272f31 272f3027 5d747075 2295ee0e
0067 62444373 366b5779 41713243 73566954 7a355952 33744b70 4351346b 71327877 5476f588
0068 446d4552 6d463563 47657434 35437348 41715238 77625147 52696e36 667a4c4a 4dc84439
0069 67436472 4b665974 73346464 79544a37 5a6f346f 636d4731 7a484b33 76773731 b9ef1e6a
0070 6d683332 776a3235 576d7354 2f3c303b 313e2f2a 29292332 7076376d 6e367a0a c55141d6
0071 62697274 68646179 20313031 0a736176 696e6773 0a646573 63726970 746f7220 2c303090
0072 74722874 70756244 364e7a56 626b7259 685a3458 4a563759 6465316b 45343469 0c7faf15
0073 54506156 73394d51 5933626f 766a704a 4b5a4d47 564c4164 74714252 736a5132 e8a9aede
0074 45564a71 57557546 39755641 56354a42 56483858 775a7338 6f667935 42475476 3cf13d17
0075 51547a72 4e527a36 41564a4d 4331734e 754e2f3c 303b313e 2f2a2c7b 6d756c74 fb5f1ca2
0076 695f6128 322c5b37 33633564 6130612f 3837272f 31272f30 275d7470 75624443 803d9ff6
0077 4368686f 7a355164 726b6e37 5a374b58 61777136 41643672 3341344d 556b436f 74453a24
0078 56547165 57786654 6b413662 484e4a33 43485545 74414c51 646b4e65 69784e7a 6a629cbd
0079 34343436 5063416d 7734574b 636a336d 56327662 32394837 73673945 507a6279 234a0040
0080 43553179 326d6572 772f3c30 3b313e2f 2a2c5b62 38363838 6466312f 3837272f 45256c2d
0081 31272f30 275d7470 75624443 34345a46 65626246 6d355a58 6e473558 74386b39 7940a47d
0082 50706453 5a585551 34487975 55386538 75357a66 75745975 75384557 344d4c4b 5b83154b
0083 53353445 46546647 59676571 74375032 6a454564 31694346 356b5350 69375872 510e0876
0084 6a36455a 63735374 4363416a 50793937 4453694c 622f3c30 3b313e2f 2a2c5b32 fa75c83c
0085 38363435 3030362f 3837272f 31272f30 275d7470 75624443 73366b57 79417132 078f002b
0086 43735669 547a3559 5233744b 70435134 6b713278 77446d45 526d4635 63476574 f034f5ad
0087 34354373 48417152 38776251 4752696e 36667a4c 4a674364 724b6659 74733464 57c406e0
0088 6479544a 375a6f34 6f636d47 317a484b 33767737 316d6833 32776a32 35576d73 258378e0
0089 542f3c30 3b313e2f 2a292c61 6e645f76 28763a70 6b285b33 66363335 6136332f bc443106
0090 3837272f 31272f30 275d7470 75624443 326a6935 415a6e59 6d587333 67335165 4fc2196e
0091 41355545 754e356e 61377a31 7a393647 52527252 6874774b 79785547 72746a67 8a5c1f4a
0092 6f48584e 767a736a 5a34414d 4e733931 7839326a 514e3471 68465577 5a574e66 73f47669
0093 76746579 484d7558 736d6138 44366446 6d6f6537 696d396b 4e2f3c30 3b313e2f d5915d8b
0094 2a292c6f 6c646572 28363133 32302929 7d292334 75356e79 3366660a 62697274 5e0ee84f
0095 68646179 20313530 0a686569 722d6465 6c617920 36313332 300a6368 65636b69 cbabc52f
0096 6e2d696e 74657276 616c2035 32353630 0a636865 636b696e 2d6d6172 67696e20 308b8cba
0097 38373630 0a726566 72657368 2d77696e 646f7720 38373630 0a b43ef95e
-----END SATKEEP EXIT KIT-----
```

### 6.1 The second phone (method 2)

The same wallet id, date, phrase, setup, data key and nonces as above, locked instead to a second phone (3.4). The phone's key, the ephemeral key and the session key are simple byte runs too. In real use, the ephemeral key and the session key are random every time.

<!-- vector: second-phone-inputs -->
```text
phone-secret      707172737475767778797a7b7c7d7e7f808182838485868788898a8b8c8d8e8f
ephemeral-secret  909192939495969798999a9b9c9d9e9fa0a1a2a3a4a5a6a7a8a9aaabacadaeaf
session-key       e0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f5f6f7f8f9fafbfcfdfeff
```

**Outputs.** The phone's public key `Q` and its device id `D`:

<!-- vector: phone-public-key -->
```text
02544bffd77cb94e5707541feed75dea06e44a57751ce22d5a7a5a7af8cc2fefe7
```

<!-- vector: phone-device-id -->
```text
dc5decf4134711610714f7e58a424c4767773c43396b91e2b732c9b9746d2c17
```

The ephemeral key `E`, which is the entry's request:

<!-- vector: ephemeral-key -->
```text
03cff1393b518538ef4ce47235c606ebdf7c38765ceb662d22043774783dc71471
```

The shared x-coordinate `Z`, the unlock secret `S`, and the KEK:

<!-- vector: shared-x -->
```text
df34e088e7468038458e692af522658cef7d8b85818de34a6eeb5791235f4052
```

<!-- vector: phone-unlock-secret -->
```text
56d4a439d65172fd15ce229cfb28ee9c13e994d88bf78f25ba76330ccb9d4f50
```

<!-- vector: phone-kek -->
```text
fbee6db0528187a56e5164a475718c19b049859269d33ecc70c5a48eb85aad18
```

The wrapped key:

<!-- vector: phone-wrapped-key -->
```text
1a45c647d5df5761a7cf4ed80bf80ba6333e414ebde9b962e59bbcf9b34140d1
e561916d9874b5d38015bc061f50b2d4
```

The backup file, 1,678 bytes, in hex:

<!-- vector: phone-backup -->
```text
534b424b01000102030405060708090a0b0c0d0e0f000000006adaa3800101dc
5decf4134711610714f7e58a424c4767773c43396b91e2b732c9b9746d2c1702
002103cff1393b518538ef4ce47235c606ebdf7c38765ceb662d22043774783d
c71471101112131415161718191a1b1c1d1e1f20212223242526271a45c647d5
df5761a7cf4ed80bf80ba6333e414ebde9b962e59bbcf9b34140d1e561916d98
74b5d38015bc061f50b2d4303132333435363738393a3b3c3d3e3f4041424344
454647000005c770069d0fde1ff4170c23a115e73257990e2e5922367d2c384d
db6eda9f032ae759527792a4707e57e6c1cff6f30b0e5f5252271cf96677a03e
0510ff95b6387ee1038364e0ecb62b5cbb411bfaf46e6cad80d4ffeb5cdc0086
41465d61e0afcd27b3c79131292981dc8d8de5d0e9d15614a0f62ed77d7708fe
61512ba2cb5606b6f4cdd2de36017b8be26771f2f2f32edf5e7dc332a37c1c81
2e5fa5a63c62ff73d6ba6b93c1d714f6ad939f433649ed54aecc397a715321f5
61bc5da174ce10183c4bb0f65592baba3e1f9dd33992f4138ede5f9e1bc19d6e
e64aa5c079a79f97870b4647ed1ae4ee8ce720a6e9aeeb7a85e5cd252b8d2d05
56446020b302b2816a456804204e9b1c3ee422a4a43a9e9b32371d9a78737f00
a52b9b624d209a248b7f48e438262a960673608ec674a980179f586f425b7b91
e9b2ec35e16a6ece581d8c2e244c5cf116eccb72daa5d401273805ba8d36a7c6
76e0fa54db2c9d1071fda2f5862adf9302677a93d13d7e6c70ad67eed247713d
eb50d6ef560bccbd69915a75a9b7b5ae975002b2c5a08d82d2b352923a9e8a92
9dd054a5ad7eecc5b564f872cad9ae5fee773fdc09a7779ddea075de33eb2bcd
6306123c984da023dd13c69a6bcc42f248b6c008b845a0bee031168117497a72
ae493a890f5fb4de84820e150525afa82a7adabb92589488775fd8b4c556b6d9
809001bc56177052cd5717c5065d3ac6827bed64091d8f06b327fad887b56f9c
ce97b77c5817c596002641e62f543527569ea5ba9d0d1588a6e2103d66a1e0e0
fdc694396d65c5a6db7233ba8ef96d183a28b5f9922e42bad9c452b4bc190ba2
475cb135c3d4ca4ddc34c5da7d12f1ee768af9545cc203314f910102a4be0ad0
ae2c2a703a0f41c54dc26dd4cb292ef40376865666e9888316eedba5b84e1f65
67192c7d83e76a37c928103aa53edbc719dfa0cdddb5d728b5d7c6416b7a044a
d8283fe6f716e7938d9d20a0fa7d0d6fded13ea07f5cbcc0895828b39b687d2e
20289ff5d45216df85f5c00ebc55600f247f8a4eb758d183ebba6ed17f060ee6
1635614a9777a8a4adf005f0093f4b8a19732ef22a3a19e2145805e21af1b240
0465a6bb6b5de3191551168fdbf232a5f9572ef1e00946535761a87db54eac08
bbab078eaaa02813b0cea2d5279e45f74d218086849bad93f7e0ba6dd8f191ec
eb5ce6c6f86ae8dfeae8b60b922260358400ca695ce5a1431dddbc7272faa106
a9085f8d52994defea9ffa7fbbf42d065584ce64834e1b7ff919d3b4f474fe33
aaa844ed01264d062fd0fed1dada97a2c4b4795033e242babc1bac4301c1a905
be2c5772e8fe98fdc94b0e6d45c1522496355419eb6a31c2f88d57d01cac7fdd
2ad12855aca7aeee45826358279f6ceb7ecadf663295279d31a86762271ff218
e68a1fead052b1238ed117ee48f814a90c580bfb4951394756df45a89dc92c51
5eef53ea345fe3df32eab251bac8e9b85cf8972d297f21f4d53761054c718be9
f283b3f97d78ad42f90984d906956d352d23304cdca6873bdf2979b10e9d77d1
3861aa6544114abd8080786a43c9cac255768838eab92328e80e9a5fa6cdc8ec
2b2e859535719a1492487f647abc6cf9156a45260ccf99e1fb9db9b07d66e062
a758b30971faad2ddb8d0967af97a5a12a7fbcb6ce4254fbda0ec14a3a100423
bc0e572810764542f77bdd7cb584480cd507cd26707353409117bcbdb2d27aab
60d2f9732f0f2e67be6c4ed869ea0e0961ebbb96a0f3eb12e40abeadbb446d74
b74b69b107dc234e3a854c3970eea776ae2e0df3dfd9a280f7da3183591e86ea
c5d0a8001a5bb3a32f2bdc2bd68a80211cc4aa5ee4a9ab727d69d86a690a46e6
0ca94809a02379f88c11ff5a4eb1722f78a5123333e068a6a918b539f67554a7
509e5c24b797ec4584da841109a32c83b600edf4e2d046de3827d905dc3d11b7
75f4fb2b502f2973233389b0ca0f86b2e86d06f5783edfa7c6463f45a2b4b9a1
1786576739c7d6dd2863ba15ad3403da7807e7304160a20e1dc9c2b423a50b81
e9cb32878e7438a2cd63c7303f9e
```

The unlock request, with the session key above, as the tool shows it:

<!-- vector: phone-request -->
```text
534b 5251 0100 0102 0304 0506 0708 090a
0b0c 0d0e 0fdc 5dec f413 4711 6107 14f7
e58a 424c 4767 773c 4339 6b91 e2b7 32c9
b974 6d2c 1703 cff1 393b 5185 38ef 4ce4
7235 c606 ebdf 7c38 765c eb66 2d22 0437
7478 3dc7 1471 e0e1 e2e3 e4e5 e6e7 e8e9
eaeb eced eeef f0f1 f2f3 f4f5 f6f7 f8f9
fafb fcfd feff 7e2e 887f
```

The phone's answer:

<!-- vector: phone-answer -->
```text
9338 ab48 b9a9 11c7 1b8a 816f 5b43 1020 783d 292d 2f7e a47c fe20 80b1 b223 7109 8c99 568d
```

## 7. Assumptions and open points

- **Not reviewed.** The construction (HKDF-derived KEK, XChaCha20-Poly1305 wraps, header as associated data) follows `phone-key-backup.md` section 2. It needs the outside review before launch, like the timelocks.
- **The 4-byte line check** misses a typo about once in 4 billion. A miss is caught anyway, because the backup is authenticated, and the plain setup is compared with the locked one.
- **Device ids and the request on real devices.** Trezor's CipherKeyValue path and how its device id is computed come with Trezor support (restore-tool.md, step 6). The request text above is fixed now, because the device shows it.
- **The second phone (3.4) on real phones, to be checked when the app is built:**
  - that the Secure Enclave (CryptoKit's key agreement) and Android Keystore (ECDH key agreement, Android 12 and later) both give the raw x-coordinate as `Z`;
  - how the app shows the request's scan and the answer.
- **The warning's wording (3.4, step 4)** was set by the owner in the review of the second-phone PR. It belongs in the brief's Section 19 too, which wins on wording.
- **One second phone per backup in the tool.** The format allows several; the tool refuses a backup locked to more than one, because it can't tell which phone is at hand.
- **Version 1 replaces the prototype's layout.** The prototype's payload carried a free-form setup string. Old prototype backups don't open, and they were all test data.
