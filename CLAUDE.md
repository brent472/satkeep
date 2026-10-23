# Twofold — working rules

Twofold is a phone-first bitcoin self-custody wallet. Spending is a single phone-key wallet. Savings needs any 2 of 3 keys (phone, a second device, a replaceable helper), plus delayed recovery and inheritance paths. Full spec: `docs/master-brief.md` in the private repo `brent472/twofold-private` (clone it beside this repo, as `../twofold-private`, to read it). Current task: `docs/milestone-1.md`.

## Non-negotiables

- **Test networks only.** Use regtest and signet. Never mainnet, never real bitcoin, not even "just to check".
- **No real keys.** Generate fresh keys inside each test run. Never commit keys, seeds, xprvs, WIFs, wallet databases or `.env` files. If one ever lands in a commit, stop and tell the human.
- **No key telemetry.** Nothing that could reveal key material is logged, printed in CI, or sent anywhere.
- **Write assumptions down.** Every assumption about policy, timelocks, fees or signing goes in the README of the crate or in `docs/`.
- **The helper can never spend alone.** Any design or code where one key — including the helper — can move Savings without a delay is a bug.
- **Timelocks are placeholders** until an outside Bitcoin reviewer signs off (9 months ≈ 39,420 blocks; 14 months ≈ 61,320 blocks; relative-timelock cap is 65,535 blocks). Keep them configurable.
- **Check-in rule:** the refresh transaction must run sooner than the shortest delayed path (plan: every 6 months).

## How we work

- Small branches, one change each, merged by pull request. Don't push straight to `main`.
- Tests come with the code: every spending path needs a test that it works and a test that the forbidden version fails.
- Prefer established libraries (BDK, rust-miniscript) over hand-rolled crypto or script.

## Copy and design

- Product copy comes from Section 19 of the master brief (in the private repo `brent472/twofold-private`); don't invent UI wording.
- Typography is Inter throughout; the brand sheet in `docs/brand/` is the visual master.
- Plain words in the app: key, approve, network fee, Spending, Savings. No "multisig", "UTXO", "PSBT", "xpub" or "seed" in main flows.
