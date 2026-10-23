The plan for the open-source restore tool: `docs/restore-tool.md`. **Doc only, no code.** Test networks only.

## What the plan says

**Inputs and outputs.** The tool unlocks the phone key backup from the exit kit file and the second key alone: no Twofold app, account, server or helper. It exports what Liana and Bitcoin Core need for Savings, in both shapes, and for the everyday pocket.

**The main finding.** Once Twofold is gone, so is the helper. Getting Savings out then needs PHONE (from the tool) **and** the second key. So the second key matters twice: it unlocks the backup and it signs.

**Decisions:**
- **D1:** the tool is a crate in this repo, with signed, reproducible releases and a source mirror.
- **D2:** the exit kit is a PDF. The backup and wallet setup are embedded between clear markers and also printed as armoured text, so the tool needs no PDF library.
- **D3:** for Bitcoin Core, the tool imports straight into the owner's own node by default, with a key file on request. For Liana, it shows the 12 words only on request (`--show-phrase`). Nothing secret is printed by default.
- **D4, decided in brief v3.12:** an old phone still counts as a second key, with three requirements:
  - its app signs and unlocks fully offline and can export its key (12 words, or straight to Liana or Bitcoin Core);
  - the exit kit explains that step;
  - the app recommends, never requires, a hardware second key once Savings is large.

  For the tool, this means one more kind of device: a second phone, reached by an offline request and answer (QR code or text).
- **D5:** one shared backup-format crate, a written spec and fixed test vectors.
- **D6:** a simulated device first, then the Trezor emulator in CI once Trezor support is built (a download, so I'll ask first).
- **D7:** a versioned wallet-setup format, with room for the pocket's descriptor and for earlier Savings wallets that may still hold leftovers.

**Threats covered:**
- a fake restore tool;
- a changed kit (the tool trusts only the authenticated copy inside the locked backup);
- the restore computer;
- a device asked to unlock the wrong label;
- a stale kit.

**Order of work, one PR each:**
1. this plan;
2. the format crate and spec;
3. the tool, with the simulated device;
4. the exit drill through the tool;
5. the second-phone exchange;
6. Trezor support.

**Still open:**
- the everyday pocket's descriptor (brief 3.11);
- whether Liana v15 signs with a Trezor;
- which Trezor client to use.

## Security review

None. The diff is documentation only. The plan's own threat section is above.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
