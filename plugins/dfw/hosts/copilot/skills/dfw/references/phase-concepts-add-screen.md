# Adding a screen to an existing design

Goal: one more screen that sits in the right module, continues the flow the user already has (something leads to it, it leads somewhere), and looks like its neighbours; not a loose frame at the end of the canvas.

Skills (budget): the concepts budget (`references/phase-concepts.md`); usually none beyond `frontend-design`, plus `ux-copy` for its copy.

Exit: the user agreed where it goes and its blueprint; the Flow page shows arrows into and out of it; the checks pass. After G2 this needs a reopen first (step 6).

Use this whenever the user asks for a screen while screens already exist (during concepts, review, or after G2), and for screens you discover are missing (a confirmation, an error page, a step of a wizard).

## 1. Place it before you draw it

- **Read the current structure.** Run `dfw blueprint list` (every screen's goal, layout, primary action) and read `pages` in `design/design.json` (modules and their screen order). Look at the Studio's **Flow** page, or the `data-dfw-goto` and links in the screens, to see which screen leads where.
- **Name the place in one line.** Which page (module) it belongs to, which screen comes before it and which after, and which screens will link to it. Example: "Orders page, between `order-list` and `order-detail`; `order-list` opens it with Review; it continues to `order-detail` on Confirm."
- **No page fits?** Propose a new page (id, title, one-line description) instead of forcing it into an unrelated module; never leave it on no page.
- **Confirm in one line** with the user before building, unless the user already said exactly where it goes. A wrong place costs a rebuild of the flow.

## 2. Blueprint first

- **Add it with `dfw screen add`.** `dfw screen add <id> --page <page> --after <previous screen> --from <screens that open it> --title "<title>" --route <route>` puts it in the page right after `--after` (else at the end of the page), adds its manifest entry, and writes a skeleton framed like its sibling (same `app-shell` variant and `current`). It never edits the `--from` screens: it lists the files to link from.
- **Write its blueprint** in its `screens[]` entry: goal, primaryAction, layout, regions, aboveFold, data, feedback (`references/phase-concepts.md` step 2). Use the same layout pattern as the screens around it doing the same kind of job (`references/layout-patterns.md` rule 5). Declare its `states`.
- **Show it** with `dfw blueprint <id>` and get the user's OK together with the place.

## 3. Reuse before adding

- **Compose from the system.** Run `dfw system list`; build it from existing masters and the sibling's page variant. A missing piece is a new master or variant first (`references/phase-concepts.md` step 3), never styles in the screen.
- **A new navigation destination** (the screen gets its own entry in the menu) is a change to the `app-shell` master first, so every screen shows it; then set `current` on the new screen.

## 4. Wire the flow both ways

- **Into it.** In each file `dfw screen add` listed, put `data-dfw-goto="<id>"` on the control that opens it (the button, the row, the link). An entry reached only from outside the design (deep link, email, notification) gets `"entry": true` in its manifest entry instead.
- **Out of it.** Its primary action goes to the next screen, back and cancel go to the screen it came from; no dead end.
- **Around it.** Update what the neighbours show about the sequence: breadcrumbs, steppers ("Step 2 of 4"), tabs, a list that now has one more destination.

## 5. Check

- **Flow page.** Arrows into and out of the new screen, and none of its neighbours lost one. `dfw lint` warns on `manifest-unpaged-screen`, `link-unknown-screen` and `screen-unreachable`: fix them, they are what the canvas hides.
- **Layout and checks.** `dfw layout check --changed`, then `dfw check` once; fix MUST, fix SHOULD unless a recorded decision says otherwise.
- **Render and look.** `dfw shoot --screen <id> --states`, read the images, compare with its siblings, two self-critique rounds (`references/quality-rubric.md`) before showing it.
- **Present** the new screen with its place ("between X and Y, opened from Z") and the changed neighbours.

## 6. After G2

- **The design is frozen.** Adding a screen changes what the user approved: ask, then `dfw reopen --reason "add <screen>"` (a new version). `dfw screen add` refuses while the design is locked. Then follow the steps above and approve G2 again only when the user says so.

## Undoing

- **Remove it by hand.** `dfw screen add` keeps no undo record: delete its `screens[]` entry, its id in the page and `design/screens/<id>/`.
