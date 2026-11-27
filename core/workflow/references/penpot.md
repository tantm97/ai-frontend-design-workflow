# Penpot bridge (optional)

Penpot is an optional bridge for designers who want to work in Penpot. dfw's HTML screens and DTCG tokens stay the source of truth: nothing from Penpot overwrites them blindly, and everything runs locally.

## Review canvas

Where the user reviews and comments on the screens: the dfw Studio (default) or Penpot. Ask once at `dfw init` (SKILL.md §0 step 3) with {{ASK_TOOL}}: "Where do you want to review and comment on the designs?"

- "dfw Studio (Recommended): local canvas in your browser: inspect, edit text, comment and press Send to AI; nothing else to install."
- "Penpot: dfw keeps boards in your Penpot file up to date; you comment and tweak there and the changes come back to me. The bridge panel stays open and the Studio runs in the background. The design system and states are still reviewed in the Studio."

Then:

- **Record it.** `dfw init ... --canvas studio|penpot`, and `dfw decide --level 1 --topic canvas "<choice>"`. Switch later with `dfw config set review.canvas studio|penpot` (record that too).
- **Penpot mode means:** HTML and tokens stay the source of truth. The **dfw bridge** panel keeps the boards up to date by itself while it is open (**Keep boards up to date automatically**, on by default): after each build, once the design has been quiet for 10 s, it first sends the designer's pending edits to dfw, then redraws the boards in place. Penpot comments and edits come back as dfw comments and wake your `dfw comments wait` like Studio comments.
- **Setup for the user:** `dfw studio start` (keep it running), give them the manifest URL from `dfw penpot plugin`, and the steps of "Setup" below; the first **Build boards** is pressed once in the file they choose. The panel must stay open: dfw never calls Penpot.
- **Limits:** the Design System page, screen states and themes are reviewed in the Studio; give its link for those steps.
- **Self-hosted Penpot:** `penpot.origins` is global only. Show the user `dfw config set penpot.origins '["https://penpot.example.com"]' --global` with their address, and run it only on their explicit yes (it writes the user's global config).

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
- **Bridge token.** Every write of the panel carries a token it reads from the Studio (same origin only); the Studio makes a new one each time it starts. After `dfw studio start` again the open panel reads the new token by itself on its next check; if the Studio moved to another port, close the dfw bridge plugin in Penpot and open it again.
- dfw itself makes no network call to Penpot and needs no Penpot account or access token. Penpot (cloud or self-hosted) loads the plugin in your browser from `http://127.0.0.1:<port>`; the plugin UI talks to the Studio on the same origin.
- What goes from the Studio to the plugin (inside your browser): `export.json` (screen names, layer names, texts, boxes, colours, token names and values) and the images under `design/` that the screens use. The plugin then writes them into the Penpot file you have open, which is stored by that Penpot instance like any other edit.
- What comes back: the change set described in `references/penpot-import.md`. The Studio accepts it only from its own origin (the plugin UI); manifest and plugin code are readable cross-origin (with `Access-Control-Allow-Private-Network`) so Penpot can load them.
- Browsers may ask before a public site reaches `127.0.0.1` (Chrome's local network access prompt); some browsers block it. Then use **Load export file** / **Download changes** with `dfw penpot import`.

## Not verified without a running Penpot

The export, change mapping, Studio routes and CLI are tested on fixtures. The plugin code (`studio/penpot/plugin.js`) follows the documented plugin API but was not run inside Penpot here. The points to check on first use are listed at the end of each job file.
