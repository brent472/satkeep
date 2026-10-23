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

## Check-ins

Each coin's delays count from when that coin confirmed. A check-in sends every coin back into Savings through the 2-of-3 path, so the device and heir delays start again from zero.

```text
twofold checkin --out checkin.psbt
twofold sign --psbt checkin.psbt --key-file phone.key     # shows "Check-in to approve", asks y/N
twofold sign --psbt checkin.psbt --key-file helper.key
twofold broadcast --psbt checkin.psbt
```

- **The interval.** Check-ins are due every `--checkin-interval` blocks (default 26,280, about 6 months). `create` refuses an interval that doesn't leave at least `--checkin-margin` blocks (default 4,320, about 30 days) before the device delay. So an interval at or past the device delay is always refused.
- **What `balance` says:**
  - Each coin's line shows its confirmations and when its device and heir paths open.
  - Before the interval it ends with `Next check-in due in N blocks`.
  - Once the oldest coin reaches the interval, the coin is marked `CHECK-IN DUE` and a warning says how many blocks are left before its device path opens.
  - Once any coin's device path is open, the coin is marked `OVERDUE` and the warning says DEVICE alone can already spend it.
- **Every confirmed coin.** `checkin` includes every confirmed coin.
  - Unconfirmed incoming coins are left out, and `checkin` says so: their delays haven't started, and including them would let whoever sent them stall the check-in. The library README explains this.
  - `sign` and `broadcast` check again. `broadcast` syncs first, so if a coin confirmed after the check-in was prepared, it refuses and asks you to run `twofold checkin` again.
- **Always 2-of-3.** A check-in carries no delay, so DEVICE or HEIR alone can't approve it, even once their paths are open.
- **Older wallets.** Wallets created before check-ins existed have no interval in `savings.json`. They use the defaults if those fit their device delay. Otherwise `balance` says check-ins aren't set up, and never says "due". It still shows each coin's delays and still warns when a coin is overdue, whatever the device delay.

## Commands

| Command | What it does | Needs a node |
| --- | --- | --- |
| `keygen --name NAME` (or `--out FILE.key`) | Makes a fresh test key and saves it. Prints only the public keys. | No |
| `create --phone … --device … --device-r … --helper … --heir …` | Creates the Savings wallet from five public keys. `--device-delay` and `--heir-delay` default to 39,420 and 61,320. `--checkin-interval` and `--checkin-margin` default to 26,280 and 4,320. | Yes |
| `address` | Shows a new receive address | No |
| `balance` | Syncs and shows the total, and each coin's confirmations and when its delayed paths open. Marks coins `CHECK-IN DUE` or `OVERDUE`, and says when the next check-in is due. | Yes |
| `checkin [--fee-rate 2] --out FILE.psbt` | Prepares a check-in: every coin, through the 2-of-3 path, into one new Savings address. Approve and send it with `sign` and `broadcast`. | Yes |
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

## Demo on your own regtest node (PowerShell)

This walks through every Savings path on a regtest node you run yourself:
- a check-in that clears an overdue warning and restarts both delays
- a 2-of-3 payment, including a declined approval and a refused single approval
- DEVICE alone, refused one block early, then sent
- HEIR alone, refused one block early and refused with HELPER's key, then sent

These are the exact steps that were run against Bitcoin Core v31.1.0; the check-in step was added with the check-in.

**What it changes:**
- It **mines about 133 blocks** on the node.
- It **saves four test keys** in `%USERPROFILE%\.twofold\regtest\keys`, and the wallet in `%USERPROFILE%\.twofold\regtest`.
- Nothing else on the node changes: it creates no node wallet, and funds Savings by mining a block straight to it.
- Payments go to an address derived from the NUMS point, which nobody can spend from.

The demo wallet uses **short delays (10 and 20 blocks) and a 5-block check-in interval** (with a 2-block margin), so the delayed paths open in minutes rather than the placeholder 39,420 and 61,320 blocks.

Run it from the repo root with the node already running on regtest. `bitcoin-cli` and `twofold` find the node and its cookie in their default places. Paste one step at a time; each `sign` shows the payment and asks `Approve? (y/N)`. Answer as noted.

```powershell
# 0. Setup
cargo build -p twofold-cli
$tf    = ".\target\debug\twofold.exe"
$bcli  = "C:\Program Files\Bitcoin\daemon\bitcoin-cli.exe"
$node  = @("-regtest")                       # arguments for bitcoin-cli
$base  = if ($env:TWOFOLD_HOME) { $env:TWOFOLD_HOME } else { "$env:USERPROFILE\.twofold" }
$demo  = "$base\regtest"                     # where twofold keeps the wallet
$keys  = "$demo\keys"                        # where keygen saves the keys

# 1. Four demo keys. keygen prints the public keys on lines starting with two spaces.
$pub = @{}
foreach ($n in "phone", "device", "helper", "heir") {
    $pub[$n] = @(& $tf keygen --name $n | Where-Object { $_ -like "  *" } | ForEach-Object { $_.Trim() })
}

# 2. The Savings wallet. DEVICE's second key is its delayed-path key (DEVICE_R).
& $tf create --device-delay 10 --heir-delay 20 --checkin-interval 5 --checkin-margin 2 --phone $pub.phone[0] --device $pub.device[0] --device-r $pub.device[1] --helper $pub.helper[0] --heir $pub.heir[0]

# 3. An address nobody can spend from, to pay to and mine to.
$nums = (& $bcli @node getdescriptorinfo "rawtr(50929b74c1a04954b78b4b6035e97a5e078a5a0f28ec96d547bfee9ace803ac0)" | ConvertFrom-Json).descriptor
$burn = (& $bcli @node deriveaddresses $nums | ConvertFrom-Json)[0]

# 4. Fund Savings: one block's reward straight to it, then 100 more blocks so that reward can be spent.
$null = & $bcli @node generatetoaddress 1 (& $tf address).Trim()
$null = & $bcli @node generatetoaddress 100 $burn
& $tf balance                                # 50 BTC, 101 confirmations: OVERDUE, both paths open

# 5. Check-in: every coin back into one new Savings address, restarting both delays.
& $tf checkin --out $demo\checkin.psbt
& $tf sign --psbt $demo\checkin.psbt --key-file $keys\phone.key    # answer y
& $tf sign --psbt $demo\checkin.psbt --key-file $keys\helper.key   # answer y
& $tf broadcast --psbt $demo\checkin.psbt                          # sent
$null = & $bcli @node generatetoaddress 1 $burn
& $tf balance                                # 1 confirmation, device path opens in 9 blocks, next check-in due in 4

# 6. Everyday payment: any 2 of PHONE, DEVICE, HELPER.
& $tf send --to $burn --amount 1 --out $demo\pay1.psbt
& $tf sign --psbt $demo\pay1.psbt --key-file $keys\phone.key     # answer n: nothing is signed
& $tf sign --psbt $demo\pay1.psbt --key-file $keys\phone.key     # answer y
& $tf broadcast --psbt $demo\pay1.psbt                           # refused: one approval isn't enough
& $tf sign --psbt $demo\pay1.psbt --key-file $keys\helper.key    # answer y
& $tf broadcast --psbt $demo\pay1.psbt                           # sent
$null = & $bcli @node generatetoaddress 1 $burn
& $tf balance                                # 49 BTC of change, a new coin with 1 confirmation

# 7. DEVICE alone: refused until the coin has 10 confirmations.
& $tf send --to $burn --amount 1 --path device --out $demo\pay2.psbt   # refused: has 1
$null = & $bcli @node generatetoaddress 8 $burn
& $tf send --to $burn --amount 1 --path device --out $demo\pay2.psbt   # refused: has 9
$null = & $bcli @node generatetoaddress 1 $burn
& $tf send --to $burn --amount 1 --path device --out $demo\pay2.psbt   # prepared
& $tf sign --psbt $demo\pay2.psbt --key-file $keys\device.key          # answer y (adds 2: DEVICE and DEVICE_R)
& $tf broadcast --psbt $demo\pay2.psbt                                 # sent
$null = & $bcli @node generatetoaddress 1 $burn

# 8. HEIR alone: refused until the coin has 20 confirmations.
& $tf send --to $burn --all --path heir --out $demo\pay3.psbt          # refused: has 1
$null = & $bcli @node generatetoaddress 18 $burn
& $tf send --to $burn --all --path heir --out $demo\pay3.psbt          # refused: has 19
$null = & $bcli @node generatetoaddress 1 $burn
& $tf send --to $burn --all --path heir --out $demo\pay3.psbt          # prepared
& $tf sign --psbt $demo\pay3.psbt --key-file $keys\helper.key          # answer y: adds a signature...
& $tf broadcast --psbt $demo\pay3.psbt                                 # ...but HELPER alone is refused
& $tf sign --psbt $demo\pay3.psbt --key-file $keys\heir.key            # answer y
& $tf broadcast --psbt $demo\pay3.psbt                                 # sent
$null = & $bcli @node generatetoaddress 1 $burn
& $tf balance                                # 0 BTC

# 9. The descriptor, for backup or another wallet.
& $tf export
```

**To run it again:**
- `keygen` won't overwrite the keys, and `create` won't overwrite the wallet. So first move or delete `%USERPROFILE%\.twofold\regtest` (it's test data only), or run in another folder by setting `$env:TWOFOLD_HOME` first.
- To use a different node, set `$node` (for example `@("-regtest", "-rpcport=18543", "-datadir=D:\other")`). Also set `$env:TWOFOLD_RPC_URL` and `$env:TWOFOLD_RPC_COOKIE`.
- Regtest's block reward halves every 150 blocks and reaches zero after about 4,950 blocks. On a long-lived node, step 4 stops funding Savings after roughly 35 runs.

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
| `tests/checkin.rs` | An interval at or past the device delay, or leaving less than the margin, is refused. Warnings change at exactly the interval (due) and the device delay (overdue). A check-in restarts both delays and clears the warning. One that leaves out a coin that confirmed later is refused at `broadcast` and at `sign`. An unconfirmed incoming coin is left out and doesn't stop the check-in being sent. DEVICE, HEIR or HELPER alone can't send one, even with the delayed paths open. `balance` still works, and still warns when overdue, for a wallet with no check-in rule. |
| `tests/keys.rs` | A private key anywhere on the command line is refused and never repeated, and there's no `--key` flag. `keygen` defaults to `~/.twofold/<network>/keys`, writes only `tprv` keys, never overwrites, and refuses mainnet, odd names and non-`.key` files. Wallet files default to `~/.twofold/<network>` and must match `--network`. |
| `tests/wallet.rs` | create, address, balance (including when each delayed path opens) and export work. The tool refuses: a node on another network, a mainnet `--network`, creating over an existing wallet, and any policy the library refuses. |
| `tests/payments.rs` | The 2-of-3 flow: one approval is refused, two are sent, and change comes back. DEVICE and HEIR alone are refused one block early and work on time. HELPER alone, a stranger's key and bad input are all refused. Keys from `keygen` work end to end. |

## Not here yet

- The signet demo
- The Liana/Sparrow exit drill
