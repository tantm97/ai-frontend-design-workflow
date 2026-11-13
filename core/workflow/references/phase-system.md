# Phase: system (gate DS)

Goal: a complete, reviewed design system **before any screen**: tokens (color, type, spacing, shape, elevation, motion) and component masters with every variant and state, shown on the Studio's Design System page. Screens later only compose these components.

Skills (budget): base `frontend-design:frontend-design`; support `ui-ux-pro-max:ui-ux-pro-max` (palette/type data, UX rules) and `impeccable:impeccable` (`critique` on the system page). Marketing surface: Hallmark leads instead (custom mode with DESIGN.md).

Method: `references/design-system-thinking.md` (read it before changing any colour or size). Every decision gets a line in DESIGN.md `## Rationale`.

## Steps (fast path first: start from the kit, then adapt)

1. **Start from the kit + the chosen style.** `dfw system init --style <id>` (already done at G1 in most cases). This copies 21 token-driven components (button, icon-button, field, select, filter-chip, checkbox, segmented, badge, avatar, card, page-header, nav, nav-item, tabs, data-table, empty-state, banner, stat, dialog, kbd, text), the style's tokens, fonts and motif master, and the icons they use. (`--preset crisp|soft|industrial` gives a neutral base without a style.) Use `--only a,b,c` to take a subset.
2. **Make the tokens the product's own** (this is what makes it look designed by a person):
   - Palette entries live in `color.palette.*` with names from the product's world (e.g. `lam`, `giay`, `kim`, `thep`, `vang-bang-keo`) and a `$description` that states the **usage rule** ("only for large surfaces with text ≥ 24px", "ornament only, never text"). Roles (`color.accent`, `color.ink`, ...) are aliases to palette entries.
   - Neutrals are tinted toward the concept (paper, steel, linen), shadows are tinted with a brand colour, never pure black/grey defaults.
   - Typography has real contrast: display sizes (`font.size.display-*`) at least ~2.5× body for marketing and ~1.8× for product UI; an eyebrow style; the display family used for titles and big numbers.
   - Keep the token names of the kit contract (`references/design-format.md`); add new tokens as needed.
   - Fonts: pick from `references/type-pairings.md`. When any UI text is Vietnamese (lang vi or bilingual), every family rendering it must ship the `vietnamese` subset (`dfw fonts check "<Family>"`); English-only products may use Latin-only families (`dfw style apply <id> --fonts latin`). Update `design/system/head.html`.
   - Contrast: text 4.5:1, large text and UI boundaries 3:1. Write the ratios into DESIGN.md "Color".
3. **Signature motifs** (1-2, from the style or the concept): masters in `design/system/components/motif.html` with a usage rule each ("once per view", "never behind text"). Brand assets (logos, ornaments, photos) go in `design/assets/` and are used as supplied, never redrawn.
4. **Component inventory from the product.** List the components the screens in scope need (PRODUCT.md workflows). Reuse kit components; add product-specific ones (e.g. `scan-slip`, `item-line`, `step-rail`, `feedback-band`) as new masters in `design/system/components/`. Every master: tokens only, all states in CSS, every variant/size/state in `<dfw-examples>` with realistic content. Icons: `dfw icons search <words>` then `dfw icons add <names>`; never hand-draw icons.
5. **Senior review + self-critique, 2 rounds:** `dfw system audit` (contrast, accent vs status hues, neutral temperature, hue budget, type rhythm and hierarchy, usage rules, rationale) — fix every MUST, fix or justify every SHOULD; then `dfw shoot --system`, score against `references/quality-rubric.md`, fix, repeat once. When you change colours, regenerate ramps with `dfw palette build` instead of nudging hex values by eye; when you change sizes, use `dfw type scale --write`.
6. **Present.** Studio URL → Design System page (it shows the concept, do/don't, palette with usage rules and contrast, type scale, spacing, motifs, assets, and a spec line per component). In chat: what is new vs the kit and 2-3 decisions you want the user to look at.
7. On approval: update DESIGN.md (Components section lists every master; `## Rationale` ≥ 5 product-specific lines — the DS gate refuses without it), `dfw approve DS --note "<what was approved>"`, `dfw phase concepts`.

Changing the system after DS: allowed during concepts (it propagates to all screens); tell the user what changed. After G2 the system is frozen with the design.
