## A flaky test's private-key check, and the second phone's warning (brief v3.24)

### The flaky test

`policy::tests::export_is_public_checksummed_and_parses_back` (satkeep-savings) failed once on main after #26 merged, then passed when rerun.

- **The cause:** it checked the exported descriptor with `!exported.contains("prv")`. The test's keys are new every run, and a random `tpub` or the checksum can contain the letters "prv" by chance. It failed once in 300 local runs.
- **Not a leak:** `export_descriptor` writes a `Descriptor<DescriptorPublicKey>`, which can't hold a private key.
- **The fix:** the test now uses `satkeep_backup::setup::holds_private_key`, the piece-by-piece check the setup already uses, which looks at each key on its own. It also checks:
  - the separate receive and change descriptors;
  - the check still works: the same descriptor with one key turned into a `tprv` is caught.
- **Confirmed:** 0 failures in 5,000 local runs of the test binary. At the old rate, about 17 would have failed.
- **No change to non-test code.**

### The second phone's warning

The warning now matches Section 19 of brief v3.24, in `docs/backup-format.md` 3.4, the exit guide, the restore README and `restore-tool.md` threat 7.6 (which quotes it too):

> Only do this if you're restoring your own Satkeep on a computer in front of you right now. No one from Satkeep, or from any company, will ever ask you to read this answer to them, by phone, message or email.

**If an answer was seen,** the app gives the owner new keys and moves all their bitcoin to them, pocket and Savings. This replaces "the app should make a new backup" in spec 3.4, threat 7.6 and the exit guide. A new backup alone isn't enough, because once the backup has been opened the phone key itself is out. The spec's section 7 now cites brief v3.24 for the wording.

### Checks

- `satkeep-savings` tests (regtest included) and `satkeep-backup` tests pass. The backup-format vector tests read `docs/backup-format.md`, and they pass unchanged.
- Clippy (`-D warnings`) is clean on the workspace.

### Security review

**None.** The review checked:
- **The exports can't hold a private key:** they're `to_string()` of `Descriptor<DescriptorPublicKey>`, and no product code changed.
- **The new check misses nothing the old one caught:** in these descriptors every key ends up as its own piece, after `tr(`, `pk(`, a comma or an origin, so a `tprv` or `xprv` is always caught. It now covers the receive and change exports too, and the added assertion shows it isn't always false.
- **Nothing printed:** the assertion messages are fixed text, never the descriptor.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
