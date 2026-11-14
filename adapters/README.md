# Agent adapters

The workflow core (`core/`) is agent-agnostic: the `dfw` CLI, Design Studio, schemas, templates, checks and the workflow documents. Each adapter packages it for one AI coding agent.

| Adapter | Status | Output |
|---|---|---|
| `claude-code/` | Supported | `plugins/dfw` (Claude Code plugin: skill, agents, hooks), assembled by `npm run build` |
| `copilot/` | Supported | `plugins/dfw-copilot` (GitHub Copilot plugin named `dfw`: skill, `*.agent.md` agents, `hooks.json` v1), listed in `.github/plugin/marketplace.json` |
| OpenCode, Codex, Cursor | Planned | entries in `core/lib/host.mjs` with status `planned` |

The skill (`core/workflow/SKILL.md`), agents and references are shared; host wording uses placeholders (`{{SKILL_DIR}}`, `{{ASK_TOOL}}`...) or `<!-- host:<id> -->` blocks rendered at build time from `core/lib/host.mjs`.

Adding a host: (1) an entry in `core/lib/host.mjs` (status, CLI, placeholder values, hook input/output differences); (2) `adapters/<host>/assemble.json` + `plugin/` with that host's hook config calling `dfw hook <name> --host <host>` (if the host has no pre-write hook, the gate is instruction-only and must be documented as such); (3) a `local` entry in `registry/skills.json` with the host's marketplace file; (4) an installer branch in `core/lib/install.mjs`; (5) tests in `tests/unit/host.test.mjs`. Keep host-specific wording out of `core/`.
