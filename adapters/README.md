# Agent adapters

The workflow core (`core/`) is agent-agnostic: the `dfw` CLI, Design Studio, schemas, templates, checks and the workflow documents. Each adapter packages it for one AI coding agent.

| Adapter | Status | Output |
|---|---|---|
| `claude-code/` | Supported | `plugins/dfw` (Claude Code plugin: skill, agents, hooks), assembled by `npm run build` |

Adding an agent: create `adapters/<agent>/` with that agent's instruction format (skill/rules file), its way of enforcing the write gate (hooks or equivalent; if the agent has no pre-write hook, the gate is instruction-only and must be documented as such), and an `assemble.json`. Keep agent-specific wording out of `core/`.
