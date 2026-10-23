# Exit drill: leave Twofold without Twofold

Milestone 1, step 4. Can someone holding the Savings keys and the wallet descriptor get their bitcoin out using only independent, open-source software, if Twofold disappears?

**Yes, on regtest, two independent ways:** with **Liana** v15.0, and with **Bitcoin Core** 31.1 alone. Both spend through every path:
- any 2 of 3 keys, at any time
- DEVICE alone once its delay has passed
- HEIR alone once its delay has passed

Both refuse a delayed spend one block early. Sparrow can't import a Savings wallet at all (see [Sparrow](#sparrow-cant-import-a-savings-wallet)).

## Result

From run [39960059017](https://github.com/brent472/twofold/actions/runs/39960059017) of the `Exit drill` workflow on 2026-10-23, Ubuntu 24.04. The demo wallet uses a device delay of 10 blocks and a heir delay of 20.

| Check | Liana v15.0 | Bitcoin Core 31.1 alone |
| --- | --- | --- |
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

**No key material reached the log.** The only `***` masks in that run are GitHub's own checkout token.

## What "without Twofold" means here

Twofold runs only at the very start:
- `twofold keygen` makes four fresh test keys.
- `twofold create` and `twofold export` produce the descriptor.

After that, no Twofold code runs. The holders' recovery materials are that descriptor and their private keys:
- **Liana** (`lianad` + `liana-cli`) watches the wallet, builds every spend (`createspend` for 2-of-3, `createrecovery` for the delayed paths), adds the signatures (`updatespend`), finalizes and broadcasts (`broadcastspend`).
- **Bitcoin Core** signs, in both parts. Each holder's key sits in its own Core wallet (`walletprocesspsbt`). Liana's built-in signer only accepts a BIP39 recovery phrase, and Twofold's test keys are raw master keys, not phrases. See [findings](#findings).
- **Bitcoin Core alone** does the same spends a second time, with no Liana:
  - a watch-only wallet with the public descriptor builds the spend (`walletcreatefundedpsbt`, with the delay as the input's sequence for a delayed path)
  - the holders' wallets sign it
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
| Bitcoin Core 31.1, `bitcoin-31.1-x86_64-linux-gnu.tar.gz` | bitcoincore.org | `SHA256SUMS`, which must carry at least 3 good signatures from the builder keys in `bitcoin-core/guix.sigs` (the run had 11) |

**Keeping keys out of the log.**
- Private keys are read from the key files and immediately registered with GitHub's log masking.
- They reach Bitcoin Core only on stdin (`bitcoin-cli -stdin`), never as command-line arguments.
- A failed key import reports no detail, and Liana's own log is never printed.

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
3. **Bitcoin Core signers.** One wallet per holder (`createwallet <name> blank=true`). Import the receive and change descriptors with that holder's `tpub…` replaced by `tprv…/87h/1h/<account>h`, passing the descriptor on stdin (`bitcoin-cli -stdin importdescriptors`).
4. **Fund.** Send to a Liana address, or on regtest mine a block to one and then 100 more.
5. **2 of 3.**
   ```bash
   liana-cli createspend '{"<address>": <sats>}' '[]' 2
   ```
   Then `bitcoin-cli -rpcwallet=phone walletprocesspsbt <psbt> true DEFAULT true false`, and the same for `helper`. Then `liana-cli updatespend <psbt>` and `liana-cli broadcastspend <txid>`.
6. **DEVICE alone.** Before the coin has 10 confirmations, `liana-cli createrecovery <address> 2 10 '["<txid>:<vout>"]'` refuses. At 10 it returns a PSBT: sign it with `device`, then `updatespend` and `broadcastspend`.
7. **HEIR alone.** The same with timelock 20 and the `heir` wallet.
8. **Bitcoin Core alone.** Make a watch-only wallet (`disable_private_keys=true`) and import the public receive and change descriptors.
   - **Build:** `walletcreatefundedpsbt` with the coin as input. For a delayed path, give that input `"sequence": <delay>`.
   - **Sign:** with the holders' wallets.
   - **Send:** `finalizepsbt`, then `sendrawtransaction`. Early, `testmempoolaccept` says `non-BIP68-final`.

## Findings

- **Liana needed our internal key built its way (fixed).**
  - Liana only treats a Taproot internal key as unspendable if its chain code hashes the tree's keys in tree order. We hashed them in role order, so Liana refused our descriptor: *"Descriptor is not compatible with a Liana spending policy."*
  - Fixed in [#6](https://github.com/brent472/twofold/pull/6), which changed every Savings address. Tests now pin the key to Liana's own test vectors and code.
- **Same signer on two paths.**
  - Liana counts a key as the xpub *plus* its derivation paths. It accepts our separate-account DEVICE and DEVICE_R keys, and also its own convention of one xpub with `<0;1>` and `<2;3>`.
  - Exporting to Liana needs no change to Twofold. Importing a wallet made *in* Liana would need `SavingsKeys::new` to allow the same xpub when the paths differ.
- **Liana's signer wants a BIP39 phrase.**
  - Liana's built-in signer only takes a BIP39 recovery phrase, and Twofold's test keys are raw master keys, so Bitcoin Core signed here.
  - For a real exit, each key must exist in a form an independent signer can use: a BIP39 phrase, a hardware signer Liana supports, or an extended private key Bitcoin Core can import. **To decide before real keys exist.**
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
- **Real signers.** Once Twofold's key format is decided, rerun the drill signing with that format, not with Bitcoin Core.
- **Real delays.** The drill uses 10 and 20 blocks. The 39,420 and 61,320-block placeholders are covered by Twofold's own full-length test, not by Liana.
