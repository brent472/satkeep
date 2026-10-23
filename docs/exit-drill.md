# Exit drill: leave Twofold without Twofold

Milestone 1, step 4. Can someone holding the Savings keys and the wallet descriptor get their bitcoin out using only independent, open-source software, if Twofold disappears?

**Yes, on regtest, two independent ways:** with **Liana** v15.0, and with **Bitcoin Core** 31.1 alone. Both spend through every path:
- any 2 of 3 keys, at any time
- DEVICE alone once its delay has passed
- HEIR alone once its delay has passed

Both refuse a delayed spend one block early. Sparrow can't import a Savings wallet at all (see [Sparrow](#sparrow-cant-import-a-savings-wallet)).

## Result

From [run 40000729825](https://github.com/brent472/twofold/actions/runs/40000729825) of the `Exit drill` workflow on 2026-10-23, Ubuntu 24.04, with keys made from 12-word recovery phrases. The demo wallet uses a device delay of 10 blocks and a heir delay of 20. (The first run, 39960059017 in the private archive, used the older raw `tprv` keys and had Bitcoin Core sign for Liana.)

| Check | Liana v15.0 | Bitcoin Core 31.1 alone |
| --- | --- | --- |
| Signs with each holder's recovery phrase | ✅ Liana's own signer (`HotSigner`) reads the phrase | ✅ from a `tprv` made from the phrase with Python's standard library; Core's key fingerprints match Twofold's public keys |
| Imports the descriptor from `twofold export` | ✅ | ✅ |
| Same addresses as Twofold (first 5 receive and change) | ✅ | ✅ |
| 2 of 3 keys | ✅ PHONE + HELPER | ✅ PHONE + DEVICE |
| DEVICE alone, one block early (9 of 10 confirmations) | ✅ refused: *"Coin … is not recoverable with timelock '10'"* | ✅ the node refuses it: `non-BIP68-final` |
| DEVICE alone at exactly 10 confirmations | ✅ | ✅ |
| HEIR alone, one block early (19 of 20) | ✅ refused: *"Coin … is not recoverable with timelock '20'"* | ✅ the node refuses it: `non-BIP68-final` |
| HEIR alone at exactly 20 confirmations | ✅ | ✅ |

**Checking how each spend went through.** The script checks that every spend was mined, and which path it used:
- **2-of-3 spends:** the witness has 3 signature slots plus the script and control block (5 items), and the input sequence carries no delay.
- **Delayed spends:** the witness has 1 signature (3 items), and the input sequence is exactly the delay.

**Liana and the network agree to the block.** Liana refuses at 9 confirmations and spends at 10, the same as the node's own rule. It's neither stricter nor looser.

**No key material reached the log.** The full log has no `tprv` or `xprv`, and no more than 5 BIP39 words in a row anywhere (a phrase is 12). Its only two `***` masks are GitHub's own checkout token.

## What "without Twofold" means here

Twofold runs only at the very start:
- `twofold keygen` makes four fresh test keys, each saved as a 12-word recovery phrase.
- `twofold create` and `twofold export` produce the descriptor.

After that, no Twofold code runs. The holders' recovery materials are that descriptor and their recovery phrases:
- **Liana** (`lianad` + `liana-cli`) watches the wallet, builds every spend (`createspend` for 2-of-3, `createrecovery` for the delayed paths), adds the signatures (`updatespend`), finalizes and broadcasts (`broadcastspend`).
- **Liana's own signer** signs Liana's spends. `liana-cli` has no signing command, so [`contrib/exit-drill/twofold_sign.rs`](../contrib/exit-drill/twofold_sign.rs), a few lines around `liana::signer::HotSigner` (the signer the Liana app uses), reads each holder's phrase from their key file and signs. It's built from Liana's signed v15.0 source, inside Liana's tree, with Liana's own `Cargo.lock`.
- **Bitcoin Core alone** does the same spends a second time, with no Liana:
  - a watch-only wallet with the public descriptor builds the spend (`walletcreatefundedpsbt`, with the delay as the input's sequence for a delayed path)
  - the holders' wallets sign it. Core can't read a recovery phrase, so the script turns each phrase into a master key (`tprv`) with Python's standard library, following BIP39 and BIP32, separately from Twofold's and Liana's code. It then checks Core sees the same key fingerprint as Twofold's public key.
  - `finalizepsbt` and `sendrawtransaction` finish and send it

The Python script [`contrib/exit-drill/drill.py`](../contrib/exit-drill/drill.py) only runs these commands and checks their results. It has no wallet logic of its own.

## How it's run

[`.github/workflows/exit-drill.yml`](../.github/workflows/exit-drill.yml) runs **manually**: Actions → *Exit drill* → *Run workflow*, or from a terminal:

```bash
gh workflow run exit-drill.yml
```

It runs on a GitHub-hosted Ubuntu machine with a fresh regtest node. Keys are generated inside the run and thrown away with the machine.

**Downloads, and how they're verified. Any mismatch fails the job.**

| What | From | Verified against |
| --- | --- | --- |
| Liana v15.0, `liana-15.0-x86_64-linux-gnu.tar.gz` | Liana's GitHub release | `liana-15.0-shasums.txt`, which must carry a valid signature from Edouard Paris's key (primary `5B63 F3B9 7699 C7EE F3B0 40B1 9B7F 629A 53E7 7B83`, signing subkey `4730 … DC8F`, committed as [`.github/exit-drill/liana-signing-key.asc`](../.github/exit-drill/liana-signing-key.asc)). The tarball's SHA256 must match that file **and** the value pinned in the workflow (`24c3b5e7…abe0`). The same key signs the `v15.0` git tag; `git verify-tag` passed on 2026-10-22. |
| Liana v15.0 source, for the signer | Liana's GitHub repository, tag `v15.0` | `git verify-tag` against the same pinned key (any subkey of primary `5B63…7B83`), in a keyring holding only that key, **and** the tag must point at the pinned commit `4684d5cb…fb38`. Built with `cargo build --locked`, so every crate matches Liana's `Cargo.lock`. Built with the runner's stable Rust, not Liana's pinned 1.88.0, which would also download Windows and macOS targets. |
| Bitcoin Core 31.1, `bitcoin-31.1-x86_64-linux-gnu.tar.gz` | bitcoincore.org | `SHA256SUMS`, which must carry at least 3 good signatures from the builder keys in `bitcoin-core/guix.sigs` (the run had 11) |

**Keeping keys out of the log.**
- **Nothing secret is ever printed, masked or not.** There's no `::add-mask::`: masking still writes the secret into the log stream, and GitHub's code scanning (CodeQL) rightly flags that as clear-text logging.
- Phrases never leave the key files: Liana's signer is given the file's path. The `tprv`s reach Bitcoin Core only on stdin (`bitcoin-cli -stdin`), never as command-line arguments.
- Any command that receives a key or phrase (key imports, Liana's signer, `twofold keygen`) fails without repeating its output. Liana's own log is never printed.
- **Two scans fail the run if anything key-like gets out** ([`scan_log.py`](../contrib/exit-drill/scan_log.py)):
  - The drill's own output is checked against the run's actual phrases: no `tprv` or `xprv`, no 12 BIP39 words in a row, and no 3 words in a row from any holder's phrase.
  - A second job (`scan-log`) fetches the drill job's whole log once it has finished and checks it for `tprv`, `xprv` and 12 BIP39 words in a row.
  - Neither scan prints what it matched, only which check failed.

## The drill by hand (Linux or macOS)

The same steps the workflow runs, for a person with a Linux or macOS machine, a regtest `bitcoind`, Liana's release and `twofold`. Windows users go through Liana's desktop app instead (see [findings](#findings)).

1. **Keys and descriptor.** These are the only Twofold steps.
   ```bash
   twofold keygen --name phone   # and device, helper, heir
   twofold create --device-delay 10 --heir-delay 20 --checkin-interval 5 --checkin-margin 2 --phone … --device … --device-r … --helper … --heir …
   twofold export                # the descriptor for Liana
   twofold export --split        # receive and change descriptors for Bitcoin Core
   ```
2. **Liana.** Write a `lianad.toml` with `main_descriptor` set to the export, `network = "regtest"` and the node's cookie, then start `lianad --conf lianad.toml`. Check that `liana-cli --conf lianad.toml listaddresses 0 5` matches `twofold address`.
3. **Signers.**
   - **Liana:** build [`twofold_sign.rs`](../contrib/exit-drill/twofold_sign.rs) as the workflow does (copy it to `liana/examples/` in Liana's v15.0 tree, then `cargo build --release --locked -p liana --example twofold_sign`). Then `twofold_sign phone.key < in.psbt > out.psbt` signs as PHONE. In the Liana desktop app, the phrase can be imported as a hot signer instead.
   - **Bitcoin Core:** one wallet per holder (`createwallet <name> blank=true`). Turn the holder's phrase into its master `tprv` (BIP39 seed with an empty passphrase, then BIP32 master key; `phrase_to_tprv` in [`drill.py`](../contrib/exit-drill/drill.py) does it with Python's standard library). Import the receive and change descriptors with that holder's `tpub…` replaced by `tprv…/87h/1h/<account>h`, passing the descriptor on stdin (`bitcoin-cli -stdin importdescriptors`).
4. **Fund.** Send to a Liana address, or on regtest mine a block to one and then 100 more.
5. **2 of 3.**
   ```bash
   liana-cli createspend '{"<address>": <sats>}' '[]' 2
   ```
   Then sign with `twofold_sign phone.key`, then `twofold_sign helper.key`. Then `liana-cli updatespend <psbt>` and `liana-cli broadcastspend <txid>`.
6. **DEVICE alone.** Before the coin has 10 confirmations, `liana-cli createrecovery <address> 2 10 '["<txid>:<vout>"]'` refuses. At 10 it returns a PSBT: sign it with `twofold_sign device.key`, then `updatespend` and `broadcastspend`.
7. **HEIR alone.** The same with timelock 20 and `heir.key`.
8. **Bitcoin Core alone.** Make a watch-only wallet (`disable_private_keys=true`) and import the public receive and change descriptors.
   - **Build:** `walletcreatefundedpsbt` with the coin as input. For a delayed path, give that input `"sequence": <delay>`.
   - **Sign:** with the holders' wallets.
   - **Send:** `finalizepsbt`, then `sendrawtransaction`. Early, `testmempoolaccept` says `non-BIP68-final`.

## Findings

- **Liana needed our internal key built its way (fixed).**
  - Liana only treats a Taproot internal key as unspendable if its chain code hashes the tree's keys in tree order. We hashed them in role order, so Liana refused our descriptor: *"Descriptor is not compatible with a Liana spending policy."*
  - Fixed in PR #6 (in the private archive), which changed every Savings address. Tests now pin the key to Liana's own test vectors and code.
- **Same signer on two paths.**
  - Liana counts a key as the xpub *plus* its derivation paths. It accepts our separate-account DEVICE and DEVICE_R keys, and also its own convention of one xpub with `<0;1>` and `<2;3>`.
  - Exporting to Liana needs no change to Twofold. Importing a wallet made *in* Liana would need `SavingsKeys::new` to allow the same xpub when the paths differ.
- **Liana's signer wants a BIP39 phrase (resolved).**
  - Liana's built-in signer only takes a BIP39 recovery phrase. Twofold's first test keys were raw master keys, so in the first run Bitcoin Core signed for Liana.
  - Every Twofold key now comes from a 12-word BIP39 phrase with no passphrase, the same as Liana's own hot wallets. Liana's `HotSigner` now signs every Liana spend in the drill, and Bitcoin Core signs from the same phrases. See the [derivation](../crates/twofold-savings/README.md#recovery-phrases-and-derivation).
- **On Windows, `liana-cli` doesn't exist.**
  - Liana's JSON-RPC server and `liana-cli` are Unix-only by design: [`lianad/src/bin/cli.rs` line 1](https://github.com/wizardsardine/liana/blob/v15.0/lianad/src/bin/cli.rs#L1) is `#![cfg(not(target_os = "windows"))]`, and the RPC module is [`#[cfg(unix)]`](https://github.com/wizardsardine/liana/blob/v15.0/lianad/src/jsonrpc/mod.rs#L1-L4).
  - `lianad.exe` builds and runs on Windows, but nothing can send it commands. Liana's Windows release is the desktop app (`liana-15.0.exe`).
  - So **Windows users exit through the Liana desktop app.** That's why this drill runs on Linux.
- **Bitcoin Core alone is a second, independent exit.**
  - Core 31.1 imports the descriptor, derives the same addresses, builds, signs and finalizes every path's spend, including choosing the right leaf for a delayed spend once the sequence is set.
  - It takes more manual steps than Liana: the delay has to be set as the input's sequence by hand, and nothing stops a too-early spend except the node rejecting it.

### Sparrow can't import a Savings wallet

Sparrow 2.5.5 refuses any Taproot descriptor that has a script tree. Its descriptor parser, `drongo`, at the commit Sparrow 2.5.5 uses, has this in [`OutputDescriptor.java` lines 534–537](https://github.com/sparrowwallet/drongo/blob/e1c26413e4973752c9d7d43e37e39b411f21b48c/src/main/java/com/sparrowwallet/drongo/OutputDescriptor.java#L534-L537):

> "Taproot descriptors with script path spends are not supported"

Sparrow's wallet types are single-signature, plain multisig and silent payments ([`PolicyType.java`](https://github.com/sparrowwallet/drongo/blob/e1c26413e4973752c9d7d43e37e39b411f21b48c/src/main/java/com/sparrowwallet/drongo/policy/PolicyType.java#L8)). So it has no wallet type with timelocked paths, Taproot or SegWit v0. This comes from reading Sparrow's source; Sparrow wasn't installed. Sparrow isn't an exit route for Savings unless that changes.

## Not done yet (later steps)

- **A walk-through in the Liana desktop app,** on Windows and macOS, by hand:
  - import the descriptor
  - check addresses against `twofold address`
  - do a 2-of-3 spend and a recovery spend, signing with an external signer or an imported PSBT

  This is the route Windows users have, and nobody has tried it yet.
- **The signet run** of this drill (Milestone 1's "signet demo transaction for each allowed path").
- **Hardware and app signers.** The drill signs with Liana's hot signer and with Bitcoin Core. Signing through the Liana desktop app, and through any hardware device Twofold supports, is still to do.
- **Real delays.** The drill uses 10 and 20 blocks. The 39,420 and 61,320-block placeholders are covered by Twofold's own full-length test, not by Liana.
