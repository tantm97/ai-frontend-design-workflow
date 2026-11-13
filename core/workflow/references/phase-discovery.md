# Phase: discovery

Goal: understand the product well enough that design decisions can be justified. Output: `PRODUCT.md` (root), assets in `design/assets/`, open questions answered or recorded as assumptions.

Skills: lead `ux-research:user-research` (planning questions, research methods) and, when the user supplies research data (interviews, tickets, analytics), `ux-research:research-synthesis`. Optional support: `impeccable:impeccable` with the `shape` command for UX planning of a single feature. Do not load visual design skills yet.

## Steps

1. **Collect inputs.** Read everything the user supplied: documents, screenshots, logos, icons, brand colors, references, links. Copy brand assets into `design/assets/` (keep original names; never alter logos). For screenshots of an existing product, note what works and what fails.
2. **Existing codebase (existing-project / ui-refinement):**
   - `dfw stack detect --write` (draft for G3; do not approve yet).
   - Inspect routes/pages, shared components, styling approach, existing tokens/theme, icon set, i18n, accessibility state. Summarize in PRODUCT.md "Platforms and constraints" and DESIGN.md (current state).
   - If the UI runs locally, capture it: `dfw shoot --url http://localhost:<port>/<route>` (needs Playwright; ask before `dfw setup --playwright`).
3. **Fill PRODUCT.md** (template sections): summary, roles with goals and tasks, core workflows (trigger, steps, success, failure), business rules, data shapes and extremes, platforms, brand inputs, success metrics.
4. **Gaps -> questions.** List what is missing or ambiguous. Ask the user the questions that change the design (roles/permissions, critical workflows, data volume, devices, brand constraints, must-keep elements, deadlines). Group them: one AskUserQuestion call with up to 4 questions, or a short numbered list in chat for open-ended ones. Everything else becomes a documented assumption in PRODUCT.md.
5. **Domain research.** For domain-specific UX (e.g. inventory: stock levels, units, batch/lot, reorder points, barcode flows, audit trails), note the patterns users expect. Use `ui-ux-pro-max:ui-ux-pro-max` only for its UX guideline data if helpful (`--domain ux`), not for styles yet.
6. Record key confirmations: `dfw decide --level 1 --topic <topic> "<decision>"`.
7. Summarize discovery to the user in 5-10 lines and move on: `dfw phase direction`.

Do not produce visual designs in this phase.
