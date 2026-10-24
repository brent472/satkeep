The easy routes: the restore tool says which app to use, a `sparrow` command, a draft exit guide, and the heir's route through Liana in the exit drill. Test networks only. No new dependencies.

## What changes

- **Which app.** After unlocking, `core`, `liana` and `sparrow` say which app suits each Savings wallet:
  - **Liana** for the one with a beneficiary (the Taproot shape);
  - **Sparrow** for the one without (the plain 2 of 3);
  - **Bitcoin Core** (`core`), always offered as the fallback for experts.

  Each ends with one plain "Next:" step.
- **`satkeep-restore sparrow --kit FILE --out FILE`** writes Savings with no beneficiary to a new file, for Sparrow's output-descriptor import. It writes only the public descriptor and never overwrites. A kit with only Savings with a beneficiary is refused, pointing to Liana. `liana` is unchanged. Both show the 12 words only with `--show-phrase`, after the warning.
- **[`docs/exit-guide.md`](docs/exit-guide.md), a draft:** the step-by-step for a family member, helper or heir. It leads with Liana or Sparrow (download from the official site, check the signature, import, connect the second key, spend). Bitcoin Core is in "For experts" at the end. Plain words, marked as a draft for review and not app copy. Steps that depend on releases or on an app's exact screens are marked *to be decided* or *to be checked*.
- **The exit drill's new section C** closes the "not tested through Liana" gap:
  - **A kit is made** by a new test-only example, `make_test_kit`, with a simulated second key (`required-features = ["simulated-device"]`; it uses the `satkeep-savings` dev-dependency, so nothing new). It's restored with `satkeep-restore`.
  - **`liana`:** a `lianad` on its output sees the same coins as Bitcoin Core.
  - **`sparrow`:** its file passes the drill's port of Sparrow's descriptor parser.
  - **`heir` through Liana:** Liana refuses HEIR one block early, then spends with HEIR alone, signed by Liana's own signer, at the heir delay.
  - **On the runner only,** the workflow builds the tool with its test device and the example.

## Checks

- **The exit drill on this branch:** [run 40127302040](https://github.com/brent472/satkeep/actions/runs/40127302040). Every check in A, B and C passed, and scan-log passed. `docs/exit-drill.md` has the C table.
- **Regtest tests:** 16 in `satkeep-restore`. New ones check:
  - `sparrow` writes the plain wallet's descriptor once, and the 12 words only on request;
  - it refuses a kit with only Savings with a beneficiary;
  - `core`, `liana` and `sparrow` name Liana, Sparrow and Bitcoin Core per wallet and end with exactly one "Next:" step.
- **The whole workspace** passes; clippy (`-D warnings`) is clean.
- **Still manual:** a real import into Sparrow, because Sparrow is a desktop app.

## Security review

**No code issues.** One point about the guide, fixed here:
- **The problem:** the heir's kit is unverified, so a changed setup could give the heir a wallet that also has someone else's key on an immediate path. A claim sent to an address from that same wallet could then be taken.
- **The fix:** the guide and the `heir` command's "Next:" line now say to send the claim to a separate wallet only the heir controls, never to an address from the restored wallet.

**Checked clean:**
- **The phrase** is printed only with `--show-phrase`, after the warning on stderr.
- **`make_test_kit`** prints one fixed line. The device secret and PIN go only to a new file.
- **The drill** never prints the tool's output, and its new files aren't logs, so `diagnostics()` doesn't tail them. `scan_log.py` is neither tripped nor defeated.
- **`sparrow`** writes only the public descriptor from the authenticated setup, and never overwrites.
- **The example and the simulated device** can't reach a release build: the example has `required-features`, and the feature doesn't leak into a plain build.
- **The workflow** adds one `cargo build --locked` of in-repo code and two env vars with fixed paths. There's no untrusted input, and permissions are unchanged.
- **The guide's commands** match the CLI, and its Liana key fingerprint matches the pinned key.

**CodeQL:** two `rust/cleartext-logging` alerts, dismissed as false positives with your approval (the same class as #5–#7 on #22):
- **#8, `main.rs:172`:** `sparrow` prints the wallet list: each wallet's shape, heir delay and birthday block. All public.
- **#9, `main.rs:228`:** `liana`'s "Next:" line prints how many Savings wallets there are.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
