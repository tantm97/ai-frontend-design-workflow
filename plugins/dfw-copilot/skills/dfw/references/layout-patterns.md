# Layout patterns (no overlap, no voids, no drifting feedback)

Every screen = `app-shell` (one frame for the product) + `page` (one of three layout behaviours) + components. Screens never invent their own frame, paddings or scrolling.

| Pattern | `page` variant | Use for | Behaviour |
|---|---|---|---|
| Task | `task` | one job at a time: counter, kiosk, checkout, scanning, wizard step | Fits the viewport exactly (inside `app-shell variant="focus"`). The work object (usually a raised card with `body="scroll"`) stretches to the available height; lists scroll **inside** it; the primary action sits in the object's footer and the `feedback` slot is anchored under it. Nothing moves when data grows. Spare height is given a job (e.g. "scan the next item" hint), never left empty. |
| Flow | `flow` | dashboards, lists, reports, settings, long forms | Page scrolls. Sections stack with the standard gap. Feedback = inline banners at the top of the affected section, or toasts for background results. |
| Split | `split` | list + detail, inbox, master/detail editing | Fixed-width list column + detail column; both scroll independently on large screens. |

## Rules
1. **Anchors.** Mark what must always be visible with `data-dfw-anchor` (the `page` feedback slot is anchored by the master; add `data-dfw-anchor="primary-action"` to the main confirm button of a task). The layout check fails if an anchor leaves the first viewport, including in stress mode.
2. **Growth.** Mark every list that can grow with `data-dfw-grow="12"` (or the realistic maximum). The layout check clones its items to prove the screen still works with long data.
3. **No voids.** A band of empty space between blocks, or a first screen that ends early, is a defect: give the space to the work object (task), tighten the grid (flow), or add meaningful content (next step, recent activity) — never decorative filler.
4. **No overlaps, no clipping.** Overlap between components is a MUST; clipped text is a SHOULD (truncate on purpose with an ellipsis or line clamp and a tooltip, never by accident).
5. **Same kind of screen, same page variant.** Two task screens (borrow, return) must look and behave the same; two flow screens share paddings and section rhythm. The layout check compares components across screens and flags drift.
6. **Feedback placement is part of the blueprint.** Decide per screen: anchored panel (task), inline banner (flow), toast (background) — and whether it persists until dismissed (errors at a counter: yes).

## Self-calibration loop (automatic)
- The Stop hook re-renders every screen whose file, masters or tokens changed (all breakpoints, states, stress) and blocks once with the findings. Fix them in the masters or the page variant first, then `dfw layout check --changed`.
- Run `dfw layout check --states` before presenting any page and before every gate.
