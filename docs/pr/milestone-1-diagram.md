A diagram section in the README: who can move the bitcoin, in plain words. It ticks the Milestone 1 box "README has a diagram of the spending paths". Docs only.

## What's in it

Three Mermaid diagrams under **Who can move the bitcoin**:

1. **Enforced by Bitcoin itself:**
   - **the everyday pocket:** PHONE alone;
   - **Savings with no beneficiary:** any 2 of PHONE, DEVICE, HELPER;
   - **the move when a beneficiary is added:** one 2-of-3 payment;
   - **Savings with a beneficiary:** any 2 of 3 at any time, or HEIR alone after the heir delay, the backstop nobody can block.

   One line under it says what DEVICE, HELPER and HEIR are.
2. **Enforced by the helper, not by Bitcoin:** the helper-assisted claim from brief 3.12. The beneficiary starts a claim, and the helper waits (placeholder 3 months) and alerts the owner. If the owner stops it, they get a new phone key and Savings moves to it. If nobody stops it, the helper co-signs with the restored phone key.
3. **Payments count as check-ins:** the payment rule (#9). A payment or top-up also spends coins older than about 10 months, and the change comes back with a new heir delay. With nothing paid for a year, the yearly check-in does the same.

**Marked as not built yet:** the pocket (brief 3.11) and the helper-assisted claim (brief 3.12). Timings are marked as placeholders.

The technical diagram, with descriptors and leaves, stays in the library README, and the new section links to it.

**Checked on GitHub:** all three diagrams render on the branch. My first version, with nested groups, was cropped by GitHub's diagram viewer, so the second commit switches to simple top-to-bottom flows.

## Security review

None. The diff is documentation only: `README.md`, plus one checklist line in `docs/milestone-1.md`.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
