# Phase: review (inside concepts) and audit (review-only)

## Interactive review loop

The user reviews in the Studio (`dfw studio start`), or in Penpot when `dfw status` says `canvas: penpot` (`references/penpot.md` > "Review canvas"): then the dfw bridge panel keeps the boards up to date after each of your builds, and Penpot threads and edits arrive as comments with `provider: "penpot"` through the same wait. Comments are stored in `.dfw/reviews/rv_*.json` using the feedback contract (`schemas/feedback.schema.json`): screen, variant, breakpoint, state, theme, target (`dfwId`, CSS selector, bounding box, accessible role/name, text, computed style summary, source file:line), comment text, status, thread.

### Live comments (the user comments, you fix, no relaying)
- As soon as the Studio is shown to the user (with its link in your reply), start listening: run `dfw comments wait --timeout 3600` **in the background** (Bash `run_in_background`). The user writes comments as drafts and hands them over with the Studio's **Send to AI** button; the wait exits the moment they are sent and prints them; you are re-invoked with that output. Apply them (below), then start the wait in the background again **before you end your turn**, also after a batch a hook delivered. Stop listening only when the user says review is over or the phase moves on.
- The Studio shows the user whether you listen, are working on their comments, or do not listen (then it tells them to ask you). The Stop hook reminds you when the Studio is open and nothing listens.
- The hooks are the safety net: new comments are also injected with the user's next message (UserPromptSubmit) and before you stop (Stop hook). Each comment is delivered once, drafts only after they are sent; `dfw comments inbox --peek` shows undelivered ones.
- The Studio shows live status per comment: Draft → Sent, waiting for the AI → Received by the AI → AI is working (ack) → Fixed, please verify → Resolved by the user.
- Talk in the comment, not only in chat: `dfw comments reply <id> --message "<question or answer>"` shows in the Studio thread, and the user answers there with **Reply** (the answer reaches you like a new comment). Finish each comment with `dfw comments fixed <id> --message "<what changed>"`.

### Applying comments
1. Read the delivered comments (or `dfw comments list --status active --json`), including `target.source`, `target.component` and `target.styles`.
2. Group comments that touch the same component or token: fix the master in `design/system/components/` (or the token) so every screen gets it; fix the screen only for layout/content.
3. For each comment: `dfw comments ack <id>`; apply the change in the referenced file; if the request conflicts with a higher-precedence rule (accessibility, business rule, approved system), do not silently apply or ignore it: explain, propose the closest compliant option, `dfw comments reply <id> --message "<question>"` and ask the user in chat.
4. Verify each change actually happened: re-render (`dfw shoot --screen <id> --variant <v> --bp <bp>` or `--selector '<selector>'` for an element shot) and inspect the image; run `dfw check <file>`.
5. `dfw comments fixed <id> --message "<what changed, where>" [--evidence .dfw/shots/...png]`. Never mark `resolved` yourself; the user resolves in the Studio ("Looks right") or tells you in chat (then `dfw comments resolve <id> --by user`).
6. Tell the user which comments are ready to verify (the Studio's "To verify" tab; in Penpot, the `[dfw] ... fixed` reply in the thread, which they resolve to accept), and any that need their decision.
7. Comments with `provider: "penpot"` come from a designer's edits or comment threads in Penpot (`references/penpot-import.md`, `references/penpot-live.md`). Treat them like any other comment: apply text, colour and position changes through masters, tokens and screen layout (never paste Penpot geometry into the HTML), and confirm token changes with the user before `dfw tokens import`.

### Critique (before asking for approval)
- Spawn `dfw:visual-reviewer` with the screens in scope. It runs `dfw check`, screenshots every screen/variant/breakpoint, uses `impeccable:impeccable` critique/audit (and `emil-design-eng:review-animations` if motion changed; Hallmark finish review on marketing pages it led) and returns a Must/Should/Nice report with evidence.
- Fix Must items; present Should/Nice items to the user with your recommendation.

### Approval (gate G2)
1. Preconditions (`dfw approve G2` checks them itself: every check, axe with states): no active or "to verify" comments (or the user explicitly accepts them: `--accept-open-comments`), `dfw check` has no MUST findings (or the user explicitly accepts them: `--accept-findings`, recorded), and every configured check ran. A check that could not run blocks G2: install what is missing (`dfw doctor`) or turn the check off. Add `--accept-unverified` only when the user, told which checks did not run, accepts freezing without them (it is recorded in the decisions).
2. Ask the user to approve the design, naming exactly what is approved: screens, chosen variant per screen (set `approvedVariant` in the manifest), tokens, components.
3. `dfw approve G2 --note "<summary>"` freezes `design/` + PRODUCT.md + DESIGN.md into `design/approved/vN/` with a sha256 lock. The hook then locks the design workspace.
4. `dfw handoff` writes `design/handoff/vN.md`; fill its TODO sections (behavior not shown, responsive rules, assets, component mapping) with the user's answers.
5. Next: `dfw phase stack` (unless design-only: `dfw phase done`).

### Changing an approved design
Only with the user's approval: `dfw reopen --reason "<why>"` -> new draft version, review again, approve again (G2 for vN+1). Implementation of the previous version pauses until then. Before asking for the new approval, show what changed: `dfw shoot` the new version and `dfw diff .dfw/shots/vN .dfw/shots/vN+1` (percentage changed per screen, red = changed); every changed screen should map to a reason the user agreed to.

## Audit (review-only mode)
Nothing is modified outside `.dfw/` (the hook enforces this).
1. Determine the target: design files, a local URL, or screenshots provided by the user.
2. Spawn `dfw:visual-reviewer` (URL or files). For a running app: `dfw check --url <url>` and `dfw shoot --url <url>`.
3. Deliver a report: summary, Must/Should/Nice findings with evidence (file:line, screenshot path, WCAG criterion), and recommended next mode (ui-refinement or quick-change) if the user wants fixes.
