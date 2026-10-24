# Signet demo

One transaction for each allowed Savings path, in both shapes, on the public signet test network, made with the `twofold` command-line tool. It ticks the Milestone 1 box "Signet demo transaction for each allowed path". Regtest covers the same paths and every forbidden case in the test suites; this run shows them on a public network with real block times.

**Test bitcoin only.** Signet coins have no value. The four keys were fresh test keys made by `twofold keygen` for this run, kept outside the repo, and never printed.

## What ran

Run on 23–24 October 2026, blocks 327,707 to 327,726, with Bitcoin Core on signet, pruned (`-prune=550`). Payments went to an address nobody can spend from (a Taproot output to the NUMS point), so the coins are gone afterwards.

| # | Path | Approved by | Block | Transaction |
| --- | --- | --- | --- | --- |
| 0 | Funding, from a public faucet | the faucet | 327,707 | [`29a00622…d0473`](https://mempool.space/signet/tx/29a006226ec9d616ca57c186acca177122a23fd2cb6aef31e40070b9b89d0473) |
| 1 | Savings with no beneficiary: 2 of 3 | PHONE + DEVICE | 327,708 | [`85a14c8b…49323`](https://mempool.space/signet/tx/85a14c8b19ebd2024b1369de07ccf40966a4024d1d5d772625dbf8a32eb49323) |
| 2 | Savings with no beneficiary: 2 of 3 | PHONE + HELPER | 327,709 | [`c9e72f24…d2f77`](https://mempool.space/signet/tx/c9e72f24f71e6989f102865cb526b39b9aa6158fed013af9f17ded3eb2ed2f77) |
| 3 | Savings with no beneficiary: 2 of 3 | DEVICE + HELPER | 327,710 | [`e813fc2c…b6c26`](https://mempool.space/signet/tx/e813fc2c5c3942ca6b7111fda284333fc8e32b23adbc0526adf093aa412b6c26) |
| 4 | Adding a beneficiary: the move, in one payment | PHONE + DEVICE | 327,711 | [`8a76a44e…0d861`](https://mempool.space/signet/tx/8a76a44e0b9d8577cc382941dfef2f1775cbd10ae9b20027fb72a2fad040d861) |
| 5 | Savings with a beneficiary: 2 of 3 | DEVICE + HELPER | 327,712 | [`c7ad74d1…af266`](https://mempool.space/signet/tx/c7ad74d1ba8a026b772668bc3d0fd8dcebce7037d0011508189924192b3af266) |
| 6 | Savings with a beneficiary: 2 of 3 | PHONE + DEVICE | 327,713 | [`d4e02a2d…d0007`](https://mempool.space/signet/tx/d4e02a2d2d8ec1382392f2052903f5f54eb27858676584a9e9553271727d0007) |
| 7 | Savings with a beneficiary: the check-in | PHONE + HELPER | 327,714 | [`3f8c86c7…b391c`](https://mempool.space/signet/tx/3f8c86c705f26a4589b6ed5ff11160e0598c73d8e503948beceb6edeef0b391c) |
| 8 | Savings with a beneficiary: HEIR alone, after the heir delay | HEIR | 327,726 | [`12c56933…9d245`](https://mempool.space/signet/tx/12c56933c34cb2c4c90e8068d0aa263b7e6b6c9844f5b0e8b5532eb465c9d245) |

The DEVICE + HELPER pair with a beneficiary (5) is the pair with no phone: the 2-of-3 path never needs the phone.

**Refused on the way:** before transaction 8, `twofold send --path heir` refused HEIR alone while the check-in's coin was younger than the heir delay. The first refusal, one block after the check-in, read "input 0 needs 12 confirmations before it can be spent this way, and has 1". The script tried again every 5 minutes, and it kept refusing until block 327,725, the coin's 12th confirmation; transaction 8 went into the next block. The delay counted from the check-in (7), which had made that coin.

## Checked on the node

Each transaction was read back from the node (`getrawtransaction` with its block hash) to confirm how it was spent:

- **1–4** spend the P2WSH `sortedmulti` script (`OP_2 <key> <key> <key> OP_3 OP_CHECKMULTISIG`) with two signatures.
- **5–7** spend the Taproot `multi_a` leaf (ending `OP_CHECKSIGADD OP_2 OP_NUMEQUAL`) with two signatures.
- **8** spends the heir leaf (ending `OP_CHECKSIGVERIFY OP_12 OP_CHECKSEQUENCEVERIFY`) with one signature, and its input's `nSequence` is 12.
- **4** has one output, the new wallet's first address. The move check in `sign` and `broadcast` compared it with the beneficiary and heir delay set up in the old wallet's settings.

## The two wallets

Public descriptors only, test keys on signet, so anyone can check the addresses above:

Savings with no beneficiary (birthday 327,699):

```
wsh(sortedmulti(2,[db6fe03c/87'/1'/0']tpubDCWWw4qXBPuCHnodDhymzGEkKFwXEgLuNkQHZpfuBaX7PpQQsbyhMGhV1aJvuuw7DSAE5soYjUeSdvDyiTs9NoBhVhFA5GZCffsvj6D3vHr/<0;1>/*,[ba7e4bd0/87'/1'/0']tpubDCAp2ws4f4AoL6BQZdVDGYcdN2yPBhFEs8rhMuFaG9P45uiTva2FvGuWG9QSDB8GrS1DeuqvLpkKK8wdRHr3skxUZHLGCfgv5rvd311GC9A/<0;1>/*,[9f2f0163/87'/1'/0']tpubDCR1MpRLW4vSyZn7WqYXVeRqTfpoizYr19T8rswUd7qpHYzGdjVUKimY1NY7ZhEgxMjZLeb3WBJp6sRBgtKAXzcXWzybHFHzHWCYdGMhreT/<0;1>/*))#f2jk34gy
```

Savings with a beneficiary (birthday 327,710):

```
tr(tpubD6NzVbkrYhZ4YbF7xf6jW2puRf71YNVncZjJ8oQVc6VA3JmoLbnnRSLLWT2BLHZY31RPD7R5tGnJ3v4ae48iPAzxzXs3JH6AMer4wxxm4rq/<0;1>/*,{multi_a(2,[db6fe03c/87'/1'/0']tpubDCWWw4qXBPuCHnodDhymzGEkKFwXEgLuNkQHZpfuBaX7PpQQsbyhMGhV1aJvuuw7DSAE5soYjUeSdvDyiTs9NoBhVhFA5GZCffsvj6D3vHr/<0;1>/*,[ba7e4bd0/87'/1'/0']tpubDCAp2ws4f4AoL6BQZdVDGYcdN2yPBhFEs8rhMuFaG9P45uiTva2FvGuWG9QSDB8GrS1DeuqvLpkKK8wdRHr3skxUZHLGCfgv5rvd311GC9A/<0;1>/*,[9f2f0163/87'/1'/0']tpubDCR1MpRLW4vSyZn7WqYXVeRqTfpoizYr19T8rswUd7qpHYzGdjVUKimY1NY7ZhEgxMjZLeb3WBJp6sRBgtKAXzcXWzybHFHzHWCYdGMhreT/<0;1>/*),and_v(v:pk([d9c7f941/87'/1'/0']tpubDCbmpjEnH5guSgeK9rwDoWP6WSm2e4tuGDiKtzV955d5v5tVqFqWXcAjEMkY271Drf9z2BSqKyEKLFcF41qvnXyhVC5rhhDdbfitLi4M6D5/<0;1>/*),older(12))})#wv60dvd2
```

The keys are PHONE, DEVICE, HELPER and HEIR, in that order (fingerprints `db6fe03c`, `ba7e4bd0`, `9f2f0163`, `d9c7f941`).

## Assumptions and limits

- **Short test delays.** Heir delay 12 blocks (about 2 hours), check-in every 6 blocks with a 3-block margin, refresh window 2 blocks, minimum heir delay lowered to 10. The placeholders (61,320, 52,560 and 8,760 blocks; minimum 26,280) would take over a year on signet. Twofold's regtest tests cover the full-length heir delay.
- **Network fee:** the CLI's default, 2 sat/vB.
- **The payment rule didn't fire.** Payments 5 and 6 spent a coin with one confirmation, below the 4-confirmation refresh age, so nothing extra was refreshed. The rule is covered by the regtest tests.
- **Forbidden cases ran on regtest, not here,** apart from HEIR before the delay. One key alone, HELPER alone and a stranger's key are refused by the CLI and library tests. A refused transaction can't be put on a public chain anyway.
- **Signed by Twofold's own tool.** Leaving through Liana or Bitcoin Core on signet is still listed in the [exit drill](exit-drill.md).
- **A pruned node.** Running this found that a new wallet couldn't sync from a pruned node; fixed on the same branch (see the CLI README).

## Running it again

From the repo root, with a signet node running and synced, in Windows PowerShell:

```
.\contrib\signet-demo\demo.ps1 setup     # fresh keys, Savings with no beneficiary, a funding address
.\contrib\signet-demo\demo.ps1 plain     # 2 of 3, once per pair of keys
.\contrib\signet-demo\demo.ps1 move      # add a beneficiary: the move
.\contrib\signet-demo\demo.ps1 taproot   # 2 of 3 with the heir path in place, and a check-in
.\contrib\signet-demo\demo.ps1 heir      # HEIR alone, once the heir delay has passed
```

`setup` prints an address to fund from a signet faucet (about 0.0005 signet BTC is plenty). Each step waits for its transactions to confirm. The whole run takes about three hours, mostly waiting out the heir delay. Wallets and keys go under `%USERPROFILE%\.twofold` (`signet`, `signet-heir`), and the transaction IDs into `signet-demo-txids.txt` there. `setup` refuses to run twice, so remove those to start over.
