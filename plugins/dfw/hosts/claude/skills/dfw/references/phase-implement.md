# Phase: implement

Preconditions (enforced by the hook): approved design (G2) for the current version, approved stack (G3), phase `implement`.

Inputs: `design/handoff/vN.md` (complete its TODO sections with the user first), `design/approved/vN/` (source of truth; never edit), `.dfw/stack.json`, the stack guide `${CLAUDE_SKILL_DIR}/stacks/<stack>.md`.

Skills (budget): no aesthetic skill leads here; the approved design is the authority. Support only when needed: `emil-design-eng:animate` for motion in scope; `ux-research:ux-copy` for copy not covered by the design.

## Steps
1. Plan the work per screen and component; map each design component to an existing codebase component (reuse first) or a new one. Write the mapping into the handoff table.
2. If G3 approved `code-inspector-plugin`: `dfw inspector setup --write` (edits the bundler config and package.json devDependencies), then the project's install command. For bundlers dfw does not edit (it says so), apply the printed snippet by hand.
3. Tokens: generate platform tokens from `design/approved/vN/tokens/tokens.json` (Style Dictionary or the stack guide approach) or reuse the existing theme by mapping names; never paste raw values into components.
4. Build components with all states (default, hover, focus-visible, active, disabled, loading, empty, error) and accessible semantics (labels, roles, keyboard, focus order). Props mirror the design system's explicit variants (`variant="danger"`), not stacks of booleans (`isDanger isSmall isGhost`). Apply `references/web-interface-guidelines.md` (Interactions, Forms, Performance, Hydration) while building.
5. Build screens with the approved variant; responsive rules from the handoff; behavior from the handoff section 4 (validation, loading, errors, permissions).
6. Follow project conventions (lint, formatting, folder structure, naming, tests). Do not rewrite unrelated code. Do not add dependencies beyond the approved list; if one becomes necessary, stop and ask (G3 is re-opened by `dfw stack set`).
7. If the approved design cannot be implemented as drawn (technical constraint), do not improvise a new look: describe the deviation, propose the closest option, and ask. Material changes go through `dfw reopen`.
8. Run the project's tests/linters. Then `dfw phase verify`.
