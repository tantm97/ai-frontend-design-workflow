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

## Screens

```bash
dfw penpot export [--page <id>] [--bp <ids>] [--out file] [--json]
dfw penpot plugin                       # manifest URL to install in Penpot
dfw penpot import <changes.json> [--dry-run] [--json]
```

1. `dfw studio start`, then `dfw penpot plugin` prints the manifest URL (`http://127.0.0.1:<port>/__dfw/penpot/manifest.json`).
2. In Penpot, open a file, import `.dfw/penpot/tokens.penpot.json` in the Tokens panel (so colours link to tokens), open the Plugin Manager (Ctrl+Alt+P), paste the URL, install, and open **dfw bridge**.
3. **Build boards**: the plugin asks the Studio to render the design (or uses the last `dfw penpot export`) and creates one Penpot page per dfw page plus a `dfw components` page. Options in the plugin: dfw page id, breakpoints, render now.
4. **Live comments** (on by default while the plugin panel is open): every few seconds the plugin sends the comment threads that sit on dfw boards and main components to the Studio. This is the same loop as Studio comments, inside Penpot:
   - a new thread becomes one dfw comment (`provider: "penpot"`), and the agent is notified at once;
   - what the agent does comes back as replies in the same thread, prefixed `[dfw]`: seen (`dfw comments ack`), fixed with the note, dismissed, or a question (`dfw comments reply`);
   - a new message from the designer in the thread is added to the same dfw comment, and reopens it when it was fixed, resolved or dismissed;
   - resolving the thread in Penpot accepts a fix (`resolved`, by the user), or dismisses a comment that was not fixed yet; un-resolving it reopens.
   Threads elsewhere in the file are ignored. Replies are posted by the plugin as the Penpot user who has the panel open (Penpot has no bot identity), hence the `[dfw]` prefix. The link between a thread and its comment is kept in `.dfw/penpot/threads.json` (`<file id>#<thread number>`).
5. **Update boards**: after a fix, the panel says the design changed; **Update boards** renders again and redraws the boards already in the file in place, so their comment threads stay. Layer edits in Penpot that were not sent to dfw are lost by the redraw. Main components already in the file are reused when their structure is unchanged; a new or changed master is drawn as plain layers until the next full **Build boards** in a fresh file.
6. Layer edits (text, moves, fills, masters) are not live. **Send changes to dfw** posts the change set to the Studio, which records it as dfw comments (the agent is notified like for Studio comments). **Download changes** saves the same JSON for `dfw penpot import <file>` (when the Studio is not running or the browser blocks 127.0.0.1).

What the export contains (`.dfw/penpot/export.json`, history in `.dfw/penpot/exports/<id>.json`):
- one board per screen x variant x breakpoint, named `<screen> / <variant> @ <breakpoint>`, laid out per page (one row per breakpoint); board height = the rendered page height;
- layers read from the rendered page (headless Chrome, `lib/cdp.mjs`): boxes, fills, uniform borders as strokes (one-sided borders as thin rectangles), corner radii, box shadows, opacity, clipping; plain layout wrappers without id or paint are flattened;
- layer names = `data-dfw-id` (component instances without an id: `<component>/<variant>/<size>`); every layer also carries its path and nearest `data-dfw-id` in plugin data, so renames do not break the mapping;
- text as real text layers (font family, size, weight, line height, letter spacing, colour, alignment, transform, decoration); inputs show their value or placeholder;
- colours and radii linked to tokens by name when a token has exactly that value (text colours prefer `ink`/`text`/`on-*` names);
- every component master (component + variant + size) becomes a Penpot component, built from its first rendered instance; other instances with the same structure are Penpot instances with text/size/position overrides, the rest are drawn as plain boards;
- icons as SVG, images uploaded from the Studio (`/design/...`), CSS gradients and pseudo-elements are not exported (noted on the layer).

What comes back (`dfw penpot import`, or the plugin's Send): the plugin reports every layer it built (by path), new layers, deleted boards, comment threads and the file's token sets. dfw compares them with the export they were built from and writes:
- per element (`data-dfw-id`), one feedback comment (`provider: "penpot"`, screen, variant, breakpoint, file, `target.dfwId`, selector, source line, bbox) listing text changes, moves (a layer that moved with its parent is not listed), resizes (not for text: Penpot's font metrics differ), colour changes (with the matching token, or a note that no token has the new value), renames, removed and added layers;
- one comment per Penpot comment thread (author and replies), attached to the element under the pin; resolved threads are skipped;
- token differences as one comment, and the Penpot sets as `.dfw/penpot/tokens.from-penpot.json` for `dfw tokens import ... --dry-run` (never applied automatically).

### Component masters (the `dfw components` page)

Each main component on the `dfw components` page carries the key of its dfw master (`component|variant|size`) in plugin data (`dfw-main`, on the main shape and on the library component), and every layer inside keeps its exported path. **Send changes** also reads every main component of the file's library (`library.local.components`, `component.mainInstance()`) with box, fill, stroke and corner radii per layer (coordinates relative to the main), plus main components that were deleted and Penpot comments pinned on a main. dfw compares them with the master as exported and writes, per master element (component + layer), one feedback comment:

- `file` = the master file (`design/system/components/<name>.html`), no screen/variant/breakpoint; `target.component` = `{ name, variant, size }`, `target.layer` = the exported layer path; `target.source` = `file:line` of the CSS rule that sets the changed property (a rule naming the variant or size first, the base rule next, `:hover`/`:focus`/state rules last), else the element in the `<template>` (`<dfw-slot>` for text, by class or tag for children), else the `<dfw-component>` tag. Other lines of the same comment show their own rule when it differs;
- the note "Change the master ... it affects every screen using it. Do not patch single screens.";
- text changes in the example content (slot/default text), fill and text colours (with the matching colour token or "no colour token has this value"), stroke colour/width, added and removed borders, corner radius (with the `radius.*` token of that value), size (with a `size.*`/`width`/`height` token), spacing = the children's insets per side (padding; with the `space.*` token of that value, the border width subtracted first), component and layer renames, added layers (attached to the smallest element containing them), removed layers and deleted main components;
- Penpot comment threads pinned on a main become comments on its master.

Dimension tokens are matched by px value against the project's current `design/tokens/tokens.json` (rem counts as 16px). Text sizes are not compared (Penpot's font metrics differ), so right/bottom insets come only from non-text children. Masters are exported from their first rendered instance in the screens, so the example text is that instance's text.

Re-sending the same changes does not duplicate comments (`.dfw/penpot/imported.json`), for screens and masters alike. dfw never edits `design/` from Penpot: the agent applies the comments in the normal review loop. When the design is frozen (G2) the comments say so: they are recorded, and applying them needs `dfw reopen`.

Not exported back: layer order, effects other than fill/stroke colour (shadows, opacity, blur), typography changes (font, size, weight), variant/size additions made in Penpot, and new pages.

### Mechanism and why

| Option | Verdict |
|---|---|
| **Penpot plugin served by the Studio** (chosen) | Official, documented API (`doc.plugins.penpot.app`): create pages, boards, text, components and instances, upload images, read comments and token sets, store plugin data on shapes. Plugins load from a manifest URL and `localhost` is supported for development, so the plugin is served by the local Studio. The same code builds boards and reads changes back, so mapping by layer path and `data-dfw-id` is reliable. |
| Generating a `.penpot` file | v3 is a ZIP of JSON whose authoritative schema is Penpot's source code; shapes need internal fields (`selrect`, `points`, transform matrices, feature flags, migrations) that change between releases, and there is no stated stability policy for third-party producers. No way to get changes back except parsing the same internal format. |
| SVG import per frame | One-way. Penpot turns SVG into paths/groups: no text layers, components or token links, and nothing to map edits back. |
| REST/RPC API with an access token | Documented only for access tokens and webhooks; file content changes use internal RPC commands and Transit payloads. It would also need a token and a network call to the Penpot server. |

### Network and privacy

- dfw itself makes no network call to Penpot and needs no Penpot account or access token. Penpot (cloud or self-hosted) loads the plugin in your browser from `http://127.0.0.1:<port>`; the plugin UI talks to the Studio on the same origin.
- What goes from the Studio to the plugin (inside your browser): `export.json` (screen names, layer names, texts, boxes, colours, token names and values) and the images under `design/` that the screens use. The plugin then writes them into the Penpot file you have open, which is stored by that Penpot instance like any other edit.
- What comes back: the change set described above. The Studio accepts it only from its own origin (the plugin UI); manifest and plugin code are readable cross-origin (with `Access-Control-Allow-Private-Network`) so Penpot can load them.
- Browsers may ask before a public site reaches `127.0.0.1` (Chrome's local network access prompt); some browsers block it. Then use **Load export file** / **Download changes** with `dfw penpot import`.

### Not verified without a running Penpot

The export, change mapping (screens and component masters: `native/src/penpot/export_tests.rs` and `tests/golden/penpot.golden.mjs`), Studio routes and CLI are tested on fixtures. The plugin code (`studio/penpot/plugin.js`) follows the documented plugin API but was not run inside Penpot here. Points to check on first use: whether `appendChild` keeps absolute positions, instance child naming and plugin data copied into instances, `fonts.findByName` / `applyToText`, `openPage` while a plugin runs, comment thread positions (absolute vs board-relative), the `TokenSet`/`Token` fields read back, and token default properties when applied to text and boards. For component masters: whether `createComponent([shape])` keeps the shape (and its plugin data) as the main or wraps it (the key is also written on the library component and on `mainInstance()` to cover both), whether `LibraryComponent.setPluginData` is available, whether `library.local.components` lists every main of the file, the units of `borderRadiusTopLeft`... and `strokes[0].strokeWidth` read back, whether `CommentThread.board` is the main component when a pin sits on the `dfw components` page, and whether deleting a main removes it from `library.local.components` (reported as "main component deleted").

## Sources (checked 2026-11)

- Penpot user guide, Design tokens (types, import/export, sets, themes, single vs multi file): https://help.penpot.app/user-guide/design-systems/design-tokens/
- Penpot source, token import/export (`$themes` fields, `$metadata` `tokenSetOrder`/`activeThemes`/`activeSets`, sets as top-level keys, `/` set paths, a file without `$themes`/`$metadata` is one set): https://github.com/penpot/penpot/blob/develop/common/src/app/common/types/tokens_lib.cljc
- Penpot source, DTCG type names (`fontFamilies`, `fontSizes`, `fontWeights`, `borderRadius`, `borderWidth`, `spacing`, `sizing`, `dimension`, `letterSpacing`, `textCase`, `textDecoration`, `typography`, `shadow`, `lineHeights` inside typography, `fontFamily`/`fontSize`/`fontWeight`/`boxShadow` accepted on import): https://github.com/penpot/penpot/blob/develop/common/src/app/common/types/token.cljc
- Penpot community, "Token export is: Tokens Studio format, not DTCG 2025.10" (values are strings such as `"16px"`, `"#0066cc"`, `"16"`): https://community.penpot.app/t/token-export-is-tokens-studio-format-not-dtcg-2025-10/10544
- Penpot plugin API, typography token value (`fontFamilies`, `fontSizes`, `fontWeights`, `letterSpacing`, `lineHeight`, `textCase`, `textDecoration`): https://doc.plugins.penpot.app/interfaces/TokenTypographyValue
- Penpot plugin API reference: `Penpot` (createPage/openPage, createBoard, createText, createShapeFromSvg, uploadMediaData, ui.open/sendMessage/onMessage), `Board`, `Text`, `Page` (findCommentThreads), `CommentThread`, `Library` (createComponent, tokens), `TokenCatalog`, `TokenSet`, `TokenBase` (applyToShapes), `File` (pages, plugin data): https://doc.plugins.penpot.app/
- Penpot help, Create a plugin (manifest fields, `version: 2` relative paths, permissions, loading from `http://localhost:<port>/manifest.json` in the Plugin Manager, CORS): https://help.penpot.app/plugins/create-a-plugin/
- Penpot help, `.penpot` file format v3 (ZIP + JSON, source code is the authoritative schema): https://help.penpot.app/technical-guide/developer/data-model/penpot-file-format/
- Penpot help, Integrations (access tokens `Authorization: Token <token>`, RPC at `/api/rpc/command/<name>`, webhooks): https://help.penpot.app/technical-guide/integration/

Not verified against a running Penpot: whether Penpot keeps rem/em units in `dimension`/`letterSpacing` tokens, and the exact `activeThemes` path format (`group/name` is what dfw writes; Penpot recomputes active sets from the themes either way).
