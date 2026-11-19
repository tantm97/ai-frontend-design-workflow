# Agent adapters

The workflow core (`core/`) is agent-agnostic: the `dfw` CLI, Design Studio, schemas, templates, checks and the workflow documents. Each adapter packages it for one AI coding agent.

| Adapter | Status | Output |
|---|---|---|
| `claude-code/` | Supported | `plugins/dfw/hosts/claude/` (skill, `*.md` agents, `hooks.json`), named by `plugins/dfw/.claude-plugin/plugin.json` (agents listed file by file); listed in `.claude-plugin/marketplace.json` |
| `copilot/` | Supported | `plugins/dfw/hosts/copilot/` (skill, `*.agent.md` agents, `hooks.json` v1), named by the legacy-format `plugins/dfw/plugin.json`; listed in `.github/plugin/marketplace.json` |
| OpenCode, Codex, Cursor | Planned | entries in `native/src/core/host.rs` with status `planned` |

One plugin folder, `plugins/dfw`, serves every supported host; `npm run build` assembles it. The files that are the same for every host (`adapters/core-copy.json` `shared`: the `bin/` launchers, schemas, templates) are copied once to its root. The host text (`hostText`: the skill tree and the agents, rendered per host) and the adapter's own `plugin/` files go to `hosts/<host>/`, and only that host's manifest names them. The root holds no location a host reads by default (`skills/`, `agents/`, `hooks/`, `hooks.json`, `SKILL.md`, `.mcp.json`, `.plugin/`, `.github/plugin/plugin.json`): Claude Code always loads a root `skills/`, so one host could load the other's text. The build refuses such a path and `node scripts/validate.mjs` checks the layout (`layoutCheck` in `scripts/lib/checks.mjs`).

The skill (`core/workflow/SKILL.md`), agents and references are shared; host wording uses placeholders (`{{SKILL_DIR}}`, `{{ASK_TOOL}}`...) or `<!-- host:<id> -->` blocks rendered at build time by `scripts/lib/host.mjs` (the binary renders the MCP guide with the same table in `native/src/core/host.rs`; `tests/golden/host-text.golden.mjs` keeps the two equal).

Adding a host: (1) an entry in `native/src/core/host.rs` (status, CLI, placeholder values, hook input/output differences) and its text in `scripts/lib/host.mjs`; (2) `adapters/<host>/assemble.json` (host, `plugin: "dfw"`, manifest location, `manifestExtra` naming only `hosts/<host>/...` paths, the adapter's own copy list with targets relative to `hosts/<host>/`; the core copy list shared by all hosts is `adapters/core-copy.json`) + `plugin/` with that host's hook config calling `dfw hook <name> --host <host>` (if the host has no pre-write hook, the gate is instruction-only and must be documented as such). The manifest must sit where no other host reads one, and nothing may land at a default location of any host at the plugin root; (3) a `local` entry in `registry/skills.json` (`plugin: "dfw"`) with the host's marketplace file; (4) an installer branch in `native/src/tools/install.rs`; (5) tests in `native/src/core/host.rs` and the golden tests. Keep host-specific wording out of `core/`.
