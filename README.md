# AI Frontend Design Workflow

**An AI-powered UI/UX and frontend design workflow for AI coding agents, starting with Claude Code.** It takes a product from requirements to an approved visual design to working frontend code: UX research, design direction, a local visual design studio with click-to-comment review, design and technology approval gates, implementation in the stack you choose (React, Vue, Angular, Svelte, plain HTML...), and verification against the approved design.

```
discovery -> direction (G1) -> visual concepts + click-to-comment review (G2: design frozen)
          -> technology approval (G3) -> implementation -> verification (G4)
```

The workflow is called **dfw** (design-first workflow): `/dfw` in Claude Code, `dfw` on the command line.

- **Design before code.** Designs are framework-neutral HTML/CSS screens + W3C DTCG tokens in `design/`, rendered in a local **Design Studio** (multi-screen canvas, variants, breakpoints, themes/states) where you click any element and leave a comment that Claude receives with selector, source line, accessible name and bounding box.
- **Hard gates.** A PreToolUse hook blocks implementation files until the design is approved (G2) and you approved the stack (G3). Approved designs are frozen in `design/approved/vN/` with a sha256 lock.
- **No silent stack choices.** The design phase never assumes React/Tailwind/etc.; the stack is decided at G3 (existing projects keep their stack unless you approve a change).
- **One orchestrator, curated skills.** dfw routes to one lead + at most two supporting skills per phase, with written precedence rules and conflict rulings.
- **Claude is the only paid service.** Everything else is open source and local. Telemetry and update checks of bundled tools are off.

## What is in this repository

| Plugin | Origin | Role in dfw |
|---|---|---|
| `dfw` | original (MIT), assembled from `core/` + `adapters/claude-code/` | Orchestrator skill `/dfw`, CLI, hooks, Design Studio, agents (design-director, visual-reviewer, skill-steward), policies |
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
- Optional: Playwright + Chromium for screenshots (`dfw setup --playwright`, installs a pinned version into `~/.config/dfw/runtime`).

## Install

```bash
# 1. Add this repository as a marketplace (local path or GitHub owner/repo)
claude plugin marketplace add tantm97/ai-frontend-design-workflow   # or a local clone path

# 2. Install the orchestrator and the skills it routes to (user scope = all projects)
for p in dfw frontend-design ux-research ui-ux-pro-max impeccable emil-design-eng hallmark taste-reference; do
  claude plugin install "$p@dfw-marketplace"
done

# 3. In Claude Code: /reload-plugins (or restart). Then, from any project:
dfw setup --marketplace-repo /path/to/your/clone   # optional: lets dfw find the skill maintenance scripts
dfw check --setup        # downloads Impeccable's checksum-verified engine once (GitHub releases)
dfw setup --playwright   # optional: screenshots for review/verification
dfw doctor               # environment report
```

If you already installed any of these skills from their original marketplaces, disable those copies (`dfw doctor` warns about duplicates), otherwise both load.

Per project you can turn plugins off in `.claude/settings.json`:

```json
{ "enabledPlugins": { "hallmark@dfw-marketplace": false } }
```

## Use

Open Claude Code in a project and ask, for example:

> /dfw Design a professional inventory management application based on these requirements, screenshots and brand assets.

dfw asks your communication language once per session (artifacts stay in English), detects whether the project is new or existing, and walks the phases. Modes: `new`, `existing`, `design-only`, `implement` (approved design), `refine`, `quick`, `review`, `skills`.

Review loop: Claude starts the Studio (`dfw studio start`, default http://127.0.0.1:4600). Press **C**, click an element (hold **Alt** for the exact element), write the change. Tell Claude to apply the comments; fixed comments appear under **To verify** where you resolve or reopen them.

Useful commands (`dfw help` for all): `dfw status`, `dfw check`, `dfw comments list`, `dfw decisions`, `dfw shoot`, `dfw pause|resume`.

## Project files created by dfw

```
PRODUCT.md, DESIGN.md        canonical product and design docs (also read by Impeccable and Hallmark)
design/                      design.json manifest, tokens/ (DTCG), styles/, components/, screens/, assets/,
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
  workflow/               phase guides, precedence + conflict policies, stack guides
adapters/claude-code/     Claude Code packaging: skill, agents, hooks, plugin.json (+ assemble.json)
plugins/                  GENERATED marketplace plugins (dfw + vendored skills); do not edit by hand
registry/                 vendored skill registry, lock (pinned commits), xref and conflict-lint rules
overlays/                 declarative adaptations applied to vendored skills at build time
scripts/                  build, validate, skill update management, CI helper
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
- Penpot import/export, a live token editor, and an MCP server for feedback are planned (Phase 2/3).
