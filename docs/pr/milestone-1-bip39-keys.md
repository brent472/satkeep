Every Twofold key now comes from a standard 12-word BIP39 recovery phrase, and the exit drill signs Liana's spends with Liana's own signer. Bitcoin Core stays the second exit.

## 1. Keys from recovery phrases (`6b7d263`)

- **`HolderKey`** now holds a BIP39 phrase instead of a raw `tprv`.
  - **Making a key:** 16 random bytes from the operating system give 12 English words.
  - **Deriving keys:** the BIP39 seed (empty passphrase) gives the BIP32 master key on the test network.
  - **Paths:** account keys are unchanged, at `m/87'/1'/<account>'`.
  - **Liana:** this is exactly what Liana's `HotSigner` does, so the same phrase signs there.
- **`from_secret_str`:**
  - It accepts exactly 12 words with a valid checksum. Extra spaces, CRLF and a byte-order mark are fine.
  - An old `tprv` key file gets its own error ("make a new key with keygen").
  - No error repeats its input.
- **Wiping from memory:**
  - The phrase inside `HolderKey`, its random bytes, the seed, and the key text `sign` reads in are all wiped from memory (`zeroize`).
  - The derived master key isn't, because `bitcoin`'s `Xpriv` doesn't support it. That's written down in the README.
- **`twofold keygen`:**
  - It writes the phrase to the `.key` file and prints only the public keys.
  - Before writing, it checks the phrase reads back as the same key.
- **Command-line guard:** it now also refuses 12 BIP39 words in a row, whether quoted as one argument, spread over several, or inside `--key-file=…`, as well as `tprv8`/`xprv9`.
- **Derivation is documented** in the library README under *Recovery phrases and derivation*, with its assumptions:
  - 12 words, no passphrase
  - test-only paths
  - the phrase itself has no network
  - what is and isn't wiped from memory
  - where the phrase will live in the app
- **Old key files stop working.** Any existing `~/.twofold` test keys, including the demo's, hold `tprv`s; Twofold no longer reads them and says so. Make new keys and a new wallet with `keygen`.

### New dependency: `bip39` 2.2.2

- **Features:** `std` and `zeroize` only. There's no `rand` feature, because the random bytes come from the source we already use (`secp256k1`'s OS-seeded generator).
- **Reasons:**
  - It's rust-bitcoin's BIP39 implementation, so there's no hand-rolled word list, checksum or normalisation.
  - It's the version series Liana's signer uses.
  - It works with our existing `bitcoin_hashes` 0.14, so no second version enters the build.
  - It's CC0 licensed.
- **What it brings in:** `unicode-normalization` and `tinyvec` (BIP39's NFKD normalisation), plus `zeroize` and **`zeroize_derive`**.
  - ⚠️ `zeroize_derive` is one more crate than I listed when asking: bip39's `zeroize` feature turns it on. It's a small macro crate from the same RustCrypto project as `zeroize`.
  - `zeroize` is also a direct dependency of both crates now, for wiping the seed and the key text. That's the same crate, not a new download.
- **`cargo deny check`:** advisories, bans, licences and sources all pass.

## 2. Exit drill signs with Liana's HotSigner (`d2ba19e`)

- **The signer:**
  - [`contrib/exit-drill/twofold_sign.rs`](../../contrib/exit-drill/twofold_sign.rs) is a few lines around `liana::signer::HotSigner`. It takes a key file path and the PSBT on stdin, and never prints anything secret.
  - `liana-cli` has no signing command, which is why this program exists.
- **How the workflow builds it:**
  1. Fetches Liana's `v15.0` tag.
  2. Runs `git verify-tag` in a keyring holding only the pinned key; the signature must be from primary `5B63…7B83`.
  3. Checks the tag points at the pinned commit `4684d5cb…fb38`.
  4. Builds the signer inside Liana's tree as an example of the `liana` crate, with `--locked`, so it's Liana's code built with Liana's `Cargo.lock`.
  - It uses the runner's stable Rust, not Liana's pinned 1.88.0, which would also pull in Windows and macOS targets.
- **Liana's 2-of-3, DEVICE and HEIR spends** are now all signed by HotSigner.
- **Bitcoin Core stays the second exit:**
  - `drill.py` turns each phrase into its master `tprv` with Python's standard library (BIP39 PBKDF2 seed, BIP32 master key). That's written from the BIPs, separately from Twofold's code and Liana's.
  - It's checked against BIP86's published vector.
  - Core then has to show the same key fingerprint as Twofold's public key.
- **Keeping secrets out of the log:**
  - Nothing secret is printed, masked or not. There's no `::add-mask::` (see *Fix after review* below).
  - Phrases never leave the key files.
  - `tprv`s reach `bitcoin-cli` only on stdin.
  - Any command that receives a key or phrase fails without repeating its output.
  - Two scans fail the run if anything key-like gets out (`contrib/exit-drill/scan_log.py`).

## Fix after review: CodeQL "Clear-text logging of sensitive information" (`py/clear-text-logging-sensitive-data`, high)

CodeQL alert #1 flagged the `::add-mask::` lines in `drill.py`. Masking still writes the secret into the log stream; GitHub just hides it on display. The alert has two instances: this PR (line 233), and **`main` (line 181), where the original exit drill already masked its `tprv`s.** The one on `main` closes when this merges.

- **Removed** both `::add-mask::` lines. Nothing secret is printed, masked or not.
- **No error path echoes a secret.** Every command that receives a key or phrase fails without its stdout or stderr:
  - key imports into Bitcoin Core (`bitcoin-cli -stdin`)
  - Liana's signer
  - `twofold keygen`
  - `twofold_sign`'s own errors, which are now all fixed text
- **Log scanning that fails the run.** Correction: before this, there was no automated scan; the "log is clean" result in this PR came from my own scan of the downloaded log. Now [`scan_log.py`](../../contrib/exit-drill/scan_log.py) runs twice, and never prints what it matched:
  - **In the drill job:** the drill's own output is checked against that run's real phrases. It fails on an extended private key prefix, on 12 BIP39 words in a row, or on 3 words in a row from any holder's phrase.
  - **A new `scan-log` job:** it fetches the drill job's **whole** log through the API once that job has finished (with read-only `actions` permission), and checks it for an extended private key prefix or 12 BIP39 words in a row.
  - **Tested locally** on the last real log (passes) and on planted logs: a full phrase, 3 words from a known phrase, and a `tprv` prefix each fail it, while 2 words from a phrase pass.
- **The first `scan-log` run failed, though nothing had leaked.** There were two reasons, both fixed:
  - `gh api` won't write a log containing colour codes unless it's given `--allow-escape-sequences`.
  - The scanner's own pass message said "no tprv/xprv", which tripped the full-log scan.
- **CLAUDE.md:** published BIP test vectors (such as BIP39's "abandon … about") are allowed in tests.

## Checks

- **`cargo test --workspace`** passes, with throwaway regtest nodes. That's 42 library unit tests and every library and CLI integration suite.
- **New tests:**
  - The derivation is pinned to BIP86's published account key for BIP39's public test phrase.
  - Bad checksum, 11 words, 24 words, an old `tprv` key and garbage are each refused without echo.
  - Phrase detection is checked, including uppercase, split, and 11-word non-matches.
  - Every CLI test now also fails if any output has 12 BIP39 words in a row, or 3 words in a row from any phrase the test knows.
  - A phrase on the command line is refused whether quoted, unquoted or inside `--key-file=`, and so is an old `tprv`.
- **Other checks:**
  - `cargo clippy -D warnings` passes for the workspace and the fuzz crate.
  - The `holder_key` fuzz target now also edits phrases word by word, swapping, dropping or repeating words.
  - `cargo deny check` passes.
- **Exit drill, [run 40004699830](https://github.com/brent472/twofold/actions/runs/40004699830) on this branch, after the CodeQL fix: 15 of 15 checks pass, and both key-material scans pass.** (The first run, [40000729825](https://github.com/brent472/twofold/actions/runs/40000729825), also passed 15 of 15.)
  - Liana and Core each spend 2-of-3, and DEVICE and HEIR alone at exactly their delays, and each refuses those one block early. Liana's spends are signed by HotSigner.
  - Core's keys made from the phrases match Twofold's.
  - **The log is clean, and nothing is masked:**
    - The drill's output has nothing from the run's four phrases, and the full job log has no extended private key and at most 5 BIP39 words in a row.
    - No `::add-mask::` was printed. The only `***` are GitHub's own tokens (checkout, and the scan job's API token).
- `docs/exit-drill.md` is updated with this run.

## Security review

**Findings: none.** `/security-review` was run on the full diff before opening this PR.

- **What it checked:**
  - phrase leaks through errors, the CLI, `drill.py` and `twofold_sign.rs`
  - randomness and derivation, including the stdlib conversion against the BIPs
  - subprocess injection
  - the workflow's tag check: a keyring holding only the pinned key, an anchored `VALIDSIG` match, the pinned commit, `--locked`, and dispatch-only with read-only permissions
- **What it missed:** it judged the `::add-mask::` lines acceptable, because the values were masked. CodeQL flagged them, and they're now gone (see *Fix after review*). The fix commits were reviewed by hand: they only remove output and error detail, and add the log scans.
- **Notes below the bar** (both already written down): the `String` from `to_secret_string` and the derived `Xpriv` aren't wiped from memory, and a revoked or expired pinned Liana key would still verify, though the pinned commit limits that.
- **Also caught before committing:** my first version of the derivation test had BIP86's published root `xprv` as a literal. It's a public test vector, but CLAUDE.md says never commit xprvs, so the test now compares against BIP86's published account **xpub** instead.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
