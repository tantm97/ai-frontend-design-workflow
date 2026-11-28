# Design format: screens

Part of the design workspace format (`references/design-format.md` has the layout and `design.json`; `references/design-format-system.md` the masters and tokens a screen composes).

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
- `<style data-dfw-layout>` (in the head) holds the user's Studio drag/resize edits: one `[data-dfw-id='<id>']:not(#dfw) { grid-column | width | min-height }` rule per element (the `:not(#dfw)` only raises specificity so the rule beats class-based screen rules), optionally inside `@media (max-width: Npx)`. Keep that format when you touch it (the Studio refuses to rewrite a block it cannot parse); fold it into the screen's own layout CSS only if the user asks.
- Plain HTML elements are fine for structure (main, section, grids, text, tables inside `data-table`).
- `data-dfw-id` on every meaningful element and instance (`id` attribute on `<dfw-use>`): lowercase, dot-separated, unique per file, stable.
- Semantic HTML, realistic content and edge cases (long Vietnamese names, zero/huge numbers, empty, errors, permissions).
- States: wrap alternative content in `.when-<state>` / `.unless-<state>` blocks (Studio State menu sets `html[data-state]`), or `html[data-state="empty"] ...` layout rules.
- No frameworks, no CDN scripts, no build step. Small inline JS only to demo interaction.
