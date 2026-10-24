# satkeep-restore

`satkeep-restore` gets the phone key back from a Satkeep exit kit with the second key, and hands Savings to Bitcoin Core or Liana. It needs no Satkeep app, server or helper. **Test networks only.**

The plan, the threats and the decisions are in [`docs/restore-tool.md`](../../docs/restore-tool.md). The kit's format is [`docs/backup-format.md`](../../docs/backup-format.md), read through [`satkeep-backup`](../satkeep-backup/README.md). This is the plan's step 3, first part: unlock and export. The heir's route comes next.

It's a developer tool: its wording isn't product copy.

## Commands

| Command | What it does |
| --- | --- |
| `show --kit FILE` | What the kit holds, without unlocking anything: the backup's date, which second key unlocks it, and the plain wallet setup, marked **unverified** |
| `core --kit FILE …` | Unlock, check the phone key, and import every Savings wallet into a **new** wallet in your own Bitcoin Core (`--core-wallet`, default `satkeep-restore-<wallet id>`), over local RPC with the cookie (`--rpc-url`, `--rpc-cookie`; defaults as `satkeep`'s) |
| `core --kit FILE … --out FILE` | The same, but write Bitcoin Core's `importdescriptors` request to a new file instead |
| `liana --kit FILE …` | Unlock, check the phone key, and print each Savings wallet's public descriptor for Liana. Liana can't hold Savings with no beneficiary, so that one is marked for Bitcoin Core or Sparrow. |
| `liana … --show-phrase` | Also print the phone key's 12 words, after a warning, for Liana's hot signer |

`--kit` takes the kit PDF, its armoured text (pasted or typed back in), or the raw backup file.

## What it checks

1. **The kit:** every line's check, and refuses two different kits in one file (the format's rules).
2. **The unlock:** the second key must be the one the backup is locked to, after its own PIN. It's only ever asked for this wallet's label.
3. **The setup:** the kit's plain wallet setup must match the copy locked inside the backup, or the kit has been changed. Only the locked copy is used.
4. **The phone key is PHONE in every Savings wallet** (T5). The tool derives the phone key's public key from its 12 words, exactly as Satkeep does: BIP39 seed with no passphrase, BIP32 test-network master key, account m/87'/1'/0' ([derivation](../satkeep-savings/README.md#recovery-phrases-and-derivation)). Then it looks for that key, with its fingerprint and path, in each descriptor. A backup and a setup that don't belong together export nothing.
5. **The node** is on this machine, on the setup's network, and never mainnet.

## What Bitcoin Core gets

- **Two descriptors per Savings wallet,** receive and change, each with PHONE's `tprv` in place of its `tpub`. The other keys stay public.
- **A rescan** from each wallet's birthday block (`timestamp` = that block's time).
- **Active descriptors:** only the newest wallet of each shape is active, since Core keeps one per address type. Earlier ones are still watched and spendable.
- **Spending needs a second signature:** Core can sign as PHONE, but a Savings payment still needs DEVICE. The tests check Core can't finalize with PHONE alone, and that PHONE from Core plus DEVICE spends both shapes on regtest.

## The second key

- **Release builds** don't talk to any second key yet. Trezor and the second phone come in later steps (restore-tool.md, section 9). `show` works without one.
- **Test builds** (`--features simulated-device`) add `--simulated-device FILE`. The file holds a simulated device's state, its secret and its PIN, and the PIN to try is typed on standard input, as if on the device.

## Rules for anything secret

- **The command line:** no key or phrase is ever taken on it. An argument that looks like one is refused before parsing and never repeated, even when it isn't valid text. That's `tprv`, `xprv`, or 6 BIP39 words in a row, so a phrase that's mistyped or cut short is caught too.
- **Output:** nothing secret goes to standard output except the 12 words with `--show-phrase`, and nothing secret ever goes to standard error. The tests check every command's output for `tprv`, `xprv` and runs of the phrase.
- **Errors:** an error from Bitcoin Core's import is never repeated, since its message could quote the request. Only its position and error code are shown.
- **The file from `--out`** is created new, never overwritten, and readable only by its owner on Unix. On Windows it takes its folder's permissions. If writing it fails partway, it's removed.
- **Only a node on this machine:** the import sends PHONE's key over plain HTTP, so `--rpc-url` must be `http://127.0.0.1`, `http://localhost` or `http://[::1]`.
- **Memory:**
  - the phrase, its seed and the descriptors with PHONE's key are wiped from memory after use;
  - the JSON request sent to Core isn't, because `serde_json` can't wipe its strings. It lives only until the call returns.

## Assumptions

- **Test networks only.** The wallet setup format itself refuses mainnet, and so does the connection to Core.
- **The pocket** isn't handled yet. Its descriptor isn't designed (brief 3.11).
- **The heir's route** is the next PR. That's HEIR's key with the kit's plain setup, which is unverified without the second key. How it's started is waiting for the owner's decision.

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
- `liana` prints the descriptors, and the 12 words only on request.

**Forbidden:**
- the wrong second key, a wrong PIN, and no second key;
- a changed plain setup, and another phone key's backup (T5);
- a node on another network, a node on another machine, and an existing Core wallet;
- a key on the command line: the phrase whole, split over several arguments, 7 of its words, or a `tprv`;
- PHONE alone finalizing a spend.
