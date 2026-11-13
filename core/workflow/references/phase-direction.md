# Phase: direction (gate G1)

Goal: 2-3 genuinely different visual directions grounded in a concept, **visible on a real screen**, quickly; the user picks one (or merges). Output: DESIGN.md "Concept/Direction", the chosen style guide applied as tokens; G1 approved.

Skills (budget: base + 1 lead + 2 support):
- Base: `frontend-design:frontend-design` (keep for all design phases).
- Lead: the `dfw:design-director` agent (research + direction briefs).
- Support: `ui-ux-pro-max:ui-ux-pro-max` (palettes, font pairings, product-type data; run its search with `python3`); marketing surfaces: `hallmark:hallmark` instead (custom mode) and optionally `taste-reference:taste-reference`.

## Steps (target: directions on screen within minutes)

1. Surface: `"surface": "product"|"marketing"` in `design/design.json`.
2. **Brief first** (`references/design-system-thinking.md` §1): fill `design/system/brief.json` (use context, brand, personality 1-5 on 7 axes) from PRODUCT.md; ask the user about personality axes that are unclear (one AskUserQuestion, max 4 questions).
3. **Concept.** Find the product's metaphor in its own world (domain objects, place, culture, materials): a shop's signboard, a tool crib's label tape, a ledger, a field notebook. Write it as one sentence. Human-looking design starts here, not from a palette.
4. **Pick 3 style guides with reasons:** `dfw style match` (ranks the 12 guides against the brief and lists mismatches), `dfw style show <id>` for the candidates (it prints each guide's rationale). Choose three that differ on at least two personality axes (one closest fit, one adjacent, one bolder) and write one line each on why. Spawn `dfw:design-director` in the background with PRODUCT.md, the concept and the 3 guide ids, to sharpen each into a direction (palette names in the product's language, signature motif, type pairing) — or to propose a custom guide when none fits.
5. While it runs: `dfw system init --style <first id>` (components + tokens), then build **one** key screen (the most important workflow, primary breakpoint) from components in `design/screens/_directions/key.html`, real content, using the type scale (`text` display variants, eyebrows) and the signature motif once.
6. `dfw system options <id1> <id2> <id3>` renders the same screen in the three styles (Themes toggle in the Studio). Only when directions differ in **structure**, add a second layout variant.
7. Self-check (`references/quality-rubric.md`, one round): `dfw shoot --screen _directions --themes`, look at every image, fix obvious issues.
8. Present each direction: name, concept sentence, why it was chosen (brief axes it matches, what it trades off), named palette with its rules, typography (license, Vietnamese support when there is Vietnamese text; English-only fonts are fine for English-only products — `--fonts latin`), signature motif, density, why it fits these users. Recommend one. Ask the user to choose / combine / redirect (AskUserQuestion).
9. On the choice: `dfw system choose <id>`; adapt it to the product (rename palette entries to the product's world, adjust hues, write usage rules in each token's `$description`); write DESIGN.md (Concept, Principles, Color with rules, Typography, Layout, Motifs, Motion); `dfw approve G1 --note "<direction + key choices>"`; `dfw phase system`.
   - No library guide fits? Write a project guide (same JSON format as `kits/styles/*.json`: id, name, concept, base, fonts, fontsLatin, type, palette {name: {hex, usage}}, roles, radius, shadowTint, density, motifs, layout, components, do, dont) to `design/system/style-source.json` and `dfw style apply design/system/style-source.json`.

Branding choices (logo usage, brand colors/fonts) always need explicit user approval.
