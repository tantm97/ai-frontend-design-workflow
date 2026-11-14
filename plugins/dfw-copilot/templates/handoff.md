# Handoff: {{product}} design {{version}}

- Approved design: `design/approved/{{version}}/` (sha256 `{{hash}}`, approved {{approvedAt}})
- Stack: {{stack}}
- Breakpoints: {{breakpoints}}
- Rule: implementation must match the approved design. Any material visual deviation needs user approval (dfw reopen) before it ships.

## 1. Screens
| Screen | Title | Approved variant (source of truth) | Route / location in app |
|---|---|---|---|
{{screens}}

## 2. Components
{{components}}

TODO: map each design component to an existing component in the codebase (reuse first) or a new one, with its states.

| Design component | Implementation (existing/new, path) | States to implement |
|---|---|---|
| | | |

## 3. Tokens
{{tokens}}

TODO: token mapping. Prefer generating platform outputs from the DTCG file (Style Dictionary) over copying values.

| Token group | Implementation target (CSS vars / theme object / Tailwind theme / SCSS) | Notes |
|---|---|---|
| color | | |
| font | | |
| space | | |
| radius | | |

## 4. Behavior the design does not show (TODO, confirm with the user)
- Validation rules and messages:
- Loading states and skeletons:
- Empty states:
- Error states and recovery:
- Keyboard interaction and focus order:
- Permissions per role:
- Real-time updates, pagination, sorting, filtering:

## 5. Responsive rules
TODO: what changes between breakpoints (navigation, tables, density).

## 6. Assets
TODO: logos, icons (library + license), images, fonts (license, hosting).

## 7. Acceptance criteria
- Visual: each screen at each breakpoint matches `design/approved/{{version}}` (dfw verify compares screenshots).
- Accessibility: WCAG 2.2 AA; no MUST findings from dfw check.
- Tokens: no hard-coded colors/sizes outside the token outputs.
- Interactions listed in section 4 implemented and tested.

## 8. Review history
Resolved review comments for this version:
{{resolved}}
