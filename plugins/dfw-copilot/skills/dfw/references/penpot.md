# Penpot bridge (optional)

Penpot is an optional bridge for designers who want to work in Penpot. dfw's HTML screens and DTCG tokens stay the source of truth: nothing from Penpot overwrites them blindly, and everything runs locally.

## Tokens

```bash
dfw tokens export --format penpot [--out file] [--opt-themes] [--json]
dfw tokens import <file|folder> [--from penpot] [--dry-run] [--prune] [--json]
```

- **Export** writes `.dfw/penpot/tokens.penpot.json` (or `--out`). In Penpot: Tokens panel > Tools > Import, pick the file. Penpot's import replaces the file's token sets.
- **Import** reads a file exported from Penpot (Tokens panel > Tools > Export, single file or multiple files/folder), shows a diff summary per set (changed, added, removed, missing) and writes `design/tokens/tokens.json` + `design/tokens/themes/<name>.json`, then rebuilds `tokens.css`. `--dry-run` only shows the diff. While the design is frozen (G2 approved) a real import is refused: show the diff with `--dry-run`, and only after the user agrees run `dfw reopen --reason <why>` and import again.
- Tokens present in dfw but missing in Penpot are kept (listed as "missing"); `--prune` removes them. Tokens of types Penpot cannot hold are never touched.

### Sets and themes

| dfw | Penpot |
|---|---|
| `design/tokens/tokens.json` | set `base` |
| `design/tokens/themes/<name>.json` | set `themes/<name>` (a folder `themes` in the sets list) |
| base only | theme `default` in group `dfw` (active) |
| theme `<name>` | theme `<name>` in group `dfw`, sets `base` + `themes/<name>` |
| `opt-*` option themes (temporary preset comparisons) | left out unless `--opt-themes` |

On import, the set named `base` (or, without one, the first non-theme set in Penpot's set order; a single-set file is the base) feeds `tokens.json`; `themes/<name>` (or `theme/<name>`) feeds a theme. Other sets are listed as skipped: rename them in Penpot to import them.

### Types

| dfw (DTCG 2025.10) | Penpot type | Value |
|---|---|---|
| `color` sRGB | `color` | `#rrggbb`, or `rgba(r, g, b, a)` when alpha < 1 |
| `color` oklch / other spaces | `color` | converted to sRGB hex (lossy, reported); other spaces need a `hex` fallback |
| `dimension` named `radius.*` | `borderRadius` | `6px` |
| `dimension` named `space.*`, `gap`, `inset`, `padding` | `spacing` | `16px` |
| `dimension` named `size.*`, `width`, `height` | `sizing` | `40px` |
| `dimension` named `border.width`, `focus.width`, `stroke-width` | `borderWidth` | `1px` |
| `dimension` named `font.size.*` (px) | `fontSizes` | `16px` |
| `dimension` named `tracking`, `letter-spacing` (px) | `letterSpacing` | `0.5px` |
| other `dimension` (and rem/em values) | `dimension` | `1.25rem` (rem/em reported: Penpot works in px) |
| `fontFamily` | `fontFamilies` | `Inter, Segoe UI, sans-serif` |
| `fontWeight` | `fontWeights` | `600` (named weights such as `Bold` import as numbers) |
| `number` | `number` | `1.5` |
| `duration` | `number` | milliseconds (reported); imports back as `duration` for an existing duration token or a name containing `duration`/`delay` |
| `string` with a text-case / text-decoration value | `textCase` / `textDecoration` | `uppercase` |
| `shadow` (one or a list) | `shadow` | `{ color, offsetX, offsetY, blur, spread, inset }` (px numbers as strings) |
| `typography` | `typography` | `{ fontFamilies, fontSizes, fontWeights, letterSpacing, lineHeights, textCase, textDecoration }` |
| `cubicBezier`, `transition`, `border`, `strokeStyle`, `gradient`, other `string` | none | not exported; listed as "Not exported" |
| Penpot `opacity`, `rotation` | `number` | `45%` -> `0.45` |
| Penpot `boolean`, `other`, `string` | none | listed as "Not imported" |

- Aliases (`{color.blue.600}`) are kept as is in both directions; descriptions travel as `$description`.
- Penpot math (`{space.2} * 2`, `round(...)`) imports as `calc(...)` so `tokens.css` stays valid CSS, and exports back without the `calc()`.
- **Round trip.** dfw -> Penpot -> dfw in the same project is exact: a token whose Penpot value is what dfw exported keeps dfw's own representation (colour components, colour space, units, group `$type`, key order). Imported into a project that does not have the tokens yet, colours are 8-bit sRGB (hex) values and the rest is equal.

## Sources (checked 2026-11)

- Penpot user guide, Design tokens (types, import/export, sets, themes, single vs multi file): https://help.penpot.app/user-guide/design-systems/design-tokens/
- Penpot source, token import/export (`$themes` fields, `$metadata` `tokenSetOrder`/`activeThemes`/`activeSets`, sets as top-level keys, `/` set paths, a file without `$themes`/`$metadata` is one set): https://github.com/penpot/penpot/blob/develop/common/src/app/common/types/tokens_lib.cljc
- Penpot source, DTCG type names (`fontFamilies`, `fontSizes`, `fontWeights`, `borderRadius`, `borderWidth`, `spacing`, `sizing`, `dimension`, `letterSpacing`, `textCase`, `textDecoration`, `typography`, `shadow`, `lineHeights` inside typography, `fontFamily`/`fontSize`/`fontWeight`/`boxShadow` accepted on import): https://github.com/penpot/penpot/blob/develop/common/src/app/common/types/token.cljc
- Penpot community, "Token export is: Tokens Studio format, not DTCG 2025.10" (values are strings such as `"16px"`, `"#0066cc"`, `"16"`): https://community.penpot.app/t/token-export-is-tokens-studio-format-not-dtcg-2025-10/10544
- Penpot plugin API, typography token value (`fontFamilies`, `fontSizes`, `fontWeights`, `letterSpacing`, `lineHeight`, `textCase`, `textDecoration`): https://doc.plugins.penpot.app/interfaces/TokenTypographyValue

Not verified against a running Penpot: whether Penpot keeps rem/em units in `dimension`/`letterSpacing` tokens, and the exact `activeThemes` path format (`group/name` is what dfw writes; Penpot recomputes active sets from the themes either way).
