# AI Frontend Design Workflow

**An AI-powered UI/UX and frontend design workflow for AI coding agents, starting with Claude Code.** It takes a product from requirements to an approved visual design to working frontend code: UX research, a reasoned design direction, a design system built first, a local Figma-like design studio with live click-to-comment review, design and technology approval gates, implementation in the stack you choose (React, Vue, Angular, Svelte, plain HTML...), and verification against the approved design.

```
discovery -> direction (G1) -> design system (DS) -> pages of screens + review (G2: design frozen)
          -> technology approval (G3) -> implementation -> verification (G4)
```

## Install (one command)

```bash
npx github:tantm97/ai-frontend-design-workflow
```

This adds the `dfw-marketplace` to Claude Code, installs `dfw` (its plugin dependencies pull in the seven design skills), installs the pinned screenshot runtime and the Impeccable engine (telemetry off), and runs `dfw doctor`. Options: `--scope project` (share via `.claude/settings.json`), `--no-runtime`, `uninstall`. Then restart Claude Code (or `/reload-plugins`) and type `/dfw`.

Inside Claude Code without npx: `claude plugin marketplace add tantm97/ai-frontend-design-workflow` then `claude plugin install dfw@dfw-marketplace`.

The workflow is called **dfw** (design-first workflow): `/dfw` in Claude Code, `dfw` on the command line.

- **Design system first, like Figma.** Tokens (W3C DTCG) and component masters with every variant and state come first; screens are composed from component instances (`<dfw-use>`), so changing a master updates every screen. A token-driven starter kit (21 components incl. a type component, 3 neutral presets) plus 12 curated style guides (concept, named palette with usage rules, display type, motifs) and a curated icon set (Lucide) make the first version fast.
- **Pages and a canvas.** Screens are grouped in pages per module. The local **Design Studio** shows the Design System page and every page as an infinite canvas of frames (screen × variant × breakpoint × state/theme): pan/zoom, layers, an inspector that shows which tokens and component an element uses, direct edits written back to the source (with undo), a full-window **Present** mode and an **Interact** tool for clickable prototypes, and click-to-comment review. Pages are laid out like a pencil.dev file: a section per module with rows per breakpoint, variants, states and annotation notes.
- **Live review.** Comments you leave in the Studio reach Claude directly (a background `dfw comments wait`, plus hooks on your next message and before Claude stops); each comment shows its live status: waiting → Claude is working → fixed, please verify.
- **Senior design reasoning, not taste by accident.** A design brief (use context + 7 personality axes) ranks 12 curated style guides (`dfw style match`). Palettes are generated as perceptual OKLCH ramps with roles picked by contrast targets (`dfw palette build`), type follows a modular scale (`dfw type scale`), and `dfw system audit` reviews the system like a senior designer: contrast, accent vs error/success hues, neutral temperature, hue budget, type rhythm, usage rules per colour, and a written rationale for every decision (required at the DS gate).
- **Quality loop.** Two self-critique rounds against a rubric before you see anything, 2-3 options where a real choice exists, and automated checks (dfw lint, Impeccable, Hallmark sloplint) on the composed HTML.
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

Vendored plugins are built from pinned commits (`registry/skills.lock.json`) with declarative overlays (`overlays/`); upstream files are never edited by hand. See `THIRD_PARTY_NOTICES.md`.

## Requirements

- Claude Code with a Claude subscription (no API key needed).
- Node.js 20+ (Node 22+ for Taste's copy-lint), Python 3 (UI UX Pro Max search), git.
- The installer downloads a pinned Playwright + Chromium into `~/.config/dfw/runtime` (screenshots) unless `--no-runtime`.

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
| Clickable preview | **P** (or double-click a frame title) for full-window Present; **I** to interact on the canvas; ← → between screens |
| Comment | **C**, click an element, write (Cmd/Ctrl+Enter). Claude receives it immediately; fixed comments appear under **To verify** where you resolve or reopen them |

Useful commands (`dfw help` for all): `dfw status`, `dfw style match|show|apply`, `dfw system init|options|choose|audit|list`, `dfw palette build`, `dfw type scale`, `dfw icons search|add`, `dfw fonts check`, `dfw check`, `dfw shoot --page <id> --states`, `dfw comments list|wait`, `dfw decisions`, `dfw pause|resume`.

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
  workflow/               phase guides, precedence + conflict policies, stack guides
adapters/claude-code/     Claude Code packaging: skill, agents, hooks, plugin.json (+ assemble.json)
plugins/                  GENERATED marketplace plugins (dfw + vendored skills); do not edit by hand
registry/                 vendored skill registry, lock (pinned commits), xref and conflict-lint rules
overlays/                 declarative adaptations applied to vendored skills at build time
scripts/                  installer (npx), build, validate, skill update management, presets, style guides, icon vendoring, CI helper
tests/                    unit tests and a browser end-to-end test of the studio
```

Other AI coding agents can be supported by adding an adapter under `adapters/` (see `adapters/README.md`); `core/` stays shared.

## Development

```bash
npm test                 # unit tests
npm run test:e2e         # studio end-to-end (needs dfw setup --playwright)
npm run build            # assemble plugins/dfw and rebuild vendored plugins from the lock
npm run build:check      # committed plugins/ must equal a clean build
npm run validate         # structure, cross-references, no upstream hooks, conflict lint, claude plugin validate
npm run styles           # regenerate the 12 style guides (fails on any senior-audit error)
npm run presets          # regenerate the neutral presets
npm run icons -- --tag <lucide tag>   # re-vendor the Lucide icon bundle
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

When you change `core/` or the Claude Code adapter, run `npm run build` and bump `version` in `adapters/claude-code/plugin/.claude-plugin/plugin.json` so installed copies update.

## Known limitations

- The write gate covers Edit/Write/MultiEdit/NotebookEdit. Shell commands could still write files; the skill forbids it, but it is not technically enforced.
- Impeccable's direction catalog fetch (impeccable.style) still happens when its `shape`/new-design flow runs; it sends no project data.
- Visual comparison of design vs. implementation is screenshot-based with Claude's judgement; there is no pixel-perfect diff.
- Element source mapping in a *running app* (not the design studio) is not in the MVP; it is planned via code-inspector-plugin.
- No free-form drag-and-drop or resizing on the canvas: layout changes go through comments or the code. Direct edits in the Studio cover text, component variant/size/props and spacing tokens; colors and typography are changed in the design system (tokens or masters), by Claude or by hand.
- Penpot import/export and an MCP server for feedback are planned.
