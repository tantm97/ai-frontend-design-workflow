# Design-system thinking (how a senior designer decides)

Every visual decision must trace back to **context → principle → decision → check**. If you cannot write the "because", the decision is arbitrary: change it or ask the user. Write the chain in DESIGN.md `## Rationale` (≥ 5 lines; `dfw system audit` and the DS gate check it).

## 1. Context before taste (design/system/brief.json)
Fill the brief before choosing anything:
- **Use context:** who, where, how long, with what device/input, at what distance and lighting, how often, what a mistake costs. (A counter PC read from 1 m with gloves ≠ a phone in the sun ≠ an analyst's 27" monitor.)
- **Brand:** what is fixed (logo, colours, fonts, assets), what must be avoided.
- **Personality (1-5):** formality, energy, era, warmth, density, richness, seriousness. Ask the user when two axes are unclear; they are the strongest lever on "does this feel right".
Then `dfw style match` ranks the 12 guides against the brief and explains each mismatch. Pick 3 that differ on at least two axes; never three colour swaps of one idea.

## 2. Colour, the senior way
1. **Roles before colours.** Decide the roles (canvas, surface, ink, muted, border, rule, accent, brand, signature, status) and the proportion: ~60% neutrals (canvas/surfaces), ~30% ink and secondary text, ~10% accent + brand. Status colours are not decoration.
2. **One accent hue.** The accent marks *what to do next*. Exactly one hue for actions/links/selection. A second chromatic hue is either brand (large surfaces, identity) or signature (ornament) and never competes with the accent.
3. **Semantic collisions are bugs.** The accent must be ≥ 40° (OKLCH hue) from error red and success green; otherwise primary buttons read as "danger" or "done". When the brand hue *is* red or green, keep it for brand surfaces/ornament and make the accent ink (neutral 900) or another hue. Info merges into a blue accent (one blue, not two).
4. **Neutrals have a temperature.** Tint all neutrals toward one hue (paper = warm 70-90°, steel = cool 230-250°) with low chroma (0.004-0.02). Mixed temperatures (warm ink on a cool canvas) look dirty. Large surfaces stay at chroma ≤ 0.03.
5. **Ramps are perceptual, steps are chosen by contrast.** Build 11-step OKLCH ramps (`dfw palette build`) with even lightness; then pick the role step by target, not by eye: ink ≥ 13:1, muted ≥ 5:1 on sunken, subtle/placeholder ≥ 4.5:1, borders ≥ 3:1 (WCAG 1.4.11), accent ≥ 4.5:1 on surface and its label ≥ 4.5:1.
6. **Harmony is a reason, not a vibe.** Signature hue = a stated relation to the brand hue: analogous (±30°, calm, material), split-complementary (±150°, lively but safe), complementary (180°, maximum energy). Write which and why.
7. **Status colours stay recognisable.** Red is red, green is green; only a small temperature pull (≤ 6°) toward the neutrals. Always paired with icon + word.
8. **Every colour has a usage rule** in its `$description`: what it is for and what it must never do ("gold: ornament only, never body text").
9. **Elevation is tinted.** Shadows use the darkest neutral, not #000, so depth belongs to the palette.

## 3. Type
1. **Base size from context:** 16 default; 17-18 for public/forms/older users; 14-15 for dense pro tools; never < 12 for any text.
2. **Two ratios:** UI steps at 1.125-1.25 (calm, close), display steps at 1.333-1.5 (dramatic). `dfw type scale --base --ratio --display-ratio --from` generates both; no hand-picked sizes like 15/17/22/26.
3. **Hierarchy needs contrast:** largest display ≥ 1.8× body for product UI, ≥ 3× for marketing. No two sizes within 6% of each other.
4. **Families:** one for UI, optionally one display, plus mono for identifiers. Display family for titles and key numbers only. Vietnamese text → every family that renders it ships the `vietnamese` subset; English-only products may use Latin-only families.
5. **Details:** tabular numbers for data; tracking −0.01…−0.03em only at ≥ 24px; eyebrows (small caps, +0.08…0.24em) to open sections; line length 60-75ch for reading.

## 4. Space, shape, density
- Spacing on a 4px grid with a scale (4, 8, 12, 16, 20, 24, 32, 40, 48, 64); related items close, groups separated by at least two steps.
- Density from context: control height 32 (dense pro), 40 (default), 44-56 (touch, counter, gloves).
- 3-4 radius steps with a reason (2-4 = precise/industrial/editorial, 8-12 = friendly, 16+ = playful). Never mix sharp tables with bubbly buttons without saying why.

## 5. Signature, not decoration
One or two signature motifs from the concept (signboard frame, hazard tape, ledger double rule, field-note margin). Each has a usage rule (where, how often). Remove any decoration that has no rule.

## 6. Review like a senior (before showing anything)
Run `dfw system audit` and read the Design System page's "Senior review" block. Fix every MUST; justify or fix every SHOULD in `## Rationale`. Then ask: "Could this palette and type belong to a different product?" If yes, the concept is not in it yet.
