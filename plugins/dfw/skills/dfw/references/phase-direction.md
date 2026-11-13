# Phase: direction (gate G1)

Goal: 2-3 genuinely different, well-argued visual directions; the user picks one (or merges). Output: DESIGN.md "Direction" + first real tokens; G1 approved.

Skills (budget: base + 1 lead + 2 support):
- Base: `frontend-design:frontend-design` (load and keep for all design phases).
- Lead: the `dfw:design-director` agent (research + direction briefs).
- Support: `ui-ux-pro-max:ui-ux-pro-max` (palettes, font pairings, product-type data; run its search with `python3`); for marketing/landing surfaces, `hallmark:hallmark` instead of UI UX Pro Max (custom mode, DESIGN.md + brand passed in) and optionally `taste-reference:taste-reference` for copy/section structure.

## Steps

1. Decide the surface: product UI (apps, dashboards, tools) or marketing (websites, landing pages). Set `"surface": "product"|"marketing"` in `design/design.json`.
2. Spawn `dfw:design-director` with: the PRODUCT.md summary, roles and workflows, brand inputs (paths), constraints (stack if fixed, accessibility floor), what the user likes/dislikes, and the surface. Ask for 2-3 direction briefs.
3. Review the briefs against PRODUCT.md and the precedence rules. Reject directions that rely on generic AI looks (see frontend-design's list) unless the brief asked for them.
4. **Make the directions visible.** For each direction, build one representative screen (the most important workflow screen, desktop) as a quick concept in `design/screens/_directions/<direction-id>.html` with its own token block, register them in `design/design.json` as one screen `_directions` with variants `a`, `b`, `c` (`mode: "style"`), start the Studio (`dfw studio start`) and give the user the URL.
5. Present each direction in chat: name, one-sentence concept, typography (with license), color roles with contrast notes, density and layout model, navigation model, motion stance, why it fits these users. Mark one as recommended with the reason. Ask the user to choose, combine, or redirect (AskUserQuestion with the directions as options; free text for combinations).
6. On the user's choice:
   - Write DESIGN.md: Direction, Principles, Color, Typography, Layout and density, Motion stance.
   - Write `design/tokens/tokens.json` (DTCG 2025.10: colors as `{colorSpace, components, hex}`, dimensions as `{value, unit}`), then `dfw tokens build`.
   - `dfw approve G1 --note "<direction name + key choices>"`.
   - Remove or keep `_directions` (keep for history; mark it `"archived": true` in the manifest).
7. `dfw phase concepts`.

Branding choices (logo usage, brand colors/fonts changes) always need explicit user approval.
