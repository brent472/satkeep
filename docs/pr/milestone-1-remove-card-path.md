Removes the card-only delayed path, and adds Savings with no beneficiary, following master brief v3.8–v3.11. Test networks only.

## The two shapes

| | No beneficiary yet | With a beneficiary |
| --- | --- | --- |
| **Descriptor** | `wsh(sortedmulti(2, PHONE, DEVICE, HELPER))`, the standard P2WSH multisig format | `tr(UNSPENDABLE, { multi_a(2, PHONE, DEVICE, HELPER), and_v(v:pk(HEIR), older(61320)) })` |
| **Delayed path** | None | HEIR alone after the heir delay (placeholder 61,320 blocks, about 14 months) |
| **Check-in** | None needed (refused) | Every 52,560 blocks (about 12 months), with an 8,760-block margin (about 2 months), checked against the heir delay |
| **Exit without Twofold** | Bitcoin Core, Sparrow, Electrum, Nunchuk and other standard multisig wallets | Liana and Bitcoin Core |

- **Why P2WSH for the plain shape:** Liana v15.0 refuses any policy with no timelocked path (*"A Liana policy requires at least one recovery path"*, `liana/src/descriptors/analysis.rs`). The exit drill confirmed it for a Taproot 2-of-3. The P2WSH format is what most multisig wallets read. Written up in the library README under *Why two formats*.
- **DEVICE alone never spends**, in either shape, at any age. DEVICE has one key now, like everyone else.
- **The internal key** of the Taproot shape keeps Liana's tree-order convention (now PHONE, DEVICE, HELPER, HEIR).

## Adding or changing a beneficiary

- **New `migrate` module:**
  - **Target wallet:** the same PHONE, DEVICE and HELPER plus HEIR, in the Taproot shape.
  - **One 2-of-3 transaction** moves every confirmed coin from the old wallet (P2WSH, or Taproot when changing the beneficiary) to its first address, for one network fee.
  - **The PSBT carries the HEIR key and delay**, so `check_move` can rebuild the target and check the single output goes there and nowhere else.
- **New CLI command `add-heir`** creates the new wallet folder and prepares the move.
- **`sign` and `broadcast` re-check the move.** `sign` shows the HEIR key and the delay, and `add-heir` prints them with the destination address. (From the security review: whoever prepares a move chooses HEIR, so approvers must be able to see exactly which key.)

## What else changed

- **Library:**
  - `Delays` is `none()` or `with_heir(blocks)`.
  - `SavingsKeys` takes an optional HEIR.
  - `Role` has four roles.
  - `HolderKey` signs on account 0 only, with the SegWit v0 signer for P2WSH coins.
  - `finalize` reports an unsatisfied P2WSH input as "not enough approvals".
- **CLI:**
  - `create` makes either shape: `--heir` is optional, and the check-in flags need it. `--device-r`, `--device-delay` and `--path device` are gone.
  - `keygen` prints one public key.
  - `balance` shows the heir path only when there is one.
  - An old wallet with DEVICE_R gets a clear error.
  - The demo in the CLI README is rewritten. **It hasn't been run against a node yet**; the CLI tests cover each step.
- **Fuzz:** the `psbt` target covers the move (if `check_move` accepts, the PSBT must be a real move to a same-keys wallet). The `config` corpus has both shapes plus the old one.
- **Docs and CLAUDE.md:**
  - The library and CLI READMEs, `docs/milestone-1.md` and `docs/exit-drill.md` describe both shapes.
  - `CLAUDE.md` takes the private repo's v3.11 first paragraph (one balance with an everyday pocket), the plain words "key, approve, network fee, Savings, everyday payments", and the heir-only timelock and yearly check-in rules. The public-repo pointers stay.

## Checks

- **`cargo test --workspace`** passes, with throwaway regtest nodes. Every spending path has a test that it works and a test that the forbidden version fails, in both shapes:
  - **Allowed:** any 2 of 3 in both shapes; HEIR at exactly its delay.
  - **Forbidden (Twofold *and* the node refuse):**
    - PHONE, DEVICE or HELPER alone, even after the heir delay
    - HEIR alone, or with any one other key, before the delay
    - a shorter sequence to skip the delay
    - with no beneficiary: any single key at any age, the heir path, or a long sequence
  - **Move:** both coins in one transaction, then HEIR spends from the new wallet at its delay. One key alone can't approve a move. Refused: a move that pays elsewhere, names a different HEIR, isn't marked, has an extra output, goes to a wallet with other keys, or leaves out a coin.
  - **Check-in:** only with a beneficiary; restarts the heir delay; the yearly defaults fit the 61,320-block placeholder exactly.
- **`cargo clippy -D warnings`** (workspace and fuzz crate) and **`cargo deny check`** pass.
- **Exit drill, [run 40025126932](https://github.com/brent472/twofold/actions/runs/40025126932): 17 of 17 checks pass.**
  - **Taproot shape:** Liana (HotSigner) and Bitcoin Core each spend 2 of 3 and HEIR at exactly 20 blocks, and refuse HEIR one block early. DEVICE alone is refused.
  - **P2WSH shape:**
    - The export is `wsh(sortedmulti(2, …))`, in the form Sparrow's own parser accepts (patterns ported from its source; Sparrow not installed).
    - Bitcoin Core derives the same addresses, refuses each single key, and spends with 2 of 3.
  - Both key-material scans pass.

## Security review

SECURITY_REVIEW

🤖 Generated with [Claude Code](https://claude.com/claude-code)
