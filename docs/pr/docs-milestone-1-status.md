## Milestone 1 status, open decisions, and the steps that need you

A new page, [`docs/milestone-1-status.md`](../milestone-1-status.md). Docs only; no code changes.

- **Every Milestone 1 item, done or left:**
  - the four build items, the tests and the "Done when" boxes, each with the PR that did it;
  - what was built along the way: the security baseline, the phone key backup, the restore tool's steps 1–5, the RustCrypto move and the exit guide.
- **The open decisions:**
  - the outside Bitcoin reviewer, and every placeholder they'd sign off;
  - the cryptographer for the backup format;
  - Trezor's emulator (D6);
  - the restore tool's releases;
  - the everyday pocket's descriptor;
  - the points that come with the app;
  - one GitHub setting: `Fuzz second_phone (60 s)` isn't a required check yet, and the other fuzz targets, the drill and CodeQL are.
- **Two numbered step lists for Windows PowerShell:**
  - **A, the signet run:** the signet demo again, with the renamed `satkeep` tool. The earlier run was still `twofold`.
  - **B, the Sparrow import by hand:** import the plain Savings wallet, check the addresses, add PHONE and DEVICE from their 12 words, make one 2-of-3 payment back to Savings in Sparrow, and check Satkeep sees it.

**How the Sparrow steps were checked:** the menu, button and field names come from Sparrow 2.5.5's own screen files and code:
- **File › Import Wallet... › Output Descriptor**;
- **Import...** on a watch-only keystore, then **Software Wallet** and **Mnemonic Words (BIP39)**, **Show Derivation...** and **Import Keystore**;
- the Server tab's **Bitcoin Core**, **Data Folder** and **Test Connection**;
- **Create Transaction**, **Finalize Transaction for Signing**, **Sign** and **Broadcast Transaction**.

They haven't been tried on the screen; that's what step list B is for.

**About this PC's setup:**
- Bitcoin Core v31.1 keeps its data in `%LOCALAPPDATA%\Bitcoin`, with no `bitcoin.conf`. So step B4 sets Sparrow's data folder by hand.
- There's no `.satkeep` folder yet, so `demo.ps1 setup` will start fresh. The earlier run's files stay in `.twofold`.

**The full-length test** (61,320 blocks) was run for this page on this PC, and it passed in about 12 minutes. CI doesn't run it.

### Security review

**None.** It's a docs-only change. The page holds no keys and no private data. Its steps:
- never put a recovery phrase on the command line;
- use test keys made fresh for this run;
- send me only transaction IDs, which are public.

Typing test words into Sparrow on your own PC is the step's purpose, and the page says never to do it with a real phrase.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
