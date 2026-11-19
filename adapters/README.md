# Agent adapters

The workflow core (`core/`) is agent-agnostic: the `dfw` CLI, Design Studio, schemas, templates, checks and the workflow documents. Each adapter packages it for one AI coding agent.

| Adapter | Status | Output |
|---|---|---|
| `claude-code/` | Supported | `plugins/dfw` (Claude Code plugin: skill, agents, hooks), assembled by `npm run build` |
| `copilot/` | Supported | `plugins/dfw-copilot` (GitHub Copilot plugin named `dfw`: skill, `*.agent.md` agents, `hooks.json` v1), listed in `.github/plugin/marketplace.json` |
| OpenCode, Codex, Cursor | Planned | entries in `native/src/core/host.rs` with status `planned` |

The skill (`core/workflow/SKILL.md`), agents and references are shared; host wording uses placeholders (`{{SKILL_DIR}}`, `{{ASK_TOOL}}`...) or `<!-- host:<id> -->` blocks rendered at build time by `scripts/lib/host.mjs` (the binary renders the MCP guide with the same table in `native/src/core/host.rs`; `tests/golden/host-text.golden.mjs` keeps the two equal).

Adding a host: (1) an entry in `native/src/core/host.rs` (status, CLI, placeholder values, hook input/output differences) and its text in `scripts/lib/host.mjs`; (2) `adapters/<host>/assemble.json` (host, manifest location, the adapter's own copy list; the core copy list shared by all hosts is `adapters/core-copy.json`) + `plugin/` with that host's hook config calling `dfw hook <name> --host <host>` (if the host has no pre-write hook, the gate is instruction-only and must be documented as such); (3) a `local` entry in `registry/skills.json` with the host's marketplace file; (4) an installer branch in `native/src/tools/install.rs`; (5) tests in `native/src/core/host.rs` and the golden tests. Keep host-specific wording out of `core/`.
