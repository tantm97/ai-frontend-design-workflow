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
  system/head.html            font links etc. ({{base}} = path to design/; also works in masters, examples and screens: {{base}}assets/logo.png)
  system/brief.json           design brief: context, brand, personality axes (direction phase; dfw style match reads it)
  system/style.json           the style guide dfw style apply wrote (written by dfw)
  system/inventory.json       components the product needs: name, purpose, usedBy, states (masters are designed from it)
  system/components/<name>.html   component masters (<dfw-component>)
  system/components.css       generated from the masters (never edit)
  system/icons/<name>.svg     icons in use (dfw icons add)
  styles/base.css             base styles from tokens and the .when-<state> switches (dfw template)
  styles/*.css                optional shared layout helpers (no visual styling)
  screens/<screen>/<variant>.html
  assets/                     user-supplied brand assets (unaltered) and placeholders
  approved/vN/                frozen snapshots (immutable) + approval.lock.json
  handoff/vN.md               design-to-implementation contract
```
The hook regenerates `tokens.css` and `components.css` after every edit; `dfw build` does it by hand.

The format is split by job; load the part the phase needs:
- this file: the workspace layout and `design.json` (pages, screens, variants, prototype links);
- `references/design-format-system.md`: component masters and DTCG tokens with the token contract (design system phase, masters);
- `references/design-format-screens.md`: screen files, instances, layout-only styling, states (concepts, adding a screen).

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
- The Studio always shows a generated **Design System** page first (foundations + every component and variant), then one page per entry in `pages` (a module or feature area; `<studio url>?page=<page id>` opens one, `?page=system` the Design System page), each a canvas of frames: screen × variant × breakpoint (× state / theme when toggled).
- Each page is laid out like a pencil.dev section: title + description, one row per breakpoint, a Variants row, a States row, and annotation panels (`notes`) beside the rows. Override the rows with `sections: [{ title, kind: breakpoint|variants|states|custom, breakpoint, screens, states, notes }]`. In the Studio the user picks which breakpoints and states are shown (multi-select menus with All, default all); state frames, including `kind: states` sections, appear only for checked states. The Studio camera button screenshots every screen of every page, and Present has a Screenshot button for the screen being shown; both save to `.dfw/shots/studio/<time>/`. "All pages" stacks every page on one canvas.
- Screens whose id starts with `_` (direction explorations) appear on an "Explorations" page.
- Prototype links: normal `<a href="../other/default.html">` or `data-dfw-goto="<screen-id>[/<variant>]"` on any element; they work in Present mode (P) and with the Interact tool (I). The Flow page is computed from them (`flows` in design.json is ignored). `"entry": true` marks a screen users reach from outside the design; `dfw lint` warns on screens on no page, links to unknown screens and unreachable screens. Add a screen later with `dfw screen add` (`references/phase-concepts-add-screen.md`).
