# Phase: stack (gate G3)

Goal: the user explicitly approves the implementation technology. Output: `.dfw/stack.json` approved.

Mandatory before implementation. May be pulled forward when the stack constrains the design (e.g. an existing component library).

## Existing projects
1. `dfw stack detect --write` (if not done). Read the result and the codebase conventions (folder structure, component patterns, styling, state management, testing).
2. Present: "This project uses <framework> with <styling> and <ui libraries>. I will implement the approved design within this stack, reusing <components>." List any new dependency you think is needed, each with purpose, license, size/maintenance, and an alternative without it.
3. Major changes (new framework, styling system, UI library, state library, migrating anything) need explicit approval; default is no.

## New projects
1. Ask which framework the user wants (AskUserQuestion). If undecided, recommend 2-3 options derived from PRODUCT.md (team skills, hosting, SEO/SSR needs, interactivity, data density, ecosystem for tables/charts, long-term maintenance), with trade-offs. Stack guides: `${CLAUDE_SKILL_DIR}/stacks/` (react, vue, angular, svelte, html); others are fine but say they have no stack guide.
2. Then styling approach (CSS modules / plain CSS with tokens / Tailwind with tokens mapped / CSS-in-JS), UI component library (none / headless / full kit), and significant dependencies (charts, tables, forms, i18n). `emil-design-eng:pick-ui-library` can be used as the single support skill for library suggestions; its picks are recommendations (level 5).
3. Do not pick for the user. If they say "you choose", make the recommendation, state it plainly, and ask for a one-word confirmation.

## Dev tooling for live review (propose with the stack)
For JavaScript projects served by a bundler, propose one dev-only dependency in the same G3 question: `code-inspector-plugin` (pinned by dfw, MIT). In development builds it tags every element with its source location (`data-insp-path="src/Foo.tsx:42:7:div"`), so comments the user leaves on the running app (`dfw review`, phase verify) point at file:line; it does nothing in production builds. Run `dfw inspector setup` and show its output (version, bundler, the exact config change). If the user declines, live review still works without file:line (selector + text). Never install it without approval: `dfw inspector setup --write` refuses until G3 is approved with the dependency listed.

## Record
- `dfw stack set --json '{"kind":"javascript","framework":{"name":"..."},"styling":[{"name":"..."}],"uiLibraries":[...],"dependencies":[{"name":"...","purpose":"...","license":"..."}],"packageManager":"...","notes":"..."}'`
- Dev tooling goes in the same `dependencies` list with `"dev":true`, e.g. `{"name":"code-inspector-plugin","version":"<from dfw inspector setup>","dev":true,"license":"MIT","purpose":"live review source mapping"}`.
- After explicit approval: `dfw approve G3 --note "<framework, styling, libraries>"`.
- Next: `dfw phase implement` (requires G2 too).
