# satkeep-ledger

A Ledger as DEVICE, the second key of a Satkeep Savings wallet. **Step 1: approving Savings payments only.** Test networks only.

The Ledger runs Ledger's own Bitcoin app (the **Bitcoin Test** version). Satkeep talks to it through Liana's [`async-hwi`](https://github.com/wizardsardine/async-hwi), Ledger only.

## What it does

1. **DEVICE's key** (`Ledger::public_key`, `satkeep ledger-key`):
   - The Ledger shows its public key at `m/87'/1'/0'` and gives it out only once the owner approves it on the screen. The Bitcoin app always asks at that path, because it isn't one of the app's standard paths.
   - The key becomes DEVICE in `satkeep create --device`.
2. **Registering the wallet** (`Ledger::register`, `satkeep ledger-register`):
   - The Ledger shows the wallet's name, its rules and every key, and registers it once the owner approves.
   - It answers with an **approval code**: an HMAC over the wallet's name and rules, under a key only that Ledger has. The Ledger signs for a registered wallet only when handed back its code.
   - Both Savings shapes register: plain 2 of 3 (`wsh(sortedmulti(2,…))`), and with a beneficiary (`tr(…,{multi_a(2,…),and_v(v:pk(HEIR),older(N))})`).
   - **Before a request reaches the Ledger,** `register` checks that the wallet's DEVICE key has this Ledger's fingerprint.
3. **Signing** (`Ledger::sign`, `satkeep sign --ledger`):
   - The Ledger shows the payment: amounts, addresses and network fee. It signs as DEVICE once the owner approves.
   - It signs the 2-of-3 leaf, or the multisig script of plain Savings.
   - Satkeep keeps the result only if the Ledger signed every coin in the payment. Otherwise the PSBT is left as it was.

The Ledger's signature alone moves nothing. Savings still needs a second key, or HEIR alone after the heir delay. The helper can't spend alone either, Ledger or not.

## Not in this step

- **The phone key backup isn't locked to a Ledger.** The backup format is unchanged.
  - A Ledger as DEVICE approves Savings payments, but it can't open the phone key backup.
  - An owner whose second key is a Ledger has no backup unlock yet. Using a Ledger for that (message signing, `phone-key-backup.md` 3.2) is a later step.
- **No Bluetooth and no phone.** USB from a computer only, through the developer CLI.
- **Not tested on a Stax or Flex screen.** CI emulates a Nano X. The code is the same for every model, but the screens and buttons differ.

## Assumptions

Each one is a placeholder or an observation, written down here as the project rules require.

- **Key path `m/87'/1'/0'`.** The same test-only placeholder as every Satkeep key: BIP 87 multisig, test network, account 0. The Bitcoin app treats it as non-standard, so it shows the key and asks every time Satkeep reads it. A mainnet key (the Bitcoin app rather than Bitcoin Test) is refused.
- **Wallet name "Satkeep Savings"**, the same for both shapes.
  - The Ledger shows it at registration and when signing. It also shows the rules and keys, which tell the two shapes apart.
  - The name isn't from Section 19 of the brief, which has no Ledger wording yet. It's on the list to confirm.
- **The approval code is kept, not printed.**
  - It's saved in the wallet folder as `ledger.json`, beside `savings.json`.
  - It isn't secret: it only proves to the same Ledger that it registered this exact wallet. A changed name, rule or key needs a new code.
  - Even so, nothing in Satkeep prints it: not the CLI, not an error, not `Debug`, and no CI step.
  - `ledger.json` is in `.gitignore` with the other wallet files.
- **One registration per wallet.** Adding a beneficiary makes a new wallet, which needs its own registration before the Ledger can approve its payments.
  - The move itself is signed under the old wallet's registration.
  - The Ledger shows the move as a payment of everything to one outside address: the new wallet's first address, which `satkeep sign` also shows.
- **The internal key.** In Savings with a beneficiary, the Ledger lists one more key than PHONE, DEVICE, HELPER and HEIR: the Taproot internal key that nobody can sign with (built from the NUMS point H; see `satkeep-savings`). The Bitcoin app labels such a key `dummy` from version 2.2.2.
- **Bitcoin app version.** Both shapes need Taproot miniscript, so 2.2.1 or later, and the latest is best. CI uses 2.4.6, the version Satkeep was tested against.
- **The network fee and amounts.** For plain Savings (P2WSH), every PSBT carries each coin's whole previous transaction, which the Ledger checks against the amount it signs. A Taproot signature covers every coin's amount anyway.

## Tests

- **`tests/speculos.rs`,** against Ledger's [Speculos](https://github.com/LedgerHQ/speculos) emulator running the Bitcoin Test app, on regtest:
  - **Allowed:** for both shapes, the Ledger registers the wallet, and the Ledger plus PHONE, or plus HELPER, spends it.
  - **Forbidden, refused by Satkeep and, built by hand, by the node:**
    - the Ledger alone, even after the heir delay;
    - the Ledger and HEIR before the heir delay.
  - **Forbidden on the Ledger:**
    - signing with an altered approval code;
    - signing for a wallet it never registered;
    - signing another wallet's payment;
    - registering a wallet whose DEVICE isn't this Ledger.
- **They're ignored by a plain `cargo test`,** since they need Speculos and `BITCOIND_EXE`. CI's **Ledger** workflow (`.github/workflows/ledger.yml`) builds the app and runs them:

  ```text
  cargo test -p satkeep-ledger --test speculos --locked -- --ignored --test-threads=1
  ```

- **The Ledger's buttons in Speculos** are pressed through Speculos's REST API (`tests/support/approver.rs`). It presses right until one of the screens Ledger's own tests approve on: "Approve", "Register account", "Sign transaction" or "Continue anyway". Then it presses both buttons.
- **Speculos uses its public default seed,** which holds nothing. The other keys are made fresh in each test.
- **What a failing test prints:** only the screens it saw, with every long string (keys, addresses, codes) cut out.
- **`satkeep-cli`'s `tests/ledger.rs`** covers the CLI without a Ledger. It checks that a missing, unreadable or wrong-wallet `ledger.json` is refused before any device is contacted, and that the approval code is never shown.

## Dependencies

Approved for this step on 2026-10-25, each pinned in `Cargo.lock`, none published in the 14 days before:

- **`async-hwi` `=0.0.32`** (BSD-3-Clause; from May 2026), with only its `ledger` feature. It brings:
  - `ledger_bitcoin_client`, Ledger's Bitcoin app protocol (Apache-2.0);
  - `ledger-transport-hidapi`, `ledger-transport` and `ledger-apdu` (Apache-2.0);
  - `hidapi` (MIT; it builds its bundled C USB library);
  - `tokio`, `futures`, `async-trait`, `regex` and a few small crates.

  `deny.toml` allows its BSD-3-Clause licence, read from its LICENSE file because its manifest names none.
- **`tokio` `=1.53.2`, with `rt`.** async-hwi is async, and a current-thread runtime runs each call to the end. It's already in the build through async-hwi.
- **Build scripts:** `hidapi`'s compiles its bundled C (`etc/hidapi`) and looks up `libudev` with pkg-config. It downloads nothing. `thiserror` 1, `syn` 1 and `doc-comment` only run `rustc --version`.
- **On Linux, building needs `libudev-dev`** (`sudo apt-get install libudev-dev`). CI installs it in every job that builds Rust.
