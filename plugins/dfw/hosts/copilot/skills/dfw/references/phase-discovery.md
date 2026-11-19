# Phase: discovery

Goal: understand the product well enough that design decisions can be justified. Output: `PRODUCT.md` (root), assets in `design/assets/`, open questions answered or recorded as assumptions.

Skills: lead `user-research` (planning questions, research methods) and, when the user supplies research data (interviews, tickets, analytics), `research-synthesis`. Optional support: `impeccable` with the `shape` command for UX planning of a single feature (planning only: stop before its design-direction phase, which deals catalog directions; see policy R3). Do not load visual design skills yet.

## Steps

1. **Collect inputs.** Read everything the user supplied: documents, screenshots, logos, icons, brand colors, references, links. Copy brand assets into `design/assets/` (keep original names; never alter logos). For screenshots of an existing product, note what works and what fails.
2. **Existing codebase (existing-project / ui-refinement):**
   - `dfw stack detect --write` (draft for G3; do not approve yet).
   - Inspect routes/pages, shared components, styling approach, existing tokens/theme, icon set, i18n, accessibility state. Summarize in PRODUCT.md "Platforms and constraints" and DESIGN.md (current state).
   - If the UI runs locally, capture it: `dfw shoot --url http://localhost:<port>/<route>`, and seed its current tokens with `dfw tokens extract --url http://localhost:<port>/<route>` (a starting point for the design system, not the design system) (uses the installed Chrome/Edge; if `dfw doctor` finds none, ask before `dfw setup --browser`, a one-time download).
3. **Fill PRODUCT.md** (template sections): summary, roles with goals and tasks, core workflows (trigger, steps, success, failure), business rules, data shapes and extremes, platforms, brand inputs, success metrics.
4. **Gaps -> questions.** List what is missing or ambiguous. Ask the user the questions that change the design (roles/permissions, critical workflows, data volume, devices, brand constraints, must-keep elements, deadlines). Group them: one the ask_user tool call with up to 4 questions, or a short numbered list in chat for open-ended ones. Everything else becomes a documented assumption in PRODUCT.md.
5. **Domain research.** For domain-specific UX (e.g. inventory: stock levels, units, batch/lot, reorder points, barcode flows, audit trails), note the patterns users expect. Use `ui-ux-pro-max` only for its UX guideline data if helpful (`--domain ux`), not for styles yet.
6. **Competitors, donors and reviews** (skip parts the user waives; needs web access, otherwise ask the user for links):
   - **Competitors:** name 5-8 direct competitors and propose the list back to the user in one the ask_user tool ("any major one missing or wrong?"). From them note only the conventions users rely on (keep these) and the category's visual default (the look to avoid). Then put them aside: copying near neighbours reproduces their flaws.
   - **Donors:** 2-3 adjacent domains with the same user task under higher stakes (counter lending studies library and tool-crib checkout; fintech studies aviation checklists). Write the structural map: their entity -> ours, their constraint -> ours. Borrow structure, never surface.
   - **Reviews:** when the product or competitors have public reviews (app stores, G2, Trustpilot, forums), read the 2-3 star ones: unmet needs in users' words. Rank themes by severity x frequency and turn the top 3-5 into concrete feature or content recommendations.
   - Record all of it in PRODUCT.md "Research" (sources as links) and the important calls with `dfw decide --level 2`.
7. Record key confirmations: `dfw decide --level 1 --topic <topic> "<decision>"`.
8. Summarize discovery to the user in 5-10 lines and move on: `dfw phase direction`.

Do not produce visual designs in this phase.
