Milestone 1, part 2 of 3: `twofold`, a developer command-line tool for the Savings wallet. Test networks only. The check-in comes next, as its own PR.

## What's here

**`crates/twofold-cli`**: the `twofold` binary, built on the `twofold-savings` library, BDK's sqlite store and `bdk_bitcoind_rpc`.

| Command | What it does |
| --- | --- |
| `keygen --name NAME` | Fresh test key, saved by default to `~/.twofold/<network>/keys/NAME.key`, outside the repo. Prints only public keys. |
| `create …` | Savings wallet from five public keys, on `--network regtest` (default) or `signet`. Delays default to the 39,420 / 61,320 placeholders. |
| `address`, `balance` | Receive. Balance shows each coin's confirmations and when its device and heir paths open. |
| `send --path two-of-three\|device\|heir` | Unsigned PSBT through the library's `build_spend`, so a delayed path is refused until every coin used is old enough |
| `sign (--key-file FILE \| --key-stdin) [--yes]` | Shows each output (marking change back to Savings), the network fee and the approval path, asks `Approve? (y/N)`, and only then signs with one holder's key. Refuses a key that adds nothing. |
| `broadcast` | Library `finalize` (refuses too-early or under-approved), then `sendrawtransaction` |
| `export [--split]` | Public descriptor for backup or import |

**Approval prompt (from review)**
- Before signing, `twofold sign` prints every output's address and amount. Money returning to Savings is marked, as change or as one of your own addresses. It also prints the network fee and the approval path, which comes from the inputs' sequences through a new library helper, `spend::path_of`.
- Then it asks `Approve? (y/N)` and signs only on `y`/`yes`. Anything else, or no input, signs nothing and exits with an error.
- The key is read only after approval, so a declined payment never touches it.
- With `--key-stdin`, standard input is the answer line, then the key line. `--yes` skips the question for scripts and tests.
- `sign` now reads the wallet folder, so it can mark change and refuse a payment that doesn't spend this wallet's coins.

**Keys and wallet files stay out of the repo (from review)**
1. **No private key on the command line.** `sign` takes a key only from `--key-file` or `--key-stdin`; there's no `--key`. As a backstop, if any argument contains an extended private key, `twofold` refuses before parsing anything. It never repeats the key, and it warns that the key should be treated as exposed. (Without this check, clap would repeat an unexpected argument in its error.)
2. **`keygen` writes outside the repo by default**, to `%USERPROFILE%\.twofold\<network>\keys\<name>.key`.
   - It never overwrites an existing file.
   - There is no mainnet option.
   - It writes only `tprv` keys, and checks that before writing.
   - `--out` must end in `.key`, and `--name` is limited to letters, digits, `-` and `_`.
3. **Wallet files default to the same place**, `%USERPROFILE%\.twofold\<network>\savings.json` and `wallet.sqlite`. `savings.json` is added to `.gitignore` as a backstop.

`TWOFOLD_HOME` replaces the base folder; the tests point it at a temporary folder, so they never touch the real one.

**Safety rails**
- `--network` has no mainnet value.
- Every command that talks to a node first checks the node's chain matches the wallet's, and refuses mainnet outright.
- `savings.json` and `wallet.sqlite` never hold a private key.
- Private keys come only from `--key-file`/`--key-stdin` and are never printed. Every CLI test fails if any output contains `tprv` or `xprv`.
- The CLI goes through the library for every policy check, so it can't create a wallet or send a payment the library would refuse.

**Shared code (one commit, no behaviour change to part 1)**
- `HolderKey` moved from the test harness into the library, so the CLI and the tests sign the same way. It's a test-network master key: never `Debug` or `Display`, it refuses mainnet, and parse errors never repeat the input.
- The throwaway regtest `Node` moved into a test-only crate, `twofold-testenv`, so the CLI tests use the same one.

## For review

- **A PSBT carries every leaf.** A holder can add a signature the chosen path doesn't need; for example, HELPER can sign the 2-of-3 leaf inside a heir-path payment. It's harmless: `broadcast` only sends what actually satisfies a path, and a test checks exactly that case.
- **Key files are plain text.** `keygen` keys have no encryption and no OS permissions set. That's fine for regtest and signet, and not how the app will hold keys.
- **Sync starts at the wallet's creation height.** That's stored in `savings.json`.
- **This is a developer tool.** Its wording isn't product copy (Section 19 doesn't apply), and it says PSBT where the app wouldn't.

## Tests

The CLI tests run the real binary against throwaway regtest nodes; your running node is never touched. `TWOFOLD_HOME` points at a temporary folder for each test. Keys are generated in memory and passed through stdin, except in the `keygen` and `--key-file` tests, which write fresh keys into that temporary folder; it's deleted when the test ends.

- **`tests/approve.rs`:**
  - the summary shows each output, change and your own addresses, the fee and the path, before the question
  - only `y` signs: `n`, `N`, `no`, an empty line, a space, `maybe`, `yy` or no input leave the PSBT unchanged
  - a declined payment never reads the key file
  - a payment from another wallet is refused
- **`tests/keys.rs`:**
  - a key passed as `--key`, `--key-file KEY`, `--key-file=KEY`, a bare argument, or inside another command's argument is refused, never repeated, and the PSBT is left unchanged
  - there's no `--key` flag
  - `keygen` defaults to `<home>/<network>/keys`, writes only `tprv`, never overwrites, and refuses mainnet, path-like names and non-`.key` files
  - wallet files default to `<home>/<network>`, and `--wallet-dir` must agree with `--network`

- **`tests/wallet.rs`:**
  - create, address, balance (including "opens in N blocks" and "open") and export, checked against the library
  - refuses a node on another network, a mainnet `--network`, creating over an existing wallet, and a policy the library refuses
- **`tests/payments.rs`:**
  - 2-of-3: zero and one approval are refused, a repeat signature is refused, two approvals are sent, and change comes back
  - DEVICE alone and HEIR alone: refused one block early, sent on time
  - HELPER alone (including inside a heir-path payment), a stranger's key, a wrong-network address, a bad amount, a zero fee rate, not enough funds and an existing output file are all refused
  - `keygen` keys work end to end

```bash
BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe" cargo test -- --include-ignored
```

```powershell
$env:BITCOIND_EXE="C:\Program Files\Bitcoin\daemon\bitcoind.exe"; cargo test -- --include-ignored
```

Last run, on Windows with Bitcoin Core v31.1.0: **73 passed, 0 failed**, plus the full-length test.
- CLI: 5 approve, 6 keys, 7 payments, 7 wallet
- Library: 28 unit, 5 allowed, 12 forbidden, 1 export, 2 harness
- The full-length 61,320-block test passed in about 11.5 minutes on its own, before the approval prompt was added. The prompt doesn't touch anything that test uses.

Without the full-length test, the suite runs in about 30 s.

**One harness fix along the way:** the throwaway node now mines in batches of 100 blocks instead of 500. On the first full-length run, other tests were building alongside it, and one 500-block batch took longer than the RPC client's 15-second timeout. Run alone, with 100-block batches, it passes.

## Not in this PR

- A demo on a long-running node. It needs blocks mined on that node and demo keys saved between runs, so it waits for your go-ahead.
- The check-in (next PR)
- The signet demo
- The Liana/Sparrow exit drill

🤖 Generated with [Claude Code](https://claude.com/claude-code)
