# Phase: review (inside concepts) and audit (review-only)

## Interactive review loop

The user reviews in the Studio (`dfw studio start`). Comments are stored in `.dfw/reviews/rv_*.json` using the feedback contract (`schemas/feedback.schema.json`): screen, variant, breakpoint, state, theme, target (`dfwId`, CSS selector, bounding box, accessible role/name, text, computed style summary, source file:line), comment text, status, thread.

### Applying comments
1. `dfw comments list --status active` (or `--json`). Read each comment fully, including `target.source` and `target.styles`.
2. Group comments that touch the same component; a change to a shared component must be applied in `design/components/` and every screen using it.
3. For each comment: `dfw comments ack <id>`; apply the change in the referenced file; if the request conflicts with a higher-precedence rule (accessibility, business rule, approved system), do not silently apply or ignore it: explain, propose the closest compliant option, `dfw comments reply <id> --message "<question>"` and ask the user in chat.
4. Verify each change actually happened: re-render (`dfw shoot --screen <id> --variant <v> --bp <bp>` or `--selector '<selector>'` for an element shot) and inspect the image; run `dfw check <file>`.
5. `dfw comments fixed <id> --message "<what changed, where>" [--evidence .dfw/shots/...png]`. Never mark `resolved` yourself; the user resolves in the Studio ("Looks right") or tells you in chat (then `dfw comments resolve <id> --by user`).
6. Tell the user which comments are ready to verify (the Studio's "To verify" tab), and any that need their decision.

### Critique (before asking for approval)
- Spawn `dfw:visual-reviewer` with the screens in scope. It runs `dfw check`, screenshots every screen/variant/breakpoint, uses `impeccable:impeccable` critique/audit (and `emil-design-eng:review-animations` if motion changed; Hallmark finish review on marketing pages it led) and returns a Must/Should/Nice report with evidence.
- Fix Must items; present Should/Nice items to the user with your recommendation.

### Approval (gate G2)
1. Preconditions: no active or "to verify" comments (or the user explicitly accepts them), `dfw check` has no MUST findings (or the user explicitly accepts them).
2. Ask the user to approve the design, naming exactly what is approved: screens, chosen variant per screen (set `approvedVariant` in the manifest), tokens, components.
3. `dfw approve G2 --note "<summary>"` freezes `design/` + PRODUCT.md + DESIGN.md into `design/approved/vN/` with a sha256 lock. The hook then locks the design workspace.
4. `dfw handoff` writes `design/handoff/vN.md`; fill its TODO sections (behavior not shown, responsive rules, assets, component mapping) with the user's answers.
5. Next: `dfw phase stack` (unless design-only: `dfw phase done`).

### Changing an approved design
Only with the user's approval: `dfw reopen --reason "<why>"` -> new draft version, review again, approve again (G2 for vN+1). Implementation of the previous version pauses until then.

## Audit (review-only mode)
Nothing is modified outside `.dfw/` (the hook enforces this).
1. Determine the target: design files, a local URL, or screenshots provided by the user.
2. Spawn `dfw:visual-reviewer` (URL or files). For a running app: `dfw check --url <url>` and `dfw shoot --url <url>`.
3. Deliver a report: summary, Must/Should/Nice findings with evidence (file:line, screenshot path, WCAG criterion), and recommended next mode (ui-refinement or quick-change) if the user wants fixes.
