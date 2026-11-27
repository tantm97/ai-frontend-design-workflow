# AI Frontend Design Workflow

**An AI-powered UI/UX and frontend design workflow for AI coding agents: Claude Code and GitHub Copilot as plugins, OpenCode, Codex and Cursor through MCP.** It takes a product from requirements to an approved visual design to working frontend code: UX research, a reasoned design direction, a design system built first, a local Figma-like design studio with live click-to-comment review, design and technology approval gates, implementation in the stack you choose (React, Vue, Angular, Svelte, plain HTML...), and verification against the approved design.

```
discovery -> direction (G1) -> design system (DS) -> pages of screens + review (G2: design frozen)
          -> technology approval (G3) -> implementation -> verification (G4)
```

## Install (one command)

No Node.js needed (downloads the native `dfw` binary for your OS from the GitHub release, checks its SHA-256 against the release's `SHA256SUMS`, then registers everything with Claude Code and GitHub Copilot, whichever is installed):

```bash
curl -fsSL https://raw.githubusercontent.com/tantm97/ai-frontend-design-workflow/main/install.sh | sh
```

With Node.js (the same download and checksum check, done by a small Node script):

```bash
npx github:tantm97/ai-frontend-design-workflow
```

On Windows without a POSIX shell: download `dfw-<version>-win32-x64.exe` from the [releases](https://github.com/tantm97/ai-frontend-design-workflow/releases), check it against `SHA256SUMS`, save it as `%USERPROFILE%\.config\dfw\bin\<version>\dfw.exe` and run `dfw.exe install`.

Both add the `dfw-marketplace` to each tool, install `dfw` (its plugin dependencies pull in the seven design skills), set up the Impeccable engine (telemetry off) and run `dfw doctor`. Screenshots and the layout check use the Chrome, Edge or Chromium already on your machine; only when none is found does the installer download Google's headless Chrome once. Options: `--scope project` (share via `.claude/settings.json`), `--source <path|owner/repo>` (another marketplace source, for example a local clone; a marketplace registered from a different source is replaced), `--no-runtime`, `uninstall`. Running the installer again upgrades an installed dfw to the newest release. Then restart Claude Code (or `/reload-plugins`) and type `/dfw`.

How dfw runs: dfw is one native program (written in Rust, about 7 MB, no runtime to install). It carries the Studio, the templates, the starter kit, the style guides, the icons and the workflow guide inside, so it runs from any folder. The plugin's `bin/dfw` launcher (`bin/dfw.cmd` on Windows), which Claude Code puts on `PATH` and every hook calls, starts the binary installed for the plugin's version, `~/.config/dfw/bin/<version>/dfw` (`DFW_HOME` or `XDG_CONFIG_HOME` move that folder). When the plugin was updated before the matching binary was installed, it runs the newest installed binary and says so on stderr; when none is installed it prints how to install one and exits with 127. `dfw setup --binary` downloads the binary of the plugin's version; `DFW_BIN=<path>` runs another build (development).

The installer registers dfw with every supported tool it finds (`--host claude|copilot|all` to choose). Without the installer: Claude Code `claude plugin marketplace add tantm97/ai-frontend-design-workflow` then `claude plugin install dfw@dfw-marketplace`; GitHub Copilot CLI `copilot plugin marketplace add tantm97/ai-frontend-design-workflow` then `copilot plugin install dfw@dfw-marketplace` (and the seven skills the same way). Both tools install the same plugin folder, `plugins/dfw`: Claude Code reads `.claude-plugin/plugin.json`, Copilot the root `plugin.json`, and each loads only its own skill, agents and hooks from `hosts/claude/` or `hosts/copilot/`.

**Upgrading GitHub Copilot from 0.6.1 or earlier** (when Copilot had its own `plugins/dfw-copilot` folder): run `dfw install --host copilot` once with a newer dfw (the one-line installer above gets it). It uninstalls the old dfw, installs the one from `plugins/dfw` and prints `Replaced the old dfw for GitHub Copilot (<path>): two-folder layout.` By hand: `copilot plugin uninstall dfw`, then `copilot plugin install dfw@dfw-marketplace` (a plain `copilot plugin update dfw` may keep the old files). If you installed `plugins/dfw-copilot` by path, uninstall it the same way and install from the marketplace. `dfw doctor` warns while the old layout is installed. Claude Code users only update as usual.

OpenCode, Codex, Cursor: `dfw mcp install opencode|codex|cursor|all` adds dfw's MCP server (workflow guide, CLI and a write-gate check) to that tool's config (`--print` to preview). MCP cannot block the tool's own file writes, so the approval gates are advisory there.

The workflow is called **dfw** (design-first workflow): `/dfw` in Claude Code, `dfw` on the command line.

- **Design system first, like Figma.** Tokens (W3C DTCG) and component masters with every variant and state come first; screens are composed from component instances (`<dfw-use>`), so changing a master updates every screen. A token-driven starter kit (21 components incl. a type component, 3 neutral presets) plus 12 curated style guides (concept, named palette with usage rules, display type, motifs) and a curated icon set (Lucide) make the first version fast.
- **Pages and a canvas.** Screens are grouped in pages per module. The local **Design Studio** shows the Design System page and every page as an infinite canvas of frames (screen × variant × breakpoint × state/theme): pan/zoom, layers, an inspector that shows which tokens and component an element uses, direct edits written back to the source (with undo), a full-window **Present** mode and an **Interact** tool for clickable prototypes, and click-to-comment review. Pages are laid out like a pencil.dev file: a section per module with rows per breakpoint, variants, states and annotation notes.
- **Live review.** Comments you leave in the Studio are drafts until you press **Send to AI**, which wakes Claude at once (it keeps a background `dfw comments wait` running while you review; hooks on your next message and before Claude stops are the safety net). The Studio shows whether the agent is listening or working, each comment's live status (sent → received → working → fixed, please verify), and you answer the agent's questions with **Reply**, without the terminal.
- **Senior design reasoning, not taste by accident.** A design brief (use context + 7 personality axes) ranks 12 curated style guides (`dfw style match`). Palettes are generated as perceptual OKLCH ramps with roles picked by contrast targets (`dfw palette build`), type follows a modular scale (`dfw type scale`), and `dfw system audit` reviews the system like a senior designer: contrast, accent vs error/success hues, neutral temperature, hue budget, type rhythm, usage rules per colour, and a written rationale for every decision (required at the DS gate).
- **Quality loop.** Two self-critique rounds against a rubric before you see anything, 2-3 options where a real choice exists, and automated checks on the composed HTML:
  - dfw lint, with AI-default tells, interface guidelines and missing states;
  - a rendered layout check, including 320px with text at 125%;
  - axe-core for rendered accessibility;
  - Impeccable and Hallmark sloplint;
  - `dfw diff` for a pixel diff of design vs. implementation screenshots (pixelmatch: % changed plus a diff image).

  A check that could not run is reported as NOT VERIFIED, never as passing. `npm run bench` measures the checks on seeded defects.
- **Hard gates.** A PreToolUse hook blocks implementation files until the design is approved (G2) and you approved the stack (G3). Approved designs are frozen in `design/approved/vN/` with a sha256 lock. G2 also needs every configured check to have run; `dfw approve G2 --accept-unverified` freezes without them only when the user accepts that, and it is recorded.
- **No silent stack choices.** The design phase never assumes React/Tailwind/etc.; the stack is decided at G3 (existing projects keep their stack unless you approve a change).
- **One orchestrator, curated skills.** dfw routes to one lead + at most two supporting skills per phase, with written precedence rules and conflict rulings.
- **Claude is the only paid service.** Everything else is open source and local. Telemetry and update checks of bundled tools are off; dfw's own usage counts are described under [Telemetry and privacy](#telemetry-and-privacy).

## What is in this repository

| Plugin | Origin | Role in dfw |
|---|---|---|
| `dfw` | original (MIT), assembled from `core/` + `adapters/claude-code/` + `adapters/copilot/` (one folder for both hosts) | Orchestrator skill `/dfw`, CLI, hooks, Design Studio, starter kit + style guides + icons, agents (design-director, screen-builder, visual-reviewer, skill-steward), policies |
| `frontend-design` | [anthropics/skills](https://github.com/anthropics/skills) (Apache-2.0) | Aesthetic base in design phases |
| `ux-research` | [anthropics/knowledge-work-plugins](https://github.com/anthropics/knowledge-work-plugins) (Apache-2.0) | Discovery research and UX copy |
| `ui-ux-pro-max` | [nextlevelbuilder/ui-ux-pro-max-skill](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill) (MIT) | Data: palettes, font pairings, UX guidelines (core skill only) |
| `impeccable` | [pbakaus/impeccable](https://github.com/pbakaus/impeccable) (Apache-2.0) | Critique/audit/polish; detector used by `dfw check` (hooks stripped) |
| `emil-design-eng` | [emilkowalski/skills](https://github.com/emilkowalski/skills) (MIT) | Motion craft and review; library picks at G3 |
| `hallmark` | [nutlope/hallmark](https://github.com/nutlope/hallmark) (MIT) | Lead for marketing/landing pages; `sloplint` used by `dfw check` |
| `taste-reference` | [Leonxlnx/taste-skill](https://github.com/Leonxlnx/taste-skill) (MIT) | Reference mode only: website copy/section references + `copy-lint` |

Also used, pinned and unmodified:

| Item | Source | Use |
|---|---|---|
| Knowledge | [vercel-labs/web-interface-guidelines](https://github.com/vercel-labs/web-interface-guidelines) (MIT) | `references/web-interface-guidelines.md` (interactions), `-visual.md` (concepts and review) and `-performance.md` (implement and verify), split by phase |
| Tool | [axe-core](https://github.com/dequelabs/axe-core) 4.14.0 (MPL-2.0) | Downloaded once by `dfw setup --a11y` with its npm integrity hash checked; injected into rendered screens by `dfw check` |
| Code | [pixelmatch](https://github.com/mapbox/pixelmatch) 8.0.0 (ISC) | Ported into the binary for `dfw diff` (`native/src/browser/pixeldiff.rs`); the original is kept in `core/assets/vendor/pixelmatch.js` |
| Data | [Lucide](https://github.com/lucide-icons/lucide) icons (ISC) | The icon set of `dfw icons`, embedded in the binary (`core/assets/icons/`) |
| Tool, optional | [Lighthouse](https://github.com/GoogleChrome/lighthouse) 13.6.0 (Apache-2.0) | `dfw check --url <u> --lighthouse` at G4 only, through npx, error reporting off |

Vendored plugins are built from pinned commits (`registry/skills.lock.json`) with declarative overlays (`overlays/`); upstream files are never edited by hand. See `THIRD_PARTY_NOTICES.md`.

## Requirements

- Claude Code with a Claude subscription (no API key needed), or GitHub Copilot CLI.
- The `dfw` binary (installed by `install.sh` or the installer). Node.js is not needed to run dfw. It is optional, for two things only: the vendored skills' own scripts (Hallmark sloplint, Taste copy-lint), which `dfw check` reports as NOT VERIFIED without it, and Lighthouse (`dfw check --lighthouse`, through `npx`). Python 3 (UI UX Pro Max search), git.
- Release binaries: macOS (Apple silicon, Intel), Linux (x64, arm64; glibc 2.39 or newer, as on Ubuntu 24.04) and Windows (x64).
- A Chromium-based browser for screenshots and the layout check: your installed Chrome / Edge / Chromium / Brave (driven headless over the DevTools protocol, no npm packages), `DFW_BROWSER=/path/to/chrome`, or `dfw setup --browser` (Google's chrome-headless-shell, once, into `~/.config/dfw/browser`). Web fonts used by designs are cached in `~/.config/dfw/cache/fonts`.
- The Studio updates frames and comment status when files change: it uses file-system events and also polls modification times every second, so it keeps working where file events are unreliable (CI runners, network drives, WSL, Docker bind mounts). `DFW_STUDIO_WATCH=poll` turns file events off and polls only. A changed design file is patched into the frame in place (no reload, scroll position kept; a frame whose scripts changed reloads as before). The frames near the view are patched first, three at a time; frames far off screen or on another page catch up when they come into view. With many screens the canvas keeps at most 48 frames loaded, the ones nearest the view (the selected frame always among them); the others show a placeholder and load again when you come near. While Claude edits, the canvas shows where: a dashed box and a cursor on the element it is about to edit (announced by the pre-tool-use hook through `.dfw/presence.json`), then a solid mark on what changed, which fades after a few seconds. On the Design System page each component is documented in rows (summary, variants, sizes and props with their meaning, rules, tokens, master file) in three text sizes.

If you already installed any of these skills from their original marketplaces, disable those copies (`dfw doctor` warns about duplicates), otherwise both load.

Per project you can turn plugins off in `.claude/settings.json`:

```json
{ "enabledPlugins": { "hallmark@dfw-marketplace": false } }
```

## Use

Open Claude Code in a project and ask, for example:

> /dfw Design a professional inventory management application based on these requirements, screenshots and brand assets.

dfw asks your communication language once per session (artifacts stay in English), detects whether the project is new or existing, and walks the phases. Modes: `new`, `existing`, `design-only`, `implement` (approved design), `refine`, `quick`, `review`, `skills`.

Review loop: Claude starts the Studio (`dfw studio start`, default http://127.0.0.1:4600) and listens for your comments.

| In the Studio | How |
|---|---|
| Move around | scroll / space-drag (H), Cmd/Ctrl-scroll or pinch to zoom, Shift+1 fit, Shift+2 zoom to selection |
| Pages | Design System, one page per module, Explorations, or **All pages** on one canvas |
| Inspect | click an element (Cmd/Alt-click for the exact one): layers, tokens in use, component + variant, "Go to master" |
| Edit directly | double-click text; change variant/size/props or spacing tokens in the inspector; Cmd/Ctrl+Z undoes (the newest 200 edits are kept) |
| Rearrange and resize | select an element with a `data-dfw-id`, then drag it among its flex/grid siblings (a blue line shows where it lands; the source is moved) or into an empty flex/grid container that has a `data-dfw-id` (dashed highlight), or drag its handles: in a grid the width snaps to a column span (`span 2 / 4`), elsewhere to a `size.*` token or 25/33/50/67/75/100%, and the height to a `min-height` spacing/size token. Keyboard: Alt+arrows move, Alt+Shift+arrows step the size, Alt+Shift+0 resets it to auto (so do double-clicking a handle and the inspector's "Reset size to auto"). Resizes go to a `<style data-dfw-layout>` block in the screen with selectors that outrank class-based screen rules; in a narrower breakpoint frame they go inside `@media (max-width: <frame width>px)`, so they apply there and below. If a stronger rule (id selector, inline style, `!important`) still wins, the Studio names it. Slot content of a component (e.g. an app shell) can be rearranged; elements that only exist in a master are refused with a pointer to the master. Edits are refused once the design is approved (G2) |
| Clickable preview | **P** (or double-click a frame title) for full-window Present; **I** to interact on the canvas; ← → between screens. Present hides comment pins unless you press **Pins** (Shift+C) or comment |
| Comment | **C**, click an element, write (Cmd/Ctrl+Enter), then **Send to AI** when you are done (Claude gets the drafts at once); fixed comments appear under **To verify** where you resolve or reopen them |
| Breakpoints and states | top-bar menus with checkboxes and **All** (default: all shown); only checked breakpoints/states get frames |
| Screenshot | top-bar camera: pick the screens to shoot (grouped by page, all checked by default, choice remembered), at the shown breakpoints/states and theme, to PNG in `.dfw/shots/studio/<time>/`; **Screenshot** in Present (also fullscreen): the screen being shown, saved and downloaded |
| Comment pins | click a pin to read the comment and its thread, and resolve or reopen it in place. **Shift+C** (or the **Pins** chip) cycles open → all → off; resolved comments are hidden by default |

Useful commands (`dfw help` for all; `-h`, `--help` or `dfw help <command>` only print help and never run the command): `dfw status`, `dfw style match|show|apply`, `dfw system init|inventory|ref|options|choose|audit|list`, `dfw palette build`, `dfw type scale`, `dfw icons search|add`, `dfw fonts check`, `dfw check`, `dfw shoot --page <id> --states`, `dfw comments list|wait`, `dfw decisions`, `dfw pause|resume`.

Review the running app (phase verify): `dfw review start --url http://localhost:5173` opens a local proxy (loopback only; HMR passes through; printed as `http://localhost:<port>/` for a localhost app so its login cookies still apply, and a strict CSP is extended only for the injected review scripts) with a **Comment** button (or Alt/Option+C; change it with `--shortcut <combo>` or `dfw config set review.shortcut <combo>`, `off` for the button only; never while typing in a field) on the app itself. Comments go to the same inbox as Studio comments, with the route, a screenshot, and the element's `file:line:col` when the project has [code-inspector-plugin](https://github.com/zh-lx/code-inspector) (MIT, dev builds only). dfw never adds that dependency silently: `dfw inspector setup` prints the pinned devDependency and the Vite/webpack/Next config change to propose at G3; `--write` applies it only after G3 approved it and keeps your original config as `<config>.dfw-backup` (other bundlers, and a config written as a function with a body: printed instructions). Without it, comments carry selector + text.

Penpot (optional bridge; dfw's HTML and DTCG tokens stay the source of truth): `dfw tokens export --format penpot` writes a file for Penpot's Tokens panel (sets `base` + `themes/<name>`, themes, aliases); `dfw tokens import <file> [--dry-run]` brings Penpot token edits back with a diff summary (refused while the design is frozen). Screens: `dfw penpot export` renders every screen x breakpoint, and the **dfw bridge** Penpot plugin (served by the Studio on 127.0.0.1; `dfw penpot plugin` prints its URL) builds one board per screen x breakpoint, with `data-dfw-id` layer names, text layers, token-linked colours and component instances. Comment threads on those boards are live while the plugin panel is open: a new thread reaches Claude like a Studio comment, Claude's ack/fix/reply comes back as a `[dfw]` reply in the thread, resolving the thread accepts the fix and replying reopens it; **Update boards** redraws the boards in place after a fix. Penpot can be the review canvas: `dfw init --canvas penpot` (or `dfw config set review.canvas penpot`; the default is the Studio) turns on **Keep boards up to date automatically** in the panel, which then updates the boards by itself after each build, sending the designer's pending edits to dfw first; the design system page, states and themes are still reviewed in the Studio. Layer edits and token changes come back as dfw comments (`provider: "penpot"`) and a token diff when the designer presses Send (`dfw penpot import <changes.json>` for a downloaded file); dfw never overwrites its HTML from Penpot. Token names `__proto__`, `constructor` and `prototype` in a Penpot file are refused, and `.dfw/penpot/exports/` keeps the newest 20 exports. No Penpot account token or cloud call is used. Which Penpot may drive the bridge is set in the global config only: `penpot.origins` (default `["https://design.penpot.app"]`; a self-hosted one with `dfw config set penpot.origins '["https://penpot.example.com"]' --global`), and every write of the plugin carries a token the Studio makes when it starts. Mapping, data flow and limits: `core/workflow/references/penpot.md` and the job files it links.

## Telemetry and privacy

dfw can send anonymous usage counts to help decide what to improve. It is off unless the binary knows a collector address.

- **Off without an address.** A binary you build yourself, and any fork, has no address and sends nothing (`dfw telemetry status` says so). Release binaries may carry an address: the release workflow compiles it in from the repository's build secrets (`DFW_OTLP_ENDPOINT`, `DFW_OTLP_HEADERS`), so the address is not in the source.
- **What is sent:** a random install id, the dfw version, OS and CPU type, the host (Claude Code, Copilot...), command and hook names, a duration range, the exit code, the workflow mode and phase, approved gate ids, and check names with finding counts. Every text field is a name from a list built into the binary.
- **Never sent:** project names, paths, file names or contents, comments, design tokens, URLs, usernames, environment variables.
- **When:** with an address, the first command prints a notice on stderr and records nothing; later commands record. Hooks and the MCP server never print the notice, so they record nothing until a command has shown it. Events wait in `~/.config/dfw/telemetry/spool.jsonl` (at most 1 MB) and are sent at most once a minute, in the background, with a 3 second timeout.
- **See and control it:** `dfw telemetry show` prints what is waiting to be sent; `dfw telemetry off` turns it off and deletes the install id; `DO_NOT_TRACK=1` or `DFW_TELEMETRY=off` turn it off for a session, and it is always off when `CI` is set. `dfw telemetry endpoint <url>` sends to your own collector instead.

The Studio and the review proxy listen on 127.0.0.1 only, and no other part of dfw calls home. The downloads it makes are the ones listed above (the release binary, Chrome only when none is installed, axe-core, the Impeccable engine), each from a fixed host and verified before use.

## Why plain HTML/CSS as the design format

Designs are framework-neutral HTML/CSS composed from design-system components, not a proprietary canvas format. Any stack can implement them (the stack is chosen at G3), the browser renders them directly (Studio, screenshots, Impeccable and sloplint all read HTML), they diff cleanly in git, and HTML/CSS is what language models write best. The Studio adds the Figma/pencil.dev experience on top: pages, canvas, layers, inspector, direct edits and comments.

## Project files created by dfw

```
PRODUCT.md, DESIGN.md        canonical product and design docs (also read by Impeccable and Hallmark)
design/                      design.json manifest (pages, sections, screens), tokens/ (DTCG), system/ (brief.json,
                             style.json, component masters, icons, head.html), screens/, assets/,
                             approved/vN/ (frozen), handoff/vN.md
.dfw/                        state.json, config.json, decisions.jsonl, reviews/rv_*.json (feedback contract)
```

Configuration layers: plugin defaults < `~/.config/dfw/config.json` < `.dfw/config.json` < `.dfw/config.local.json`; explicit decisions in `.dfw/decisions.jsonl` take precedence when the orchestrator applies rules. By default the write gate keeps `design/approved/**`, `.dfw/state.json`, `.dfw/config.json`, `.dfw/config.local.json`, `.dfw/decisions.jsonl`, `.dfw/stack.json` and `.dfw/reviews/**` immutable for the agent's own tools (`gate.immutable`, a union across layers that a project cannot shrink): dfw commands change them. Schemas: `plugins/dfw/schemas/`.

## Repository layout

```
core/                     agent-agnostic workflow core (files the binary embeds, and the plugin launcher)
  bin/dfw, bin/dfw.cmd    plugin launchers: run the installed native binary
  studio/                 local Design Studio browser code (vanilla HTML/CSS/JS, no build) and the Penpot plugin
  schemas/                JSON Schemas: state, config, design manifest, feedback contract
  templates/              PRODUCT.md, DESIGN.md, design/ workspace, handoff, screen skeleton
  kits/base/              starter design system: 21 token-driven components + presets (crisp, soft, industrial)
  kits/styles/            12 generated style guides (OKLCH palettes, rationale, personality axes)
  assets/icons/           Lucide icon bundle (ISC, pinned tag)
  workflow/               SKILL.md, agents, phase guides, precedence + conflict policies, stack guides (host placeholders rendered at build)
  manifest.json           name, version, dependencies: source of every host's plugin manifest
native/                   the dfw binary (Rust): CLI, hooks, gates, design tools, Studio server, review proxy, MCP
                          server, browser driver, Penpot bridge (native/PORTING.md: module map)
adapters/core-copy.json   core files of plugins/dfw: shared ones once at its root, host text per host
adapters/claude-code/     Claude Code packaging: hooks + assemble.json -> plugins/dfw/hosts/claude + .claude-plugin/plugin.json
adapters/copilot/         GitHub Copilot packaging: hooks + assemble.json -> plugins/dfw/hosts/copilot + plugin.json
plugins/                  GENERATED marketplace plugins (dfw, one folder for both hosts, and the vendored skills); do not edit by hand
.claude-plugin/, .github/plugin/  GENERATED marketplaces for Claude Code and GitHub Copilot
registry/                 vendored skill registry, lock (pinned commits), xref and conflict-lint rules
overlays/                 declarative adaptations applied to vendored skills at build time
scripts/                  installer (npx), build, validate, skill update management, presets, style guides, icon vendoring, CI helper
tests/                    golden tests of the binary (tests/golden), unit tests of the scripts and browser code,
                          a browser end-to-end test of the studio, fixtures
```

Other AI coding agents can be supported by adding an adapter under `adapters/` (see `adapters/README.md`); `core/` stays shared.

## Development

The binary is the Rust crate in `native/` (Rust 1.100.0, pinned by `native/rust-toolchain.toml`). Development needs Rust and Node.js 22+ for the scripts and tests; `npm ci` installs the pinned dev tools (esbuild for the Studio bundles, the linters, the DOM the browser-code tests use).

```bash
cd native && cargo build # the debug binary native/target/debug/dfw
cargo test               # Rust unit tests and the tests that run the binary (in native/)
cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo build --release    # release build for this machine (native/target/release/dfw)
node --test tests/golden/           # golden tests: the binary against recorded results (from the repo root)
DFW_TEST_CHROME=1 node --test tests/golden/   # also the cases that render in Chrome
DFW_GOLDEN_UPDATE=1 node --test tests/golden/ # record what the binary does now, after a deliberate behaviour change; review the diff
npm test                 # unit tests of the scripts, the marketplace and the browser code
DFW_BIN=native/target/debug/dfw npm run test:e2e   # studio end-to-end (dev only: needs the playwright package)
npm run build            # assemble plugins/dfw and rebuild vendored plugins from the lock
npm run build:check      # committed plugins/ must equal a clean build
npm run validate         # structure, cross-references, no upstream hooks or hook installers, no unresolved <skill-dir>, conflict lint, claude plugin validate
npm run styles           # regenerate the 12 style guides with the binary's colour maths (fails on any senior-audit error)
npm run presets          # regenerate the neutral presets
npm run icons -- --tag <lucide tag>   # re-vendor the Lucide icon bundle
npm run bench            # check benchmark: seeded defects -> detection, false positives, time (tests/bench/README.md)
npm run knowledge -- check|sync|bump <id>   # vendored knowledge files (registry "knowledge")
```

## Maintaining external skills

```bash
node scripts/skills.mjs status
node scripts/skills.mjs check                 # what changed upstream
node scripts/skills.mjs propose impeccable    # isolated build + report in reports/proposals/
node scripts/skills.mjs apply impeccable --to <sha>   # only after you approve the report
node scripts/skills.mjs rollback impeccable
npm test && npm run build:check && npm run validate
```

Proposal reports cover: trigger/description changes, added/changed executable code and hooks, files dfw depends on (cross-reference check), new conflict-lint hits, and instruction size. A weekly GitHub Action (`.github/workflows/upstream-check.yml`) opens one PR per update (merging is the approval) and starts `validate.yml` on it, or opens an issue when an update is blocked (one issue per upstream commit). After merging: `claude plugin marketplace update dfw-marketplace` and `/reload-plugins`.

When you change `core/` or an adapter, run `npm run build`; a change to files the binary embeds (`core/studio`, `templates`, `kits`, `assets`, `workflow`) also needs a new `cargo build` (`DFW_PLUGIN_ROOT=core` makes a debug binary read them from the source instead). Bump `version` in `core/manifest.json`, `package.json`, `package-lock.json`, `native/Cargo.toml` and `native/Cargo.lock` together (`node scripts/validate.mjs` fails when they differ) so installed copies update. A release tag builds binaries only after the `verify` job passes: the tagged commit is on `main`, `npm test`, `cargo test` and the golden tests pass and `build --check` is clean; then each runner builds its own target with cargo and the release gets `SHA256SUMS`. Workflow actions are pinned by commit SHA.

## Known limitations

- Release binaries are not code-signed. Linux binaries need glibc 2.39 or newer (they are built on Ubuntu 24.04). The Windows binary and `bin\dfw.cmd` are built and smoke-tested in CI only, not on a real Windows machine.
- Version 0.5.x was the last Node.js-based dfw. Reinstalling 0.5.x (`DFW_VERSION=0.5.2 sh install.sh`) brings the Node runtime back; project files are unchanged between the two.
- The write gate covers Edit/Write/MultiEdit/NotebookEdit. Shell commands could still write files; the skill forbids it, but it is not technically enforced.
- Impeccable is not fully offline. dfw itself runs only `impeccable detect` (no network call observed) and, once, the engine download from GitHub (sha256 pinned in this repo; dfw verifies the binary again before every run). Telemetry and the update check are off. The other calls below happen only when the Impeccable skill, or a user, runs more of it. "Observed" means seen at runtime in the network audit of engine 0.1.23 (2026-11-16); "read" means taken from the skill text or the engine's strings and not exercised.
  - Observed: `impeccable.style/api/roll`, an anonymous card-catalog request from `concept-seed` (Impeccable's own new-design flow). It carries a random key and no project data; no variable turns it off.
  - Observed: `impeccable.style/api/commands` from `impeccable help`; no variable turns it off. Neither dfw nor the skill text runs `help`.
  - Observed, and off under dfw: `impeccable.style/api/version` (update check in `context`) and `impeccable.style/api/chosen` (choice ping after `concept-seed`).
  - Read: card images under `impeccable.style/worlds/cards/`, fetched only when the agent or a browser opens the URLs `concept-seed` prints.
  - Read: `api.openai.com` from `generate-image` when `OPENAI_API_KEY` is set and the host has no image tool.
  - Read: `fonts.googleapis.com` from the pages the engine builds for its own screenshots (`comp-*`, `component-review`).
  - Read: `api.impeccable.style` for account features (`login`, `health`, `critiques`, `setup`, `loop connect`), which can upload scan counts and saved critiques once signed in; the skill bundle from `impeccable.style` on `impeccable install`/`update`; a GitHub release zip on `impeccable loop install`.
  - Local only: `live` commands and the overlay step of `critique` open a server on `localhost`; `detect <url>` (`dfw check --url`) loads your own dev server.
- The Impeccable engine is a closed binary that runs with your user's rights, not in a sandbox. dfw runs it only from `~/.impeccable/bin/<version>/` and only when its sha256 matches `engine-checksums.txt`. To run an engine you built yourself (`IMPECCABLE_BIN`, `~/.impeccable/bin/impeccable`, or `impeccable` on `PATH`), opt in for your user: `dfw config set impeccable.allowUnverifiedEngine true --global`. A project config cannot grant this.
- `env` in a project's `.dfw/config.json` or `.dfw/config.local.json` is limited to `NO_COLOR`, `FORCE_COLOR`, `TZ`, `LANG` and `LC_ALL`; other keys are ignored with a warning, so an opened repository cannot switch telemetry back on or choose what dfw runs. Set anything else in the global config (`dfw config set env.<NAME> <value> --global`).
- Canvas drag and resize are constrained: you can reorder elements among their flex/grid siblings and resize with snapping to grid spans and tokens, but free-form absolute positioning is intentionally not supported (designs are responsive flow layouts that become real code). Dropping into a container works only for an empty flex/grid container with a `data-dfw-id`; other layout changes (new grid templates, nested containers, a component master's own layout) go through comments or the code. Colors and typography are changed in the design system (tokens or masters), by Claude or by hand.
- The Studio and the `dfw review` proxy serve the local user only. A request whose `Host` is not the server's own local address (Studio: `127.0.0.1:<port>` or `localhost:<port>`; review: `localhost`, `127.0.0.1` or `[::1]`) gets 403, and so does a write (any method other than GET, HEAD or OPTIONS) whose `Origin` is another site. Without this, a web page you visit could post comments the agent reads as yours, or edit design files. Opening them through another hostname, a tunnel or a reverse proxy therefore does not work.
- `dfw system init --style <id>` stops when `design/tokens/tokens.json` already holds a design system; `--force` replaces the tokens, fonts and motif master.
- Penpot is an optional bridge, not a second source of truth: Penpot edits return as comments for the agent to apply, not as HTML. The plugin follows Penpot's documented plugin API but is tested here on fixtures and a fake of that API only (no running Penpot in CI). Live comments need the plugin panel open; replies appear under the Penpot user who has it open.
