# Milestone 1 — Prove the Savings wallet rules

**What Twofold is:** a phone-first bitcoin self-custody wallet. Spending is a normal phone-key wallet. Savings needs any 2 of 3 keys (phone, a second device, a replaceable helper), plus a delayed inheritance path. Full spec: the master brief, `docs/master-brief.md` in the private repo `brent472/twofold-private`.

**Goal:** prove the Savings spending rules work and are safe, on test bitcoin only, before any app UI exists.

## Build

1. **Savings policy in miniscript.** Two shapes (block counts are placeholders until review):

   With no beneficiary yet, plain 2 of 3, with no delayed path, in the standard P2WSH multisig format, `wsh(sortedmulti(2,PHONE,DEVICE,HELPER))`, which the most wallets can import:

   ```
   thresh(2,pk(PHONE),pk(DEVICE),pk(HELPER))
   ```

   With a beneficiary, a Taproot policy (Liana and Bitcoin Core):

   ```
   or(thresh(2,pk(PHONE),pk(DEVICE),pk(HELPER)),
      and(pk(HEIR),older(61320)))
   ```

   - About 14 months for inheritance. The relative-timelock cap is 65,535 blocks. Keep the delay configurable.
   - The heir delay has a minimum: placeholder 26,280 blocks (about 6 months), also configurable.
   - Adding a beneficiary later moves Savings to the second shape in one transaction. Each signer checks the move against its own settings (its keys, the beneficiary and the heir delay it set up), never against what the payment claims.
   - The 9-month device-only path was removed in brief 3.8: the phone key backup covers a lost phone, and removing it takes away a stolen card's slow route to Savings.
2. **A small Rust command-line tool** using BDK and rust-miniscript: create the wallet, receive, build PSBTs, sign with any 2 keys, broadcast.
3. **Check-in** (only with a beneficiary): a send-to-self transaction that restarts the heir delay and consolidates coins. It must run sooner than the heir delay. The plan is once a year (52,560 blocks), with a margin of about 2 months (8,760 blocks).
   - **Payment rule** (brief 3.12): every Savings payment and pocket refill also spends the coins within a window of needing a check-in (placeholder: older than about 10 months), with change back to Savings, so it counts as their check-in. A wallet setting, on by default; off for privacy.
4. **Leave without Twofold:** export the wallet descriptor, import it into Liana or Sparrow, and spend with no Twofold code involved.

## Tests

Run on **regtest** (blocks can be mined on demand to skip time), then one demo on **signet**.

- **Each allowed path spends:** any 2 of 3, in both shapes; the heir after its delay.
- **Each forbidden case fails:**
  - phone, device or helper alone, at any age
  - the heir before its delay
  - with no beneficiary, any single key, at any age
- **The check-in:** after one, the heir delay starts over from zero. Savings with no beneficiary has no check-in.

## Rules

Signet and regtest only, never mainnet. Fresh keys per test run; no real keys anywhere. Write down every assumption in the README. See [`CLAUDE.md`](../CLAUDE.md).

## Out of scope

App UI, the helper server, the hardware card, Lightning, inheritance legal flows.

## Recovery contact

Decided in brief 3.9–3.10: the recovery contact is not a Savings key. They help unlock the phone key backup if the phone and second key are both lost. See [`phone-key-backup.md`](phone-key-backup.md).

## Done when

- [ ] All tests pass on regtest
- [x] Signet demo transaction for each allowed path ([signet-demo.md](signet-demo.md))
- [x] Exit through Liana or Sparrow works with no Twofold code
- [x] README has a diagram of the spending paths
- [ ] One experienced Bitcoin developer has reviewed the policy

## Next

**Milestone 2:** a clickable iPhone prototype built from the Twofold App Screens design, using Sections 18 and 19 of the master brief for type and copy. Fake data only. Then run the mom test (5 bitcoin-naive adults; setup under 5 minutes; 4 of 5 finish every task unaided).
