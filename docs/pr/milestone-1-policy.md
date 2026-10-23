Milestone 1, part 1 of 3: the Savings wallet rules and their tests. The command-line tool and the check-in come as separate PRs after review.

## What's here

A Rust workspace with one library crate, `crates/twofold-savings`, built on BDK 3.2.0 and rust-miniscript 12.3.7 (used through BDK's re-exports so the versions always match).

- **The policy** as a Taproot descriptor with no key-path spend:
  ```text
  tr(UNSPENDABLE, { multi_a(2, PHONE, DEVICE, HELPER),
                    { and_v(v:pk(DEVICE_R), older(39420)),
                      and_v(v:pk(HEIR),     older(61320)) } })
  ```
  The delays are configurable (`Delays::new`). Block-based only, capped at 65,535, heir longer than device. Mainnet keys and a mainnet network are refused.
- **A delay guard**, `spend::build_spend` / `spend::finalize`. It refuses any delayed-path spend before the coin is old enough. See the finding below.
- **Descriptor export**, `export_descriptor()` (with `<0;1>`) and `export_receive_and_change()`.
- **The crate README** has the spending-path diagram and every assumption about policy, delays, script type, keys and fees.

## ⚠️ Finding for the reviewer: BDK alone finalizes delayed spends too early

BDK 3.2.0 with miniscript 12.3.7 will finalize a DEVICE-alone or HEIR-alone spend as soon as the input's sequence matches the leaf's delay, **however young the coin is**. When finalizing, BDK passes miniscript three satisfiers: the PSBT, a height check and an age check. miniscript combines them with **OR** (`impl_tuple_satisfier`, `miniscript/src/miniscript/satisfy.rs`). The PSBT satisfier only compares the delay with the sequence, so BDK's age check is never consulted.

**Funds were never at risk.** The network enforces the delay (BIP 68 / CSV), and the node rejects these spends with `non-BIP68-final`. But Twofold would have handed the user, or an heir, a transaction that can never confirm.

**Fix:** `spend::check_delays` compares each input's sequence-required delay with the coin's confirmations as of the next block, and returns `Error::TooEarly`. `build_spend` and `finalize` both run it. When BDK only knows an upper bound for the confirmation height, the computed age can only come out too young, never too old. `finalize` also passes BDK `assume_height = tip + 1`, because BDK's own check compares against the current tip and is one block stricter than the network.

**Tests:**
- `library_refuses_to_build_a_device_spend_one_block_early` and `…_heir_spend_one_block_early`: the library refuses at N − 1 confirmations and builds at N.
- Every forbidden-path test requires a library refusal **and** a node rejection.
- `bdk_alone_would_finalize_an_early_delayed_spend` pins BDK's behaviour, so we notice if it changes.
- With the guard switched off, 8 of the 12 forbidden-path tests fail.

## Tests

Each test starts its own throwaway `bitcoind` (from `BITCOIND_EXE`) in a temp folder, on a random local RPC port, with P2P off. It never touches a long-running node. Keys are generated in memory per test and never printed or saved.

- **Static checks:** the rules read back from the compiled descriptor match an independently written rule for all 32 combinations of the five keys, at ages around each delay. The forbidden cases are also named one by one, including the two requested in review:
  - HEIR + any one other key before the heir delay (helper–heir collusion)
  - DEVICE with both its keys before its delay
- **Allowed, on regtest:** PHONE+DEVICE, PHONE+HELPER and DEVICE+HELPER; DEVICE alone at exactly its delay; HEIR alone at exactly its delay. Each test checks the revealed leaf and that the spend is mined.
- **Forbidden, on regtest**, each tried through all three leaves:
  - PHONE alone, HELPER alone (also after both delays)
  - DEVICE with both keys one block early
  - HEIR one block early
  - HEIR+HELPER, HEIR+PHONE, HEIR+DEVICE before the relevant delay
  - signing a shorter sequence to skip either delay (the script fails on CSV)
- **Full length:** the real 39,420 / 61,320 delays, mining all 61,320 blocks. It takes 5–10 minutes, so it's `#[ignore]`d; run it with `--include-ignored`.
- **Export:** Bitcoin Core reads the exported descriptor and derives exactly the addresses BDK hands out.
- **Harness self-test:** a hand-built spend with enough signatures is accepted, so the rejections elsewhere are the policy's doing, not the harness's.

```bash
BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe" cargo test -- --include-ignored
```

```powershell
$env:BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe"; cargo test -- --include-ignored
```

Last full run, on Windows with Bitcoin Core v31.1.0: **44 passed, 0 failed.**
- 23 unit
- 5 allowed
- 12 forbidden
- 1 full-length (about 10 minutes when run alongside the rest)
- 1 export
- 2 harness

Without the full-length test, the suite runs in about 13 s.

## For the reviewer to judge (also in the README)

- After 61,320 confirmations, DEVICE alone and HEIR alone can both spend, so whichever acts first wins.
- A stolen DEVICE plus a missed check-in can spend once the device delay passes. The check-in PR addresses this.
- Every coin has its own clock (new deposits and change included), so a check-in must sweep every coin.
- The internal key is the NUMS point H with chain code SHA256(the five keys), following Liana's approach. Byte-for-byte Liana compatibility is not yet verified.
- The heir delay is 4,215 blocks (about 1 month) under the relative-timelock cap.
- Liana may write a key used in two paths as the same xpub with different multipath steps (`<0;1>` and `<2;3>`). `SavingsKeys::new` currently refuses that. We'll check during the exit drill and, if so, allow the same xpub when the derivation paths differ. This is listed under "Not yet verified" in the crate README.

## Not in this PR

- The command-line tool (next PR)
- The check-in (the PR after)
- The signet demo
- The Liana/Sparrow exit drill
- The recovery contact path

🤖 Generated with [Claude Code](https://claude.com/claude-code)
