# Penpot bridge: import changes

Turn a designer's layer edits in Penpot, on screens and on component masters, into dfw comments. Overview, setup and privacy: `references/penpot.md`. The boards come from `references/penpot-export.md`.

```bash
dfw penpot import <changes.json> [--dry-run] [--json]
```

Layer edits (text, moves, fills, masters) are not live. **Send changes to dfw** posts the change set to the Studio, which records it as dfw comments (the agent is notified like for Studio comments). **Download changes** saves the same JSON for `dfw penpot import <file>` (when the Studio is not running or the browser blocks 127.0.0.1).

## Screens

What comes back (`dfw penpot import`, or the plugin's Send): the plugin reports every layer it built (by path), new layers, deleted boards, comment threads and the file's token sets. dfw compares them with the export they were built from and writes:

- per element (`data-dfw-id`), one feedback comment (`provider: "penpot"`, screen, variant, breakpoint, file, `target.dfwId`, selector, source line, bbox) listing text changes, moves (a layer that moved with its parent is not listed), resizes (not for text: Penpot's font metrics differ), colour changes (with the matching token, or a note that no token has the new value), renames, removed and added layers;
- one comment per Penpot comment thread (author and replies), attached to the element under the pin; resolved threads are skipped;
- token differences as one comment, and the Penpot sets as `.dfw/penpot/tokens.from-penpot.json` for `dfw tokens import ... --dry-run` (never applied automatically; see `references/penpot-tokens.md`).

## Component masters (the `dfw components` page)

Each main component on the `dfw components` page carries the key of its dfw master (`component|variant|size`) in plugin data, and every layer inside keeps its exported path. **Send changes** also reads every main component of the file's library (`library.local.components`, `component.mainInstance()`) with box, fill, stroke and corner radii per layer (coordinates relative to the main), plus main components that were deleted and Penpot comments pinned on a main. dfw compares them with the master as exported and writes, per master element (component + layer), one feedback comment:

- `file` = the master file (`design/system/components/<name>.html`), no screen/variant/breakpoint; `target.component` = `{ name, variant, size }`, `target.layer` = the exported layer path; `target.source` = `file:line` of the CSS rule that sets the changed property (a rule naming the variant or size first, the base rule next, `:hover`/`:focus`/state rules last), else the element in the `<template>` (`<dfw-slot>` for text, by class or tag for children), else the `<dfw-component>` tag. Other lines of the same comment show their own rule when it differs;
- the note "Change the master ... it affects every screen using it. Do not patch single screens.";
- text changes in the example content (slot/default text), fill and text colours (with the matching colour token or "no colour token has this value"), stroke colour/width, added and removed borders, corner radius (with the `radius.*` token of that value), size (with a `size.*`/`width`/`height` token), spacing = the children's insets per side (padding; with the `space.*` token of that value, the border width subtracted first), component and layer renames, added layers (attached to the smallest element containing them), removed layers and deleted main components;
- Penpot comment threads pinned on a main become comments on its master.

Dimension tokens are matched by px value against the project's current `design/tokens/tokens.json` (rem counts as 16px). Text sizes are not compared (Penpot's font metrics differ), so right/bottom insets come only from non-text children. Masters are exported from their first rendered instance in the screens, so the example text is that instance's text.

## Rules

- Re-sending the same changes does not duplicate comments (`.dfw/penpot/imported.json`), for screens and masters alike.
- dfw never edits `design/` from Penpot: the agent applies the comments in the normal review loop.
- When the design is frozen (G2) the comments say so: they are recorded, and applying them needs `dfw reopen`.
- Not exported back: layer order, effects other than fill/stroke colour (shadows, opacity, blur), typography changes (font, size, weight), variant/size additions made in Penpot, and new pages.

## Not verified without a running Penpot

The change mapping (screens and component masters: `native/src/penpot/export_tests.rs` and `tests/golden/penpot.golden.mjs`), Studio routes and CLI are tested on fixtures; the plugin code (`studio/penpot/plugin.js`) was not run inside Penpot here. Points to check on first use: comment thread positions (absolute vs board-relative) and the `TokenSet`/`Token` fields read back. For component masters: whether `library.local.components` lists every main of the file, the units of `borderRadiusTopLeft`... and `strokes[0].strokeWidth` read back, whether `CommentThread.board` is the main component when a pin sits on the `dfw components` page, and whether deleting a main removes it from `library.local.components` (reported as "main component deleted").
