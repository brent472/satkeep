# A real Ledger as DEVICE, on signet

Step-by-step instructions for testing `satkeep-ledger` (step 1, approving Savings payments) with a real Ledger, on your Windows PC. **Test bitcoin only.** The Ledger runs the **Bitcoin Test** app, which can't sign for real bitcoin.

The Ledger gets a new key on the test path, and the wallets here are new: nothing from the earlier signet run is used or changed. It takes about an hour, mostly waiting for confirmations.

**What it checks,** with your Ledger as DEVICE and fresh test keys for PHONE, HELPER and HEIR:

- **Savings with no beneficiary:**
  - the Ledger shows and gives its key, and registers the wallet;
  - the Ledger plus PHONE pays;
  - the Ledger alone is refused.
- **Adding a beneficiary:**
  - the Ledger approves the move;
  - it registers the new wallet;
  - the Ledger plus HELPER pays.
- **At the end:** what's left goes back to the faucet.

Run everything in one Windows PowerShell window, from `C:\Users\Brent\Documents\twofold`, on `main`.

**Paste one command at a time, on an empty prompt.** Pasting several at once, or onto a line that isn't empty, can run them together on one line.

## Results: passed on 29 October 2026

**Every step worked as written,** on `main` at `91ce841`, with a real **Ledger Nano X** running **Bitcoin Test 2.5.1**, on signet (blocks 328,592–328,598). Each transaction below was checked against the signet node, and each spends the one before it.

| Step | What | Approved by | Block | Network fee | Transaction |
| --- | --- | --- | --- | --- | --- |
| 5 | Funding, from a public faucet: output 37, 2,531,619 sats | the faucet | 328,592 | — | [`12b3a861…01e9f`](https://mempool.space/signet/tx/12b3a8612fcf4a442b3603879069b7955e0b50801a2e195e771267b728a01e9f) |
| 6 | Savings with no beneficiary pays 1,000 sats to the faucet | Ledger + PHONE | 328,594 | 402 sats (201 vB) | [`40a16019…3b619`](https://mempool.space/signet/tx/40a1601992561bfcda9554502490cdd6d2a05ce37aea775a30cabccf57f3b619) |
| 7 | The Ledger alone: refused by `broadcast` with "not enough keys have approved this payment"; nothing sent | Ledger | — | — | — |
| 9 | The move to the wallet with HEIR: everything, in one payment | Ledger + PHONE | 328,596 | 316 sats (158 vB) | [`1daacb8c…35ef9`](https://mempool.space/signet/tx/1daacb8c129c1f859c8be9e394c32df8ea92c95f5200e6cdf5f70cfe50935ef9) |
| 11 | The Taproot wallet pays 1,000 sats to the faucet | Ledger + HELPER | 328,597 | 428 sats (214 vB) | [`bb9820c4…32a9b`](https://mempool.space/signet/tx/bb9820c41962315c23dc8e0501c7627c69ca5dda1c60b0e8399c1ea371332a9b) |
| 12 | The rest, 2,528,131 sats, back to the faucet | Ledger + PHONE | 328,598 | 342 sats (171 vB) | [`0f8368fc…f145e`](https://mempool.space/signet/tx/0f8368fc347fcce1a137623df170174d049b6fc0af4a949cadd5b467873f145e) |

- **The faucet sent far more than needed:** about 0.025 signet BTC, not 0.0002. Step 12 gave it all back, less the network fees.
- **The move's fee** was 0.01% of what it moved, far inside the 5% ceiling.
- **One thing to fix in these steps:** twice, pasted commands ran together onto one line in PowerShell. The guide now says to paste one command at a time, on an empty prompt.

## Before you start

1. **Start the signet node** in its own PowerShell window, pruned as in the signet run, and leave it running:
   ```powershell
   & "C:\Program Files\Bitcoin\daemon\bitcoind.exe" -signet -prune=550
   ```
   Then, in your main window, check it has caught up:
   ```powershell
   & "C:\Program Files\Bitcoin\daemon\bitcoin-cli.exe" -signet getblockchaininfo
   ```
   It must show `"initialblockdownload": false`.
2. **Update your Ledger.** In Ledger Live, update its firmware if offered.
3. **Install the Bitcoin Test app:**
   - In Ledger Live, go to **Settings**, then **Experimental features**, and turn on **Developer mode**.
   - Then, in **My Ledger**, install **Bitcoin Test** from the app catalog. It needs version 2.2.1 or later; take the latest.
   - Leave the normal Bitcoin app alone.
4. **Close Ledger Live completely,** including from the system tray. Only one program can talk to the Ledger at a time.
5. **Plug in the Ledger with its USB cable.** `satkeep` talks to a Ledger over USB only, so a Nano X's Bluetooth won't work.
6. **Get `main` and build the tool:**
   ```powershell
   cd C:\Users\Brent\Documents\twofold
   git switch main
   git pull
   cargo build -p satkeep-cli
   $sk = ".\target\debug\satkeep.exe"
   ```
7. **Open the Bitcoin Test app:** unlock the Ledger with its PIN, and open **Bitcoin Test**.

## A. Savings with no beneficiary

1. **Three fresh test keys:** PHONE, HELPER and HEIR. They go in your `.satkeep\signet\keys` folder, under new names:
   ```powershell
   $phone = (& $sk --network signet keygen --name ledger-phone | Where-Object { $_ -like "  *" }).Trim()
   $helper = (& $sk --network signet keygen --name ledger-helper | Where-Object { $_ -like "  *" }).Trim()
   $heir = (& $sk --network signet keygen --name ledger-heir | Where-Object { $_ -like "  *" }).Trim()
   $phone; $helper; $heir
   ```
   Each prints one public key, starting `[` and ending `/<0;1>/*`.
2. **Your Ledger's key, DEVICE:**
   ```powershell
   $device = (& $sk --network signet ledger-key | Where-Object { $_ -like "  *" }).Trim()
   ```
   - The Ledger shows a path, `m/87'/1'/0'`, and a public key starting `tpub`. Read through the screens with the right button, and approve with both buttons.
   - Then type:
     ```powershell
     $device
     ```
     It should start with your Ledger's fingerprint in brackets, and its `tpub` should be the one the Ledger showed.
3. **Create the wallet,** in its own folder:
   ```powershell
   $w = "$env:USERPROFILE\.satkeep\signet-ledger"
   & $sk --network signet --wallet-dir $w create --phone $phone --device $device --helper $helper --min-heir-delay 10
   ```
4. **Register it on the Ledger:**
   ```powershell
   & $sk --wallet-dir $w ledger-register
   ```
   - The tool prints the wallet's name and its three keys.
   - The Ledger shows the same: the name **Satkeep Savings**, the wallet's rules, then each key. Check each key on the Ledger against the tool's list, then approve.
   - The tool ends with "Registered on your Ledger" and saves `ledger.json` in the wallet folder.
5. **Fund it.** Get an address:
   ```powershell
   & $sk --wallet-dir $w address
   ```
   Send it about **0.0002 signet BTC** (20,000 sats) from a signet faucet. Wait for one confirmation (about 10 minutes), then check:
   ```powershell
   & $sk --wallet-dir $w balance
   ```
6. **The Ledger plus PHONE pays 1,000 sats** back to the faucet's return address:
   ```powershell
   $faucet = "tb1p4tp4l6glyr2gs94neqcpr5gha7344nfyznfkc8szkreflscsdkgqsdent4"
   & $sk --wallet-dir $w send --to $faucet --amount 0.00001 --out "$w\pay-ledger-phone.psbt"
   & $sk --wallet-dir $w sign --psbt "$w\pay-ledger-phone.psbt" --ledger
   ```
   - The tool shows the payment.
   - The Ledger shows the same payment: 0.00001 to the faucet's address, the network fee, and the wallet name. Approve it on the Ledger.
   - The tool says "Added 1 signature".

   Then PHONE approves (answer `y` at "Approve? (y/N)"), and it's sent:
   ```powershell
   & $sk --wallet-dir $w sign --psbt "$w\pay-ledger-phone.psbt" --key-file "$env:USERPROFILE\.satkeep\signet\keys\ledger-phone.key"
   & $sk --wallet-dir $w broadcast --psbt "$w\pay-ledger-phone.psbt"
   ```
   **Note the transaction ID** it prints.
7. **The Ledger alone is refused.** After the payment above confirms:
   ```powershell
   & $sk --wallet-dir $w balance
   & $sk --wallet-dir $w send --to $faucet --amount 0.00001 --out "$w\ledger-alone.psbt"
   & $sk --wallet-dir $w sign --psbt "$w\ledger-alone.psbt" --ledger
   & $sk --wallet-dir $w broadcast --psbt "$w\ledger-alone.psbt"
   ```
   Approve on the Ledger as before. The last command must refuse with "not enough keys have approved this payment", and send nothing.

## B. Adding a beneficiary

8. **Prepare the move** to a new wallet with HEIR. It uses short test delays: the heir path opens after 12 blocks, and check-ins come every 6.
   ```powershell
   $w2 = "$env:USERPROFILE\.satkeep\signet-ledger-heir"
   & $sk --wallet-dir $w add-heir --heir $heir --heir-delay 12 --checkin-interval 6 --checkin-margin 3 --refresh-window 2 --new-wallet-dir $w2 --out "$w\move.psbt"
   ```
   - Note the address it prints as "the new wallet's first address".
   - The move's network fee must be within this wallet's limit: at most 0.005 BTC and 5% of what it moves. At 2 sat/vB on about 20,000 sats it's about 2%. If it's refused as too high, stop and send me the message: don't add `--allow-high-fee`.
9. **Approve the move with the Ledger and PHONE:**
   ```powershell
   & $sk --wallet-dir $w sign --psbt "$w\move.psbt" --ledger
   ```
   - The tool shows "Move to approve", with HEIR's key and the heir delay.
   - The Ledger shows the move as one payment of everything, less the fee, to one address. That address must be the one the tool shows for the move, which `add-heir` also printed as "the new wallet's first address". Approve on the Ledger.

   Then PHONE approves (answer `y`), and it's sent:
   ```powershell
   & $sk --wallet-dir $w sign --psbt "$w\move.psbt" --key-file "$env:USERPROFILE\.satkeep\signet\keys\ledger-phone.key"
   & $sk --wallet-dir $w broadcast --psbt "$w\move.psbt"
   ```
   Note the transaction ID, and wait for one confirmation.
10. **Register the new wallet on the Ledger:**
    ```powershell
    & $sk --wallet-dir $w2 ledger-register
    ```
    - The Ledger shows the name, the new rules (with `older(12)`) and five keys: PHONE, DEVICE, HELPER, HEIR, and the internal key nobody can sign with. The Ledger may label that last one `dummy`.
    - Check the keys against the tool's list, and approve.
11. **The Ledger plus HELPER pays 1,000 sats:**
    ```powershell
    & $sk --wallet-dir $w2 balance
    & $sk --wallet-dir $w2 send --to $faucet --amount 0.00001 --out "$w2\pay-ledger-helper.psbt"
    & $sk --wallet-dir $w2 sign --psbt "$w2\pay-ledger-helper.psbt" --ledger
    & $sk --wallet-dir $w2 sign --psbt "$w2\pay-ledger-helper.psbt" --key-file "$env:USERPROFILE\.satkeep\signet\keys\ledger-helper.key"
    & $sk --wallet-dir $w2 broadcast --psbt "$w2\pay-ledger-helper.psbt"
    ```
    Approve on the Ledger when it asks, and answer `y` for HELPER. Note the transaction ID.
12. **Send what's left back to the faucet,** with the Ledger and PHONE (answer `y`), after the payment above confirms:
    ```powershell
    & $sk --wallet-dir $w2 balance
    & $sk --wallet-dir $w2 send --to $faucet --all --out "$w2\return.psbt"
    & $sk --wallet-dir $w2 sign --psbt "$w2\return.psbt" --ledger
    & $sk --wallet-dir $w2 sign --psbt "$w2\return.psbt" --key-file "$env:USERPROFILE\.satkeep\signet\keys\ledger-phone.key"
    & $sk --wallet-dir $w2 broadcast --psbt "$w2\return.psbt"
    ```

## Send me

- **Your Ledger:** its model, and the Bitcoin Test app's version (shown under the app's About or Version screen).
- **What worked:** whether each step did, and any message from the tool or the Ledger that differed from what's written here, word for word.
- **The transaction IDs** from steps 6, 9, 11 and 12. They're public.

**Don't send:**
- anything from the `.key` files;
- `ledger.json`;
- your Ledger's 24 words or PIN, which nothing here ever asks for.

## Notes

- **If the tool says "no Ledger found",** check that the Ledger is plugged in by USB, unlocked, with Bitcoin Test open, and that Ledger Live is fully closed. Then try again.
- **If you decline on the Ledger,** the tool says the Ledger refused, and the PSBT file is unchanged.
- **Nothing here touches your real bitcoin.** Bitcoin Test uses a different key tree (coin type 1') and refuses mainnet addresses.
