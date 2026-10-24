# Design references

- **Brand sheet (visual master):** [`brand/SATKEEP_brand_sheet.png`](brand/SATKEEP_brand_sheet.png) (brief v3.15). Where anything else disagrees with this image, the image wins, except on wording: Section 19 of the brief wins on copy.
- **Older images, history only:**
  - The Satkeep concept image the sheet was patched from stays in the private repo. Its screens are out of date, so don't design from it.
  - The pre-rename Twofold sheet was removed from this repo with the rename (PR #12), and is still in git history and in the private repo. Neither is a reference any more.
- **Editable screens:** the "Twofold App Screens" design canvas (made before the rename; the Milestone 2 prototype replaces it) on claude.ai (private to the owner; share it from the canvas's Share menu). It holds Welcome, Home, Protection, Family, Move money and Desktop.
- **Typography:** Inter throughout (SIL Open Font License). Sizes and weights: Section 18 of the master brief (`docs/master-brief.md` in the private repo `brent472/satkeep-private`).
- **Copy:** Section 19 of the master brief. Use it verbatim.

## Colors (sampled from the old Twofold sheet; the Satkeep sheet keeps the same palette within a few shades)

Where they differ from the brand sheet or the tokens in Section 02 of the brief, those win.

| Token | Hex | Use |
| --- | --- | --- |
| Background | `#0E1216` | App background |
| Surface | `#1D2126` | Dark cards and rows |
| Savings card | `#23282C` | Savings wallet card |
| Ivory | `#FDF7EF` | Light cards (Family, desktop panels) |
| Everyday card | `#FFF3DD → #EBCBA1` | Everyday payments card gradient |
| Gold | `#F2C58A` | Active tab, accents, icons |
| Gold button | `#E8C595 → #D2A570` | Primary buttons |
| Green | `#6FC08C` | Received, verified |
| Red | `#E07A5F` | Sent |
| Amber | `#E8B46A` | Not active yet, needs attention |
| Muted text | `#8E9498` | Secondary text |

<!-- Throwaway change to test the exit drill skip (PR #17); never merged. -->
