# twofold-savings

The rules for the Twofold **Savings** wallet, as a Taproot descriptor, plus the checks Twofold runs before it builds or finalizes a Savings spend.

**Test networks only.** The library refuses mainnet, and all tests run on throwaway regtest nodes with keys generated fresh in memory. The delays are placeholders until an outside Bitcoin reviewer signs off.

## Spending paths

```mermaid
flowchart LR
    coin(["Savings coin<br/>Taproot · no key-path spend"])
    coin -->|"leaf 1 · any time"| two["Any 2 of<br/>PHONE · DEVICE · HELPER"]
    coin -->|"leaf 2 · once the coin has 39,420 confirmations (about 9 months)"| dev["DEVICE alone<br/>(signs with its DEVICE_R key)"]
    coin -->|"leaf 3 · once the coin has 61,320 confirmations (about 14 months)"| heir["HEIR alone"]
```

| Who approves | Allowed? |
| --- | --- |
| Any 2 of PHONE, DEVICE, HELPER | Yes, any time |
| DEVICE alone | Only once the coin has waited the device delay |
| HEIR alone | Only once the coin has waited the heir delay |
| PHONE alone, HELPER alone | Never, however long the coin waits |
| HEIR + HELPER, HEIR + PHONE | Not before the heir delay (then HEIR could act alone anyway) |
| HEIR + DEVICE | Not before the device delay (then DEVICE could act alone anyway) |
| DEVICE with both its keys | Not before the device delay |

The descriptor, with placeholder names for the keys:

```text
tr(UNSPENDABLE/<0;1>/*,
   { multi_a(2, PHONE/<0;1>/*, DEVICE/<0;1>/*, HELPER/<0;1>/*),
     { and_v(v:pk(DEVICE_R/<0;1>/*), older(39420)),
       and_v(v:pk(HEIR/<0;1>/*),     older(61320)) } })
```

## Using it

```rust
let keys = SavingsKeys::new(phone, device, device_r, helper, heir)?; // tpub…/<0;1>/* keys
let policy = SavingsPolicy::new(keys, Delays::default(), Network::Regtest)?;
let descriptor = policy.export_descriptor();          // for backup, Liana, Bitcoin Core
let psbt = spend::build_spend(&mut wallet, policy.delays(), spend::Path::Device, |b| { /* recipients, fee */ })?;
HolderKey::from_secret_str(&key_text)?.sign(&mut psbt)?; // one holder approves (test networks only)
let tx = spend::finalize(&wallet, &signed_psbt)?;     // refuses if too early or not enough approvals
```

## Check-in

Each coin's delay clock starts when that coin confirms. A **check-in** spends every confirmed Savings coin, through the 2-of-3 path, into one new Savings address, so the device and heir delays start again from zero. It must happen before the oldest coin reaches the device delay; after that, DEVICE alone could spend it.

- **Interval rule.** `checkin::CheckinRule::new(interval, margin, delays)` requires `interval + margin <= device delay`, with both at least 1 block. So an interval at or past the device delay is always refused. Defaults: interval 26,280 blocks (about 6 months), margin 4,320 blocks (about 30 days). Against the 39,420-block device delay, that leaves 13,140 blocks. All three are placeholders.
- **Coin status.** For each coin, `checkin::coins` gives:
  - blocks until its device path and its heir path open
  - **due:** the coin has reached the interval
  - **overdue:** its device path is already open

  `checkin::reminder` sums these up for the whole wallet.
- **Building and checking.**
  - `checkin::build_checkin` drains every confirmed coin to one new change address through the 2-of-3 path, and marks the PSBT with a proprietary field (`twofold`/`checkin`) that other software ignores. Unconfirmed coins are left out.
  - `checkin::check_checkin` refuses a check-in that leaves out any confirmed coin, spends an unconfirmed one, pays anywhere but one Savings address, or uses a delayed path.
  - The CLI runs the check again at `sign` and at `broadcast`, so a coin that confirmed after the check-in was built gets it refused.
- **Why it can't be a delayed path.** The check-in's sequence carries no delay, so its delayed leaves can't be satisfied. DEVICE or HEIR alone can't approve it even when their paths are open, and the network agrees (CSV fails). The tests check both.

## Finding for the reviewer: BDK alone finalizes delayed spends wrongly, both ways

**What we found.** When BDK 3.2.0 finalizes a PSBT, it gives miniscript 12.3.7 three satisfiers: the PSBT itself, a height check and an age check. miniscript combines them with **OR** (`impl_tuple_satisfier` in `miniscript/src/miniscript/satisfy.rs`), which goes wrong in two directions.

1. **Sequence without age** (found in part 1). If the input's sequence matches a leaf's delay, the PSBT satisfier says yes, however young the coin. BDK finalizes a DEVICE-alone or HEIR-alone spend too early, and the network rejects it with `non-BIP68-final`.
2. **Age without sequence** (found while testing the check-in). Once the coin is old enough, BDK's age check says yes even when the input's sequence carries no delay. BDK then finalizes through a delayed leaf, for example DEVICE alone signing a check-in, and the network rejects it because `OP_CHECKSEQUENCEVERIFY` fails.

BDK's own code has a `// TODO: test >= / >` beside its age check.

**Why funds were never at risk.** The network enforces both rules itself (BIP 68 and `OP_CHECKSEQUENCEVERIFY`), and the tests confirm every such transaction is rejected. The risk was in the product: Twofold would have handed the user, or an heir, a transaction that can never confirm.

**The fix.**
- `spend::check_delays` covers the first case. It compares each input's sequence-required delay with the coin's confirmations as of the next block, and returns `Error::TooEarly` if the coin is too young. Time-based delays are refused outright. `spend::build_spend` and `spend::finalize` both run it. Where BDK only knows an upper bound for a coin's confirmation height, the computed age can only come out too young, never too old.
- `spend::finalize` covers the second case. It no longer uses BDK's finalizer: it uses miniscript's `PsbtExt::finalize_mut`, whose only satisfier is the PSBT. A delayed leaf is then used only when the input's sequence commits to that delay.

Together, these match the network's rule. Tests check the boundary exactly: refused at N − 1 confirmations, accepted at N.

**How it's tested.**
- `library_refuses_to_build_a_device_spend_one_block_early` and `…_heir_spend_one_block_early`: the library refuses one block early and builds one block later.
- Every forbidden-path test requires both a library refusal and a node rejection. With `check_delays` switched off, 8 of them fail.
- `a_check_in_can_only_use_the_two_of_three_path`: with both delayed paths open, DEVICE or HEIR alone can't finalize a check-in, and the node rejects a hand-built attempt. With BDK's finalizer, this test failed.
- Two tests pin BDK's behaviour, so we notice if it ever changes: `bdk_alone_would_finalize_an_early_delayed_spend` and `bdk_alone_would_finalize_a_delayed_leaf_whose_sequence_has_no_delay`.

## Assumptions

Every assumption about policy, timelocks, fees or signing in this crate, per the repo rules.

### Policy

- The policy is exactly the one in `docs/milestone-1.md`. The **recovery contact** path (for example contact + helper after a wait) is left out on purpose, and has to be decided before Milestone 2.
- **DEVICE holds two keys:** DEVICE for the 2-of-3 path and DEVICE_R for its delayed path. In the tests they come from the same device seed on different accounts. Liana expects one key per spending path, and SegWit v0 miniscript forbids repeated keys. It doesn't change who can spend.
- All five keys must be different extended keys. `SavingsKeys::new` refuses a repeat.
- **After 61,320 confirmations, DEVICE alone and HEIR alone can both spend**, so whichever acts first wins. For the reviewer to judge.
- **A stolen DEVICE plus a missed check-in is enough to spend once the device delay has passed.** The check-in (a later PR) is what keeps the device delay from opening.

### Delays

- Defaults: device 39,420 blocks (about 9 months), heir 61,320 blocks (about 14 months). Placeholders, and configurable through `Delays::new`.
- They are **relative** delays (`older`, BIP 68/112), counted in **blocks only**. Delays are stored as `u16`, which caps them at the 65,535-block consensus limit and rules out time-based values. The heir delay must be longer than the device delay, and the device delay at least 1 block.
- Each coin's clock starts when **that coin** confirms. New deposits start their own clock. Change from any Savings spend goes to a new Savings address, so it gets a fresh clock too. A check-in must therefore include every coin.
- **Check-in timing.** The interval (default 26,280 blocks) plus a margin (default 4,320 blocks) must fit inside the device delay. The margin is room for fees, confirmations and a late check-in. Coins are "due" from the interval and "overdue" once their device path is open. These are placeholders, like the delays.
- **A check-in spends only confirmed coins.** Unconfirmed coins are left out, and they don't count as missing, because their delays haven't started, so there's nothing to restart. Including them would be a risk. Anyone can send to a Savings address, and a check-in can't confirm before the coins it spends. So a stranger could stall the check-in with a coin paying a very low fee, or break it by replacing their coin (RBF). A check-in that does spend an unconfirmed coin is refused (`Error::CheckinSpendsUnconfirmed`). Once a coin confirms, its clock starts, and the next check-in must include it.
- **Immature coinbase coins.** A coinbase coin that hasn't matured is confirmed but can't be spent, so the check-in is refused until it matures. That only happens on regtest and signet, where Savings can be funded by mining.
- The 61,320-block heir delay sits 4,215 blocks (about 1 month) below the cap, which leaves little room to lengthen it.
- "Has N confirmations" means the funding block counts as the first. A spend with a delay of N can go in the block N blocks after the coin's block, which is when the coin has N confirmations from the next block's point of view.

### Script type and keys

- **Taproot.** Only the leaf actually used is revealed on-chain, so everyday 2-of-3 spends don't reveal the inheritance path. The 2-of-3 leaf sits highest in the tree, so it gets the shortest proof.
- **No key-path spend.** The internal key is an extended key whose public key is BIP 341's NUMS point H (`0250929b…3ac0`). Nobody can know its private key at any index.
  - **Chain code.** SHA256 of the public keys of every key in the tap tree, in **tree order**: PHONE, DEVICE, HELPER (the 2-of-3 leaf), then DEVICE_R, then HEIR. A key that appears twice counts twice.
  - **Why the chain code matters.** Because it differs per wallet, different wallets don't share an internal key, and anyone with the descriptor's public keys can recompute it.
  - **Liana compatibility.** This is Liana's convention exactly (`unspendable_internal_key`), so Liana recognises the key as unspendable and imports the wallet. Tests pin it to Liana's published test vectors and to a value from Liana's own code for the Savings tree.
- **⚠️ Every Savings address changed when this was fixed.** Earlier versions hashed the keys in role order (PHONE, DEVICE, DEVICE_R, HELPER, HEIR). Liana v15.0 read that internal key as a real, spendable key and refused the descriptor: "Descriptor is not compatible with a Liana spending policy."
  - Switching to tree order changed the internal key, and with it every Savings address.
  - **Wallets made by older versions must be recreated.** That includes any `~/.twofold` test wallet and the demo wallet. Their coins stay spendable by those old descriptors, and they're only on test networks.
- The test keys use derivation paths `m/87'/1'/0'` (and `m/87'/1'/1'` for DEVICE_R), with `/<0;1>/*` for receive and change. These paths are **test-only**; the real ones get decided with the reviewer.
- Keys must be test-network extended public keys (`tpub`). Mainnet keys and a mainnet `Network` are both refused.

### Fees

- The library doesn't choose fees: `build_spend` lets the caller set the fee rate.
- Tests use a fixed 2 sat/vB, because regtest has no fee estimates. That's above the default relay minimum.
- No fee bumping yet. BDK's default sequence for the 2-of-3 path signals RBF, and the delayed paths' sequences (the delay itself) do too.

### Export

- `export_descriptor()` returns the public descriptor with `<0;1>` and its checksum. `export_receive_and_change()` returns separate receive and change descriptors.
- Neither contains private keys, but both reveal every Savings address and balance, so treat them as private.
- Bitcoin Core reads the export and derives exactly the addresses BDK hands out (`tests/export.rs`).
- **Leaving without Twofold:** Liana v15.0 imports the export, and so does Bitcoin Core 31.1 on its own. Each one spends through every path on regtest and refuses a delayed spend one block early. See the [exit drill](../../docs/exit-drill.md). Sparrow can't import Savings at all.

## Not yet verified

- **The exit through Liana's desktop app**, the route Windows users have, and the exit on signet. Both are listed in the [exit drill](../../docs/exit-drill.md).
- **One signer on two paths.** Liana's own wallets write one signer used on two paths as the *same* xpub with different multipath steps (`<0;1>` and `<2;3>`); its published test vectors do this.
  - Liana counts a key as the xpub *plus* its paths, so it also accepts our separate-account keys for DEVICE and DEVICE_R. Exporting to Liana needs no change.
  - `SavingsKeys::new` refuses the same xpub on two paths, so Twofold couldn't import a wallet made *in* Liana. If we ever need that, allow the same xpub when the paths differ, while still refusing any overlap that would make two roles derive the same key.

## Tests

Tests start a throwaway `bitcoind` per test. Each one runs in a temp folder, on a random local RPC port, with P2P off. It never touches a node you already run. Point `BITCOIND_EXE` at your binary.

Bash (Git Bash, macOS, Linux):

```bash
BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe" cargo test
```

PowerShell:

```powershell
$env:BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe"; cargo test
```

The full-length test mines all 61,320 blocks and takes about 10–12 minutes, so a plain `cargo test` skips it. Run everything, including that test, with:

```bash
BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe" cargo test -- --include-ignored
```

```powershell
$env:BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe"; cargo test -- --include-ignored
```

| File | What it proves |
| --- | --- |
| `src/policy.rs` (unit) | Reading the rules back out of the descriptor matches a rule written independently. Checked for all 32 combinations of the five keys, at ages around each delay. Also names each forbidden case. The internal key matches Liana v15.0's published test vectors and a value from Liana's own code for the Savings tree, and isn't the old role-order key. |
| `tests/allowed_paths.rs` | Each allowed path spends on regtest, through the expected leaf, at exactly its delay |
| `tests/forbidden_paths.rs` | Each forbidden case, tried through all three leaves, is refused by Twofold **and** rejected by the node. This includes the case of signing a shorter sequence to skip a delay. |
| `tests/checkin.rs` | A check-in restarts both delays for the new coin, to the block. A check-in that leaves out a confirmed coin (one that confirmed later, or one hand-picked) is refused, as is one that pays out or uses a delayed path. An unconfirmed incoming coin neither blocks a check-in nor joins it, and one that tries to spend it is refused. DEVICE or HEIR alone can't approve a check-in even with their paths open. |
| `tests/default_delays.rs` | The real 39,420 / 61,320 delays open at exactly those confirmations (ignored by default) |
| `tests/export.rs` | Bitcoin Core derives the same addresses from the export |
| `tests/harness.rs` | The harness's hand-built spends are valid when they have enough signatures, so a rejection elsewhere is the policy's doing |

Keys are generated in memory for each test and never printed, logged or written to disk. While mining, the harness moves each throwaway node's clock forward 10 minutes per block (`setmocktime`). Without that, a node refuses blocks mined thousands per second once their timestamps run 2 hours ahead.

The [`twofold` command-line tool](../twofold-cli/README.md) wraps all of this.

## Not here yet

- The signet demo
- The Liana/Sparrow exit drill
- The recovery contact path
