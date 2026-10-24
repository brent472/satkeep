The restore tool, first part: `satkeep-restore` unlocks the exit kit with the second key and hands Savings to Bitcoin Core or Liana, with no Satkeep app, server or helper. This is step 3 of [`docs/restore-tool.md`](docs/restore-tool.md). The heir's route is the second PR. Test networks only.

## Commands

| Command | What it does |
| --- | --- |
| `show --kit FILE` | What the kit holds, without unlocking: date, second key, and the plain wallet setup marked **unverified** |
| `core --kit FILE …` | Unlock, check the phone key is PHONE in every Savings wallet (T5), and import each wallet's receive and change descriptors, with PHONE's `tprv`, into a **new** Bitcoin Core wallet over local RPC, rescanning from each birthday |
| `core … --out FILE` | Write Core's `importdescriptors` request to a new file instead: created new, never overwritten, owner-only on Unix |
| `liana --kit FILE …` | The public descriptors for Liana. The no-beneficiary shape is marked for Core or Sparrow, since Liana can't hold it. |
| `liana … --show-phrase` | Also the phone key's 12 words, after a warning |

`--kit` takes the PDF, its armoured text or the raw backup.

**The second key:**
- **Release builds** talk to no second key yet; Trezor and the second phone are later steps. `show` works without one.
- **Test builds** (`--features simulated-device`) add `--simulated-device FILE`: the device's secret and PIN in the file, with the PIN to try typed on stdin.
- **For that,** `satkeep-backup` gains `SimulatedDevice::from_secret`, behind its feature. It also re-exports `bip39` and `zeroize`, so the tool needs no dependency of its own on them.

## Secrets

- **The command line:** keys are never taken on it. An argument that looks like one (`tprv`, `xprv`, 12 BIP39 words) is refused before parsing and never repeated.
- **Output:** nothing secret on stdout except the 12 words with `--show-phrase`, and nothing secret ever on stderr.
- **Core's import errors** are never repeated, because they could quote the request; only position and error code are shown.
- **Memory:** the phrase, the seed and the private descriptors are wiped after use. The JSON request to Core can't be wiped (`serde_json`), and the README says so.

## Tests (regtest, throwaway nodes)

**Works:**
- `show`;
- `core` imports both shapes, and Core finds all 3 BTC from the birthday;
- for each coin, Core signs as PHONE, **PHONE alone can't finalize**, and with DEVICE's signature the node accepts it, in both shapes;
- the `--out` file imports into a blank wallet as it is;
- `liana`, and `--show-phrase`.

**Forbidden:**
- the wrong second key, a wrong PIN, no second key;
- a changed plain setup;
- another phone key's backup with this setup and second key (T5);
- a node on another network, or on another machine;
- an existing Core wallet;
- a second `--out` to the same file;
- a key on the command line (the phrase whole, split over arguments, 7 of its words, or a `tprv`).

**Every command's output** is checked for `tprv`, `xprv` and runs of the phone key's phrase.

**Mainnet:** the wallet setup format and the node check both refuse it. There's no mainnet node to test against.

**Results:**
- `cargo test --workspace` passes (8 regtest tests here, plus 2 unit tests);
- `cargo clippy --workspace --all-targets -D warnings` is clean;
- `cargo deny check` passes;
- a release build's help has no `--simulated-device`.
- The exit drill runs in full on this PR, because it adds a crate under `crates/`.

## Dependencies

**Approved 2026-10-24, before any code:** `bitcoin` 0.32.102, `miniscript` 12.3.7, `bitcoincore-rpc` 0.19.0, `clap` 4.6.7, `serde_json` 1.0.154, `anyhow` 1.0.104.
- **Nothing new in the build:** each was already in `Cargo.lock` at that version. The only lockfile change is the new crate's own entry.
- **No `bdk_wallet`.**
- **Still guarded:** `cargo deny` still fails if `bitcoin`, `secp256k1` or `miniscript` ever end up at two versions.

## Security review

**A separate review of the diff found no high-severity issues, and six smaller ones, all fixed here:**
1. **Medium: arguments that aren't valid text.** `std::env::args()` panics on one, and the panic message prints the whole argument (a pasted phrase with one bad byte) before the key check runs. Now `args_os()` is read losslessly first.
2. **Low: a phrase with a typo, or cut short,** slipped past the 12-word check and could be echoed by the parser or in a path. Now any 6 BIP39 words in a row are refused (tested with 7 of the words).
3. **Low, test builds only: a malformed simulated-device file** could panic mid-slice and print the device secret. Now it's checked as hex first.
4. **Low: `--rpc-url` could point at another machine,** sending PHONE's `tprv` over plain HTTP. Now only `http://127.0.0.1`, `http://localhost` or `http://[::1]` are accepted, with unit tests including `127.0.0.1.example.com` and `localhost@example.com`, and a regtest test.
5. **Low: a failed `--out` write** could leave a partial file holding the key. Now it's removed.
6. **Low: test gap.** The other-phone test now also checks that phone's own phrase never appears in the output.

**Checked clean:**
- the PHONE check needs the same xpub, fingerprint and path;
- `with_private_key` puts the `tprv` only on PHONE's key;
- no `Debug` on anything secret;
- Core's import errors are never repeated;
- the wallet name is restricted before it goes into the URL;
- a plain `cargo build -p satkeep-restore` can't reach the simulated device.

**Also fixed here:** the backup format's vector tests read `docs/backup-format.md` with `include_str!`. On a Windows checkout with `core.autocrlf` that file has CRLF line endings and the tests failed; they now normalise line endings first. CI on Linux wasn't affected.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
