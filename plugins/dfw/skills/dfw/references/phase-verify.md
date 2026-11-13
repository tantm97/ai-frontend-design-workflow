# Phase: verify (gate G4)

Goal: evidence that the implementation matches the approved design and quality floor; the user accepts the result or each deviation.

1. Run the app locally (ask the user for the command if unknown). Spawn `dfw:visual-reviewer` with: the approved version, the list of screens with their routes (handoff section 1), breakpoints, and the dev server URL.
2. The reviewer produces, per screen and breakpoint:
   - design screenshot (`dfw shoot --screen <id> --variant <approved>`) vs implementation screenshot (`dfw shoot --url <route> --out .dfw/shots/impl/<screen>__{bp}.png`), and a side-by-side assessment (layout, spacing, typography, color, components, states);
   - `dfw check --url <route>` (Impeccable rendered rules) and dfw lint on changed CSS;
   - accessibility: keyboard path through the main workflow, focus visibility, contrast, target sizes, labels;
   - token discipline: no hard-coded colors/sizes in changed files;
   - interactions from the handoff section 4.
3. Classify differences: **match**, **acceptable variance** (framework rendering, real data), **deviation** (needs fix or approval). Fix deviations that are implementation mistakes. Material deviations that would change the approved design need the user's decision (accept and record, or fix).
4. Present the report with screenshots paths. When the user accepts: `dfw approve G4 --note "<summary, accepted deviations>"`, then `dfw phase done`.
5. Maintenance: update DESIGN.md "Decisions" with anything learned; keep `.dfw/reviews` and `decisions.jsonl` (they are the project's design history).
