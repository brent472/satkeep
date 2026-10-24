Renames the product from Twofold to Satkeep, following master brief v3.14. Code, protocol identifiers and docs all change in this one PR. Test networks only.

## What changes

| Was | Now |
| --- | --- |
| crates `twofold-savings`, `twofold-cli`, `twofold-testenv`, `twofold-fuzz` | `satkeep-*` |
| binary `twofold` | `satkeep` |
| `~/.twofold` (`%USERPROFILE%\.twofold`) | `~/.satkeep` |
| `TWOFOLD_HOME`, `TWOFOLD_NETWORK`, `TWOFOLD_WALLET_DIR`, `TWOFOLD_RPC_URL`, `TWOFOLD_RPC_COOKIE` | `SATKEEP_*` |
| PSBT proprietary prefix `twofold` (the `checkin` and `move` markers) | `satkeep` |
| phone key backup labels: HKDF `twofold/phone-key-backup/v1/kek`, wrap AAD `…/v1/wrap`, the device unlock label "Twofold phone key backup unlock v1, wallet …", the simulated device's `twofold/simulated-device/v1/…` | `satkeep/…`, "Satkeep phone key backup unlock v1, wallet …" |
| backup file marker `TFBK` | `SKBK` |
| exit drill signer `contrib/exit-drill/twofold_sign.rs` | `satkeep_sign.rs` |

- **Protocol identifiers change too**, since nothing real depends on them yet. Test wallets, PSBTs and backups made before the rename won't work and must be made again. The root README and both crate READMEs say so.
- **The backup file marker** isn't one of the strings named in the request. It stood for TwoFold BacKup, so it changes with the rest, before the format spec fixes it. Easy to put back if you'd rather keep it.
- **The old folder isn't read or moved.** Test keys and wallets under `~/.twofold`, including the signet demo's, stay where they are.

## Docs

- **CLAUDE.md** takes the private repo's v3.14 wording: the inheritance line, the check-in rule and the design line. It keeps this repo's additions: BIP test vectors, the security review, `cargo deny`, and where the private repo lives. The design line says the concept image is in the private repo, not here yet.
- **The brand sheet** is removed from the README and from `docs/brand/`, and the new concept image isn't added. `docs/design.md` says where the identity reference is. It also marks its colour table as sampled from the old sheet, with the brief's Section 02 tokens taking precedence.
- **Historical records keep the old name** where they describe what actually ran:
  - `docs/pr/*` is unchanged.
  - The signet demo's account of the run says it ran as `twofold`, and its commands are as they ran. The transaction table is unchanged. Its "Running it again" section uses `satkeep`.
  - The exit drill's results come from a fresh run on this branch (below). The earlier runs are still linked.
- **The design canvas keeps its real name,** "Twofold App Screens", marked as the old name.
- **Repo links** use the new names, `brent472/satkeep` and `brent472/satkeep-private`. They resolve once the GitHub repos are renamed after this merges.
- **The fuzz corpus** needed no change: none of its files contain the old name.
- **CI workflow names** needed no change either (`CI`, `Exit drill`, `Fuzz (long)`). The exit drill workflow's steps, paths and variables are renamed.

## Checks

- `cargo test --workspace` against throwaway regtest nodes: all pass (the full-length 61,320-block test ignored as usual). `cargo clippy --workspace --all-targets -D warnings` is clean.
- `cargo deny check`: advisories, bans, licenses and sources ok.
- **Exit drill on this branch:** [run 40100414377](https://github.com/brent472/twofold/actions/runs/40100414377). All 17 checks pass, and the scan finds no key material. `docs/exit-drill.md` now points at this run.
  - **Two earlier runs failed, before the rename mattered:** `create` refused the drill's short test settings.
  - **Why:** PR #7 added a minimum heir delay (placeholder 26,280 blocks) and PR #9 a refresh window (placeholder 8,760, which must be shorter than the check-in interval). The drill hadn't run since either merged, so it was broken on `main`.
  - **The fix,** in two small commits: the drill passes `--min-heir-delay 20` and `--refresh-window 2`, the way the signet demo lowers them. The manual steps in the doc match.
- No new dependencies. Both lockfiles changed only in the renamed packages' names and order.

## Security review

None. Reviewed the diff for:
- **PSBT markers:** written and read through the same functions, both `satkeep`. The check-in and move markers stay distinct, and no `twofold` prefix is left anywhere.
- **Backup labels:** the key-wrapping, authentication, unlock and simulated-device labels are each defined once and still distinct. `SKBK` is used both to write and to check, and nothing hardcodes `TFBK`.
- **Settings and paths:** every `env` is now `SATKEEP_*`. The home folder falls back to `~/.satkeep` or fails, never the repo, and nothing reads `TWOFOLD_*`. `.gitignore` matches by file name, so it still covers keys and wallets.
- **The exit drill workflow:** the key-file paths match, and the key scan fails if it finds no key files, so it can't pass vacuously. Nothing new is printed.
- **Mainnet and committed keys:** the mainnet refusals are unchanged, and no key material was added. The drill's lowered settings are regtest-only test values.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
