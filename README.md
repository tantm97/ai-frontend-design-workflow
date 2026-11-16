# AI Frontend Design Workflow

**An AI-powered UI/UX and frontend design workflow for AI coding agents: Claude Code and GitHub Copilot as plugins, OpenCode, Codex and Cursor through MCP.** It takes a product from requirements to an approved visual design to working frontend code: UX research, a reasoned design direction, a design system built first, a local Figma-like design studio with live click-to-comment review, design and technology approval gates, implementation in the stack you choose (React, Vue, Angular, Svelte, plain HTML...), and verification against the approved design.

```
discovery -> direction (G1) -> design system (DS) -> pages of screens + review (G2: design frozen)
          -> technology approval (G3) -> implementation -> verification (G4)
```

## Install (one command)

No Node.js needed (downloads the single-file `dfw` binary for your OS, checks its SHA-256, then registers everything with Claude Code):

```bash
curl -fsSL https://raw.githubusercontent.com/tantm97/ai-frontend-design-workflow/main/install.sh | sh
```

With Node.js:

```bash
npx github:tantm97/ai-frontend-design-workflow
```

Both add the `dfw-marketplace` to Claude Code, install `dfw` (its plugin dependencies pull in the seven design skills), set up the Impeccable engine (telemetry off) and run `dfw doctor`. Screenshots and the layout check use the Chrome, Edge or Chromium already on your machine; only when none is found does the installer download Google's headless Chrome once. Options: `--scope project` (share via `.claude/settings.json`), `--no-runtime`, `--no-binary`, `uninstall`. Then restart Claude Code (or `/reload-plugins`) and type `/dfw`.

How dfw runs: the plugin's `bin/dfw` launcher starts the `dfw` binary for its version (`~/.config/dfw/bin/<version>/`) when present, otherwise Node.js. `dfw setup --binary` fetches the binary later; `DFW_NO_BINARY=1` forces Node.

The installer registers dfw with every supported tool it finds (`--host claude|copilot|all` to choose). Without the installer: Claude Code `claude plugin marketplace add tantm97/ai-frontend-design-workflow` then `claude plugin install dfw@dfw-marketplace`; GitHub Copilot CLI `copilot plugin marketplace add tantm97/ai-frontend-design-workflow` then `copilot plugin install dfw@dfw-marketplace` (and the seven skills the same way). OpenCode, Codex, Cursor: `dfw mcp install opencode|codex|cursor|all` adds dfw's MCP server (workflow guide, CLI and a write-gate check) to that tool's config (`--print` to preview). MCP cannot block the tool's own file writes, so the approval gates are advisory there.

The workflow is called **dfw** (design-first workflow): `/dfw` in Claude Code, `dfw` on the command line.

- **Design system first, like Figma.** Tokens (W3C DTCG) and component masters with every variant and state come first; screens are composed from component instances (`<dfw-use>`), so changing a master updates every screen. A token-driven starter kit (21 components incl. a type component, 3 neutral presets) plus 12 curated style guides (concept, named palette with usage rules, display type, motifs) and a curated icon set (Lucide) make the first version fast.
- **Pages and a canvas.** Screens are grouped in pages per module. The local **Design Studio** shows the Design System page and every page as an infinite canvas of frames (screen × variant × breakpoint × state/theme): pan/zoom, layers, an inspector that shows which tokens and component an element uses, direct edits written back to the source (with undo), a full-window **Present** mode and an **Interact** tool for clickable prototypes, and click-to-comment review. Pages are laid out like a pencil.dev file: a section per module with rows per breakpoint, variants, states and annotation notes.
- **Live review.** Comments you leave in the Studio reach Claude directly (a background `dfw comments wait`, plus hooks on your next message and before Claude stops); each comment shows its live status: waiting → Claude is working → fixed, please verify.
- **Senior design reasoning, not taste by accident.** A design brief (use context + 7 personality axes) ranks 12 curated style guides (`dfw style match`). Palettes are generated as perceptual OKLCH ramps with roles picked by contrast targets (`dfw palette build`), type follows a modular scale (`dfw type scale`), and `dfw system audit` reviews the system like a senior designer: contrast, accent vs error/success hues, neutral temperature, hue budget, type rhythm, usage rules per colour, and a written rationale for every decision (required at the DS gate).
- **Quality loop.** Two self-critique rounds against a rubric before you see anything, 2-3 options where a real choice exists, and automated checks on the composed HTML:
  - dfw lint, with AI-default tells, interface guidelines and missing states;
  - a rendered layout check, including 320px with text at 125%;
  - axe-core for rendered accessibility;
  - Impeccable and Hallmark sloplint;
  - `dfw diff` for a pixel diff of design vs. implementation screenshots (pixelmatch: % changed plus a diff image).

  A check that could not run is reported as NOT VERIFIED, never as passing. `npm run bench` measures the checks on seeded defects.
- **Hard gates.** A PreToolUse hook blocks implementation files until the design is approved (G2) and you approved the stack (G3). Approved designs are frozen in `design/approved/vN/` with a sha256 lock.
- **No silent stack choices.** The design phase never assumes React/Tailwind/etc.; the stack is decided at G3 (existing projects keep their stack unless you approve a change).
- **One orchestrator, curated skills.** dfw routes to one lead + at most two supporting skills per phase, with written precedence rules and conflict rulings.
- **Claude is the only paid service.** Everything else is open source and local. Telemetry and update checks of bundled tools are off.

## What is in this repository

| Plugin | Origin | Role in dfw |
|---|---|---|
| `dfw` | original (MIT), assembled from `core/` + `adapters/claude-code/` | Orchestrator skill `/dfw`, CLI, hooks, Design Studio, starter kit + style guides + icons, agents (design-director, screen-builder, visual-reviewer, skill-steward), policies |
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
| Knowledge | [vercel-labs/web-interface-guidelines](https://github.com/vercel-labs/web-interface-guidelines) (MIT) | `references/web-interface-guidelines.md`, read in concepts, review and implement |
| Tool | [axe-core](https://github.com/dequelabs/axe-core) 4.14.0 (MPL-2.0) | Downloaded once by `dfw setup --a11y` with its npm integrity hash checked; injected into rendered screens by `dfw check` |
| Tool | [pixelmatch](https://github.com/mapbox/pixelmatch) 8.0.0 (ISC) | `core/assets/vendor/pixelmatch.js`, used by `dfw diff` |
| Tool, optional | [Lighthouse](https://github.com/GoogleChrome/lighthouse) 13.6.0 (Apache-2.0) | `dfw check --url <u> --lighthouse` at G4 only, through npx, error reporting off |

Vendored plugins are built from pinned commits (`registry/skills.lock.json`) with declarative overlays (`overlays/`); upstream files are never edited by hand. See `THIRD_PARTY_NOTICES.md`.

## Requirements

- Claude Code with a Claude subscription (no API key needed).
- The `dfw` binary, or Node.js 22+. Node is still used for the vendored skills' own scripts (Hallmark sloplint, Taste copy-lint) when present; without it those optional checks report "unavailable". Python 3 (UI UX Pro Max search), git.
- A Chromium-based browser for screenshots and the layout check: your installed Chrome / Edge / Chromium / Brave (driven headless over the DevTools protocol, no npm packages), `DFW_BROWSER=/path/to/chrome`, or `dfw setup --browser` (Google's chrome-headless-shell, once, into `~/.config/dfw/browser`). Web fonts used by designs are cached in `~/.config/dfw/cache/fonts`.

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
| Edit directly | double-click text; change variant/size/props or spacing tokens in the inspector; Cmd/Ctrl+Z undoes |
| Rearrange and resize | select an element with a `data-dfw-id`, then drag it among its flex/grid siblings (a blue line shows where it lands; the source is moved), or drag its handles: in a grid the width snaps to a column span (`span 2 / 4`), elsewhere to a `size.*` token or 25/33/50/67/75/100%, and the height to a `min-height` spacing/size token. Keyboard: Alt+arrows move, Alt+Shift+arrows step the size. Resizes go to a `<style data-dfw-layout>` block in the screen; in a narrower breakpoint frame they go inside `@media (max-width: <frame width>px)`, so they apply there and below. Edits are refused once the design is approved (G2) |
| Clickable preview | **P** (or double-click a frame title) for full-window Present; **I** to interact on the canvas; ← → between screens. Present hides comment pins unless you press **Pins** (Shift+C) or comment |
| Comment | **C**, click an element, write (Cmd/Ctrl+Enter). Claude receives it immediately; fixed comments appear under **To verify** where you resolve or reopen them |
| Breakpoints and states | top-bar menus with checkboxes and **All** (default: all shown); only checked breakpoints/states get frames |
| Screenshot | top-bar camera: pick the screens to shoot (grouped by page, all checked by default, choice remembered), at the shown breakpoints/states and theme, to PNG in `.dfw/shots/studio/<time>/`; **Screenshot** in Present (also fullscreen): the screen being shown, saved and downloaded |
| Comment pins | click a pin to read the comment and its thread, and resolve or reopen it in place. **Shift+C** (or the **Pins** chip) cycles open → all → off; resolved comments are hidden by default |

Useful commands (`dfw help` for all): `dfw status`, `dfw style match|show|apply`, `dfw system init|inventory|ref|options|choose|audit|list`, `dfw palette build`, `dfw type scale`, `dfw icons search|add`, `dfw fonts check`, `dfw check`, `dfw shoot --page <id> --states`, `dfw comments list|wait`, `dfw decisions`, `dfw pause|resume`.

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

Configuration layers: plugin defaults < `~/.config/dfw/config.json` < `.dfw/config.json` < `.dfw/config.local.json`; explicit decisions in `.dfw/decisions.jsonl` take precedence when the orchestrator applies rules. Schemas: `plugins/dfw/schemas/`.

## Repository layout

```
core/                     agent-agnostic workflow core
  bin/dfw, lib/           CLI: state machine, gates, checks, tokens, comments, freeze, handoff
  studio/                 local Design Studio (vanilla HTML/CSS/JS, no build)
  schemas/                JSON Schemas: state, config, design manifest, feedback contract
  templates/              PRODUCT.md, DESIGN.md, design/ workspace, handoff, screen skeleton
  kits/base/              starter design system: 21 token-driven components + presets (crisp, soft, industrial)
  kits/styles/            12 generated style guides (OKLCH palettes, rationale, personality axes)
  assets/icons/           Lucide icon bundle (ISC, pinned tag)
  workflow/               SKILL.md, agents, phase guides, precedence + conflict policies, stack guides (host placeholders rendered at build)
  lib/host.mjs            everything that differs between AI tools (hook formats, skill wording, install paths)
  lib/mcp.mjs             "dfw mcp": MCP server + config install for tools without a dfw plugin
  manifest.json           name, version, dependencies: source of every host's plugin manifest
adapters/claude-code/     Claude Code packaging: hooks + assemble.json -> plugins/dfw
adapters/copilot/         GitHub Copilot packaging: hooks + assemble.json -> plugins/dfw-copilot
plugins/                  GENERATED marketplace plugins (dfw, dfw-copilot, vendored skills); do not edit by hand
.claude-plugin/, .github/plugin/  GENERATED marketplaces for Claude Code and GitHub Copilot
registry/                 vendored skill registry, lock (pinned commits), xref and conflict-lint rules
overlays/                 declarative adaptations applied to vendored skills at build time
scripts/                  installer (npx), build, validate, skill update management, presets, style guides, icon vendoring, CI helper
tests/                    unit tests and a browser end-to-end test of the studio
```

Other AI coding agents can be supported by adding an adapter under `adapters/` (see `adapters/README.md`); `core/` stays shared.

## Development

```bash
npm test                 # unit tests
npm run test:e2e         # studio end-to-end (dev only: needs the playwright package, npm i -D playwright)
node scripts/build-binary.mjs   # single-file binary for this platform into dist/ (embed an official nodejs.org node: --node <path>)
npm run build            # assemble plugins/dfw and rebuild vendored plugins from the lock
npm run build:check      # committed plugins/ must equal a clean build
npm run validate         # structure, cross-references, no upstream hooks, conflict lint, claude plugin validate
npm run styles           # regenerate the 12 style guides (fails on any senior-audit error)
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

Proposal reports cover: trigger/description changes, added/changed executable code and hooks, files dfw depends on (cross-reference check), new conflict-lint hits, and instruction size. A weekly GitHub Action (`.github/workflows/upstream-check.yml`) opens one PR per update (merging is the approval) or an issue when an update is blocked. After merging: `claude plugin marketplace update dfw-marketplace` and `/reload-plugins`.

When you change `core/` or an adapter, run `npm run build` and bump `version` in `core/manifest.json` (and `package.json`) so installed copies update.

## Known limitations

- The write gate covers Edit/Write/MultiEdit/NotebookEdit. Shell commands could still write files; the skill forbids it, but it is not technically enforced.
- Impeccable is not fully offline. dfw's checks (`impeccable detect`) make no network calls, and telemetry and update checks are off. Two calls remain:
  - the one-time engine download (GitHub, sha256-verified);
  - when Impeccable's own new-design flow runs, an anonymous card-catalog request to `impeccable.style/api/roll` that carries a random key and no project data.

  Details, triggers and offline setup: [docs/impeccable-network.md](docs/impeccable-network.md).
- Element source mapping in a *running app* (not the design studio) is not in the MVP; it is planned via code-inspector-plugin.
- Canvas drag and resize are constrained: you can reorder elements among their flex/grid siblings and resize with snapping to grid spans and tokens, but free-form absolute positioning is intentionally not supported (designs are responsive flow layouts that become real code). Other layout changes (new grid templates, moving into an empty container) go through comments or the code. Colors and typography are changed in the design system (tokens or masters), by Claude or by hand.
- Penpot import/export is planned.
