# Milestone 1: where it stands

Status on 24 October 2026, after PR #28. The milestone itself is [`milestone-1.md`](milestone-1.md). This page says, for each item, whether it's **done** or **left**, then lists the open decisions and the steps only you can do. Test networks only throughout.

## The milestone's items

### Build

| # | Item | Status | Where |
| --- | --- | --- | --- |
| 1 | **The Savings policy in miniscript, both shapes:** plain 2 of 3 (`wsh(sortedmulti(2,PHONE,DEVICE,HELPER))`), and Taproot with HEIR after the heir delay | **Done** | `satkeep-savings` (#6) |
| 1a | The heir delay is configurable, with a configurable minimum | **Done** | #6, #7 |
| 1b | Adding a beneficiary moves Savings in one payment, and each signer checks the move against its own settings | **Done** | #7 |
| 1c | The device-only path is gone (brief 3.8) | **Done** | #6 |
| 2 | **The command-line tool** (`satkeep`, BDK and rust-miniscript): create, receive, build, sign with any 2 keys, broadcast | **Done** | `satkeep-cli` |
| 2a | Keys come from 12-word BIP39 phrases, never on the command line | **Done** | #3 |
| 3 | **Check-in** with a beneficiary, which must run sooner than the heir delay | **Done** | `checkin` (#6) |
| 3a | **The payment rule:** payments and refills also spend the coins due for a check-in | **Done** | #9 |
| 4 | **Leave without Satkeep:** export, import into Liana or Sparrow, spend with no Satkeep code | **Done for Liana and Bitcoin Core; Sparrow is left** | See below |

**Leaving without Satkeep, in detail:**
- **Done:**
  - **Liana and Bitcoin Core:** the exit drill imports both shapes and spends every path on regtest, on every PR that touches what it runs and every week ([`exit-drill.md`](exit-drill.md)). It's a required check.
  - **Sparrow, by reading its code:** Sparrow's descriptor parser accepts the plain shape, and the drill checks the export against a port of it on every run.
  - **The restore tool:** `satkeep-restore` unlocks the exit kit with the second key, and hands Savings to Liana, Sparrow or Bitcoin Core (#22–#24, #26).
- **Left:**
  - **Sparrow, in the real app, by hand:** [step list B](#b-the-sparrow-import-by-hand) below. It needs you, because Sparrow is a desktop app.
  - **The Liana desktop app, by hand.** Windows has no `liana-cli`, so on Windows the way out is Liana's desktop app, and nobody has tried it yet. It's also a manual check. I can write its steps once you want to do it.
  - **Leaving through Liana and Bitcoin Core on signet.** The drill does it on regtest only (`exit-drill.md`, "Not done yet").

### Tests

| Item | Status |
| --- | --- |
| Each allowed path spends, in both shapes, and the heir after the delay | **Done** on regtest |
| Each forbidden case fails: one key alone at any age, the heir too early, any single key with no beneficiary | **Done** on regtest |
| The check-in starts the heir delay over; Savings with no beneficiary has no check-in | **Done** on regtest |
| The full-length heir delay (61,320 blocks), skipped by a plain `cargo test` and not run in CI | **Passed** on 24 October, on this PC (`the_default_heir_delay_opens_at_exactly_61320_confirmations`, about 12 minutes) |
| One demo on signet | **Done** on 23–24 October ([`signet-demo.md`](signet-demo.md)), but before the rename, with the tool still called `twofold`. **Running it again with `satkeep` is left:** [step list A](#a-the-signet-run). |

### Done when

| Box | Status |
| --- | --- |
| All tests pass on regtest | **Ticked.** All of them pass in CI on every PR. The full-length test, which CI doesn't run, was run locally on 2026-10-24 and passed (see Tests). |
| Signet demo transaction for each allowed path | **Ticked.** A run with the renamed tool is left (step list A). |
| Exit through Liana or Sparrow works with no Satkeep code | **Ticked**, through Liana. Sparrow in the real app is left (step list B). |
| README has a diagram of the spending paths | **Ticked** (#10) |
| One experienced Bitcoin developer has reviewed the policy | **Left.** It needs a reviewer (decision 1). |

### Done along the way

These weren't in the milestone's list, but were needed for it:
- **The security baseline:** cargo-deny, fuzzing (six targets now), CI on every PR, and a security review before each PR (#2).
- **The phone key backup:** research, the format, and the exit kit (#4, #14). The prototype uses a simulated security device.
- **The restore tool,** steps 1–5 of [`restore-tool.md`](restore-tool.md), section 9:
  - the plan (#8);
  - the format (#14);
  - unlock and export (#22);
  - the heir's route (#23);
  - the easy routes (#24);
  - the old-phone second key (#26).
- **RustCrypto on one current generation, Rust 1.87,** and wiping inside the libraries (#25, #27).
- **The draft exit guide** for family and helpers ([`exit-guide.md`](exit-guide.md)).

## Open decisions

1. **An outside Bitcoin reviewer.**
   - **What they'd review:** the policy and every placeholder:
     - the heir delay, 61,320 blocks;
     - its minimum, 26,280;
     - the check-in interval and margin, 52,560 and 8,760;
     - the payment rule's window, 8,760;
     - the test derivation path `m/87'/1'/0'`;
     - 12 words with no passphrase.
   - **Needs you:** to choose who. It's the milestone's last box.
2. **An outside cryptographer** for the backup format ([`backup-format.md`](backup-format.md), section 7), before launch. Needs you: to choose who, and when.
3. **Trezor support** (restore plan step 6, D6). It needs your OK to download the Trezor emulator (`trezor-user-env`, on GitHub's runners only) and for any new dependency.
4. **The restore tool's releases:** where people download it, and the signing key they check it against. The exit guide marks this *to be decided*.
5. **The everyday pocket's descriptor** (brief 3.11). The kit format leaves room for it, but its exact form comes with the pocket's design.
6. **Comes with the app, noted so it isn't lost:**
   - how the second phone reads the unlock request (scanned or typed);
   - a check that iPhones and Android phones give the same shared value for the old-phone unlock (`backup-format.md`, section 7);
   - the helper-assisted claim's wait (placeholder 3 months);
   - the recovery contact's unlock, which the format reserves but nothing builds yet.
7. **A small GitHub setting:** the new `Fuzz second_phone (60 s)` check isn't in the "Protect main" ruleset's required checks yet. The other five fuzz targets, the drill and CodeQL are. Adding it is in the repository's Settings, under Rules.

## Steps that need you

Both step lists are for your Windows PC, in Windows PowerShell, from the repo folder `C:\Users\Brent\Documents\twofold`. Test bitcoin only. Never type a real recovery phrase anywhere in these steps.

**Order:** do A's steps 1–6 first. Then do all of B, while Savings still has its coins. Then finish A from step 7.

### A. The signet run

This repeats the [signet demo](signet-demo.md) with the renamed `satkeep` tool and today's code. It takes about three hours, mostly waiting for the heir delay. You fund it from a signet faucet yourself; I never use a faucet.

1. **Start the signet node,** if it isn't running already. In its own PowerShell window, which you leave open:
   ```powershell
   & "C:\Program Files\Bitcoin\daemon\bitcoind.exe" -signet -prune=550
   ```
2. **Check that it has caught up.** In a second PowerShell window:
   ```powershell
   & "C:\Program Files\Bitcoin\daemon\bitcoin-cli.exe" -signet getblockchaininfo
   ```
   Wait until it shows `"initialblockdownload": false`. That can take a while if the node has been off.
3. **Get the latest code:**
   ```powershell
   cd C:\Users\Brent\Documents\twofold
   git checkout main
   git pull
   ```
4. **Set up:** fresh test keys, Savings with no beneficiary, and an address to fund:
   ```powershell
   .\contrib\signet-demo\demo.ps1 setup
   ```
   - The keys and wallets go under `C:\Users\Brent\.satkeep`.
   - Your earlier run's files stay in `C:\Users\Brent\.twofold` and aren't used.
   - If PowerShell refuses to run the script, use this instead:
     ```powershell
     powershell -ExecutionPolicy Bypass -File .\contrib\signet-demo\demo.ps1 setup
     ```
     Do the same for steps 7–10.
5. **Fund it:** send about 0.0005 signet BTC (50,000 sats) from a signet faucet to the address `setup` printed.
6. **Wait for one confirmation,** about 10 minutes, then check the balance:
   ```powershell
   .\target\debug\satkeep.exe --network signet balance
   ```
   **Now do step list B,** then come back here.
7. **Three 2-of-3 payments** from Savings with no beneficiary, one for each pair of keys. About 30 minutes:
   ```powershell
   .\contrib\signet-demo\demo.ps1 plain
   ```
8. **Add a beneficiary:** the move, in one payment:
   ```powershell
   .\contrib\signet-demo\demo.ps1 move
   ```
9. **Two 2-of-3 payments with the heir path in place,** and a check-in:
   ```powershell
   .\contrib\signet-demo\demo.ps1 taproot
   ```
10. **HEIR alone.** It's refused until the heir delay (12 blocks, about 2 hours) has passed since the check-in. The script tries again every 5 minutes on its own:
    ```powershell
    .\contrib\signet-demo\demo.ps1 heir
    ```
11. **Send me the results:** the file `C:\Users\Brent\.satkeep\signet-demo-txids.txt`. It holds only transaction IDs, which are public. I'll update `signet-demo.md` from it.

**If a step stops with an error,** copy the red `demo:` line and send it to me. Don't remove the `.satkeep` folder: `setup` refuses to run twice, and the folder holds what we'd need to look at.

### B. The Sparrow import, by hand

This imports Savings with no beneficiary into the real Sparrow app on signet. You check the addresses, add two keys, and make one 2-of-3 payment back to Savings in Sparrow, with no Satkeep code. It uses the wallet from step list A, after its step 6.

`satkeep export` prints the same descriptor that `satkeep-restore sparrow` writes for an heir, so this checks the file people would really import.

1. **Download Sparrow** for Windows from [sparrowwallet.com/download](https://sparrowwallet.com/download/). Check the download as that page explains, then install it. These steps were written against Sparrow 2.5.5's code.
2. **Write the descriptor to a file** Sparrow can read:
   ```powershell
   .\target\debug\satkeep.exe --network signet export | Out-File -Encoding ascii "$env:USERPROFILE\.satkeep\signet-savings.txt"
   ```
   It holds only public keys, but it shows every Savings address, so treat it as private.
3. **Start Sparrow on signet.**
   - Find `Sparrow.exe`: right-click Sparrow in the Start menu, choose Open file location, then right-click the shortcut and choose Open file location again.
   - Then, in PowerShell, with that folder in place of `FOLDER`:
     ```powershell
     & "FOLDER\Sparrow.exe" -n signet
     ```
4. **Connect Sparrow to your node.**
   - Choose **File › Settings...**, then the **Server** tab, and set the type to **Bitcoin Core**.
   - URL `127.0.0.1`, port `38332`, authentication **Default** (the cookie file).
   - **Data Folder:** set it to `C:\Users\Brent\AppData\Local\Bitcoin`. That's where Bitcoin Core keeps its data on this PC, so Sparrow's default may not find it.
   - Click **Test Connection**, then close Settings.
   - If Sparrow isn't connected, click the server toggle at the bottom right of its window.
5. **Import the wallet:**
   - Choose **File › Import Wallet**, then **Output Descriptor**, and pick `C:\Users\Brent\.satkeep\signet-savings.txt`. Name the wallet, for example `satkeep-signet`.
   - If it asks for a start date, use today: the wallet was funded today, and the pruned node only keeps about a week of blocks.
   - **If Sparrow refuses the file,** copy its exact message for me and stop here. That's a finding.
6. **Check the addresses.**
   - In Sparrow's **Addresses** tab, the first receive address must be the one `demo.ps1 setup` printed in step A4.
   - The **Transactions** tab must show the faucet's payment.
7. **Add two keys, PHONE and DEVICE:**
   1. On the **Settings** tab, find the keystore whose master fingerprint matches PHONE's. Its fingerprint is the `[xxxxxxxx/…]` at the start of PHONE's line in `C:\Users\Brent\.satkeep\signet-demo-public-keys.txt`.
   2. Click its **Import...** button, choose **Software Wallet**, then **Mnemonic Words (BIP39)**, and 12 words.
   3. Open `C:\Users\Brent\.satkeep\signet\keys\phone.key` in Notepad. Type its 12 words into Sparrow, leave the passphrase empty, then close Notepad without saving.
   4. Click **Show Derivation...**, set the derivation to `m/87'/1'/0'`, then click **Import Keystore**.
   5. Check that the keystore's master fingerprint and xpub are still the same as before. If Sparrow warns that the key doesn't match, or refuses the derivation, copy its message for me and stop here.
   6. Do the same for DEVICE, with `device.key`.
   7. Click **Apply**. A password is up to you; it's a test wallet.
8. **Make one 2-of-3 payment back to Savings:**
   1. On the **Receive** tab, copy a new address.
   2. On the **Send** tab, pay `0.0001` BTC to it. Click **Create Transaction**, then **Finalize Transaction for Signing**, then **Sign**. Sparrow signs with both keys you added.
   3. Click **Broadcast Transaction**.
9. **Check that Satkeep sees it,** after one confirmation (about 10 minutes):
   ```powershell
   .\target\debug\satkeep.exe --network signet balance
   ```
   The coins are all still in Savings, less a small network fee.
10. **Send me:**
    - whether each step worked, and any message Sparrow showed, word for word;
    - the payment's transaction ID, from Sparrow's transaction screen.

    I'll record the result in `exit-drill.md`, and replace the exit guide's "to be checked against Sparrow's screens" (Step 4b) with what you saw.
11. **Remove the test wallet from Sparrow** when you're done, if you like, with **File › Delete Wallet...**. Then carry on with step list A, step 7.
