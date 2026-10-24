The signet demo: one transaction for each allowed Savings path, in both shapes, on the public signet network. It ticks the Milestone 1 box "Signet demo transaction for each allowed path". Running it also turned up a bug: a new wallet couldn't sync from a pruned node. That's fixed here in its own commit.

## The run

[`docs/signet-demo.md`](docs/signet-demo.md) lists every transaction with a mempool.space link. It ran on 23–24 October 2026, blocks 327,707 to 327,726, against a pruned signet node (`-prune=550`), funded from a public faucet.

| Path | Approved by |
| --- | --- |
| Savings with no beneficiary: 2 of 3, three times | PHONE + DEVICE, PHONE + HELPER, DEVICE + HELPER |
| Adding a beneficiary: the move, in one payment | PHONE + DEVICE |
| Savings with a beneficiary: 2 of 3, twice | DEVICE + HELPER, PHONE + DEVICE |
| Savings with a beneficiary: the check-in | PHONE + HELPER |
| HEIR alone, after the heir delay | HEIR |

- **Refused first:** `send --path heir` refused HEIR alone ("needs 12 confirmations … and has 1") until the check-in's coin was 12 blocks old.
- **Read back from the node:** each transaction spent the expected script:
  - the plain 2-of-3 payments and the move: P2WSH `sortedmulti` with 2 signatures;
  - the 2-of-3 payments and check-in with a beneficiary: the Taproot `multi_a` leaf with 2 signatures;
  - HEIR alone: the heir leaf (`OP_12 OP_CHECKSEQUENCEVERIFY`) with 1 signature and `nSequence` 12.
- **Public descriptors:** both wallets' are in the doc, so the addresses can be checked.

**Short test delays:** heir delay 12 blocks, check-in every 6 blocks with a 3-block margin, refresh window 2 blocks, minimum heir delay 10. The doc lists the limits:
- the payment rule didn't fire (the coins were too young);
- forbidden cases other than HEIR-too-early are covered on regtest;
- signing used Twofold's own tool; leaving through Liana on signet is still open in the exit drill.

## The script

`contrib/signet-demo/demo.ps1` runs the demo in five steps: `setup`, `plain`, `move`, `taproot` and `heir`. Each step waits for its transactions to confirm.
- **Keys:** `twofold keygen` makes fresh test keys under `%USERPROFILE%\.twofold`, outside the repo. `sign` reads each key from its file with `--key-file`.
- **Logging:** nothing prints or logs a key or phrase. The transaction IDs go to `signet-demo-txids.txt` there.
- **Safety checks:** it refuses unless the node reports chain `signet`, and `setup` refuses to run twice.

## The pruned-node fix

On a new wallet's first sync, BDK's `bdk_bitcoind_rpc` emitter checks the wallet's checkpoint, the genesis block, with `getblock`. A pruned node can't serve that block ("Block not available (pruned data)").

`node::sync` now first adds a checkpoint at the block before the wallet's birthday. The emitter then agrees on a recent block, and the birthday block itself is still read.

- **Test:** `reads_from_the_birthday_block_on_and_not_before`. Coins confirmed in the birthday block are found, and an earlier block isn't read. With the checkpoint at the birthday instead, the test fails.
- **CLI README:** now says what a pruned node needs (the blocks since the wallet last synced).

## Docs

- The READMEs link the demo.
- The exit drill's to-do list says the Liana signet run is still to do.
- `docs/milestone-1.md` has the box ticked.

## Security review

None. Reviewed the diff for:
- key material on command lines, in output, in stderr capture or in the transaction log;
- command injection in the script (every native call passes an argument array of hard-coded or checked values);
- mainnet risk;
- whether the new checkpoint gives the node any power it didn't already have. It doesn't: the hash comes from the same trusted node that supplies every block, it's added only below the birthday, and spending rules are enforced by the scripts on chain.

No new dependencies.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
