# Modes

| Mode | Init | Path | Notes |
|---|---|---|---|
| new-product | `dfw init --mode new-product` | discovery -> direction (G1) -> concepts/review (G2) -> stack (G3) -> implement -> verify (G4) | Full workflow. Stack may be pulled forward if it constrains the design (e.g. mandated component library): do the stack phase early, then return. |
| existing-project | `dfw init --mode existing-project` | discovery (inspect codebase + `dfw stack detect --write`) -> direction (G1; may be "keep current identity") -> concepts/review (G2) -> stack (G3 = confirm existing stack) -> implement -> verify (G4) | Prefer the existing stack, components and conventions. Extract the current look into DESIGN.md + tokens before proposing changes. |
| design-only | `dfw init --mode design-only` | discovery -> direction (G1) -> concepts/review (G2) -> done | No stack, no implementation. Produce the handoff (`dfw handoff`) so it can be implemented later. |
| implement-approved | an approved design already exists in this project (`design/approved/vN`); `dfw mode implement-approved` | stack (G3) -> implement -> verify (G4) | If the design came from elsewhere (images, Figma export, another repo), first import it as screens in `design/` (existing-project or design-only mode) and get G2. |
| ui-refinement | `dfw init --mode ui-refinement` | discovery-lite (what is wrong, for whom; screenshots of the live UI) -> concepts (G1 waived: current identity is the direction) -> review (G2) -> stack (confirm) -> implement -> verify | Recreate only the affected screens in `design/` (from the live app, `dfw shoot --url`), redesign them, review, approve. |
| quick-change | `dfw init --mode quick-change` (or `dfw mode quick-change`) | concepts (one screen/component change, G1 waived) -> G2 (one-line confirmation of a before/after) -> stack confirm (G3, usually already approved) -> implement -> verify | For small, reversible changes. If the change turns out to affect the identity or several screens, tell the user and switch to ui-refinement. |
| review-only | `dfw init --mode review-only` | audit | Writes nothing except `.dfw/`. Output: a review report (Must/Should/Nice) with evidence. |
| skill maintenance | no project needed | `references/skills-maintenance.md` | Works in the dfw-marketplace repository. |

Switching mode later: `dfw mode <mode>`. Record why with `dfw decide --level 1 --topic mode "<reason>"` when the user asked for it.
