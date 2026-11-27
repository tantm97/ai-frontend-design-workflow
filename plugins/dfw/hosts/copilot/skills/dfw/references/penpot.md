# Penpot bridge (optional)

Penpot is an optional bridge for designers who want to work in Penpot. dfw's HTML screens and DTCG tokens stay the source of truth: nothing from Penpot overwrites them blindly, and everything runs locally.

## The jobs

Read only the file for the job at hand:

- **Tokens** (`dfw tokens export --format penpot`, `dfw tokens import`): move tokens, sets and themes between dfw and Penpot's Tokens panel. See `references/penpot-tokens.md`.
- **Export screens** (`dfw penpot export`, **Build boards**, **Update boards**): draw dfw screens and component masters as Penpot boards and components. See `references/penpot-export.md`.
- **Import changes** (`dfw penpot import`, **Send changes to dfw**): turn a designer's layer edits on screens and masters into dfw comments. See `references/penpot-import.md`.
- **Live comments** (on while the plugin panel is open): Penpot comment threads become dfw comments, and the agent's answers come back as replies. See `references/penpot-live.md`.

## Setup

```bash
dfw penpot plugin                       # manifest URL to install in Penpot
```

1. `dfw studio start`, then `dfw penpot plugin` prints the manifest URL (`http://127.0.0.1:<port>/__dfw/penpot/manifest.json`).
2. In Penpot, open a file, import `.dfw/penpot/tokens.penpot.json` in the Tokens panel (so colours link to tokens), open the Plugin Manager (Ctrl+Alt+P), paste the URL, install, and open **dfw bridge**.

## Network and privacy

- **Which Penpot may drive the bridge** is the user's choice, in the global config only (a project cannot set it): `penpot.origins`, default `["https://design.penpot.app"]`. For a self-hosted Penpot: `dfw config set penpot.origins '["https://penpot.example.com"]' --global`, then restart the Studio. The panel may be framed only by those origins and talks only to them.
- **Bridge token.** Every write of the panel carries a token it reads from the Studio (same origin only); the Studio makes a new one each time it starts. After `dfw studio start` again, close the dfw bridge plugin in Penpot and open it again, or its writes are refused.
- dfw itself makes no network call to Penpot and needs no Penpot account or access token. Penpot (cloud or self-hosted) loads the plugin in your browser from `http://127.0.0.1:<port>`; the plugin UI talks to the Studio on the same origin.
- What goes from the Studio to the plugin (inside your browser): `export.json` (screen names, layer names, texts, boxes, colours, token names and values) and the images under `design/` that the screens use. The plugin then writes them into the Penpot file you have open, which is stored by that Penpot instance like any other edit.
- What comes back: the change set described in `references/penpot-import.md`. The Studio accepts it only from its own origin (the plugin UI); manifest and plugin code are readable cross-origin (with `Access-Control-Allow-Private-Network`) so Penpot can load them.
- Browsers may ask before a public site reaches `127.0.0.1` (Chrome's local network access prompt); some browsers block it. Then use **Load export file** / **Download changes** with `dfw penpot import`.

## Not verified without a running Penpot

The export, change mapping, Studio routes and CLI are tested on fixtures. The plugin code (`studio/penpot/plugin.js`) follows the documented plugin API but was not run inside Penpot here. The points to check on first use are listed at the end of each job file.
