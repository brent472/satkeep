Use Liana's key order for the unspendable Taproot internal key, so Liana can import a Savings wallet. Found while preparing the exit drill (Milestone 1 step 4). This PR is only the fix; the drill comes in its own PR.

## ⚠️ This changes every Savings address

The internal key is part of every Taproot output, so changing how it's built changes every address a Savings wallet hands out. **Wallets made by older versions must be recreated.** That includes `~/.twofold` test wallets and the regtest demo wallet. Their coins are still spendable with their old descriptors, and everything so far is on test networks only. The savings README says this.

## What Liana did with the old key

Liana v15.0's `lianad` (built from the signed `v15.0` tag, checked with `git verify-tag`), given a `twofold export` from `main`:

```text
Error parsing config: … Error parsing 'tr(tpubD6NzVbkrYhZ4XJyx…/<0;1>/*,{multi_a(2,…),{and_v(v:pk(…),older(10)),and_v(v:pk(…),older(20))}})#u8d6x4yk':
Descriptor is not compatible with a Liana spending policy. for key `main_descriptor`
```

**Cause.**
- Liana treats a Taproot internal key as unspendable only if it is the NUMS point H with chain code SHA256 of the tree's keys **in tree order**. That's `unspendable_internal_xpub` in `liana/src/descriptors/analysis.rs`.
- We hashed them in **role order** (PHONE, DEVICE, DEVICE_R, HELPER, HEIR) instead of tree order (PHONE, DEVICE, HELPER, DEVICE_R, HEIR).
- So Liana read our internal key as a real key that could spend at any time, which doesn't fit its "primary path + delayed recovery paths" shape.

**Proof of the cause.** With only the internal key rebuilt in tree order and nothing else changed, `lianad` accepted the same descriptor and started normally.

## The fix

- `twofold_savings::unspendable_internal_key(&TapTree)` (now public) computes the key from the tree, the way Liana does:
  - every key in leaf order, a repeated key counted twice
  - the network taken from the first key
  - used as `xpub/<0;1>/*` with no origin
- `SavingsPolicy::new` builds the tree first, then swaps in that key. The descriptor shape, the policy and every spending rule are unchanged.

## Tests

The key is pinned to values from **Liana's own code**, not just our reimplementation:

- **`internal_key_matches_lianas_published_test_vectors`.** Two Taproot descriptors from Liana v15.0's own tests in `liana/src/descriptors/mod.rs` (`descriptor_unspendable_internal_key`, and the test that checks `process_unspendable_key`). Liana's tests check that each internal key is its deterministic unspendable key; our function, given each tree, must produce exactly that key. Both vectors use Liana's one-signer-two-paths convention (`<0;1>` / `<2;3>`), so they also cover a repeated key counting twice.
- **`internal_key_matches_lianas_own_code_for_the_savings_tree`.** Our policy shape, built from five public test keys taken from Liana's tests. The expected key was produced by running Liana v15.0's public `liana::descriptors::unspendable_internal_key` on our tree, and Liana's `LianaDescriptor::from_str` accepted the full descriptor. The program is below.
- **`internal_key_follows_tree_order_not_role_order`** guards against going back to the old order.
- **Mutation check:** with a repeated key counted once, the published-vector test fails.
- **Live check:** a fresh `twofold export` from this branch is accepted by `lianad` v15.0 on a throwaway regtest node.

Last run on Windows with Bitcoin Core v31.1.0: **101 passed, 0 failed.** One ignored: the full-length 61,320-block test, which doesn't depend on the internal key and wasn't rerun.

<details>
<summary>The Liana program that produced the Savings-tree vector</summary>

Saved as `liana/examples/twofold_vector.rs` in a scratch clone of Liana at the verified `v15.0` tag, and run with `cargo run --release --locked -p liana --example twofold_vector`:

```rust
use std::str::FromStr;
use liana::descriptors::{unspendable_internal_key, LianaDescriptor};
use miniscript::descriptor::{Descriptor, DescriptorPublicKey};

fn main() {
    let k = |s: &str| format!("{s}/<0;1>/*");
    let phone = k("[37435aca/48'/1'/0'/2']tpubDEcZc7Um5KPZLF11qLYNgcteH1PH7CSKtTFTuTYuL6jqsP73gBHvLY42FuyvkTWN8C4q1YiDAUAeEuA78CDuuD2x17UKJymkBacCJz8P6NU");
    let device = k("[05813578/48'/1'/0'/2']tpubDEymPgUFZDLzEePYsai2ZXc9ntsnZBohJvGuHgpZsY5bsUDJMD6de6tevVd1z1EdGPYLdNaPvD4Ck7NgcteYDUPWwExvscfaSUu19k48Mvp");
    let device_r = k("[172ba1bc/48'/1'/0'/2']tpubDEgTZEAraUrKmnbyKJuXYGFPzNCm82bjMqd2GRy2HKviJ1moLtEZrHoUeG2o6uyWLEGx4yBWpctAmxcBx1b5nrrrBo5LjskRxRMDmwkuKxq");
    let helper = k("[da2ee873/48'/1'/0'/2']tpubDEbXY6RbN9mxAvQW797WxReGGkrdyRfdYcehVVaQQcQ3kyfhxSMcnU9qGpUVRHXXALvBtc99jcuxx5tkzcLaJbAukSNpP9h2ti4XFRosv1g");
    let heir = k("[3b1913e1/48'/1'/0'/2']tpubDFeZ2ezf4VUuTnjdhxJ1DKhLa2t6vzXZNz8NnEgeT2PN4pPqTCTeWUcaxKHPJcf1C8WzkLA71zSjDwuo4zqu4kkiL91ZUmJydC8f1gx89wM");
    let draft = format!(
        "tr(0250929b74c1a04954b78b4b6035e97a5e078a5a0f28ec96d547bfee9ace803ac0,{{multi_a(2,{phone},{device},{helper}),{{and_v(v:pk({device_r}),older(39420)),and_v(v:pk({heir}),older(61320))}}}})"
    );
    let Descriptor::Tr(tr) = Descriptor::<DescriptorPublicKey>::from_str(&draft).unwrap() else { panic!() };
    let internal = unspendable_internal_key(&tr).expect("Liana computes a key");
    println!("{internal}");
    let full = Descriptor::new_tr(internal, tr.tap_tree().clone()).unwrap();
    println!("{}", LianaDescriptor::from_str(&full.to_string()).expect("Liana accepts the descriptor"));
}
```

Output (first line):
`tpubD6NzVbkrYhZ4WVca1q6LYRsqpKEgWEAQP1DEDYnmFhSGjNwTtDQSEvPXVrtjx8MGesG2LWpkU9vWeiwrsgviRw3fk2nFRYLb42QfRE6oKMW/<0;1>/*`
</details>

## Also learned (recorded in the savings README)

Liana counts a key as the xpub *plus* its derivation paths. So it accepts our separate-account DEVICE / DEVICE_R keys, and it also accepts its own convention of one xpub with `<0;1>` and `<2;3>`. Exporting to Liana needs no change to `SavingsKeys`. It would matter only for importing a wallet made in Liana.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
