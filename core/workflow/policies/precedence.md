# Precedence and conflict resolution

When instructions disagree, the higher level wins. Lower levels still apply wherever they do not conflict.

| Level | Source | Examples | Recorded as |
|---|---|---|---|
| 1 | Explicit user decisions | "Use our brand purple", "keep the sidebar", gate approvals | `dfw decide --level 1` / `dfw approve` |
| 2 | Business requirements, technical constraints, accessibility floor | PRODUCT.md rules, existing stack, WCAG 2.2 AA, legal/locale constraints | PRODUCT.md, `.dfw/stack.json` |
| 3 | Approved project design system | DESIGN.md, `design/tokens/tokens.json`, frozen `design/approved/vN` | DESIGN.md "Decisions" |
| 4 | Orchestrator rulings | `policies/conflicts.md`, phase references | `dfw decide --level 4` when applied to a real case |
| 5 | External skill recommendations | frontend-design taste rules, Impeccable bans, Hallmark Floor, UI UX Pro Max catalog picks, Taste references, Emil motion rules | — |

## Rules

1. **Level 1 vs level 2.** A user decision that would break accessibility, a business rule or a technical constraint is not silently applied or silently refused: explain the consequence once, propose the closest compliant option, and let the user decide. If they insist, record it (level 1, `--overrides "WCAG 2.2 AA <criterion>"`).
2. **Brand inputs are level 1** when the user supplied them (logo, colors, fonts, tone). External skills must not "fix" a brand: e.g. Taste's violet remap and any skill's banned-font list do not apply to a user's brand font or color.
3. **Accessibility floor (level 2):** WCAG 2.2 AA - contrast 4.5:1 text / 3:1 large text and UI components, keyboard operable, visible focus not obscured (2.4.7, 2.4.11), target size at least 24x24 CSS px (2.5.8), no color-only meaning (1.4.1), labels and error identification for forms (3.3.1, 3.3.2), reflow at 320 CSS px (1.4.10), reduced-motion respected.
4. **Level 3 vs level 5.** Once a design system is approved, external skills work inside it. A skill's taste rule ("never X") does not override an approved token or component unless the user re-opens the design.
5. **Level 5 vs level 5.** Resolve with `conflicts.md`. If no ruling covers it: the phase's lead skill wins over supporting skills within its domain; outside its domain the domain owner wins (motion -> Emil, critique/audit -> Impeccable, style/palette/font data -> UI UX Pro Max, aesthetic direction -> frontend-design, marketing-page structure -> Hallmark). Record the ruling (level 4) so it is applied consistently afterwards.
6. **Detecting conflicts.** Before applying a skill's instruction, check it against: recorded decisions (`dfw decisions`), PRODUCT.md constraints, `.dfw/stack.json`, DESIGN.md and tokens. Typical tells: a skill prescribes a framework/library/CSS approach; prescribes or bans specific fonts/colors/patterns; wants to write DESIGN.md/PRODUCT.md in its own format; wants to install hooks or change settings; adds animation the user or the system ruled out.
7. **Never resolve by averaging.** Pick the winner, apply it fully, record why.
