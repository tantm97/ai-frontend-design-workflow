# Rulings for known conflicts between external skills (level 4)

Each ruling names the conflicting instructions and decides. Re-check this file after every external skill update (`dfw-marketplace` validation lints for new stack-imposing text).

## R1. Framework and styling defaults
- Conflict: some upstream skills assume React/Tailwind/shadcn (UI UX Pro Max `ui-styling`, Anthropic `web-artifacts-builder`, Emil `prototype`). Those are not installed by dfw-marketplace. Remaining hints in installed skills (e.g. Tailwind mappings in Hallmark, stack searches in UI UX Pro Max) are inputs, not decisions.
- Ruling: design phases produce plain HTML/CSS + tokens only. Stack-specific guidance is used only after G3, and only for the approved stack.

## R2. Who owns PRODUCT.md and DESIGN.md
- Conflict: Impeccable `init`/`document` writes PRODUCT.md/DESIGN.md in its format; Hallmark reads `design.md`/`DESIGN.md` as overriding everything; UI UX Pro Max `--persist` writes `design-system/<slug>/MASTER.md`.
- Ruling: dfw owns PRODUCT.md and DESIGN.md at the project root (templates from dfw). Do not run Impeccable `init`/`document`/`extract` to (re)write them; read Impeccable's output proposals and merge by hand. Never use UI UX Pro Max `--persist`; translate its recommendations into DESIGN.md + tokens. Values live in `design/tokens/tokens.json` (DTCG); DESIGN.md references token names.

## R3. Direction is chosen by the user, not dealt
- Conflict: Hallmark deals themes from a catalog (`theme.mjs deal`), Impeccable fetches catalog directions (`/impeccable shape` / concept seed), UI UX Pro Max matches styles by keyword, frontend-design reasons from the brief.
- Ruling: the design-director brief and frontend-design reasoning produce the 2-3 directions. Catalog tools are *inputs* (inspiration, data), never the decision. For marketing pages led by Hallmark, run it in custom mode with DESIGN.md/brand passed in, or let the user pick from its dealt options explicitly.

## R4. Eyebrows, labels and typographic tics
- Conflict: Impeccable and Hallmark ban eyebrow/kicker labels absolutely; frontend-design discourages them but lets the brief win.
- Ruling: default to no eyebrows / tracked-caps labels. A user request (level 1) or an approved design system (level 3) can bring them back; record it.

## R5. Motion amount
- Conflict: Taste references require a hero entrance, card hover lifts and 1-2 "moments"; frontend-design calls per-section entrances and hover on every card AI tells; Hallmark allows "one reveal at most"; Emil: purposeful, short, interruptible.
- Ruling: product UI follows Emil (purpose first, 150-300 ms, ease-out, interruptible, reduced-motion respected). Marketing pages: at most one orchestrated moment, decided at direction time; Taste's mandatory motion rules are ignored. Motion review: `emil-design-eng:review-animations`.

## R6. Fonts
- Conflict: Impeccable flags overused fonts (e.g. Inter) and Hallmark has font rules; UI UX Pro Max recommends pairings from data; the user's brand may mandate a font.
- Ruling: brand fonts (level 1) always win. Otherwise choose with frontend-design reasoning, using UI UX Pro Max data as options. Inter-like neutral grotesques are acceptable for dense product UI when chosen on purpose and recorded; avoid them as an unexamined default.

## R7. Finish reviewers
- Conflict: Impeccable (detect/critique/audit), Hallmark (`sloplint`, finish review), Taste (gate), Emil (review-animations) each want to be the final gate.
- Ruling: dfw's checks are the gate: `dfw check` runs dfw lint + Impeccable detect + Hallmark sloplint (+ Taste copy-lint when enabled). MUST (blocks G2): dfw lint errors (integrity, accessibility), Impeccable errors, and sloplint FAILs on marketing surfaces only; on product UI sloplint FAILs count as SHOULD. Taste copy-lint never blocks. Human-level review: Impeccable `critique`/`audit` always; `review-animations` when motion changed; Hallmark finish review only on marketing pages it led. Taste's gate and any upstream hooks are never run.

## R8. Copy
- Conflict: Hallmark's word budgets (~80 words/screen), Taste copy rules, ux-copy guidance, the user's own copy.
- Ruling: user-supplied copy is verbatim (level 1). Product UI microcopy follows `ux-research:ux-copy`. Marketing copy may use Taste copy references and Hallmark budgets as guidance; `copy: verbatim` waivers when the user's copy is longer.

## R9. Images and paid services
- Ruling: no paid APIs by default. Hallmark `imagegen` (Together AI key) and Impeccable `generate` (OpenAI key) only on explicit user request with their own key. Use user assets, real product screenshots, or clearly marked placeholders.

## R10. Hooks and settings
- Ruling: upstream hooks are stripped at build time (Impeccable's three hooks, Taste's Stop hook). Never run `impeccable hooks on`, `impeccable pin`, Hallmark `install-hook.mjs`, or anything that edits `~/.claude/settings.json`; dfw provides its own check hook.

## R11. Catalog and dense data UI
- Conflict: UI UX Pro Max may recommend trend styles (glassmorphism, bento) that conflict with dense operational UI needs.
- Ruling: for data-heavy product UI, readability and density requirements (level 2) beat catalog style picks; use the catalog for palettes/type/UX guidelines, not for the overall look.
