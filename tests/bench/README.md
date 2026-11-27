# Benchmarks

Two levels. Keep an integration only if it moves a number here.

## 1. Check benchmark (automated, free): `npm run bench`

`fixtures/*.html` are small screens, each with one seeded, labelled defect (`<!-- expect: rule, ... -->`), plus clean screens (`expect: none`) that must stay silent. Optional `<!-- states: ... -->` declares the screen's states.

The runner builds a temporary dfw project from them and runs dfw lint, the rendered layout check and axe. It reports:

- **detection:** seeded defects found, among the checkers that ran;
- **coverage:** seeded defects found, among all of them;
- **false positives:** a tracked rule firing on a screen where it was not seeded;
- **time** per checker.

It compares with `baseline.json` and exits 1 when detection drops or false positives rise. `npm run bench -- --save` stores a new baseline.

It runs the dfw binary (`DFW_BIN`, else `native/target/debug/dfw`: `cd native && cargo build` first). `npm run bench -- --bin <dfw>` runs the same fixtures against another binary, for example the previous release (`~/.config/dfw/bin/<version>/dfw`).

Rules:

- Never edit a fixture to make a checker pass. If a fixture is wrong (for example, WCAG exempts it), fix it and say why in the commit.
- Add a fixture whenever a check is added or a real miss is found.

**Bias:** fixtures are written for the rules being measured. This benchmark shows that the checks work and do not cry wolf. It does not show that designs get better; level 2 does.

Apple Silicon, system Chrome (0.4.0 and 0.5.0 recorded on 2026-11-15; 0.6.11 on 2026-11-27 with a debug build, which is slower than a release):

| Version | Detection | Coverage | False positives | lint | layout | axe |
|---|---|---|---|---|---|---|
| 0.4.0 | 22% | 17% | 0 | 5 ms | 6.2 s | not available |
| 0.5.0 | 100% | 100% | 0 | 11 ms | 7.0 s | 2.2 s |
| 0.6.11 (debug build) | 100% | 100% | 0 | 177 ms | 9.4 s | 2.2 s |

## 2. Workflow benchmark (manual, uses your AI tool's quota)

Run the same briefs before and after a change, each in a fresh session with no coaching beyond the brief.

Briefs:

1. Tool-crib lending: borrow and return counter, overdue list, item detail (operators, tablets).
2. Supplier onboarding form with validation and a review step (desktop).
3. Analytics dashboard for a small shop (laptop and phone).
4. Marketing landing page for a local clinic (all breakpoints).
5. Settings area: profile, notifications, billing (desktop).

Metrics per brief:

| Metric | How |
|---|---|
| MUST findings left at G2 | `dfw check` at the freeze |
| SHOULD findings left at G2 | same |
| Rendered issues | `dfw layout check --states` |
| Time to G2 | session timestamps |
| Tokens to G2 | the tool's usage report |
| User comments needed | `dfw comments list` count (every status) |
| Blind quality score | `core/workflow/references/quality-rubric.md`, 0-2 per criterion, by someone who did not see which version made it |

Record each run in `tests/bench/workflow-runs.jsonl` (one JSON object per brief and version). A change is worth keeping when MUST findings and the blind score improve, or stay equal at lower time and tokens.
