# Milestone 1 — Prove the Savings wallet rules

**What Twofold is:** a phone-first bitcoin self-custody wallet. Spending is a normal phone-key wallet. Savings needs any 2 of 3 keys (phone, a second device, a replaceable helper), plus delayed recovery and inheritance paths. Full spec: the master brief, `docs/master-brief.md` in the private repo `brent472/twofold-private`.

**Goal:** prove the Savings spending rules work and are safe, on test bitcoin only, before any app UI exists.

## Build

1. **Savings policy in miniscript** (block counts are placeholders until review):

   ```
   or(thresh(2,pk(PHONE),pk(DEVICE),pk(HELPER)),
      or(and(pk(DEVICE),older(39420)),
         and(pk(HEIR),older(61320))))
   ```

   About 9 months for the device-only backup and about 14 months for inheritance. The relative-timelock cap is 65,535 blocks. Keep both delays configurable.
2. **A small Rust command-line tool** using BDK and rust-miniscript: create the wallet, receive, build PSBTs, sign with any 2 keys, broadcast.
3. **Check-in:** a send-to-self transaction that restarts every timelock and consolidates coins. It must run every 6 months — sooner than the shortest delay.
4. **Leave without Twofold:** export the wallet descriptor, import it into Liana or Sparrow, and spend with no Twofold code involved.

## Tests

Run on **regtest** (blocks can be mined on demand to skip time), then one demo on **signet**.

- Each allowed path spends: any 2 of 3; device after its delay; heir after its delay.
- Each forbidden case fails: phone alone; helper alone; device before its delay; heir before its delay.
- After a check-in, both delays start over from zero.

## Rules

Signet and regtest only, never mainnet. Fresh keys per test run; no real keys anywhere. Write down every assumption in the README. See [`CLAUDE.md`](../CLAUDE.md).

## Out of scope

App UI, the helper server, the hardware card, Lightning, inheritance legal flows.

## Open question

The **recovery contact** (a trusted person) must be in the wallet rules from day one — for example contact + helper after a waiting period. It's left out of this first proof to keep it small; decide before Milestone 2 whether to add the path.

## Done when

- [ ] All tests pass on regtest
- [ ] Signet demo transaction for each allowed path
- [x] Exit through Liana or Sparrow works with no Twofold code
- [ ] README has a diagram of the spending paths
- [ ] One experienced Bitcoin developer has reviewed the policy

## Next

**Milestone 2:** a clickable iPhone prototype built from the Twofold App Screens design, using Sections 18 and 19 of the master brief for type and copy. Fake data only. Then run the mom test (5 bitcoin-naive adults; setup under 5 minutes; 4 of 5 finish every task unaided).
