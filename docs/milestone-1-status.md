# Milestone 1: where it stands

Status on 25 October 2026, after the signet run and the Sparrow import. The milestone itself is [`milestone-1.md`](milestone-1.md). This page says, for each item, whether it's **done** or **left**, then lists the open decisions. Test networks only throughout.

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
| 4 | **Leave without Satkeep:** export, import into Liana or Sparrow, spend with no Satkeep code | **Done** for Liana and Bitcoin Core (regtest) and Sparrow (signet, by hand) | See below |

**Leaving without Satkeep, in detail:**
- **Done:**
  - **Liana and Bitcoin Core:** the exit drill imports both shapes and spends every path on regtest, on every PR that touches what it runs and every week ([`exit-drill.md`](exit-drill.md)). It's a required check.
  - **Sparrow, by reading its code:** Sparrow's descriptor parser accepts the plain shape, and the drill checks the export against a port of it on every run.
  - **Sparrow, in the real app, by hand,** on 25 October: Sparrow 2.5.5 on Windows imported Savings with no beneficiary on signet, matched Satkeep's addresses, took PHONE's and DEVICE's 12 words, and signed a 2-of-3 payment with no Satkeep code ([`exit-drill.md`](exit-drill.md), table B; [`signet-demo.md`](signet-demo.md), row S). The exit guide's Step 4b now follows Sparrow's real screens.
  - **The restore tool:** `satkeep-restore` unlocks the exit kit with the second key, and hands Savings to Liana, Sparrow or Bitcoin Core (#22–#24, #26).
- **Left:**
  - **The Liana desktop app, by hand.** Windows has no `liana-cli`, so on Windows the way out is Liana's desktop app, and nobody has tried it yet. It's also a manual check. I can write its steps once you want to do it.
  - **Leaving through Liana and Bitcoin Core on signet.** The drill does it on regtest only (`exit-drill.md`, "Not done yet").

### Tests

| Item | Status |
| --- | --- |
| Each allowed path spends, in both shapes, and the heir after the delay | **Done** on regtest |
| Each forbidden case fails: one key alone at any age, the heir too early, any single key with no beneficiary | **Done** on regtest |
| The check-in starts the heir delay over; Savings with no beneficiary has no check-in | **Done** on regtest |
| The full-length heir delay (61,320 blocks), skipped by a plain `cargo test` and not run in CI | **Passed** on 24 October, on this PC (`the_default_heir_delay_opens_at_exactly_61320_confirmations`, about 12 minutes) |
| One demo on signet | **Done** again on 25 October with the renamed `satkeep`, blocks 327,923–327,965: every allowed path in both shapes, HEIR alone refused until the heir delay, plus the Sparrow payment ([`signet-demo.md`](signet-demo.md)). The first run, on 23–24 October, was before the rename. |

### Done when

| Box | Status |
| --- | --- |
| All tests pass on regtest | **Ticked.** All of them pass in CI on every PR. The full-length test, which CI doesn't run, was run locally on 2026-10-24 and passed (see Tests). |
| Signet demo transaction for each allowed path | **Ticked.** Run again with `satkeep` on 25 October. |
| Exit through Liana or Sparrow works with no Satkeep code | **Ticked:** Liana on regtest (the drill), and Sparrow in the real app on signet (25 October). |
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

## The signet run and the Sparrow import: done

You did both on 25 October, on your Windows PC, with the step lists this page had (they're in the git history).

- **The signet run:** every allowed path in both shapes, with the renamed `satkeep`, signed by fresh test keys; HEIR alone refused through 10 confirmations, then spent at the heir delay. All 11 transaction IDs are in [`signet-demo.md`](signet-demo.md).
- **The faucet's extra coins:** it sent about 0.06 signet BTC, so 0.06 went back to its address before the move (row R there). Only about 55,000 sats went to the unspendable address.
- **The Sparrow import:** passed (above). It showed where the written steps differed from Sparrow's screens; the exit guide's Step 4b now follows the real ones:
  - Sparrow was already installed, per user, in `%LOCALAPPDATA%\Sparrow`; it was updated from 2.4.2 to 2.5.5.
  - The first start on signet shows welcome screens, and **Configure Server** at their end opens the Server settings. Sparrow found Bitcoin Core's data folder itself.
  - Importing needs a start date before **Create Wallet** works.
  - **Mnemonic Words (BIP39)** starts at 24 words; Satkeep's keys need **Use 12 Words**.
  - The words go in with **Create Keystore**, then **Import Keystore**, with the path already filled in. There's no "Show Derivation..." step unless the path changes.
