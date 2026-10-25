# Signet demo

One transaction for each allowed Savings path, in both shapes, on the public signet test network, made with the project's command-line tool. It ticks the Milestone 1 box "Signet demo transaction for each allowed path". Regtest covers the same paths and every forbidden case in the test suites; this run shows them on a public network with real block times.

**Test bitcoin only.** Signet coins have no value. The four keys were fresh test keys made by `satkeep keygen` for this run, kept outside the repo, and never printed.

## What ran

Run on 25 October 2026, blocks 327,923 to 327,965, with `satkeep` built from main at `4133904` and Bitcoin Core 31.1 on signet, pruned (`-prune=550`), on Windows. Payments went to an address nobody can spend from (a Taproot output to the NUMS point, `tb1p2zffkaxp…tqqeygjde`).

The run also took in two extra transactions:
- **The Sparrow import by hand** (row S): Sparrow 2.5.5 signed a 2-of-3 payment with no Satkeep code.
- **The faucet's coins, given back** (row R): the faucet sent about 0.06 signet BTC, far more than the run needs. Most of it went back to the faucet before the move, so only about 55,000 sats were spent to the unspendable address.

| # | Path | Approved by | Block | Transaction |
| --- | --- | --- | --- | --- |
| 0 | Funding, from a public faucet | the faucet | 327,923 | [`9ef5fd3c…20f35`](https://mempool.space/signet/tx/9ef5fd3c53305447c755f4d0cb63b79ac0e6b4a20bc8634e3164fc8ab4220f35) |
| S | Savings with no beneficiary: 2 of 3, **signed in Sparrow 2.5.5**, back to Savings | PHONE + DEVICE | 327,928 | [`8622625d…851da`](https://mempool.space/signet/tx/8622625da9060835a29c62182ac95612b16c93a15620c6d2c7aa4324b50851da) |
| 1 | Savings with no beneficiary: 2 of 3 | PHONE + DEVICE | 327,932 | [`d56deb5d…ed987`](https://mempool.space/signet/tx/d56deb5d6b6e3c03e5cbe063b9add929b8d77fe9ccfa14185b9215c8711ed987) |
| 2 | Savings with no beneficiary: 2 of 3 | PHONE + HELPER | 327,934 | [`47336a91…d098a`](https://mempool.space/signet/tx/47336a9197f955ca01fdd7bbb9f2e90b52b285b33eaec95ee4da3f25d5cd098a) |
| 3 | Savings with no beneficiary: 2 of 3 | DEVICE + HELPER | 327,935 | [`e375d47c…b24e0`](https://mempool.space/signet/tx/e375d47cfe95138b814f944be5b6a66fbdc1db3fb6f0fb8fa20472aa56bb24e0) |
| R | Savings with no beneficiary: 2 of 3, 0.06 BTC back to the faucet | PHONE + DEVICE | 327,947 | [`09ebb298…96cac`](https://mempool.space/signet/tx/09ebb298787098474a99264f41b91d68320de96ef449f40231a754edbc896cac) |
| 4 | Adding a beneficiary: the move, in one payment | PHONE + DEVICE | 327,948 | [`99a16c65…de845`](https://mempool.space/signet/tx/99a16c6542b5be7b07d8edf54571b47869538e5ed1967200d655ca12561de845) |
| 5 | Savings with a beneficiary: 2 of 3 | DEVICE + HELPER | 327,949 | [`03d89f51…c2e8e`](https://mempool.space/signet/tx/03d89f515f3a4345ae157d879d8dc1dd64ce1a853b4d0a2823a884caa75c2e8e) |
| 6 | Savings with a beneficiary: 2 of 3 | PHONE + DEVICE | 327,951 | [`a9d33a9e…b1f26`](https://mempool.space/signet/tx/a9d33a9e4c1aa7aeddf135d7adcf41759562616e6c2eed9605ebac7a911b1f26) |
| 7 | Savings with a beneficiary: the check-in | PHONE + HELPER | 327,952 | [`d9381aa6…761f5`](https://mempool.space/signet/tx/d9381aa6b482605dc43352007887b900bf11ea869ff87b5c334a0035c5e761f5) |
| 8 | Savings with a beneficiary: HEIR alone, after the heir delay | HEIR | 327,965 | [`e71d82cd…18678`](https://mempool.space/signet/tx/e71d82cd9e35f33a3f36ffd13fd3cfbc2dfd19af6ac3744e6c0236e933018678) |

**Who signed which payment** was read from the signatures saved in each payment's PSBT, by key fingerprint. The DEVICE + HELPER pair with a beneficiary (5) is the pair with no phone: the 2-of-3 path never needs the phone.

**Refused on the way:** before transaction 8, `satkeep send --path heir` refused HEIR alone while the check-in's coin (7, block 327,952) was younger than the heir delay.
- **The first refusal** read "input 0 needs 12 confirmations before it can be spent this way, and has 1".
- **It kept refusing,** every 5 minutes, through 10 confirmations. The earliest block HEIR alone could go in was 327,964, 12 blocks after the check-in. Transaction 8 was mined one block later, in 327,965, because the script only tries every 5 minutes and signet mined several blocks in a row.
- **This refusal is Satkeep's own check,** made before it builds anything. The network's refusal of an early heir spend (`non-BIP68-final`) is shown on regtest, in the test suites and the exit drill.

## Checked on the node

Each transaction was read back from the node (`getrawtransaction` with its block hash) to confirm how it was spent:

- **S, 1–3, R and 4** spend the P2WSH `sortedmulti` script (`OP_2 <key> <key> <key> OP_3 OP_CHECKMULTISIG`) with two signatures.
  - S was signed by Sparrow alone.
  - 4 spends two coins, each with two signatures.
- **5–7** spend the Taproot `multi_a` leaf (ending `OP_CHECKSIGADD OP_2 OP_NUMEQUAL`) with two signatures.
- **8** spends the heir leaf (`<HEIR> OP_CHECKSIGVERIFY OP_12 OP_CHECKSEQUENCEVERIFY`) with one signature, and its input's `nSequence` is 12.
- **4** has one output, the new wallet's first address (`tb1pv7kfpc96…lqzx7mmn`). Bitcoin Core derives the same address from the new wallet's descriptor. The move check in `sign` and `broadcast` compared it with the beneficiary and heir delay set up in the old wallet's settings.
- **S** has two outputs, receive address 1 and change address 0 of the same wallet, which Bitcoin Core also derives from the descriptor.
- **R** pays the faucet's address: the funding transaction (0) sends the faucet's own change there.

## The two wallets

Public descriptors only, test keys on signet, so anyone can check the addresses above.

**Savings with no beneficiary** (birthday 327,922):

```
wsh(sortedmulti(2,[e54c53b1/87'/1'/0']tpubDChvZNVMps14g7P2zZQU6xQHGCMn8tKqiEyDXqLDEAjmQr71X8dze3phJ7W3hSLjtFMEiaZQ6DRY68oa455SEtHxcWUBRsWYdzdH4eJE7zY/<0;1>/*,[535d6969/87'/1'/0']tpubDCnBpxTnoXR6biicw9U6gvwzuNBQcm7NMzWy1V65bPs5PjVwUViWvGtQK7xcvTHiBnYdLdHrqL6rrZetNVpp3vwKwZqvVhAAkhsRM1iSFtX/<0;1>/*,[9dd0ab86/87'/1'/0']tpubDCjTGz2DSBFW9NP3FDqzAhPtDhqbVXgYAr299pqcLZa8v5Y5s1gaA8Yaz4wH9QszsVf3uYe25DRmAQZad9YKQtJPKJmayNJ42YLF4uyQgC2/<0;1>/*))#0nd9aqa3
```

**Savings with a beneficiary** (birthday 327,947):

```
tr(tpubD6NzVbkrYhZ4YsjtrAJMZjXpsQJ6ht9VFpTukjDh418s5stNHaDnT7X2vnUB1YjztD6zue6ZjFqR6px4X4NMxrp77Jq445z8hUX2fyXyKWu/<0;1>/*,{multi_a(2,[e54c53b1/87'/1'/0']tpubDChvZNVMps14g7P2zZQU6xQHGCMn8tKqiEyDXqLDEAjmQr71X8dze3phJ7W3hSLjtFMEiaZQ6DRY68oa455SEtHxcWUBRsWYdzdH4eJE7zY/<0;1>/*,[535d6969/87'/1'/0']tpubDCnBpxTnoXR6biicw9U6gvwzuNBQcm7NMzWy1V65bPs5PjVwUViWvGtQK7xcvTHiBnYdLdHrqL6rrZetNVpp3vwKwZqvVhAAkhsRM1iSFtX/<0;1>/*,[9dd0ab86/87'/1'/0']tpubDCjTGz2DSBFW9NP3FDqzAhPtDhqbVXgYAr299pqcLZa8v5Y5s1gaA8Yaz4wH9QszsVf3uYe25DRmAQZad9YKQtJPKJmayNJ42YLF4uyQgC2/<0;1>/*),and_v(v:pk([10bb64cc/87'/1'/0']tpubDDSvDRiUVjNpLopxWhyyLxDfko8VXWP52HXRx9QmdC8H93xyibVrwJxrSTB6bbHKmYJbEUMFZ79U2QsgxaEvw2uPRs5G1brthCCpEA6c3qA/<0;1>/*),older(12))})#mwh3fgnm
```

The keys are PHONE, DEVICE, HELPER and HEIR, in that order (fingerprints `e54c53b1`, `535d6969`, `9dd0ab86`, `10bb64cc`).

## Assumptions and limits

- **Short test delays.** Heir delay 12 blocks (about 2 hours), check-in every 6 blocks with a 3-block margin, refresh window 2 blocks, minimum heir delay lowered to 10. The placeholders (61,320, 52,560 and 8,760 blocks; minimum 26,280) would take over a year on signet. Satkeep's regtest tests cover the full-length heir delay.
- **Network fee:** the CLI's default, 2 sat/vB. Sparrow chose its own for S: 163 sats for 201 vbytes, about 0.8 sat/vB, below the old 1 sat/vB floor. Bitcoin Core 29.1 and later relay it, and signet mined it in the next block.
- **The payment rule didn't fire.** Payments 5 and 6 spent coins with one and two confirmations, below the 4-confirmation refresh age, so nothing extra was refreshed. The rule is covered by the regtest tests.
- **Forbidden cases ran on regtest, not here,** apart from HEIR before the delay. One key alone, HELPER alone and a stranger's key are refused by the CLI and library tests. A refused transaction can't be put on a public chain anyway.
- **Signed by the project's own tool,** except S, which Sparrow signed. Leaving through Liana or Bitcoin Core on signet is still listed in the [exit drill](exit-drill.md).

## The earlier run, before the rename

The first run, on 23–24 October 2026, blocks 327,707 to 327,726, made the same paths with the tool still called `twofold` (files under `%USERPROFILE%\.twofold`, fingerprints `db6fe03c`, `ba7e4bd0`, `9f2f0163`, `d9c7f941`). It also found that a new wallet couldn't sync from a pruned node, fixed on the same branch (see the CLI README).

| # | Path | Approved by | Block | Transaction |
| --- | --- | --- | --- | --- |
| 0 | Funding | the faucet | 327,707 | [`29a00622…d0473`](https://mempool.space/signet/tx/29a006226ec9d616ca57c186acca177122a23fd2cb6aef31e40070b9b89d0473) |
| 1 | No beneficiary: 2 of 3 | PHONE + DEVICE | 327,708 | [`85a14c8b…49323`](https://mempool.space/signet/tx/85a14c8b19ebd2024b1369de07ccf40966a4024d1d5d772625dbf8a32eb49323) |
| 2 | No beneficiary: 2 of 3 | PHONE + HELPER | 327,709 | [`c9e72f24…d2f77`](https://mempool.space/signet/tx/c9e72f24f71e6989f102865cb526b39b9aa6158fed013af9f17ded3eb2ed2f77) |
| 3 | No beneficiary: 2 of 3 | DEVICE + HELPER | 327,710 | [`e813fc2c…b6c26`](https://mempool.space/signet/tx/e813fc2c5c3942ca6b7111fda284333fc8e32b23adbc0526adf093aa412b6c26) |
| 4 | The move | PHONE + DEVICE | 327,711 | [`8a76a44e…0d861`](https://mempool.space/signet/tx/8a76a44e0b9d8577cc382941dfef2f1775cbd10ae9b20027fb72a2fad040d861) |
| 5 | With a beneficiary: 2 of 3 | DEVICE + HELPER | 327,712 | [`c7ad74d1…af266`](https://mempool.space/signet/tx/c7ad74d1ba8a026b772668bc3d0fd8dcebce7037d0011508189924192b3af266) |
| 6 | With a beneficiary: 2 of 3 | PHONE + DEVICE | 327,713 | [`d4e02a2d…d0007`](https://mempool.space/signet/tx/d4e02a2d2d8ec1382392f2052903f5f54eb27858676584a9e9553271727d0007) |
| 7 | The check-in | PHONE + HELPER | 327,714 | [`3f8c86c7…b391c`](https://mempool.space/signet/tx/3f8c86c705f26a4589b6ed5ff11160e0598c73d8e503948beceb6edeef0b391c) |
| 8 | HEIR alone, after the heir delay | HEIR | 327,726 | [`12c56933…9d245`](https://mempool.space/signet/tx/12c56933c34cb2c4c90e8068d0aa263b7e6b6c9844f5b0e8b5532eb465c9d245) |

## Running it again

From the repo root, with a signet node running and synced, in Windows PowerShell:

```
.\contrib\signet-demo\demo.ps1 setup     # fresh keys, Savings with no beneficiary, a funding address
.\contrib\signet-demo\demo.ps1 plain     # 2 of 3, once per pair of keys
.\contrib\signet-demo\demo.ps1 move      # add a beneficiary: the move
.\contrib\signet-demo\demo.ps1 taproot   # 2 of 3 with the heir path in place, and a check-in
.\contrib\signet-demo\demo.ps1 heir      # HEIR alone, once the heir delay has passed
```

`setup` prints an address to fund from a signet faucet (about 0.0005 signet BTC is plenty). Each step waits for its transactions to confirm. The whole run takes about three hours, mostly waiting out the heir delay. Wallets and keys go under `%USERPROFILE%\.satkeep` (`signet`, `signet-heir`), and the transaction IDs into `signet-demo-txids.txt` there. `setup` refuses to run twice, so remove those to start over.

**If a faucet sends far more than needed,** send the excess back before `move`: `heir` spends everything left to the unspendable address. Run `satkeep send` to the faucet's return address, `satkeep sign` with two keys and `satkeep broadcast`, as for row R above.
