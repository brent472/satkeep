# satkeep-restore

`satkeep-restore` gets the phone key back from a Satkeep exit kit with the second key, and hands Savings to Bitcoin Core or Liana. It needs no Satkeep app, server or helper. **Test networks only.**

The plan, the threats and the decisions are in [`docs/restore-tool.md`](../../docs/restore-tool.md). The kit's format is [`docs/backup-format.md`](../../docs/backup-format.md), read through [`satkeep-backup`](../satkeep-backup/README.md). This is the plan's step 3: unlock and export, and the heir's route.

It's a developer tool: its wording isn't product copy.

## Commands

| Command | What it does |
| --- | --- |
| `show --kit FILE` | What the kit holds, without unlocking anything: the backup's date, which second key unlocks it, and the plain wallet setup, marked **unverified** |
| `core --kit FILE …` | Unlock, check the phone key, and import every Savings wallet into a **new, encrypted** wallet in your own Bitcoin Core (`--core-wallet`, default `satkeep-restore-<wallet id>`), over local RPC with the cookie (`--rpc-url`, `--rpc-cookie`; defaults as `satkeep`'s) |
| `core --kit FILE … --out FILE` | The same, but write Bitcoin Core's `importdescriptors` request to a new file instead |
| `liana --kit FILE …` | Unlock, check the phone key, and print each Savings wallet's public descriptor for Liana. Liana can't hold Savings with no beneficiary, so that one is marked for Bitcoin Core or Sparrow. |
| `liana … --show-phrase` | Also print the phone key's 12 words, after a warning, for Liana's hot signer |
| `sparrow --kit FILE … --out FILE` | Unlock, check the phone key, and write Savings with no beneficiary (the plain 2 of 3) to a new file that Sparrow imports as a wallet (its output-descriptor import). It refuses a kit with no such wallet and points to Liana. The 12 words only with `--show-phrase`, after the warning. |
| `heir --kit FILE --key-file FILE` (or `--key-stdin`) | **The heir's route:** with HEIR's 12 words and the kit's plain setup, and no second key, show the Savings wallets the heir can claim, for Liana. Marked **UNVERIFIED**. |

`--kit` takes the kit PDF, its armoured text (pasted or typed back in), or the raw backup file.

## What it checks

1. **The kit:** every line's check, and refuses two different kits in one file (the format's rules).
2. **The unlock:** the second key must be the one the backup is locked to, after its own PIN. A security device is only ever asked for this wallet's label. A second phone gets a request naming this wallet and that phone, and checks both itself (below).
3. **The setup:** the kit's plain wallet setup must match the copy locked inside the backup, or the kit has been changed. Only the locked copy is used.
4. **The phone key is PHONE in every Savings wallet** (T5). The tool derives the phone key's public key from its 12 words, exactly as Satkeep does: BIP39 seed with no passphrase, BIP32 test-network master key, account m/87'/1'/0' ([derivation](../satkeep-savings/README.md#recovery-phrases-and-derivation)). Then it looks for that key, with its fingerprint and path, in each descriptor. A backup and a setup that don't belong together export nothing.
5. **The node** is on this machine, on the setup's network, and never mainnet.

## What Bitcoin Core gets

- **Two descriptors per Savings wallet,** receive and change, each with PHONE's `tprv` in place of its `tpub`. The other keys stay public.
- **A rescan** from each wallet's birthday block (`timestamp` = that block's time).
- **Active descriptors:** only the newest wallet of each shape is active, since Core keeps one per address type. Earlier ones are still watched and spendable.
- **An encrypted wallet:**
  - **The passphrase** is asked for twice on standard input. They must match, and it can't be empty.
  - **The import:** the wallet is created with that passphrase, unlocked for at most 60 seconds around the import, and locked again afterwards.
  - **To sign as PHONE,** Core needs the passphrase (`walletpassphrase`). The tests check the wallet reports as encrypted and that Core can't sign until it's unlocked.
  - **The passphrase shows as it's typed** on a plain terminal. There's no approved crate to hide typing yet; a later step can fix that.
- **Spending needs a second signature:** Core can sign as PHONE, but a Savings payment still needs DEVICE. The tests check Core can't finalize with PHONE alone, and that PHONE from Core plus DEVICE spends both shapes on regtest.

## The heir's route

An heir has their own key (HEIR) and the kit, but not the second key. So `heir` never asks for one and never unlocks the backup: it reads only the kit's plain wallet setup.

- **The heir's 12 words** come from a key file (`--key-file`) or standard input (`--key-stdin`), as `satkeep sign` takes keys. They're never taken on the command line and never printed back. A file with anything but 12 English BIP39 words is refused, without repeating it.
- **It's marked UNVERIFIED.** Every output starts by saying so: without the second key nobody can check the plain setup against the locked copy, so someone with the cloud account could have changed it (docs/restore-tool.md, threat 7.2).
- **What limits the harm:** the heir's key can only ever sign for wallets that contain it.
- **Which wallets it exports, for Liana:** only Savings wallets that have exactly one Taproot leaf where HEIR alone spends after a block delay, `and_v(v:pk(HEIR),older(N))`. HEIR's key must match on fingerprint, path and `tpub`.
  - **Why so strict:** a changed setup can't make the tool claim the heir can move coins alone when HEIR is only in a 2-of-3, or shares the delayed path with someone else.
  - **Not offered,** with the reason given: a wallet that holds HEIR's key without such a leaf, or whose delay is time-based.
  - **If nothing can be offered,** it says so plainly and exports nothing.
- **For each wallet,** it shows the heir delay in blocks and roughly in hours, days or months, and says the heir can only move coins that haven't moved for at least that long.
  - **Where the delay comes from:** N in that heir leaf, which is what the coins follow.
  - **If the setup lists a different delay,** it says so.
- **The raw backup is refused:** it has no plain setup, and the heir can't unlock the locked one.
- **The key file or stdin** is read up to 4 KiB, into a buffer sized for that up front, so it never grows and leaves a copy behind. A larger file is refused.

**Tests (regtest):**
- **Works:**
  - the heir sees only the wallet with a beneficiary, from a key file and from stdin;
  - its exported descriptor, watched in Bitcoin Core, lets HEIR alone spend;
  - the node refuses that spend one block before the heir delay (`non-BIP68-final`) and accepts it at the delay.
- **Forbidden:**
  - another person's words, or a setup with no beneficiary, export nothing;
  - a changed setup with HEIR only in the 2-of-3, sharing the delayed path, or behind a time-based delay offers nothing;
  - the raw backup, an 11-word file and an oversized file are refused;
  - `heir` doesn't accept a second key.
- **Every heir run** is checked to show none of the heir's words, nor the phone key's.

## Which app

After unlocking, `core`, `liana` and `sparrow` all say which app suits each Savings wallet:
- **Liana** for the wallet with a beneficiary (the Taproot shape);
- **Sparrow** for the one without (the plain 2 of 3);
- **Bitcoin Core** (`core`), always offered for experts, works for both.

Each ends with one line starting "Next:", a single plain step. The step-by-step for a family member or helper is the draft [`docs/exit-guide.md`](../../docs/exit-guide.md).

## The second key

- **A second phone** (decision D4: an old phone or tablet with the Satkeep app) works in every build, with no cable and no server. The format is [`docs/backup-format.md`](../../docs/backup-format.md) section 3.4.
  - **The request:** the tool shows an unlock request on standard error, between `BEGIN` and `END SATKEEP UNLOCK REQUEST` lines: 244 hex digits naming the wallet, the phone, the backup's ephemeral key and a fresh session key.
  - **The phone** checks the request is for it and for one of its wallets, asks for its own face, fingerprint or passcode, and shows a 72-digit answer.
  - **The answer** is typed on standard input; case and spaces don't matter. A typo, or an answer to an earlier request, fails its check and can be typed again, 3 tries in all. It's read up to 4 KiB, never printed and never put in an error.
  - **No curve code in the tool:** it only hashes. The P-256 work is on the phone, so release builds don't include `p256`.
  - **Not yet:** a backup locked to more than one second phone is refused, since the tool can't tell which one is at hand. The app's side comes with the app.
  - **The request and the answer together** give the unlock secret, which opens the backup. Both show on the terminal, so close it afterwards, as with `--show-phrase`.
- **Trezor** comes in a later step (restore-tool.md, section 9). Until then, a backup locked only to a security device needs a test build. `show` works without any second key.
- **The exit drill** also builds `examples/make_test_kit` (test-only, `required-features = ["simulated-device"]`). It makes a kit the way the app will, from a phone key file and a wallet setup, locked to a fresh simulated second key, and uses no new dependency (`satkeep-savings` is a dev-dependency).
- **Test builds** (`--features simulated-device`) add `--simulated-device FILE`, and the simulated second phone the tests answer requests with. The file holds a simulated device's state, its secret and its PIN, and the PIN to try is typed on standard input, as if on the device.

## Rules for anything secret

- **The command line:** no key or phrase is ever taken on it. An argument that looks like one is refused before parsing and never repeated, even when it isn't valid text. That's `tprv`, `xprv`, or 6 BIP39 words in a row, so a phrase that's mistyped or cut short is caught too.
- **Output:** nothing secret goes to standard output except the 12 words with `--show-phrase`, and nothing secret ever goes to standard error. The tests check every command's output for `tprv`, `xprv` and runs of the phrase.
- **Errors:** an error from Bitcoin Core's import is never repeated, since its message could quote the request. Only its position and error code are shown.
- **The file from `--out`** holds the phone key **unencrypted**. The tool says so and tells you to delete it once it's imported. It's created new, never overwritten, and readable only by its owner on Unix. On Windows it takes its folder's permissions. If writing it fails partway, it's removed.
- **Only a node on this machine:** the import sends PHONE's key over plain HTTP, so `--rpc-url` must be `http://127.0.0.1`, `http://localhost` or `http://[::1]`.
- **Memory:**
  - the phrase, its seed, the descriptors with PHONE's key and the wallet passphrase are wiped from memory after use;
  - the phone key's account `Xpriv` (in `AccountKey`) isn't, because the `bitcoin` crate doesn't support wiping it;
  - the JSON sent to Core isn't, because `serde_json` can't wipe its strings: the import request, and the passphrase in `createwallet` and `walletpassphrase`. Each lives only until its call returns.

## Assumptions

- **Test networks only.** The wallet setup format itself refuses mainnet, and so does the connection to Core.
- **The pocket** isn't handled yet. Its descriptor isn't designed (brief 3.11).
- **The heir's route** reads an unverified setup (above). It points the heir at Liana, and its spend was checked with Bitcoin Core on regtest, as Liana doesn't run on the Windows development machine, and through Liana in the exit drill's section C.

## Dependencies

Each was already in `Cargo.lock` at this version and was approved for this crate (2026-10-24), so none adds a crate to the build. There's no `bdk_wallet`.

| Crate | For |
| --- | --- |
| `bitcoin` 0.32 | the phone key from its phrase (BIP32) |
| `miniscript` 12.3 | parsing the descriptors, the PHONE check, and writing them with PHONE's key |
| `bitcoincore-rpc` 0.19 | the import into the owner's Bitcoin Core |
| `clap` 4 | arguments, as in `satkeep` |
| `serde_json` | RPC parameters and the import file |
| `anyhow` | errors, as in `satkeep` |

`bip39` and `zeroize` come through `satkeep-backup`, which re-exports them.

## Tests

`$env:BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe"; cargo test -p satkeep-restore` runs on throwaway regtest nodes.

**Works:**
- `show` describes a kit without unlocking it;
- `core` imports both shapes, Core finds 3 BTC from the birthday on, and PHONE from Core plus DEVICE spends each coin;
- `core --out` writes a file that imports as it is, and only once;
- `liana` prints the descriptors, and the 12 words only on request;
- `sparrow` writes the plain wallet's descriptor to a new file (once), and the 12 words only on request;
- `core`, `liana` and `sparrow` name Liana, Sparrow and Bitcoin Core per wallet and end with one "Next:" step;
- **a second phone:** `show` names it, and through the request and answer `liana` prints the descriptors and `core` imports both shapes, with Core then signing as PHONE. A typo can be typed again.

**Forbidden:**
- the wrong second key, a wrong PIN, and no second key;
- a changed plain setup, and another phone key's backup (T5);
- a node on another network, a node on another machine, and an existing Core wallet;
- a key on the command line: the phrase whole, split over several arguments, 7 of its words, or a `tprv`;
- PHONE alone finalizing a spend;
- `sparrow` on a kit with only Savings with a beneficiary (it points to Liana and writes nothing);
- **a second phone:** three wrong answers, no answer, and an answer to an earlier request all unlock nothing, and the answer is never printed.

**In the exit drill, on the runner:** a `lianad` on `liana`'s output sees the same coins as Bitcoin Core, `sparrow`'s file passes the drill's port of Sparrow's descriptor parser, and the heir's route goes through Liana. A real Sparrow import stays a manual check, because Sparrow is a desktop app.
