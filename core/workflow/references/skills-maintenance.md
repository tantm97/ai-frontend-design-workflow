# Skill maintenance (external skills)

External skills are vendored, commit-pinned copies built from the dfw-marketplace repository. The repository path is in the global config (`dfw config get marketplaceRepo`; set it with `dfw setup --marketplace-repo <path>`). Work happens there, delegated to the `dfw:skill-steward` agent.

| Request | Command (run in the marketplace repo) |
|---|---|
| Status | `node scripts/skills.mjs status` |
| Check upstream for updates | `node scripts/skills.mjs check` |
| Prepare an update (no changes applied) | `node scripts/skills.mjs propose <id>` -> report in `reports/proposals/` |
| Apply after the user approves | `node scripts/skills.mjs apply <id>` (updates the lock, rebuilds, validates, logs history) |
| Roll back | `node scripts/skills.mjs rollback <id> [--to <sha>]` |
| History | `node scripts/skills.mjs history [id]` |
| Validate everything | `npm run validate` |

## Rules
1. Never apply an update without the user's approval. Present the proposal report: changed descriptions (trigger scope), added/removed files, **new or changed executable code and hooks (security-relevant)**, broken references used by dfw, new stack-imposing or conflicting instructions, size change.
2. A proposal with blocking issues (broken references, overlay patches that no longer apply, new hooks) is not applied; propose an overlay/policy fix first and show it.
3. After applying: commit in the marketplace repo (ask before committing/pushing), then the user runs `claude plugin marketplace update dfw-marketplace` and `/reload-plugins` (or restarts Claude Code).
4. Rollback restores the previous lock entry and rebuilds; commit it the same way.
5. Do not edit vendored files under `plugins/<vendored>/` by hand; change `overlays/<id>.json` and rebuild. `plugins/dfw` is generated too: edit `core/` or `adapters/claude-code/plugin/`.
