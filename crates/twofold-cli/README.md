# twofold-cli

`twofold` is a developer command-line tool for the Twofold Savings wallet. It covers making test keys, creating the wallet, receiving, preparing a payment, approving it with any 2 keys (or one key through a delayed path), sending it, and exporting the descriptor.

**Test networks only.** `--network` accepts only `regtest` (the default) or `signet`. The tool also refuses to talk to a node whose chain doesn't match the wallet's, and refuses mainnet outright. It's a developer tool, not the app: its wording isn't product copy, and it uses technical terms like PSBT where the app wouldn't.

The spending rules themselves, and every assumption behind them, are in [`twofold-savings`](../twofold-savings/README.md).

## Keys and wallet files stay out of the repo

1. **Private keys never go on the command line.** `sign` reads a key only from a file (`--key-file FILE`) or from standard input (`--key-stdin`); no flag takes the key itself. A key typed as an argument lands in shell history and is visible in the process list. As a backstop, if any argument contains an extended private key (`tprv8…` or `xprv9…`), `twofold` refuses before parsing anything. It never repeats the key, and it warns that the key should be treated as exposed. (Without this check, the argument parser would repeat an unexpected argument in its error message.)
2. **`keygen` saves outside the repo by default**, to `%USERPROFILE%\.twofold\<network>\keys\<name>.key`, or `~/.twofold/…` elsewhere.
   - It never overwrites an existing file.
   - There is no mainnet option.
   - It writes only test-network keys (`tprv…`), and checks that before writing.
   - `--out FILE` saves somewhere else, but the name must end in `.key`, which the repo's `.gitignore` covers.
   - `--name` may only use letters, digits, `-` and `_`.
3. **Wallet files default to the same folder**, `%USERPROFILE%\.twofold\<network>\savings.json` and `wallet.sqlite`. `--wallet-dir` puts them elsewhere. `savings.json` is in `.gitignore` as a backstop, alongside the existing `*.sqlite` and `*.key` rules.

`TWOFOLD_HOME` replaces `%USERPROFILE%\.twofold` as the base folder; the tests use it to work in a temporary folder.

## Approving a payment

Before signing, `twofold sign` shows exactly what's being approved, then asks:

```text
Payment to approve:
  0.40000000 BTC to bcrt1p…
  0.59999570 BTC comes back to Savings as change (bcrt1p…)
  Network fee: 0.00000430 BTC
  Approval path: any 2 of PHONE, DEVICE, HELPER
Approve? (y/N)
```

- **Outputs:** every output's address and amount. Money returning to Savings is marked, as change or as "one of your own Savings addresses".
- **Network fee and approval path:** the fee, and which path the payment needs, read from the inputs.
- **Answer:** only `y` (or `yes`) signs. Anything else, an empty line or no input at all, signs nothing, leaves the file unchanged and exits with an error.
- **The key isn't read until you approve**, so a declined payment never touches it.
- **Standard input order:** with `--key-stdin`, standard input carries the answer first and the key on the next line. `--yes` skips the question, for scripts and tests; then standard input is just the key.
- **Wrong wallet:** `sign` refuses a payment that doesn't spend this wallet's coins, which is why it reads the wallet folder. It never contacts the node, so run `twofold balance` first if the wallet hasn't synced since the payment was prepared.

## Commands

| Command | What it does | Needs a node |
| --- | --- | --- |
| `keygen --name NAME` (or `--out FILE.key`) | Makes a fresh test key and saves it. Prints only the public keys. | No |
| `create --phone … --device … --device-r … --helper … --heir …` | Creates the Savings wallet from five public keys. `--device-delay` and `--heir-delay` default to 39,420 and 61,320. | Yes |
| `address` | Shows a new receive address | No |
| `balance` | Syncs and shows the total, and each coin's confirmations and when its delayed paths open | Yes |
| `export [--split]` | Prints the public descriptor, for backup or for importing into other software | No |
| `send --to ADDR (--amount BTC \| --all) [--path two-of-three\|device\|heir] [--fee-rate 2] --out FILE.psbt` | Prepares an unsigned payment. A delayed path is refused until every coin used has waited long enough. | Yes |
| `sign --psbt FILE.psbt (--key-file FILE.key \| --key-stdin) [--yes]` | Shows what the payment does, asks `Approve? (y/N)`, and only then signs with one holder's key, updating the file in place | No (reads the wallet folder) |
| `broadcast --psbt FILE.psbt` | Checks the payment has enough approvals and is old enough, then sends it | Yes |

Global options:

| Option | Default | Environment variable |
| --- | --- | --- |
| `--network regtest\|signet` | regtest, or the wallet's own network | `TWOFOLD_NETWORK` |
| `--wallet-dir` | `~/.twofold/<network>` | `TWOFOLD_WALLET_DIR` |
| `--rpc-url` | `http://127.0.0.1:18443` on regtest, `:38332` on signet | `TWOFOLD_RPC_URL` |
| `--rpc-cookie` | Bitcoin Core's cookie for that network, for example `%LOCALAPPDATA%\Bitcoin\regtest\.cookie` on Windows | `TWOFOLD_RPC_COOKIE` |

If you pass both `--wallet-dir` and `--network`, they must agree.

## Example on regtest (PowerShell)

These commands mine blocks on the node you point them at and save test keys under `%USERPROFILE%\.twofold\regtest\keys`, so only run them against a regtest node you're happy to use for that.

```powershell
cargo run -p twofold-cli -- keygen --name phone     # and the same for device, helper, heir
cargo run -p twofold-cli -- create --phone "<PHONE key>" --device "<DEVICE key>" --device-r "<DEVICE second key>" --helper "<HELPER key>" --heir "<HEIR key>"
cargo run -p twofold-cli -- address
# fund that address and mine a block, then:
cargo run -p twofold-cli -- send --to <address> --amount 0.1 --out pay.psbt
cargo run -p twofold-cli -- sign --psbt pay.psbt --key-file $env:USERPROFILE\.twofold\regtest\keys\phone.key
cargo run -p twofold-cli -- sign --psbt pay.psbt --key-file $env:USERPROFILE\.twofold\regtest\keys\helper.key
cargo run -p twofold-cli -- broadcast --psbt pay.psbt
```

## Assumptions

- **What the wallet files hold.**
  - `savings.json` holds the five public keys, the delays, the network and the block height the wallet was created at. `wallet.sqlite` is BDK's database.
  - Neither contains a private key, but together they reveal every Savings address and balance.
  - Sync starts at the creation height.
- **Key files.**
  - Each holds one holder's test master key as plain text, with no encryption and no OS permissions set. Fine for regtest and signet only.
  - DEVICE's file covers both DEVICE (account 0) and DEVICE_R (account 1), so `keygen` prints both public keys. The derivation is described in the library README.
- **Approving.**
  - A PSBT carries every leaf of each coin, so a holder may add a signature the chosen path doesn't need (for example HELPER on an heir payment). That's harmless: `broadcast` only sends a payment whose signatures actually satisfy a path.
  - `sign` refuses a key that adds nothing.
- **Fees.** The rate is set by `--fee-rate` in sat/vB, default 2, minimum 1, with no fee estimation.
- **Node access.** Over RPC with cookie auth, using raw JSON calls, because `bitcoincore-rpc`'s typed responses lag behind newer Core versions. The tool never changes anything on the node except by broadcasting a payment.

## Tests

The tests run the real `twofold` binary against throwaway regtest nodes, the same as the library's tests, with `TWOFOLD_HOME` set to a temporary folder. Every run fails if any output contains `tprv` or `xprv`. Keys are generated in memory and passed through stdin, except in the `keygen` and `--key-file` tests, which write fresh keys into the temporary folder; it's deleted when each test ends.

```bash
BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe" cargo test -p twofold-cli
```

```powershell
$env:BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe"; cargo test -p twofold-cli
```

| File | What it proves |
| --- | --- |
| `tests/approve.rs` | `sign` shows each output (marking change and your own addresses), the fee and the path before asking. It signs on `y` only: `n`, `no`, an empty line, other text or no input leave the PSBT unchanged. A declined payment never reads the key. A payment from another wallet is refused. |
| `tests/keys.rs` | A private key anywhere on the command line is refused and never repeated, and there's no `--key` flag. `keygen` defaults to `~/.twofold/<network>/keys`, writes only `tprv` keys, never overwrites, and refuses mainnet, odd names and non-`.key` files. Wallet files default to `~/.twofold/<network>` and must match `--network`. |
| `tests/wallet.rs` | create, address, balance (including when each delayed path opens) and export work. The tool refuses: a node on another network, a mainnet `--network`, creating over an existing wallet, and any policy the library refuses. |
| `tests/payments.rs` | The 2-of-3 flow: one approval is refused, two are sent, and change comes back. DEVICE and HEIR alone are refused one block early and work on time. HELPER alone, a stranger's key and bad input are all refused. Keys from `keygen` work end to end. |

## Not here yet

- The check-in (the next PR)
- The signet demo
- The Liana/Sparrow exit drill
