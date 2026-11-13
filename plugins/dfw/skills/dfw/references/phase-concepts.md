# Phase: concepts

Goal: the screens of the agreed scope, as framework-neutral HTML/CSS using the approved tokens and shared components, with variants where a real choice exists, at all breakpoints, ready for visual review.

Format: `references/design-format.md` (read it first). Skeleton: `${CLAUDE_SKILL_DIR}/../../templates/screen.html`.

Skills (budget): base `frontend-design:frontend-design`; lead depends on surface:
- product UI: lead = you with frontend-design; support `ui-ux-pro-max:ui-ux-pro-max` (data: charts, UX guidelines, icons) and `ux-research:ux-copy` (microcopy, empty/error states).
- marketing: lead `hallmark:hallmark` (custom mode, DESIGN.md passed), support `taste-reference:taste-reference` (copy/sections reference) and `ux-research:ux-copy`.
- motion-heavy pieces: add `emil-design-eng:animate` as one of the supports (only when motion is part of the scope).

## Steps

1. **Scope.** List the screens and states to design (from PRODUCT.md workflows). Confirm the list with the user if it is not obvious. Put it into `design/design.json` (screens, flows, states).
2. **Components first.** Extract repeated parts (navigation, table, form field, button set, empty state, toast) into `design/components/<name>.html` with all states visible on one page. Register them in the manifest `components` list. Shared CSS goes to `design/styles/*.css`.
3. **Screens.** For each screen create `design/screens/<screen-id>/<variant-id>.html`:
   - realistic content from PRODUCT.md (real-looking names, numbers, long values, empty and error data), not lorem ipsum;
   - every value from tokens; every meaningful element has a stable `data-dfw-id`;
   - responsive at the manifest breakpoints (check desktop, tablet, mobile);
   - states via `html[data-state="..."]` selectors when the screen has empty/loading/error variants (declare them in manifest `states`).
4. **Variants.** Only where a real decision exists, give 2-3 variants using the Pen-style modes: `layout` (same style, different structure), `style` (same structure, different visual treatment), `creative` (broader rethink). Label them and mark one `recommended: true` with a one-line reason in `description`.
5. **Check.** `dfw check` (dfw lint + Impeccable detect + sloplint). Fix MUST findings; fix SHOULD findings unless a recorded decision says otherwise. The PostToolUse hook already reports per-file findings while you edit.
6. **Self-review visually.** `dfw studio start`, then `dfw shoot --screen <id>` (if Playwright is installed) and look at the screenshots yourself before showing the user. Fix obvious issues (overflow, clipping, broken mobile layout, contrast).
7. **Present.** Give the Studio URL, list screens and variants with one line each, and explain how to review: press C, click an element, write the change; "To verify" tab to confirm fixes. Then continue with `references/phase-review.md`.

Never write implementation code in this phase.
