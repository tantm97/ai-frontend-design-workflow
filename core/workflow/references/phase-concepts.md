# Phase: concepts (screens, after DS)

Goal: every screen in scope, organised in **pages per module**, composed only from the approved design system, at all breakpoints and states, with 2-3 variants where a real choice exists; reviewed visually; then G2.

Format: `references/design-format.md`. Skeleton: `${CLAUDE_SKILL_DIR}/../../templates/screen.html`.

Skills (budget): base `frontend-design:frontend-design`; product UI support `ui-ux-pro-max:ui-ux-pro-max` (UX rules) and `ux-research:ux-copy` (microcopy, empty/error states). Marketing: lead `hallmark:hallmark`, support `taste-reference:taste-reference` + `ux-research:ux-copy`. Motion-heavy: `emil-design-eng:animate`.

## Steps

1. **Pages and scope.** Group screens by module into `pages` in `design/design.json` (e.g. Counter: borrow, return; Equipment: list, detail, label; Reports: overview, history). Declare each screen's `states`. Confirm the list with the user only if it is not obvious from PRODUCT.md.
2. **Missing components first.** If a screen needs something the system lacks, add a master (or a variant) in `design/system/components/` before using it. Never style it inside the screen.
3. **Build pages in parallel (speed).** When there are 2+ pages, spawn one `dfw:screen-builder` agent per page **in the same message** (background), each with: page id, its screens + states + breakpoints, the PRODUCT.md workflow excerpt, DESIGN.md principles, `dfw system list` output, content rules (realistic Vietnamese/English data, edge cases), and "write only design/screens/<these screens>/". Build the most important page yourself meanwhile. Merge their manifest entries yourself (agents do not edit design.json).
4. **Variants (2-3) where a real decision exists** — at least for the key screen of each page: `layout` (different structure, same system), or `creative` (broader rethink). Mark one `recommended: true` with a one-line reason.
5. **Checks:** the hook gives fast lint on each edit. Then `dfw check` once for all (Impeccable + sloplint on composed HTML). Fix MUST; fix SHOULD unless a recorded decision says otherwise.
6. **Self-critique, 2 rounds** (`references/quality-rubric.md`): `dfw shoot --page <id> --states` (one browser run, parallel), read every image, score, fix; repeat once. Prefer fixing in the design system (fixes every screen at once).
7. **Present.** Studio URL; list pages → screens → variants (one line each); how to review: canvas (space-drag, ctrl/cmd-scroll zoom), P (or double-click a frame title) for full-screen clickable preview, I to interact on the canvas, click to inspect, double-click text to edit, C to comment (Claude receives it immediately), "To verify" tab. Start `dfw comments wait` in the background, then continue with `references/phase-review.md`.

   Page layout follows the pencil.dev convention: a page per module with a title + description, one row per breakpoint, a Variants row, a States row, and annotation notes (`pages[].notes`, `sections[].notes`: spec notes such as sizes, rules, pending decisions) next to the rows.

Speed rules: compose from components (short files); never re-read files you just wrote; batch screenshots (one `dfw shoot` per page); do not run full `dfw check` after every small edit; parallelise independent pages.

Never write implementation code in this phase.
