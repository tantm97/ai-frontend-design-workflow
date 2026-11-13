# Design workspace format (framework-neutral)

Figma-like model: **one design system, many pages, frames on a canvas.** Tokens and component masters live in the design system; screens are composed from component instances; changing a master updates every screen.

```
PRODUCT.md                    product context (dfw template; read by Impeccable/Hallmark)
DESIGN.md                     design system document (dfw-owned)
design/
  design.json                 manifest: pages, screens, variants, states, themes, breakpoints
  tokens/tokens.json          W3C DTCG 2025.10 tokens (source of truth for values)
  tokens/themes/<name>.json   overrides -> [data-theme="<name>"] (dark mode, or opt-<preset> while comparing options)
  tokens/tokens.css           generated (never edit)
  system/head.html            font links etc. ({{base}} = path to design/)
  system/components/<name>.html   component masters (<dfw-component>)
  system/components.css       generated from the masters (never edit)
  system/icons/<name>.svg     icons in use (dfw icons add)
  styles/*.css                optional shared layout helpers (no visual styling)
  screens/<screen>/<variant>.html
  assets/                     user-supplied brand assets (unaltered) and placeholders
  approved/vN/                frozen snapshots (immutable) + approval.lock.json
  handoff/vN.md               design-to-implementation contract
```
The hook regenerates `tokens.css` and `components.css` after every edit; `dfw build` does it by hand.

## Pages (design.json)
```json
{
  "schema": 1,
  "product": "Stockroom",
  "surface": "product",
  "lang": "vi",
  "breakpoints": [{ "id": "desktop", "label": "Desktop", "width": 1440, "height": 900 }, { "id": "laptop", "width": 1366, "height": 768 }],
  "themes": ["dark"],
  "states": ["empty", "loading", "error"],
  "pages": [
    { "id": "counter", "title": "Counter", "description": "Scan-first borrowing and returns at the issue counter.", "screens": ["borrow", "return"],
      "notes": [{ "title": "Frame", "items": ["Mode rail 88, step rail 56, feedback band 96 (80 at 1366)"] }, { "title": "Pending", "items": ["Due-time rule per category"] }] },
    { "id": "inventory", "title": "Inventory", "screens": ["inventory-list", "item-detail"] }
  ],
  "screens": [
    {
      "id": "inventory-list", "title": "Inventory list", "route": "/inventory",
      "description": "Find items, see stock health, start receiving.",
      "states": ["empty", "loading"],
      "variants": [
        { "id": "a", "label": "A · Dense table", "file": "screens/inventory-list/a.html", "mode": "layout" },
        { "id": "b", "label": "B · Table + health rail", "file": "screens/inventory-list/b.html", "mode": "layout", "recommended": true, "description": "Why B" }
      ]
    }
  ]
}
```
- The Studio always shows a generated **Design System** page first (foundations + every component and variant), then one page per entry in `pages` (a module or feature area), each a canvas of frames: screen × variant × breakpoint (× state / theme when toggled).
- Each page is laid out like a pencil.dev section: title + description, one row per breakpoint, a Variants row, a States row, and annotation panels (`notes`) beside the rows. Override the rows with `sections: [{ title, kind: breakpoint|variants|states|custom, breakpoint, screens, states, notes }]`. "All pages" stacks every page on one canvas.
- Screens whose id starts with `_` (direction explorations) appear on an "Explorations" page.
- Prototype links: normal `<a href="../other/default.html">` or `data-dfw-goto="<screen-id>[/<variant>]"` on any element; they work in Present mode (P) and with the Interact tool (I).

## Component masters
`design/system/components/button.html`:
```html
<dfw-component name="button" group="Actions" variants="primary secondary ghost danger" sizes="md sm lg"
  props="icon kbd" replaces="button" description="One primary per view; ...">
<style>
.btn { min-height: var(--size-control); padding: 0 var(--space-4); border-radius: var(--radius-md); ... }
.btn--primary { background: var(--color-accent); color: var(--color-on-accent); }
</style>
<template>
<button class="btn btn--{{variant}} btn--{{size}}" type="button"><dfw-icon data-dfw-if="icon" name="{{icon}}"></dfw-icon><span><dfw-slot>Button</dfw-slot></span><kbd data-dfw-if="kbd">{{kbd}}</kbd></button>
</template>
<dfw-examples>
<dfw-use c="button" variant="primary">Save changes</dfw-use>
<dfw-use c="button" variant="primary" disabled>Disabled</dfw-use>
</dfw-examples>
</dfw-component>
```
- `props="a b=default"` (or comma-separated when a default has spaces: `props="current, brand=Asset Tracking"`); `{{prop}}` (escaped), `{{variant}}`, `{{size}}`, `{{prop==value?text}}` (e.g. `aria-current` in the shared `app-shell`); `data-dfw-if="prop"` / `"!prop"` keeps an element only when the prop is set / unset.
- `<dfw-slot>` = instance content; `<dfw-slot name="actions">` is filled by `<dfw-fill slot="actions">` in the instance.
- `replaces="button input"`: the lint warns when a screen uses those raw elements instead of the component.
- Styles: tokens only (hard-coded colors are MUST errors in masters; raw lengths are SHOULD warnings). Cover every state in CSS: hover, focus-visible, active, disabled, loading (`aria-busy`), invalid/selected where relevant.
- `<dfw-examples>` must show every variant, size and state; the Design System page renders them.

## Screens
```html
<!doctype html>
<html lang="vi">
<head>
  <meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Inventory - Stockroom</title>
  <meta name="description" content="...">
  <dfw-head></dfw-head>
  <style>/* layout only: grid, flex, gap, padding, width, position */</style>
</head>
<body>
  <dfw-use c="app-shell" current="items" id="shell">
    <dfw-fill slot="header">
      <dfw-use c="page-header" title="Inventory" id="inventory.header">
        <dfw-fill slot="actions"><dfw-use c="button" variant="primary" icon="plus" id="inventory.add">Add item</dfw-use></dfw-fill>
      </dfw-use>
    </dfw-fill>
    <div data-dfw-id="inventory"> ... </div>
  </dfw-use>
</body>
</html>
```
- Every screen is framed by the shared `app-shell` master (one navigation and layout for the whole product; a counter/kiosk mode is a variant of it, not a different frame).
- `<dfw-head>` expands to the font links, tokens.css, base.css and components.css.
- `<dfw-use c="..." variant size id ...>`: `id` becomes the instance's `data-dfw-id`; `class` is appended; other non-prop attributes (aria-*, disabled, href, role, checked) are copied to the root element.
- `<dfw-icon name="search" size="16" label="...">` (decorative unless `label`). Find icons with `dfw icons search <words>`, add with `dfw icons add <names>`.
- **Screens only do layout.** Setting color, background, border, font, shadow or radius in a screen is a MUST error (`screen-visual-style`). Need a new look? Add a variant to the master (design system first), or a token.
- Plain HTML elements are fine for structure (main, section, grids, text, tables inside `data-table`).
- `data-dfw-id` on every meaningful element and instance (`id` attribute on `<dfw-use>`): lowercase, dot-separated, unique per file, stable.
- Semantic HTML, realistic content and edge cases (long Vietnamese names, zero/huge numbers, empty, errors, permissions).
- States: wrap alternative content in `.when-<state>` / `.unless-<state>` blocks (Studio State menu sets `html[data-state]`), or `html[data-state="empty"] ...` layout rules.
- No frameworks, no CDN scripts, no build step. Small inline JS only to demo interaction.

## Tokens (DTCG 2025.10)
- Colors: `{"$type":"color","$value":{"colorSpace":"srgb","components":[r,g,b],"hex":"#rrggbb"}}`.
- Dimensions: `{"$type":"dimension","$value":{"value":16,"unit":"px"}}`; durations `{value, unit:"ms"}`.
- Aliases: `"$value": "{color.blue.600}"`; group `$type` is inherited.
- The starter kit contract (names the kit components use): `color.{canvas,surface,surface-sunken,surface-hover,ink,muted,subtle,border,border-strong,rule,accent,accent-hover,accent-soft,accent-text,on-accent,on-fill,on-warn,focus,focus-ring}`, `color.status.{ok,info,warn,err}{,-tint,-text}`, `font.{family,size,weight,leading,tracking}.*`, `space.{half,1,1-5,2,3,4,5,6,8,10,12,16}`, `radius.{sm,md,lg,xl,pill}`, `border.{width,width-strong}`, `focus.{width,offset}`, `size.{control,control-sm,control-lg,target-min,icon*,sidebar,rail,rail-item,dialog*,band,avatar-xl}`, `shadow.{sm,md,lg}`, `duration.{fast,base}`, `easing.out`. Keep these names when adapting a preset; add new ones freely.
