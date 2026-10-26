# satkeep-cli

`satkeep` is a developer command-line tool for the Satkeep Savings wallet. It covers making test keys, creating the wallet, receiving, preparing a payment, approving it with any 2 keys (or one key through a delayed path), sending it, and exporting the descriptor.

**Test networks only.** `--network` accepts only `regtest` (the default) or `signet`. The tool also refuses to talk to a node whose chain doesn't match the wallet's, and refuses mainnet outright. It's a developer tool, not the app: its wording isn't product copy, and it uses technical terms like PSBT where the app wouldn't.

The spending rules themselves, and every assumption behind them, are in [`satkeep-savings`](../satkeep-savings/README.md).

## Keys and wallet files stay out of the repo

1. **Private keys never go on the command line.** A key is a 12-word recovery phrase (see the [library README](../satkeep-savings/README.md#recovery-phrases-and-derivation) for how keys come from it). `sign` reads it only from a file (`--key-file FILE`) or from standard input (`--key-stdin`); no flag takes the key itself. A key typed as an argument lands in shell history and is visible in the process list. As a backstop, `satkeep` refuses before parsing anything if the arguments hold 12 BIP39 words in a row (quoted as one argument or spread over several) or an extended private key (`tprv8…` or `xprv9…`). It never repeats the key, and it warns that the key should be treated as exposed. (Without this check, the argument parser would repeat an unexpected argument in its error message.)
2. **`keygen` saves outside the repo by default**, to `%USERPROFILE%\.satkeep\<network>\keys\<name>.key`, or `~/.satkeep/…` elsewhere.
   - It never overwrites an existing file.
   - There is no mainnet option.
   - It writes a fresh 12-word recovery phrase, and checks the phrase reads back as the same key before writing. The phrase is never printed: only the public keys are.
   - Key files from before this change held a `tprv` key. Satkeep no longer reads them and says so; make new keys (and a new wallet) with `keygen`.
   - `--out FILE` saves somewhere else, but the name must end in `.key`, which the repo's `.gitignore` covers.
   - `--name` may only use letters, digits, `-` and `_`.
3. **Wallet files default to the same folder**, `%USERPROFILE%\.satkeep\<network>\savings.json` and `wallet.sqlite`. `--wallet-dir` puts them elsewhere. `savings.json` is in `.gitignore` as a backstop, alongside the existing `*.sqlite` and `*.key` rules.

`SATKEEP_HOME` replaces `%USERPROFILE%\.satkeep` as the base folder; the tests use it to work in a temporary folder.

**Renamed from Twofold** (brief v3.14). The tool was `twofold`, its folder `~/.twofold` and its variables `TWOFOLD_*`; none of those are read any more. Test keys and wallets made before the rename stay where they were, and PSBTs made before it carry the old marker, so make them all again (test networks only, so nothing real is lost).

## Approving a payment

Before signing, `satkeep sign` shows exactly what's being approved, then asks:

```text
Payment to approve:
  0.40000000 BTC to bcrt1p…
  0.59999570 BTC comes back to Savings as change (bcrt1p…)
  Network fee: 0.00000430 BTC
  Approval path: any 2 of PHONE, DEVICE, HELPER
Approve? (y/N)
```

- **Outputs:** every output's address and amount. Money returning to Savings is marked, as change or as "one of your own Savings addresses".
- **Network fee and approval path:** the fee, and which path the payment needs, read from the inputs. Every input's amount is checked against the wallet's own record first, so the fee shown is the fee paid; a payment that misstates a coin is refused.
- **A signature on every coin:** `sign` refuses, and saves nothing, unless the key adds a signature to every coin the payment spends.
- **Answer:** only `y` (or `yes`) signs. Anything else, an empty line or no input at all, signs nothing, leaves the file unchanged and exits with an error.
- **The key isn't read until you approve**, so a declined payment never touches it.
- **Standard input order:** with `--key-stdin`, standard input carries the answer first and the key on the next line. `--yes` skips the question, for scripts and tests; then standard input is just the key.
- **Wrong wallet:** `sign` refuses a payment that doesn't spend this wallet's coins, which is why it reads the wallet folder. It never contacts the node, so run `satkeep balance` first if the wallet hasn't synced since the payment was prepared.

## Adding a beneficiary

Savings starts with no beneficiary: plain 2 of 3, with no delayed path and no check-ins. Adding one makes a new wallet and moves Savings there in one payment:

```text
satkeep add-heir --heir <HEIR's public key> --new-wallet-dir <folder> --out move.psbt
satkeep sign --psbt move.psbt --key-file phone.key      # shows "Move to approve", asks y/N
satkeep sign --psbt move.psbt --key-file helper.key
satkeep broadcast --psbt move.psbt
satkeep --wallet-dir <folder> balance                   # from now on, use the new folder
```

- **The new wallet** has the same PHONE, DEVICE and HELPER plus HEIR. `--heir-delay`, `--checkin-interval` and `--checkin-margin` default as for `create`.
- **One payment, one network fee.** It moves every confirmed coin through the 2-of-3 path. Unconfirmed coins are left out; they stay spendable by 2 of 3 and can be moved once they confirm.
- **Where it goes comes from this wallet's settings, not the payment.** `add-heir` records the beneficiary and heir delay in this wallet's `savings.json` (`moving_to`). `sign` and `broadcast` rebuild the new wallet from that and this wallet's own keys, and refuse a move that goes anywhere else: to a wallet with a different HEIR key or a shorter heir delay, with an extra output, or leaving out a confirmed coin. A payment that pays the new wallet is checked as a move even if it isn't marked as one. A wallet with no move set up refuses any move.
- **`sign` shows HEIR's key and the heir delay**, in blocks and about how long, to compare with the beneficiary's before approving.
- **Minimum heir delay.** The heir delay can't go below the wallet's minimum (`--min-heir-delay` on `create`, kept by `add-heir`; placeholder default 26,280 blocks, about 6 months).
- **One copy of the settings.** The check uses the `savings.json` in the wallet folder `sign` reads. A holder signing from their own copy of the folder needs the same `moving_to` there; in the app, this is where each device's own record of the beneficiary would go.
- **Changing the beneficiary** is the same command, run from the wallet that has one.

## Check-ins

Only Savings with a beneficiary needs check-ins: without one, nothing opens with time, and `checkin` refuses. Each coin's heir delay counts from when that coin confirmed. A check-in sends every coin back into Savings through the 2-of-3 path, so the heir delay starts again from zero.

```text
satkeep checkin --out checkin.psbt
satkeep sign --psbt checkin.psbt --key-file phone.key     # shows "Check-in to approve", asks y/N
satkeep sign --psbt checkin.psbt --key-file helper.key
satkeep broadcast --psbt checkin.psbt
```

- **The interval.** Check-ins are due every `--checkin-interval` blocks (default 52,560, about 12 months). `create` refuses an interval that doesn't leave at least `--checkin-margin` blocks (default 8,760, about 2 months) before the heir delay. So an interval at or past the heir delay is always refused.
- **What `balance` says:**
  - Each coin's line shows its confirmations and when its heir path opens.
  - Before the interval it ends with `Next check-in due in N blocks`.
  - Once the oldest coin reaches the interval, the coin is marked `CHECK-IN DUE` and a warning says how many blocks are left before its heir path opens.
  - Once any coin's heir path is open, the coin is marked `OVERDUE` and the warning says HEIR alone can already spend it.
- **Every confirmed coin.** `checkin` includes every confirmed coin.
  - Unconfirmed incoming coins are left out, and `checkin` says so: their delays haven't started, and including them would let whoever sent them stall the check-in. The library README explains this.
  - `sign` and `broadcast` check again. `broadcast` syncs first, so if a coin confirmed after the check-in was prepared, it refuses and asks you to run `satkeep checkin` again.
- **Always 2-of-3.** A check-in carries no delay, so HEIR alone can't approve it, even once its path is open.
- **Payments count as check-ins** (the payment rule, brief 3.12).
  - **What it does:** with a beneficiary, every 2-of-3 `send` also spends each coin within `--refresh-window` blocks of its check-in being due, with change back to Savings. The default window is 8,760 blocks (about 2 months), so with the default interval that's coins older than about 10 months. Their heir delay restarts once the payment confirms.
  - **What you see:** `send` and `sign` say which coins a payment refreshes. `sign` also says if a payment leaves out a coin due for a refresh; that isn't refused.
  - **The setting:** on by default, as a wallet setting. `create --no-refresh-with-payments` (or `add-heir --no-refresh-with-payments`) turns it off, and so does setting `refresh_with_payments` to `false` in `savings.json`. It links coins together on-chain, so turning it off is the private choice; only check-ins then restart the heir delay. Including old coins also adds to the network fee.
- **Older wallets.** Wallets created before check-ins existed have no interval in `savings.json`. They use the defaults if those fit their heir delay. Otherwise `balance` says check-ins aren't set up, and never says "due". It still shows each coin's heir delay and still warns when a coin is overdue.
- **Wallets made with the card-only path** (DEVICE_R, removed in brief 3.8) can't be opened; `satkeep` says so. Make a new wallet.

## Commands

| Command | What it does | Needs a node |
| --- | --- | --- |
| `keygen --name NAME` (or `--out FILE.key`) | Makes a fresh test key and saves it. Prints only its public key. | No |
| `create --phone … --device … --helper … [--heir …] [--min-heir-delay …]` | Creates the Savings wallet. Without `--heir`: plain 2 of 3 in the standard P2WSH format (`wsh(sortedmulti(2, …))`), no delayed path, no check-ins. With it: HEIR can also spend alone after `--heir-delay` (default 61,320); `--checkin-interval` and `--checkin-margin` default to 52,560 and 8,760, `--refresh-window` to 8,760 (or `--no-refresh-with-payments`), and all need `--heir`. `--min-heir-delay` (default 26,280) is the shortest heir delay the wallet accepts, now and in any move. | Yes |
| `add-heir --heir … --new-wallet-dir DIR [--heir-delay …] [--fee-rate 2] --out FILE.psbt` | Adds or changes the beneficiary: creates the new wallet in DIR, prepares the one 2-of-3 payment moving every confirmed coin there, and records the move in this wallet's settings for `sign` and `broadcast` to check against | Yes |
| `address` | Shows a new receive address | No |
| `balance` | Syncs and shows the total, and each coin's confirmations and, with a beneficiary, when its heir path opens. Marks coins `CHECK-IN DUE` or `OVERDUE`, and says when the next check-in is due. | Yes |
| `checkin [--fee-rate 2] --out FILE.psbt` | Prepares a check-in: every coin, through the 2-of-3 path, into one new Savings address. Approve and send it with `sign` and `broadcast`. Only with a beneficiary. | Yes |
| `export [--split]` | Prints the public descriptor, for backup or for importing into other software | No |
| `send --to ADDR (--amount BTC \| --all) [--path two-of-three\|heir] [--fee-rate 2] --out FILE.psbt` | Prepares an unsigned payment. The heir path is refused without a beneficiary, and until every coin used has waited long enough. With a beneficiary and the payment rule on, a 2-of-3 payment also spends the coins due for a refresh. | Yes |
| `sign --psbt FILE.psbt (--key-file FILE.key \| --key-stdin) [--yes]` | Shows what the payment does, asks `Approve? (y/N)`, and only then signs with one holder's key, updating the file in place | No (reads the wallet folder) |
| `broadcast --psbt FILE.psbt` | Checks the payment has enough approvals and is old enough, then sends it | Yes |

Global options:

| Option | Default | Environment variable |
| --- | --- | --- |
| `--network regtest\|signet` | regtest, or the wallet's own network | `SATKEEP_NETWORK` |
| `--wallet-dir` | `~/.satkeep/<network>` | `SATKEEP_WALLET_DIR` |
| `--rpc-url` | `http://127.0.0.1:18443` on regtest, `:38332` on signet | `SATKEEP_RPC_URL` |
| `--rpc-cookie` | Bitcoin Core's cookie for that network, for example `%LOCALAPPDATA%\Bitcoin\regtest\.cookie` on Windows | `SATKEEP_RPC_COOKIE` |

If you pass both `--wallet-dir` and `--network`, they must agree.

**A pruned node works.** A wallet reads blocks from its birthday (the node's height at `create` or `add-heir`) on, starting from the block before it, and later from where it last synced. The node must still have those blocks: a wallet left unsynced for longer than the node keeps blocks gets a "pruned data" error. (Bitcoin Core always keeps at least the last 288 blocks, about two days; on signet with `-prune=550` it currently keeps about a week.)

## Demo on your own regtest node (PowerShell)

This walks through every Savings path on a regtest node you run yourself:
- Savings with no beneficiary: a 2-of-3 payment, including a declined approval and a refused single approval, and no check-in to do
- adding a beneficiary, which moves Savings to a new wallet in one payment
- a check-in that clears an overdue warning and restarts the heir delay
- HEIR alone, refused one block early and refused with HELPER's key, then sent

**What it changes:**
- It **mines about 145 blocks** on the node.
- It **saves four test keys** in `%USERPROFILE%\.satkeep\regtest\keys`, and two wallets in `%USERPROFILE%\.satkeep\regtest` and `%USERPROFILE%\.satkeep\regtest-heir`.
- Nothing else on the node changes: it creates no node wallet, and funds Savings by mining a block straight to it.
- Payments go to an address derived from the NUMS point, which nobody can spend from.

The demo uses a **short heir delay (20 blocks) and a 10-block check-in interval** (with a 5-block margin), so the heir path opens in minutes rather than the placeholder 61,320 blocks.

Run it from the repo root with the node already running on regtest. `bitcoin-cli` and `satkeep` find the node and its cookie in their default places. Paste one step at a time; each `sign` shows the payment and asks `Approve? (y/N)`. Answer as noted.

```powershell
# 0. Setup
cargo build -p satkeep-cli
$tf    = ".\target\debug\satkeep.exe"
$bcli  = "C:\Program Files\Bitcoin\daemon\bitcoin-cli.exe"
$node  = @("-regtest")                       # arguments for bitcoin-cli
$base  = if ($env:SATKEEP_HOME) { $env:SATKEEP_HOME } else { "$env:USERPROFILE\.satkeep" }
$demo  = "$base\regtest"                     # where satkeep keeps the first wallet
$heirs = "$base\regtest-heir"                # the wallet with a beneficiary
$keys  = "$demo\keys"                        # where keygen saves the keys

# 1. Four demo keys. keygen prints the public key on a line starting with two spaces.
$pub = @{}
foreach ($n in "phone", "device", "helper", "heir") {
    $pub[$n] = (& $tf keygen --name $n | Where-Object { $_ -like "  *" }).Trim()
}

# 2. Savings with no beneficiary yet: plain 2 of 3. The minimum heir delay is
#    lowered from its placeholder (26,280 blocks) so the demo can use 20.
& $tf create --phone $pub.phone --device $pub.device --helper $pub.helper --min-heir-delay 10

# 3. An address nobody can spend from, to pay to and mine to.
$nums = (& $bcli @node getdescriptorinfo "rawtr(50929b74c1a04954b78b4b6035e97a5e078a5a0f28ec96d547bfee9ace803ac0)" | ConvertFrom-Json).descriptor
$burn = (& $bcli @node deriveaddresses $nums | ConvertFrom-Json)[0]

# 4. Fund Savings: one block's reward straight to it, then 100 more blocks so that reward can be spent.
$null = & $bcli @node generatetoaddress 1 (& $tf address).Trim()
$null = & $bcli @node generatetoaddress 100 $burn
& $tf balance                                # 50 BTC, 101 confirmations; nothing opens with time
& $tf checkin --out $demo\checkin.psbt       # refused: no beneficiary, no check-in needed

# 5. Everyday payment: any 2 of PHONE, DEVICE, HELPER.
& $tf send --to $burn --amount 1 --out $demo\pay1.psbt
& $tf sign --psbt $demo\pay1.psbt --key-file $keys\phone.key     # answer n: nothing is signed
& $tf sign --psbt $demo\pay1.psbt --key-file $keys\phone.key     # answer y
& $tf broadcast --psbt $demo\pay1.psbt                           # refused: one approval isn't enough
& $tf sign --psbt $demo\pay1.psbt --key-file $keys\helper.key    # answer y
& $tf broadcast --psbt $demo\pay1.psbt                           # sent
$null = & $bcli @node generatetoaddress 1 $burn

# 6. Add a beneficiary: a new wallet with HEIR, and one payment moving everything there.
& $tf add-heir --heir $pub.heir --heir-delay 20 --checkin-interval 10 --checkin-margin 5 --refresh-window 4 --new-wallet-dir $heirs --out $demo\move.psbt
& $tf sign --psbt $demo\move.psbt --key-file $keys\phone.key    # answer y: shows "Move to approve"
& $tf sign --psbt $demo\move.psbt --key-file $keys\device.key   # answer y
& $tf broadcast --psbt $demo\move.psbt                          # sent
$null = & $bcli @node generatetoaddress 1 $burn
& $tf --wallet-dir $heirs balance                               # 1 confirmation, heir path opens in 19 blocks

# 7. Let the coin reach the heir delay, then check in: the warning clears and the delay restarts.
$null = & $bcli @node generatetoaddress 19 $burn
& $tf --wallet-dir $heirs balance                               # 20 confirmations: OVERDUE
& $tf --wallet-dir $heirs checkin --out $heirs\checkin.psbt
& $tf --wallet-dir $heirs sign --psbt $heirs\checkin.psbt --key-file $keys\phone.key    # answer y
& $tf --wallet-dir $heirs sign --psbt $heirs\checkin.psbt --key-file $keys\helper.key   # answer y
& $tf --wallet-dir $heirs broadcast --psbt $heirs\checkin.psbt                          # sent
$null = & $bcli @node generatetoaddress 1 $burn
& $tf --wallet-dir $heirs balance                               # 1 confirmation, next check-in due in 9

# 8. HEIR alone: refused until the coin has 20 confirmations.
& $tf --wallet-dir $heirs send --to $burn --all --path heir --out $heirs\pay2.psbt      # refused: has 1
$null = & $bcli @node generatetoaddress 18 $burn
& $tf --wallet-dir $heirs send --to $burn --all --path heir --out $heirs\pay2.psbt      # refused: has 19
$null = & $bcli @node generatetoaddress 1 $burn
& $tf --wallet-dir $heirs send --to $burn --all --path heir --out $heirs\pay2.psbt      # prepared
& $tf --wallet-dir $heirs sign --psbt $heirs\pay2.psbt --key-file $keys\helper.key     # answer y: adds a signature...
& $tf --wallet-dir $heirs broadcast --psbt $heirs\pay2.psbt                            # ...but HELPER alone is refused
& $tf --wallet-dir $heirs sign --psbt $heirs\pay2.psbt --key-file $keys\heir.key       # answer y
& $tf --wallet-dir $heirs broadcast --psbt $heirs\pay2.psbt                            # sent
$null = & $bcli @node generatetoaddress 1 $burn
& $tf --wallet-dir $heirs balance                               # 0 BTC

# 9. The descriptors, for backup or another wallet.
& $tf export
& $tf --wallet-dir $heirs export
```

**To run it again:**
- `keygen` won't overwrite the keys, and `create` or `add-heir` won't overwrite a wallet. So first move or delete `%USERPROFILE%\.satkeep\regtest` and `regtest-heir` (test data only), or run in another folder by setting `$env:SATKEEP_HOME` first.
- To use a different node, set `$node` (for example `@("-regtest", "-rpcport=18543", "-datadir=D:\other")`). Also set `$env:SATKEEP_RPC_URL` and `$env:SATKEEP_RPC_COOKIE`.
- Regtest's block reward halves every 150 blocks and reaches zero after about 4,950 blocks. On a long-lived node, step 4 stops funding Savings after roughly 30 runs.
- **This version of the demo hasn't been run against a node yet.** The CLI tests cover every step it uses.

## Assumptions

- **What the wallet files hold.**
  - `savings.json` holds the public keys (three, or four with a beneficiary), the heir delay and check-in settings when there's a beneficiary, the minimum heir delay if it isn't the default, the beneficiary of a move set up by `add-heir`, the network and the block height the wallet was created at. `wallet.sqlite` is BDK's database.
  - Anyone who can edit `savings.json` can change where a move is checked against. It's as sensitive as the wallet folder itself.
  - Neither contains a private key, but together they reveal every Savings address and balance.
  - Sync starts at the creation height.
- **Key files.**
  - Each holds one holder's test master key as plain text, with no encryption and no OS permissions set. Fine for regtest and signet only.
  - Each key file gives one public key, on account 0. The derivation is described in the library README.
- **Approving.**
  - A PSBT carries every leaf of each coin, so a holder may add a signature the chosen path doesn't need (for example HELPER on an heir payment). That's harmless: `broadcast` only sends a payment whose signatures actually satisfy a path.
  - `sign` refuses a key that adds nothing.
- **Fees.** The rate is set by `--fee-rate` in sat/vB, default 2, minimum 1, with no fee estimation.
- **Fee ceiling on check-ins and moves.** `checkin`, `add-heir`, `sign` and `broadcast` refuse a check-in or move whose network fee is above 0.005 BTC or above 5% of what it spends (placeholders). That catches a fee-rate typo and a PSBT whose one output was edited down. To change it, set `max_fee_sat` and `max_fee_percent` in `savings.json`; `add-heir` copies them to the new wallet. Plain payments have no ceiling yet; `sign` shows their fee.
- **Node access.** Over RPC with cookie auth, using raw JSON calls, because `bitcoincore-rpc`'s typed responses lag behind newer Core versions. The tool never changes anything on the node except by broadcasting a payment.

## Tests

The tests run the real `satkeep` binary against throwaway regtest nodes, the same as the library's tests, with `SATKEEP_HOME` set to a temporary folder. Every run fails if any output contains `tprv` or `xprv`, 12 BIP39 words in a row, or 3 words in a row from any recovery phrase the test knows. Keys are generated in memory and passed through stdin, except in the `keygen` and `--key-file` tests, which write fresh keys into the temporary folder; it's deleted when each test ends.

```bash
BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe" cargo test -p satkeep-cli
```

```powershell
$env:BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe"; cargo test -p satkeep-cli
```

| File | What it proves |
| --- | --- |
| `tests/approve.rs` | `sign` shows each output (marking change and your own addresses), the fee and the path before asking. It signs on `y` only: `n`, `no`, an empty line, other text or no input leave the PSBT unchanged. A declined payment never reads the key. A payment from another wallet is refused. |
| `tests/payment_rule.rs` | `send` also spends the old coin due for a refresh and says so, and `sign` shows it before approving. Once the payment confirms, the change has a fresh heir delay and HEIR alone is refused. With `--no-refresh-with-payments`, `send` forces nothing in, and `create` and `savings.json` say so. Turning it back on in `savings.json` brings it back. A refresh window of zero, equal to the interval or longer, or the default against a 10-block interval, is refused; the default fits the placeholder interval. Plain Savings has no payment rule. |
| `tests/checkin.rs` | An interval at or past the heir delay, or leaving less than the margin, is refused; the yearly defaults fit the placeholder heir delay. Savings with no beneficiary refuses a check-in. Warnings change at exactly the interval (due) and the heir delay (overdue). A check-in restarts the heir delay and clears the warning. One that leaves out a coin that confirmed later is refused at `broadcast` and at `sign`. An unconfirmed incoming coin is left out and doesn't stop the check-in being sent. DEVICE, HEIR or HELPER alone can't send one, even with the heir path open. `balance` still works, and still warns when overdue, for a wallet with no check-in rule. |
| `tests/beneficiary.rs` | `add-heir` creates the new wallet and moves both coins in one payment; `sign` shows it as a move, with HEIR's key and the heir delay; one approval isn't enough; afterwards HEIR spends from the new wallet at its delay. It refuses a missing or duplicate HEIR key, check-in settings that don't fit, an existing folder, and (at `broadcast`) a move that leaves out a coin that confirmed later. Against this wallet's settings, `sign` refuses, saving nothing, a move to a wallet with a different HEIR key or a shorter heir delay (marked honestly or claiming the right HEIR), one with an extra output (marked or not), and any move where none is set up; `broadcast` refuses a shorter-delay move signed by two keys and an extra output added after approval. An heir delay below the minimum is refused by `create`, by `add-heir`, and at `sign`. |
| `tests/keys.rs` | A recovery phrase (quoted, unquoted or inside another argument) or an old `tprv` key anywhere on the command line is refused and never repeated, and there's no `--key` flag. `keygen` defaults to `~/.satkeep/<network>/keys`, writes a 12-word phrase, never overwrites, and refuses mainnet, odd names and non-`.key` files. Wallet files default to `~/.satkeep/<network>` and must match `--network`. |
| `tests/wallet.rs` | create, address, balance and export work in both shapes; with no beneficiary, nothing opens with time and `savings.json` holds no heir fields. The tool refuses: check-in settings without `--heir`, the old `--device-r` and `--device-delay`, a node on another network, a mainnet `--network`, creating over an existing wallet, any policy the library refuses, and an old wallet with DEVICE_R (with a clear error). |
| `tests/payments.rs` | The 2-of-3 flow: one approval is refused, two are sent, and change comes back. HEIR alone is refused one block early and works on time. DEVICE alone can never send: there's no such path, and its signature alone never satisfies 2 of 3. Without a beneficiary there's no heir path. HELPER alone, a stranger's key and bad input are all refused. Keys from `keygen` work end to end. |

## On signet

The [signet demo](../../docs/signet-demo.md) runs every allowed path with this tool on the public signet network, in both shapes, and lists the transactions. Its script is [`contrib/signet-demo/demo.ps1`](../../contrib/signet-demo/demo.ps1).
