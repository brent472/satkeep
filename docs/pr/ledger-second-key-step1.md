**Do not merge before 27 October (14-day rule for tokio 1.53.3).**

## A Ledger as DEVICE, step 1: approving Savings payments

A Ledger running the Bitcoin Test app can be DEVICE, the second key of Savings. This step covers approving payments only: **the backup format doesn't change,** and a Ledger can't unlock the phone key backup yet.

### What's new

- **`crates/satkeep-ledger`**, on Liana's `async-hwi`, Ledger only:
  - **DEVICE's key at `m/87'/1'/0'`.** The Ledger shows it and gives it out only once you approve it on the device. The Bitcoin app insists on that, because the path isn't one of its standard paths.
  - **Registering a Savings wallet,** either shape. The Ledger shows the name, the rules and every key, then returns an approval code (an HMAC).
  - **Signing as DEVICE.** The Ledger's signatures are kept only if it signed every coin in the payment.
- **CLI:**
  - `satkeep ledger-key`;
  - `satkeep ledger-register`, which saves the registration as `ledger.json` in the wallet folder. That file is now in `.gitignore`;
  - `satkeep sign --ledger`. The Ledger's screen is the approval, so it takes no `--yes` and no key.
- **The approval code isn't secret, but nothing prints it:** not the CLI, not an error, not `Debug`, and no CI step.

Every assumption is written in [`crates/satkeep-ledger/README.md`](../../crates/satkeep-ledger/README.md):
- the key path;
- the wallet name "Satkeep Savings", which needs confirming, since Section 19 has no Ledger wording yet;
- that a Savings wallet with a beneficiary needs its own registration;
- the internal key the Ledger lists as `dummy`;
- the minimum app version, 2.2.1.

### Tests

- **Against Ledger's Speculos emulator, on regtest** (`crates/satkeep-ledger/tests/speculos.rs`), for both shapes:
  - **Allowed:** the Ledger registers the wallet, and the Ledger plus PHONE, or plus HELPER, spends it.
  - **Forbidden, refused by Satkeep and, built by hand, by the node:**
    - the Ledger alone, even after the heir delay;
    - the Ledger and HEIR before the heir delay;
    - the Ledger and HEIR through 2 of 3, where HEIR's signature doesn't count.
  - **Forbidden on the Ledger:** an altered approval code, a wallet it never registered, and another wallet's payment are all refused, and the PSBT is left unchanged.
  - **Not registered:** a wallet whose DEVICE isn't this Ledger.
- **How the buttons are pressed:** through Speculos's REST API. The test presses right until one of the screens Ledger's own tests approve on, then both buttons.
- **What a failing test prints:** only redacted screens and error messages, never a PSBT, a registration or a key.
- **`crates/satkeep-cli/tests/ledger.rs`,** with no Ledger: a missing, unreadable or wrong-wallet `ledger.json` is refused before any device is contacted, and the approval code is never shown.
- **The test harness:** `satkeep-savings/tests/common` gains `Savings::with_device_key`, so the Ledger tests reuse it. Existing tests are unchanged and pass.
- **Run locally on Windows:**
  - the whole workspace's tests, regtest included;
  - clippy `-D warnings` on the workspace and the fuzz crate;
  - `cargo deny check` for bans, licences and sources.

  The Speculos tests run only in CI.

### CI

- **A new Ledger workflow** (`.github/workflows/ledger.yml`):
  - builds Ledger's Bitcoin app;
  - starts Speculos as a Nano X on its public default seed;
  - runs the Speculos tests one at a time.
- **It skips in seconds** when nothing it uses changed, like the drill. It always runs on main.
- **Every download is pinned by identity:**
  - **The app's source:** `LedgerHQ/app-bitcoin-new` at `fc8d7ab610afd834fe512542ba258cf65661ee08`, the commit behind tag 2.4.6. The job checks out that commit and checks it.
  - **Ledger's build image:** `ledger-app-builder` 5.3.2, by its digest `sha256:d231791e…2714`.
  - **Speculos:** 0.26.6 and its 41 Python dependencies, each by SHA256, with `pip install --require-hashes --only-binary :all:`. They were resolved with pip's `--uploaded-prior-to 2026-10-11`.
  - **Bitcoin Core:** by its signed checksums, with the existing script.
  - **The checkout action:** by commit, as in the other workflows.
- **Logs:**
  - Speculos's log traces every message to and from the app, so it goes to a file that's never printed or uploaded.
  - The app is a release build, so it logs nothing.
- **`libudev-dev`:** hidapi builds against it, so the CI jobs that build Rust (build, regtest, fuzz) and the drill now install it.
- **To decide:** whether to make **`Ledger (Speculos)`** a required check in "Protect main". That's your setting.

### New dependencies

Approved on 2026-10-25. Direct dependencies are pinned with `=`. Every crate is pinned in `Cargo.lock` (and `fuzz/Cargo.lock`), and CI builds `--locked`.

**Direct:**
- **`async-hwi` `=0.0.32`** (BSD-3-Clause, May 2026), with only its `ledger` feature. It's Liana's maintained hardware wallet library, the one Liana uses for Ledger. 0.0.32 has everything this needs; its Ledger code matches 0.0.35's apart from an internal option change.
- **`tokio` `=1.53.3`**, `rt` only. async-hwi is async, and a current-thread runtime runs each call. It's already pulled in by async-hwi.
  - **The 14-day exception:** 1.53.3 was published on 2026-10-13. It's the first 1.53 release without RUSTSEC-2026-0386, an unsound Unix `AsyncFd` API that nothing here uses. 1.53.2 fails `cargo deny`. No ignore was added for the advisory. Hence the note at the top.

**All new crates in `Cargo.lock`:**

| Crate | Version | Published |
| --- | --- | --- |
| aho-corasick | 1.1.5 | 2026-08-03 |
| arrayref | 0.3.9 | 2024-09-14 |
| async-hwi | 0.0.32 | 2026-05-28 |
| async-trait | 0.1.92 | 2026-08-08 |
| byteorder | 1.5.0 | 2023-10-06 |
| bytes | 1.12.1 | 2026-07-08 |
| doc-comment | 0.3.4 | 2025-10-24 |
| futures, -channel, -core, -executor, -io, -macro, -sink, -task, -util | 0.3.34 | 2026-08-11 |
| heck | 0.4.1 | 2023-02-01 |
| hex | 0.4.3 | 2021-03-03 |
| hidapi | 2.6.7 | 2026-08-27 |
| ledger-apdu | 0.10.0 | 2022-07-13 |
| ledger-transport | 0.10.0 | 2022-07-13 |
| ledger-transport-hidapi | 0.10.0 | 2023-09-14 |
| ledger_bitcoin_client | 0.6.2 | 2026-05-11 |
| mio | 1.2.4 | 2026-10-03 |
| no-std-compat | 0.4.1 | 2020-08-09 |
| pin-project-lite | 0.2.17 | 2026-02-27 |
| regex | 1.13.1 | 2026-07-15 |
| regex-automata | 0.4.18 | 2026-08-04 |
| regex-syntax | 0.8.11 | 2026-06-09 |
| slab | 0.4.12 | 2026-01-31 |
| snafu, snafu-derive | 0.7.5 | 2023-07-09 |
| socket2 | 0.6.5 | 2026-07-13 |
| syn | 1.0.109 | 2023-02-24 |
| thiserror, thiserror-impl | 1.0.69 | 2024-11-10 |
| tokio | 1.53.3 | 2026-10-13 (the exception above) |
| tokio-macros | 2.7.2 | 2026-07-29 |

- **Licences:** permissive. `deny.toml` gains one exception: `async-hwi`'s BSD-3-Clause, read from its LICENSE file, because its manifest names no licence.
- **The August arrayref attack:**
  - `arrayref` is 0.3.9, not 0.3.10.
  - None of `proc-macro1`, `proc-macro-en`, `aovine`, `arone`, `aronenao`, `tinymember`, `append-only-vec 0.1.9` or `internment 0.8.7` is in either lock file. Nor are they in Cargo's download cache on this PC (registry `src` and `cache`, and its git checkouts).
- **Build scripts:** none downloads anything.
  - `hidapi`'s compiles its bundled C (`etc/hidapi`) with `cc`, and finds `libudev` with pkg-config on Linux.
  - `thiserror` 1, `syn` 1 and `doc-comment` only run `rustc --version`.
  - The new proc-macro crates make no network, process or file-writing calls.

### Before merging

- **`cargo deny check` fails on this PR, and on main too, because of a yanked crate unrelated to the Ledger.**
  - Today RustCrypto yanked every `crypto-bigint` 0.7 release except 0.7.9, including our 0.7.7.
  - That's handled in its own small PR. Once it's merged, this branch needs main merged in.
- **The tokio date:** not before 27 October.

### A real Ledger on signet

[`docs/ledger-signet.md`](../ledger-signet.md): step-by-step on your Windows PC, about an hour.
1. Install Bitcoin Test.
2. Read and approve the key.
3. Register Savings with no beneficiary.
4. The Ledger plus PHONE pays; the Ledger alone is refused.
5. Add a beneficiary, approving the move with the Ledger.
6. Register the new wallet.
7. The Ledger plus HELPER pays.
8. The rest goes back to the faucet.

### Security review

**None.** A separate agent reviewed the diff (`origin/main...HEAD`) without editing anything, reading async-hwi 0.0.32's source where needed. It checked:
- **Ledger signing:** `sign` works on a copy, and keeps it only if every coin gained a signature. async-hwi only adds signatures and never changes other PSBT fields. A device error keeps nothing.
- **`sign --ledger`:** it runs the same payment checks as a key file: every input is this wallet's own unspent coin, every amount matches the wallet's record, there's one approval path, and moves and check-ins are validated. It can't be combined with `--yes`, a key file or standard input.
- **A tampered `ledger.json`:**
  - its descriptor must be this wallet's;
  - a changed name or approval code is refused by the Ledger, whose HMAC covers both;
  - read errors never repeat the file.
- **Leaks:**
  - `Registration`'s `Debug` leaves the code out, and the Ledger types have no `Debug`;
  - test output is redacted;
  - Speculos's log is never printed or uploaded, and it runs on its public seed.
- **Mainnet:** a mainnet key is refused, the policy is regtest or signet only, and the app is built as testnet.
- **Spending paths:** the harness refactor leaves existing tests unchanged, and the new tests cover the allowed and forbidden cases above.
- **Workflows:**
  - `contents: read`, `pull_request` (not `_target`), and no secrets;
  - PR-controlled values reach the shell only through `env:`;
  - pins as listed above, and Docker runs as the runner's own user;
  - the `ci.yml` and `exit-drill.yml` edits only add an apt install.

Low-severity notes, below the bar, left as they are:
- **The Ubuntu packages aren't pinned by version:** `libudev-dev`, `qemu-user-static` and `libsdl2-image-2.0-0`, from apt. They come from Ubuntu's signed archive, but they aren't pinned by exact identity. Pinning versions would make the job break whenever the runner image moves. Your call.
- **File names in the job summary:** the skip step writes the PR's changed file names into it, as the drill's does. The worst case is odd Markdown in the summary.
- **Screen redaction** relies on no secret ever appearing on a Ledger screen, which holds for these screens.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
