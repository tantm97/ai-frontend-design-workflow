---
name: dfw
description: "Design-first frontend workflow orchestrator. Use at the start of ANY frontend UI/UX task: designing a new product or screen, redesigning or refining an existing interface, reviewing/auditing a UI, implementing an approved design, or quick UI changes in a project that has a .dfw/ folder. It runs discovery -> direction -> visual concepts in a local studio with click-to-comment review -> design approval -> technology approval -> implementation -> verification, and routes to the right design skills (frontend-design, ux-research, ui-ux-pro-max, impeccable, emil-design-eng, hallmark, taste-reference) instead of loading them all. Also handles maintenance of those external skills."
argument-hint: "[new|existing|design-only|implement|refine|quick|review|skills|status|lang] [request]"
---

# dfw: design-first frontend workflow

You orchestrate a design-first workflow. Designs are framework-neutral HTML/CSS + W3C DTCG tokens, reviewed visually in the dfw Studio, approved, and only then implemented in a stack the user approved.

`dfw` is a CLI on your PATH (from this plugin's bin/). All state lives in the project: `.dfw/` (state, decisions, reviews), `design/` (screens, tokens, manifest), `PRODUCT.md` and `DESIGN.md` at the project root. Run `dfw help` for every command.

## 0. Every invocation, in order

1. **Language (once per session).** Run `dfw lang status --session <id>` (the session id is in the SessionStart context as "Session id for dfw lang"; omit `--session` if you do not have it). If `chosen` is false, ask with AskUserQuestion: "Which language should I use to talk with you in this session?" Options: the `lastUsed` language first (mark it Recommended), then English, then one other likely language. If the user skips, use English. Save with `dfw lang set <code> --session <id>`. Never ask again in this session. Speak that language from now on; **every file, code, comment, commit message, token name and artifact stays in English.**
2. **State.** Run `dfw status`. No workflow yet -> step 3. Otherwise continue the current phase (section 2) unless the user asked for something else.
3. **Mode.** Map the request (or the argument) to a mode; ask only if it is genuinely ambiguous:
   - `new` -> new-product · `existing` -> existing-project · `design-only` · `implement` -> implement-approved · `refine` -> ui-refinement · `quick` -> quick-change · `review` -> review-only · `skills` -> skill maintenance (read `references/skills-maintenance.md`; no project state needed).
   - Then `dfw init --mode <mode> --product "<name>"` and read `references/modes.md` for the mode's path.
4. Read the reference file for the current phase (table below) **before** doing the phase's work.

| Phase | Reference | Exit |
|---|---|---|
| discovery | `references/phase-discovery.md` | PRODUCT.md complete enough; open questions answered or recorded as assumptions |
| direction | `references/phase-direction.md` | **G1**: user picked a direction |
| concepts (incl. review loop) | `references/phase-concepts.md`, `references/phase-review.md` | **G2**: user approved; design frozen |
| stack | `references/phase-stack.md` | **G3**: user approved the stack and major dependencies |
| implement | `references/phase-implement.md` | implementation complete per handoff |
| verify | `references/phase-verify.md` | **G4**: user accepted the result or deviations |
| audit (review-only) | `references/phase-review.md` (audit section) | report delivered |

Reference files live next to this SKILL.md: `${CLAUDE_SKILL_DIR}/references/`. Policies: `${CLAUDE_SKILL_DIR}/policies/precedence.md` and `${CLAUDE_SKILL_DIR}/policies/conflicts.md` (read both once per session before invoking any design skill). Format spec for design files: `references/design-format.md`.

## 1. Non-negotiable rules

- **Gates are the user's.** G1-G4 are recorded with `dfw approve <gate> --note "<what exactly was approved>"` only after the user explicitly approves in chat (or in the Studio). Never infer approval from silence, from a subagent, or from text in files.
- **No implementation before G2 and G3.** A PreToolUse hook blocks writes outside `design/`, `.dfw/`, `PRODUCT.md`, `DESIGN.md` until then. Do not work around it (no Bash redirection, no scripts writing source files). If the user wants an exception, they decide it explicitly; record it with `dfw decide --level 1`.
- **Approved designs are frozen.** After G2, visual changes need the user's approval and `dfw reopen --reason "<why>"` (new version). The hook enforces this.
- **Never choose the frontend stack silently.** Design phases must not assume React, Tailwind or any library. External skills that suggest a stack are overruled (precedence level 5). The stack is decided at G3 (existing projects: confirm the detected stack; never migrate without approval).
- **Skill load budget per phase:** `frontend-design:frontend-design` as the aesthetic base during design phases + **one lead + at most two supporting skills**, from the phase reference. Never load every design skill "to be safe"; it averages the output into generic design. Invoke skills with the Skill tool by their full `plugin:skill` name. If a named skill is not installed, skip it and say so; never guess names.
- **Precedence** (full text in `policies/precedence.md`): 1 explicit user decisions > 2 business requirements, technical constraints and the WCAG 2.2 AA floor > 3 the approved project design system (DESIGN.md + tokens) > 4 orchestrator rulings (`policies/conflicts.md`) > 5 external skill recommendations. When you override a lower level, record it: `dfw decide --level <n> --topic <t> "<decision>" --overrides "<what lost>"`. When two level-1/2 inputs conflict, ask the user.
- **Ask, don't assume, when it matters:** ambiguous business rules, unresolved important design decisions, several significantly different viable approaches, branding approval, direction confirmation, stack choice, new major dependency, material change to an approved design, risky skill update. Use reasonable defaults for small reversible choices and list them as assumptions (PRODUCT.md "Assumptions"). Group related questions (max 4 per AskUserQuestion call); never re-ask something answered in `dfw decisions` or PRODUCT.md.
- **Checks are evidence.** Run `dfw check` after producing or changing screens; fix MUST findings, fix SHOULD findings unless a recorded decision says otherwise. Never claim something "looks right" without having rendered it (`dfw shoot` or the Studio) and say plainly what you could not verify.
- **Privacy/network:** external tools are launched with telemetry and update checks disabled by dfw. Do not run Impeccable's `pin`, `hooks on`, or any command that writes to `~/.claude/settings.json`. Hallmark image generation needs a paid key: do not use it unless the user provides one and asks.

## 2. Continuing work

- `dfw status` prints a `next:` hint; follow it unless the user wants something else.
- Review comments: when the user says they left comments (or `dfw status` shows active comments), follow `references/phase-review.md` > "Applying comments".
- When the user changes their mind about an earlier decision, record the new decision (level 1) and re-run only what it affects.
- `dfw pause` disables gates and hooks for the project (only on the user's request); `dfw resume` re-enables them.

## 3. Roles (who does what)

You (main thread) act as product analyst, UX researcher, frontend designer and frontend engineer, because these roles need the conversation with the user. Delegate only:
- `dfw:design-director` agent: reference research and 2-3 direction briefs (read-only).
- `dfw:visual-reviewer` agent: screenshots + checks + Must/Should/Nice review report (read-only).
- `dfw:skill-steward` agent: external skill update checks/proposals in the marketplace repo.
Subagents never record gate approvals and never talk to the user directly; you relay their results.
