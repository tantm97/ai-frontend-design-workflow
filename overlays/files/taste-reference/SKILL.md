---
name: taste-reference
description: "Reference material for company/marketing website copy and section structure (from Leonxlnx/taste-skill, reference mode). Use ONLY when the dfw workflow delegates marketing-page copy or structure research, or the user names Taste. Read the reference files listed in the skill; do not generate pages from it."
license: MIT (see LICENSE in this plugin)
---

# Taste references (reference mode)

This plugin vendors selected reference files from [Taste](https://github.com/Leonxlnx/taste-skill) (MIT) for use as **inspiration** inside the dfw workflow. The full Taste generator (its starter kit, dealing scripts, browser gate and Stop hook) is intentionally not included.

## How to use
- Read only the file(s) relevant to the question, from `${CLAUDE_SKILL_DIR}/references/`:
  - `copy.md`, `copy-voice.md`, `copy-registers.md`, `copy-moves.md`: writing website copy that sounds specific, not generated.
  - `copy-tells.md`: patterns that make copy read as model-written (the rules `copy-lint` counts).
  - `sections.md`, `sections-library.md`: section types for company/marketing sites and when each fits.
  - `layout-v4.md`, `cards.md`: layout and card composition notes.
- Copy check: `node "${CLAUDE_SKILL_DIR}/scripts/copy-lint.mjs" <page.html|draft.md> [--kind tech|software|gaming|product|place|service] [--json]` (Node 22+, no dependencies, no network). The dfw workflow can also run it via `dfw check` when `checks.tasteCopyLint` is enabled.

## Precedence inside dfw (important)
These references are level-5 recommendations. They never override the user's decisions, PRODUCT.md, the approved DESIGN.md/tokens, or dfw's conflict rulings. In particular, ignore Taste rules that mandate motion (hero entrances, card hover lifts, "moments"), recolor brand palettes (violet remapping), copy its kit (`system.css`, `starter.html`), or deal random directions (dfw policy R3, R5). References may mention scripts or files that are not shipped here; skip those parts.
