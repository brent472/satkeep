The heir's route: `satkeep-restore heir`. This is the second part of the restore tool's step 3 ([`docs/restore-tool.md`](docs/restore-tool.md)), started the way the owner decided on 2026-10-24. Test networks only. No new dependencies.

## What it does

```
satkeep-restore heir --kit FILE --key-file FILE      # or --key-stdin
```

- **The heir's 12 words** come from a key file or standard input, as `satkeep sign` takes keys. They're never taken on the command line (the existing check refuses them there) and never printed back. Anything but 12 English BIP39 words is refused without repeating it.
- **It reads only the kit's plain wallet setup.** It never asks for the second key (it doesn't even accept one) and never unlocks the backup. The raw backup is refused, since it has no plain setup.
- **Every output starts UNVERIFIED** (restore-tool.md, threat 7.2): without the second key nobody can check the plain setup against the locked copy.
- **It exports, for Liana, only the Savings wallets with exactly one Taproot leaf where HEIR alone spends after a block delay,** `and_v(v:pk(HEIR),older(N))`, with HEIR's key matching on fingerprint, path and `tpub`.
  - **Not offered, with the reason given:** a wallet that only mentions HEIR's key, or whose delay is time-based.
  - **If no wallet holds the key, or none can be offered,** it says so plainly, exports nothing and exits with an error.
- **For each wallet,** it shows the heir delay in blocks and roughly in hours, days or months (61,320 blocks is about 14 months, 20 blocks about 3 hours). It also says the heir can only move coins that haven't moved for at least that long.
  - **Where the delay comes from:** N in that heir leaf, which is what the coins follow.
  - **If the setup lists a different delay,** it says so.
- **The key type** is now `AccountKey` (it was `PhoneKey`): the same derivation works for any holder.

## Tests (regtest)

**Works:**
- the heir sees only the wallet with a beneficiary, from a key file and from stdin;
- the exported descriptor, watched in Bitcoin Core, lets HEIR alone sign and finalize the heir path;
- the node refuses that spend one block before the heir delay (`non-BIP68-final`), and accepts and mines it at the delay.

**Why Core rather than Liana here:** Liana doesn't run on the Windows development machine. The exit drill (step 4) will run the heir's route through Liana itself.

**Forbidden:**
- another person's words export nothing;
- a setup with no beneficiary exports nothing;
- the raw backup is refused;
- an 11-word key file, and one over 4 KiB, are refused;
- a changed setup where HEIR is only in the 2-of-3, shares the delayed path with a stranger, or has a time-based delay offers nothing;
- `heir` doesn't accept a second key.

**Every heir run** is checked to show none of the heir's words, nor the phone key's, nor any `tprv` or `xprv`.

**Unit tests:** the durations.

**Results:**
- `cargo test -p satkeep-restore`: 13 regtest tests and 3 unit tests pass;
- `cargo clippy --workspace --all-targets -D warnings` is clean.

## Security review

**A separate review of the diff found one medium issue and two low ones, all fixed here:**
1. **Medium: "the heir can move its coins alone" wasn't checked against the descriptor.**
   - **The problem:** the first version only checked that HEIR's key appeared somewhere, and read the delay from any `older()` in the text. A changed setup with HEIR in the 2-of-3 and a stranger behind the delay, or HEIR sharing the delayed path, would have been offered with that claim.
   - **The harm:** no coins could be taken, but the heir would be misled.
   - **The fix:** the tool now walks the parsed Taproot tree and requires exactly one leaf `and_v(v:pk(HEIR),older(N))`, taking N from it. Three tampered setups are tested.
2. **Low: a time-based `older()`** would have been shown as a huge block count. It's now refused, and tested.
3. **Low: the key file and stdin had no size limit.** They're now read up to 4 KiB into a buffer sized up front, so it never grows and leaves a copy. A larger file is refused, and tested.

**Checked clean:**
- **Errors** show only paths, never the words.
- **Extended keys:** a `tprv` or `xprv` is refused without being echoed. Byte-order marks are handled.
- **The descriptor exported** is the one checked.
- **Key matching** compares `tpub`, fingerprint and path.
- **The second key and the backup:** `heir` never touches the second key or the locked backup, and has no argument for a second key.
- **The phrase** is wiped. The account `Xpriv` isn't, as the README says.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
