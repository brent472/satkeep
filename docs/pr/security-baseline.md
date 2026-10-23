Security baseline: dependency checks, fuzzing of the code that reads outside input, and CI on every PR. Three commits as asked, plus a cargo-deny pin from the security review and one real bug found by the fuzzer, with its fix.

## 1. Dependency checks (`58588ae`)

- **`deny.toml`** (cargo-deny 0.20.2) fails on:
  - any RustSec advisory (vulnerable, unsound or unmaintained) and any yanked crate
  - a second version of `bitcoin`, `secp256k1` or `miniscript` in the build
  - any source other than crates.io, and any non-permissive licence
- **Proof that the duplicate rule works:** with `jsonrpc` (which really is in the tree twice) temporarily added to the same rule, `bans` failed. The three named crates each have exactly one version: `bitcoin` 0.32.102, `secp256k1` 0.29.1, `miniscript` 12.3.7.
- **Licences:**
  - Our three crates now declare `license = "MIT"`.
  - `hex_lit`'s `MITNFA` (MIT plus a no-false-attribution clause) is allowed for that crate only.
  - The allow list is trimmed to the licences actually in use.
- **`Cargo.lock`** was already committed. **README:** a new *Dependencies* section, including the rule that a new dependency needs a reason in its PR.

## 2. Fuzzing (`cd37d8c`)

`fuzz/` is its own cargo workspace, so the main build never compiles libFuzzer. It has three targets, none of which prints anything:

| Target | Input | Extra check |
| --- | --- | --- |
| `psbt` | Raw PSBT bytes, or one of 5 valid Savings PSBTs (2-of-3, device, heir, drain, check-in) edited by the input: sequences, version, locktime, dropped or duplicated inputs, outputs swapped among ours, foreign and empty, the check-in marker added or removed | Calls `path_of`, `is_checkin`, `check_checkin` and `check_delays` on a fixed wallet (coins at several ages plus one unconfirmed), with keys generated inside the target. If `check_checkin` accepts, the PSBT must be marked, 2-of-3, and have one output back to the wallet. |
| `config` | `savings.json` text → `Config` → `policy()`, `checkin_rule()` | Seeded with a valid file built from Liana's public test keys |
| `holder_key` | Key file text, raw, or a freshly generated key edited by the input | **Fails if an error message ever repeats 16 characters of its input**, which makes "never echo key material" checkable by the fuzzer |

- **New dependency:** `libfuzzer-sys` 0.4.13 (approved, fuzz crate only). Reason: it's the standard libFuzzer binding that `cargo-fuzz` builds on, and nothing else in the workspace uses it. It brings `arbitrary`, `cc` and `jobserver`; every other crate in `fuzz/Cargo.lock` is the same version as the main build.
- **CLI library target:** `twofold-cli` gains a small library target (`wallet_dir`, `node`) so the `config` target can reach `Config`. `main.rs` now uses it. No behaviour change.

### Fuzzing results: one real bug, fixed (`71282e7`)

The **`config`** target crashed within seconds of its first run.
- **The input:** a `savings.json` with DEVICE_R at `…/<1;1>/*`.
- **How it got through:** `SavingsKeys` only checked that the key had two paths, so it accepted it.
- **Why it crashed:** miniscript writes a `<1;1>` key back out as a single `/1/*`. So inside the descriptor the key was no longer multipath, the unspendable internal key couldn't be built, and **`SavingsPolicy::new` panicked**. Any crafted `savings.json`, or a `twofold create` argument, could crash `twofold`.
- **The fix:**
  - `SavingsKeys::new` requires the two paths to differ. A `<1;1>` key would also have given the wallet identical receive and change addresses.
  - `SavingsPolicy::new` now returns `Error::InternalKey` instead of panicking.
- **Regression tests:**
  - library: `rejects_a_key_whose_receive_and_change_paths_are_the_same`, which fails with the old check
  - CLI: `refuses_a_key_with_the_same_receive_and_change_path`, refused with an error, no panic
  - the crashing file is now a permanent fuzz seed; it holds only public test keys

**10-minute runs, once on this branch** ([run 39981972717](https://github.com/brent472/twofold/actions/runs/39981972717)), with the fix in place:

| Target | Executions in 601 s | Result |
| --- | --- | --- |
| `psbt` | 3,836,045 | no crash, and the check-in assertion never fired |
| `config` | 38,255,827 | no crash (before the fix: crashed, [run 39979436448](https://github.com/brent472/twofold/actions/runs/39979436448)) |
| `holder_key` | 6,274,056 | no crash, and no error message ever repeated its input |

Fuzzing ran only on GitHub's Linux runners. Nightly Rust and cargo-fuzz were never installed on the Windows PC.

## 3. CI and the review rule (`51576f2`, `0d5c34c`)

- **[`ci.yml`](../../.github/workflows/ci.yml)** runs on every PR and on `main`:
  - the build
  - `cargo clippy -D warnings`, on the workspace and on the fuzz crate
  - the unit tests
  - `cargo deny check`
  - **the regtest tests on throwaway nodes**. They're fast enough for CI. Only the `#[ignore]`d full-length 61,320-block test stays local.
  - 60 seconds of each fuzz target
- **[`fuzz-long.yml`](../../.github/workflows/fuzz-long.yml)** runs the fuzz targets for longer, by hand. Its run time goes through an env var and must be a number.
- **Every download is pinned:**
  - **Bitcoin Core 31.1:** moved into a shared script, [`fetch-bitcoin-core.sh`](../../.github/scripts/fetch-bitcoin-core.sh), used by CI and the exit drill. Its tarball SHA256 is now pinned (`b80d9c3e…3f9e`), as Liana's is, on top of at least 3 good builder signatures.
  - **cargo-deny:** its tarball must match both the release's `.sha256` and a SHA256 pinned in `ci.yml`.
  - **cargo-fuzz:** built with `cargo install --locked`.
  - **Actions:** pinned by commit (`checkout` v7.0.1, `upload-artifact` v7.0.2).
- **Nothing prints key material:** the CLI tests fail if any output contains `tprv` or `xprv`, the fuzz targets print nothing, and their saved crash inputs hold only fuzzer bytes.
- **`CLAUDE.md`:** before opening any PR, run a security review of the diff and put the findings (or "none") in its description; new dependencies need a reason.
- **`twofold_savings::Error`** now boxes the other crates' errors it wraps, which fixes clippy's `result_large_err`. Error messages are unchanged.

## Security review (`/security-review` of this branch)

**No HIGH or MEDIUM findings.** It reviewed the Bitcoin Core and cargo-deny verification, the `${{ }}` uses in shell steps, token permissions and triggers, public fuzz artifacts, the error-type change and the new library target.

One **LOW** item was raised: cargo-deny was checked only against its own release's `.sha256`. Fixed in this PR by pinning its SHA256 (`0d5c34c`).

## How this was tested

- **Locally:** all 103 tests pass, clippy is clean, and `cargo deny check` passes.
- **On GitHub:** CI and the long fuzz ran on a throwaway branch with temporary push triggers, so this branch never carries them. GitHub only lets you start a manual workflow once it's on `main`. That throwaway branch gets deleted.

Last CI run, on the final code ([run 39981972814](https://github.com/brent472/twofold/actions/runs/39981972814)): all six jobs passed.
- the build, clippy and the unit tests
- cargo deny, with the pinned binary
- the regtest tests
- 60 seconds of each fuzz target

Neither run's full log contains `tprv` or `xprv`, not even as a word.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
