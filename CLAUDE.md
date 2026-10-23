# Twofold — working rules

Twofold is a phone-first bitcoin self-custody wallet. The user sees one balance: a small everyday pocket on the phone key (up to a cap the owner sets) for Face ID payments, and Savings, which needs any 2 of 3 keys (phone, a second device, a replaceable helper), plus a delayed inheritance path once a beneficiary is added. Full spec: `docs/master-brief.md` in the private repo `brent472/twofold-private` (clone it beside this repo, as `../twofold-private`, to read it). Current task: `docs/milestone-1.md`.

## Non-negotiables

- **Test networks only.** Use regtest and signet. Never mainnet, never real bitcoin, not even "just to check".
- **No real keys.** Generate fresh keys inside each test run. Never commit keys, seeds, xprvs, WIFs, wallet databases or `.env` files. If one ever lands in a commit, stop and tell the human.
  - Published BIP test vectors (such as BIP39's "abandon … about") are allowed in tests, because they're public and never hold funds.
- **No key telemetry.** Nothing that could reveal key material is logged, printed in CI, or sent anywhere.
- **Write assumptions down.** Every assumption about policy, timelocks, fees or signing goes in the README of the crate or in `docs/`.
- **The helper can never spend alone.** Any design or code where one key — including the helper — can move Savings without a delay is a bug.
- **Timelocks are placeholders** until an outside Bitcoin reviewer signs off (heir: 14 months ≈ 61,320 blocks; relative-timelock cap is 65,535 blocks; the 9-month card-only path was removed in brief 3.8). Keep them configurable.
- **Check-in rule:** the refresh transaction must run sooner than the heir path (plan: once a year, one tap, never automatic).

## How we work

- Small branches, one change each, merged by pull request. Don't push straight to `main`.
- Tests come with the code: every spending path needs a test that it works and a test that the forbidden version fails.
- Prefer established libraries (BDK, rust-miniscript) over hand-rolled crypto or script.
- Before opening any PR, run a security review of the diff (`/security-review` if your Claude Code has it) and put the findings, or "none", in the PR description.
- A new dependency needs a reason in its PR; `cargo deny check` must pass (see the README).

## Copy and design

- Product copy comes from Section 19 of the master brief (in the private repo `brent472/twofold-private`); don't invent UI wording.
- Typography is Inter throughout; the brand sheet in `docs/brand/` is the visual master.
- Plain words in the app: key, approve, network fee, Savings, everyday payments. No "multisig", "UTXO", "PSBT", "xpub" or "seed" in main flows.
