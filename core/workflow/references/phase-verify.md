# Phase: verify (gate G4)

Goal: evidence that the implementation matches the approved design and quality floor; the user accepts the result or each deviation.

1. Run the app locally (ask the user for the command if unknown). Spawn `dfw:visual-reviewer` with: the approved version, the list of screens with their routes (handoff section 1), breakpoints, and the dev server URL.
2. The reviewer produces, per screen and breakpoint:
   - design screenshot (`dfw shoot --screen <id> --variant <approved>`) vs implementation screenshot (`dfw shoot --url <route> --out .dfw/shots/impl/<screen>__{bp}.png`), and a side-by-side assessment (layout, spacing, typography, color, components, states);
   - `dfw check --url <route>` (Impeccable rendered rules, and axe on the live page, which also clicks stateful controls: `dead-control` = a toggle/tab/sort that announces a state but changes nothing) and dfw lint on changed CSS;
   - `dfw diff <design.png> <impl.png> --out .dfw/shots/diff/<screen>__<bp>.png` for each pair: the percentage and the red areas show where to look first. It is triage, not a verdict (real data and font rendering always differ);
   - optional, only when the user agrees (one-time ~20 MB download through npx): `dfw check --url <route> --lighthouse` for performance, best practices and SEO scores;
   - `references/web-interface-guidelines.md` Performance and Hydration sections against the implementation;
   - accessibility: keyboard path through the main workflow, focus visibility, contrast, target sizes, labels;
   - token discipline: no hard-coded colors/sizes in changed files;
   - interactions from the handoff section 4.
   - check the defects that actually happen, most frequent first: content stuck off-screen (an animation or reveal that never fired; scroll the whole page), text overflow or clipping at a breakpoint (large type on mobile), hover and focus states not responding, contrast failures (especially the accent in each theme), horizontal scroll on mobile, console errors, links to routes that do not exist, motion ignoring `prefers-reduced-motion`. When something looks wrong, measure it in the page (`getComputedStyle`, `getBoundingClientRect`) instead of re-reading the CSS.
   - the reviewer looks at the screenshots cold (it did not write the code); one round of real defects, fix, re-check what changed, stop.
3. Live review with the user: `dfw review start --url <dev server url>` prints a local URL (127.0.0.1) that proxies the app and adds a **Comment** button (or C): the user clicks an element on the running app and writes. Keep `dfw comments wait` running in the background as in the Studio; the hooks deliver these comments too (variant `live`, the route as screen). For each comment open `target.source` (`file:line:col` from code-inspector-plugin) and fix it there; when `source` is null (no inspector), find the element from `target.selector`, `target.text` and `url`. Then `dfw comments fixed <id> --message ...`; the user verifies in the app. HMR passes through the proxy; if a page does not update, reload it. `dfw review stop` when done.
4. Classify differences: **match**, **acceptable variance** (framework rendering, real data), **deviation** (needs fix or approval). Fix deviations that are implementation mistakes. Material deviations that would change the approved design need the user's decision (accept and record, or fix).
5. Present the report with screenshots paths, and a "not verified" line for every check that did not run. When the user accepts: `dfw approve G4 --note "<summary, accepted deviations>"`, then `dfw phase done`.
6. Maintenance: update DESIGN.md "Decisions" with anything learned; keep `.dfw/reviews` and `decisions.jsonl` (they are the project's design history).
