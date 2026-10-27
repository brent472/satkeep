## The signet run with `satkeep`, and Sparrow by hand: results

Docs only. On 25 October 2026 you ran the signet demo again with the renamed `satkeep`, and imported Savings into the real Sparrow app by hand, both on your Windows PC. This records the results.

### Transactions

**From `signet-demo-txids.txt`,** in the order the demo logged them:

| Log line | Transaction | Block |
| --- | --- | --- |
| plain 2-of-3 (PHONE + DEVICE) | [`d56deb5d6b6e3c03e5cbe063b9add929b8d77fe9ccfa14185b9215c8711ed987`](https://mempool.space/signet/tx/d56deb5d6b6e3c03e5cbe063b9add929b8d77fe9ccfa14185b9215c8711ed987) | 327,932 |
| plain 2-of-3 (PHONE + HELPER) | [`47336a9197f955ca01fdd7bbb9f2e90b52b285b33eaec95ee4da3f25d5cd098a`](https://mempool.space/signet/tx/47336a9197f955ca01fdd7bbb9f2e90b52b285b33eaec95ee4da3f25d5cd098a) | 327,934 |
| plain 2-of-3 (DEVICE + HELPER) | [`e375d47cfe95138b814f944be5b6a66fbdc1db3fb6f0fb8fa20472aa56bb24e0`](https://mempool.space/signet/tx/e375d47cfe95138b814f944be5b6a66fbdc1db3fb6f0fb8fa20472aa56bb24e0) | 327,935 |
| move to Taproot (PHONE + DEVICE) | [`99a16c6542b5be7b07d8edf54571b47869538e5ed1967200d655ca12561de845`](https://mempool.space/signet/tx/99a16c6542b5be7b07d8edf54571b47869538e5ed1967200d655ca12561de845) | 327,948 |
| Taproot 2-of-3 (DEVICE + HELPER) | [`03d89f515f3a4345ae157d879d8dc1dd64ce1a853b4d0a2823a884caa75c2e8e`](https://mempool.space/signet/tx/03d89f515f3a4345ae157d879d8dc1dd64ce1a853b4d0a2823a884caa75c2e8e) | 327,949 |
| Taproot 2-of-3 (PHONE + DEVICE) | [`a9d33a9e4c1aa7aeddf135d7adcf41759562616e6c2eed9605ebac7a911b1f26`](https://mempool.space/signet/tx/a9d33a9e4c1aa7aeddf135d7adcf41759562616e6c2eed9605ebac7a911b1f26) | 327,951 |
| Taproot check-in (PHONE + HELPER) | [`d9381aa6b482605dc43352007887b900bf11ea869ff87b5c334a0035c5e761f5`](https://mempool.space/signet/tx/d9381aa6b482605dc43352007887b900bf11ea869ff87b5c334a0035c5e761f5) | 327,952 |
| HEIR alone refused before the heir delay | no transaction: "input 0 needs 12 confirmations before it can be spent this way, and has 1" | — |
| Taproot HEIR alone, after 12 blocks | [`e71d82cd9e35f33a3f36ffd13fd3cfbc2dfd19af6ac3744e6c0236e933018678`](https://mempool.space/signet/tx/e71d82cd9e35f33a3f36ffd13fd3cfbc2dfd19af6ac3744e6c0236e933018678) | 327,965 |

**Not in that file, also part of the run:**

| What | Transaction | Block |
| --- | --- | --- |
| Funding, from the faucet | [`9ef5fd3c53305447c755f4d0cb63b79ac0e6b4a20bc8634e3164fc8ab4220f35`](https://mempool.space/signet/tx/9ef5fd3c53305447c755f4d0cb63b79ac0e6b4a20bc8634e3164fc8ab4220f35) (output 44) | 327,923 |
| 2 of 3 signed in Sparrow 2.5.5, back to Savings | [`8622625da9060835a29c62182ac95612b16c93a15620c6d2c7aa4324b50851da`](https://mempool.space/signet/tx/8622625da9060835a29c62182ac95612b16c93a15620c6d2c7aa4324b50851da) | 327,928 |
| 0.06 BTC back to the faucet, PHONE + DEVICE | [`09ebb298787098474a99264f41b91d68320de96ef449f40231a754edbc896cac`](https://mempool.space/signet/tx/09ebb298787098474a99264f41b91d68320de96ef449f40231a754edbc896cac) | 327,947 |

**How each was checked:** every block was read from your node, and every spend's witness confirmed:
- **P2WSH spends:** two signatures and the `sortedmulti` script.
- **Taproot 2-of-3 spends:** the `multi_a` leaf with two signatures.
- **HEIR's spend:** the heir leaf with one signature, and `nSequence` 12.
- **Who signed:** read from the fingerprints in each payment's saved PSBT.

### What changes

- **[`docs/signet-demo.md`](../signet-demo.md):**
  - The new run, with every transaction above. Rows S (Sparrow) and R (the faucet return) are marked.
  - The wallets' public descriptors.
  - What was checked on the node.
  - Why HEIR alone landed one block after the earliest it could: the script only retries every 5 minutes.
  - That the HEIR refusal on signet is Satkeep's own check; the network's `non-BIP68-final` refusal is shown on regtest.
  - The first, pre-rename run kept as history.
  - A note on sending a faucet's excess back before `move`.
- **[`docs/exit-drill.md`](../exit-drill.md):**
  - Sparrow 2.5.5 is now checked in the real app, on signet, in table B and the Sparrow section, with what its screens needed.
  - "Not done yet" keeps only Electrum and Nunchuk.
- **[`docs/exit-guide.md`](../exit-guide.md), Step 4b:** follows Sparrow's real screens in place of "to be checked":
  - **File › Import Wallet... › Output Descriptor**, with the start date it requires;
  - **Use 12 Words**;
  - **Create Keystore**, then **Import Keystore**;
  - **Apply**;
  - **Create Transaction**, **Finalize Transaction for Signing**, **Sign**, **Broadcast Transaction**;
  - "Consolidate" for a payment to itself.
- **[`docs/milestone-1-status.md`](../milestone-1-status.md):**
  - Leaving without Satkeep is done for Sparrow too, the signet demo is done with `satkeep`, and the "Done when" rows say so.
  - The step lists are replaced by a summary of what you did and the corrections it found.

**Still left in Milestone 1:**
- the outside Bitcoin developer's review;
- the Liana desktop app by hand;
- leaving through Liana and Bitcoin Core on signet.

### Security review

**None.** Docs only.
- **No private data:** no keys, phrases, PSBTs or approval codes. The descriptors are the run's public test keys on signet, as before.
- **Public data only:** transaction IDs, and the faucet's address, where the funding transaction sends the faucet's own change.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
