Milestone 1, step 4: leave without Twofold. A Savings wallet is recovered and spent through every path with **Liana** v15.0, and again with **Bitcoin Core** 31.1 alone. No Twofold code runs after the keys and descriptor are made. Write-up: [`docs/exit-drill.md`](../exit-drill.md).

## Result

Run 39960059017 (run in the private archive) on Ubuntu 24.04: **13 of 13 checks passed.** The demo wallet's device delay is 10 blocks and its heir delay is 20.

| Check | Liana v15.0 | Bitcoin Core alone |
| --- | --- | --- |
| Imports `twofold export`; same first 5 receive and change addresses as Twofold and Core | ✅ | ✅ |
| 2 of 3 keys | ✅ | ✅ |
| DEVICE alone one block early (9 of 10) | ✅ refused by Liana | ✅ refused by the node (`non-BIP68-final`) |
| DEVICE alone at exactly 10 | ✅ | ✅ |
| HEIR alone one block early (19 of 20) | ✅ refused by Liana | ✅ refused by the node |
| HEIR alone at exactly 20 | ✅ | ✅ |

Each spend is checked as mined. Its path is checked too: witness size (5 items for 2-of-3, 3 for a delayed path) and the input's sequence.

## What's here

- **[`.github/workflows/exit-drill.yml`](../../.github/workflows/exit-drill.yml)** runs on **manual dispatch only**, on a GitHub-hosted Ubuntu runner with a fresh regtest node. It downloads:
  - **Liana v15.0's official Linux release.** It **fails the job** unless `liana-15.0-shasums.txt` carries a valid signature from the pinned maintainer key (Edouard Paris, primary `5B63…7B83`, signing subkey `4730…DC8F`; the same key that signs the `v15.0` tag, checked with `git verify-tag`). The tarball must also match that file *and* a pinned SHA256.
  - **Bitcoin Core 31.1.** It fails the job unless `SHA256SUMS` has at least 3 good builder signatures from `bitcoin-core/guix.sigs` (the run had 11), and the tarball matches it.
- **[`.github/exit-drill/liana-signing-key.asc`](../../.github/exit-drill/liana-signing-key.asc)**: Liana's public signing key, as fetched from keys.openpgp.org. It matched the copy on Edouard Paris's GitHub account.
- **[`contrib/exit-drill/drill.py`](../../contrib/exit-drill/drill.py)** uses `twofold` only for `keygen`, `create` and `export`. Everything else is `liana-cli` and `bitcoin-cli`; the script has no wallet logic of its own. **Bitcoin Core signs**, because Liana's built-in signer takes only a BIP39 phrase.
- **[`docs/exit-drill.md`](../exit-drill.md)** has the result, a by-hand version of the drill for Linux and macOS, the findings (Windows, the BIP39 signer, Bitcoin Core as a second exit), Sparrow with source links, and later steps.

## Keeping keys out of the log

- Keys are generated inside the run and thrown away with the runner.
- Each private key is registered with GitHub's log masking as soon as it's read. Keys reach `bitcoin-cli` only on stdin (`-stdin`), never as arguments.
- A failed key import prints no detail, and Liana's log is never printed.
- **Checked on the run:** the full log contains no `tprv` or `xprv`. Its only two `***` masks are GitHub's own checkout token.

## Findings (details in the doc)

- **Liana and the network agree to the block** on when a delayed path opens.
- **Windows:** `liana-cli` and Liana's RPC are Unix-only by design ([`cli.rs#L1`](https://github.com/wizardsardine/liana/blob/v15.0/lianad/src/bin/cli.rs#L1)), so Windows users exit through Liana's desktop app. That walk-through is listed as a later step, not done here.
- **Sparrow** 2.5.5 refuses any Taproot descriptor with a script tree ([`OutputDescriptor.java#L534-L537`](https://github.com/sparrowwallet/drongo/blob/e1c26413e4973752c9d7d43e37e39b411f21b48c/src/main/java/com/sparrowwallet/drongo/OutputDescriptor.java#L534-L537)), and has no timelocked wallet type. It isn't an exit route for Savings.
- **To decide before real keys:** the format each key takes, so that an independent signer can use it (a BIP39 phrase, a supported hardware signer, or an extended private key Core can import).
- The Liana internal-key problem was fixed separately in #6.

## Notes for review

- **How the workflow was tested.** GitHub only lets you start a manual workflow once it's on the default branch. So while testing, the workflow also ran on pushes to this branch. That trigger is removed in the last commit. The passing run used the same steps and script with that extra trigger.
- **After merge,** run it with `gh workflow run exit-drill.yml` or from the Actions tab.
- **Untouched:** no Twofold library or CLI code changed, and no tests changed. Nothing touched your local node.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
