# Penpot bridge: export screens

Draw dfw screens and component masters as Penpot boards and components. Overview, setup and privacy: `references/penpot.md`.

```bash
dfw penpot export [--page <id>] [--bp <ids>] [--out file] [--json]
dfw penpot plugin                       # manifest URL to install in Penpot
```

1. Set up the **dfw bridge** plugin as in `references/penpot.md` (Setup).
2. **Build boards**: the plugin asks the Studio to render the design (or uses the last `dfw penpot export`) and creates one Penpot page per dfw page plus a `dfw components` page. Options in the plugin: dfw page id, breakpoints, render now.
3. **Update boards**: after a fix, the panel says the design changed; **Update boards** renders again and redraws the boards already in the file in place, so their comment threads stay. Layer edits in Penpot that were not sent to dfw are lost by the redraw. Main components already in the file are reused when their structure is unchanged; a new or changed master is drawn as plain layers until the next full **Build boards** in a fresh file.

## What the export contains

`.dfw/penpot/export.json`, history in `.dfw/penpot/exports/<id>.json`:

- one board per screen x variant x breakpoint, named `<screen> / <variant> @ <breakpoint>`, laid out per page (one row per breakpoint); board height = the rendered page height;
- layers read from the rendered page (headless Chrome, `native/src/browser/cdp.rs`): boxes, fills, uniform borders as strokes (one-sided borders as thin rectangles), corner radii, box shadows, opacity, clipping; plain layout wrappers without id or paint are flattened;
- layer names = `data-dfw-id` (component instances without an id: `<component>/<variant>/<size>`); every layer also carries its path and nearest `data-dfw-id` in plugin data, so renames do not break the mapping;
- text as real text layers (font family, size, weight, line height, letter spacing, colour, alignment, transform, decoration); inputs show their value or placeholder;
- colours and radii linked to tokens by name when a token has exactly that value (text colours prefer `ink`/`text`/`on-*` names);
- every component master (component + variant + size) becomes a Penpot component, built from its first rendered instance; other instances with the same structure are Penpot instances with text/size/position overrides, the rest are drawn as plain boards;
- icons as SVG, images uploaded from the Studio (`/design/...`), CSS gradients and pseudo-elements are not exported (noted on the layer).

Each main component on the `dfw components` page carries the key of its dfw master (`component|variant|size`) in plugin data (`dfw-main`, on the main shape and on the library component), and every layer inside keeps its exported path. Masters are exported from their first rendered instance in the screens, so the example text is that instance's text.

## Mechanism and why

| Option | Verdict |
|---|---|
| **Penpot plugin served by the Studio** (chosen) | Official, documented API (`doc.plugins.penpot.app`): create pages, boards, text, components and instances, upload images, read comments and token sets, store plugin data on shapes. Plugins load from a manifest URL and `localhost` is supported for development, so the plugin is served by the local Studio. The same code builds boards and reads changes back, so mapping by layer path and `data-dfw-id` is reliable. |
| Generating a `.penpot` file | v3 is a ZIP of JSON whose authoritative schema is Penpot's source code; shapes need internal fields (`selrect`, `points`, transform matrices, feature flags, migrations) that change between releases, and there is no stated stability policy for third-party producers. No way to get changes back except parsing the same internal format. |
| SVG import per frame | One-way. Penpot turns SVG into paths/groups: no text layers, components or token links, and nothing to map edits back. |
| REST/RPC API with an access token | Documented only for access tokens and webhooks; file content changes use internal RPC commands and Transit payloads. It would also need a token and a network call to the Penpot server. |

## Not verified without a running Penpot

The export (`native/src/penpot/export_tests.rs` and `tests/golden/penpot.golden.mjs`) is tested on fixtures; the plugin code (`studio/penpot/plugin.js`) was not run inside Penpot here. Points to check on first use: whether `appendChild` keeps absolute positions, instance child naming and plugin data copied into instances, `fonts.findByName` / `applyToText`, `openPage` while a plugin runs, and token default properties when applied to text and boards. For component masters: whether `createComponent([shape])` keeps the shape (and its plugin data) as the main or wraps it (the key is also written on the library component and on `mainInstance()` to cover both), and whether `LibraryComponent.setPluginData` is available.

## Sources (checked 2026-11)

- Penpot plugin API reference: `Penpot` (createPage/openPage, createBoard, createText, createShapeFromSvg, uploadMediaData, ui.open/sendMessage/onMessage), `Board`, `Text`, `Page` (findCommentThreads), `CommentThread`, `Library` (createComponent, tokens), `TokenCatalog`, `TokenSet`, `TokenBase` (applyToShapes), `File` (pages, plugin data): https://doc.plugins.penpot.app/
- Penpot help, Create a plugin (manifest fields, `version: 2` relative paths, permissions, loading from `http://localhost:<port>/manifest.json` in the Plugin Manager, CORS): https://help.penpot.app/plugins/create-a-plugin/
- Penpot help, `.penpot` file format v3 (ZIP + JSON, source code is the authoritative schema): https://help.penpot.app/technical-guide/developer/data-model/penpot-file-format/
- Penpot help, Integrations (access tokens `Authorization: Token <token>`, RPC at `/api/rpc/command/<name>`, webhooks): https://help.penpot.app/technical-guide/integration/
