Adds the Satkeep brand sheet (brief v3.15) and makes it the visual master. Docs only.

## What changes

- **`docs/brand/SATKEEP_brand_sheet.png`:** copied from `brent472/satkeep-private`, byte for byte. Its git blob hash is `498d487…` in both repos. It's 1.7 MB, 1536 × 1024.
- **CLAUDE.md**, *Copy and design*: "the Satkeep brand sheet (`docs/brand/SATKEEP_brand_sheet.png`) is the visual master; Section 19 of the brief wins on wording".
- **README:** the sheet is back under the introduction, and the docs table says `docs/design.md` holds it.
- **`docs/design.md`:** the sheet is the visual master, with Section 19 winning on copy.
  - **Older images, history only:**
    - The concept image the sheet was patched from stays in the private repo, out of date.
    - The pre-rename Twofold sheet stays only in git history (PR #12 removed it) and in the private repo.
    - Neither is added back here.
  - **The colour table** is marked as sampled from the old sheet, which keeps the same palette within a few shades. The sheet and the brief's Section 02 tokens win.
  - **One renamed row:** "Spending card" is now "Everyday card", matching brief 3.11's one balance.

## Security review

None.
- **Metadata:** the PNG holds only image data (`IHDR`, `IDAT`, `IEND`), with no text, EXIF or other chunks.
- **Content:** what it shows is mock-ups, with fictional names and shortened mock addresses. There are no keys and nothing real.
- **No code** changes, and no dependencies.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
