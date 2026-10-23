# Twofold

**Simple to use, hard to lose, and ready to pass on — without a company as the gatekeeper.**

Twofold is a bitcoin wallet app that makes keeping your own bitcoin as easy as a banking app, without trusting a company to hold it for you.

You use your phone for everyday spending. For savings, moving money takes two of your three keys: your phone, a second device (like a tap-to-sign card), and a helper service. Losing any one key doesn't lose your money, and no single key — including ours — can move it alone.

You can replace the helper anytime, and if Twofold ever disappeared you could still get your bitcoin out with free, open-source tools. Down the road, Twofold also lets you plan who inherits your bitcoin, so your family isn't locked out if something happens to you.

![Twofold brand sheet](docs/brand/TWOFOLD_brand_sheet.png)

## Status

Pre-alpha. **Milestone 1** is in progress: proving the Savings wallet rules on test bitcoin (regtest and signet) before any app work. Nothing here touches real bitcoin.

## Where things are

| Path | What it is |
| --- | --- |
| [`docs/milestone-1.md`](docs/milestone-1.md) | Builder brief for the current milestone |
| [`docs/exit-drill.md`](docs/exit-drill.md) | Leaving without Twofold: spending Savings with Liana, or Bitcoin Core alone, and why not Sparrow |
| [`crates/twofold-savings`](crates/twofold-savings/README.md) | Savings wallet rules, their regtest tests, and every assumption behind them |
| [`crates/twofold-cli`](crates/twofold-cli/README.md) | `twofold`, a developer command-line tool for Savings on test networks |
| [`docs/design.md`](docs/design.md) | Design references: brand sheet, screens, type and color |
| [`CLAUDE.md`](CLAUDE.md) | Ground rules for anyone (or any AI assistant) working in this repo |
| [`SECURITY.md`](SECURITY.md) | How to report a vulnerability (privately, through GitHub) |

## License

MIT. See [`LICENSE`](LICENSE).
