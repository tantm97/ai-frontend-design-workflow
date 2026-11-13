# Quality rubric and self-critique loop

The user must never see a first draft. Every design-system page and every page of screens goes through **two critique rounds** before it is shown: render → score → fix → render → score → fix.

## Loop
1. Render: `dfw shoot --system` or `dfw shoot --page <id> --states` (primary breakpoint + smallest one).
2. Read every screenshot. Score each criterion (0-10 rows) 0-2 (0 = fails, 1 = acceptable, 2 = strong). Write the scores and the 3 worst problems in one short block (for yourself; not to the user unless asked).
3. Fix the 3 worst problems, preferring design-system fixes (token or master) over per-screen fixes.
4. Round 2: render and score again. Anything still at 0 is fixed or reported to the user as a known issue.
5. Optional third opinion: `impeccable:impeccable` `critique` on the composed page, or the `dfw:visual-reviewer` agent for a full report.

## Criteria
| # | Criterion | 2 = strong looks like |
|---|---|---|
| 0 | Concept | You can name the concept from the screen alone (materials, place, domain objects); palette names and the signature motif follow it; it would not fit a random other product. |
| 1 | Hierarchy | One obvious focal point per view; the primary action is unmistakable; secondary info recedes (size, weight, color), not everything bold. |
| 2 | Type | 2 families max (+ mono for ids); display sizes used for titles/key numbers with real contrast to body (e.g. 56/20/14/12), not 16/15/14; eyebrows for section openers; numbers tabular. |
| 3 | Spacing rhythm | Spacing only from the scale; related things close, groups clearly separated; consistent padding in like containers; aligned edges (shared left lines). |
| 4 | Density fit | Matches the users (operators: large targets, few items, big feedback; analysts: dense tables) — not generic airy SaaS for everything. |
| 5 | Color discipline | Neutral by default; accent only for primary action/selection; status colors only for status; no decorative gradients; contrast passes. |
| 6 | Component consistency | Every screen uses the same `app-shell` (navigation, header, frame); same thing looks the same everywhere (buttons, badges, tables); no one-off styles; icons from one set, one stroke width, consistent size. |
| 7 | Content realism | Real names, long values, edge cases, empty/error states designed; no lorem, no "John Doe", no fake perfect numbers. |
| 8 | Craft details | Optical alignment of icons and text, no orphaned words in headings, no clipped text, no double borders, no cards inside cards, consistent radii. |
| 9a | Space and anchors | No voids or overlaps (`dfw layout check`); task screens fill the viewport, feedback and primary action stay put with long data. |
| 9 | Responsiveness | Every breakpoint intentional (reflow, not squeeze); nothing overflows; primary action reachable. |
| 10 | Accessibility | Focus visible, 24px+ targets (44px on touch/counter), labels, no color-only meaning, contrast. |

## "AI look" tells to remove
- Screens of one product with different frames (a rail here, a sidebar there, different headers).
- Generic grey/white neutrals and black shadows with no relation to the concept.
- Palette named only by role (primary/secondary) with no usage rules.
- Everything at body size: no display type anywhere.
- Every element in its own rounded card with a soft shadow; cards inside cards.
- Gradient blobs, glassmorphism, glowing accents, purple-blue gradients by default.
- Thick colored left border on cards/alerts ("side-tab").
- Emoji or hand-drawn icons; mixed icon styles.
- All text the same size; everything semibold; centered everything.
- Pill badges in many colors with no meaning; status by color only.
- Generic hero-ish headers on product screens; filler subtitles ("Manage your items efficiently").
- Uniform 24px padding everywhere regardless of hierarchy.
- Placeholder data that is too clean (round numbers, identical lengths).
