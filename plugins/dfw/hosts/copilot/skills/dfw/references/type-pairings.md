# Curated type pairings

All families below are free (SIL OFL) on Google Fonts. **VI** = ships the `vietnamese` subset (checked with `dfw fonts check`, 2026-11-13). For a Vietnamese UI use only VI families; re-check any family not listed: `dfw fonts check "<Family>"`. Self-host fonts for production.

| Personality | UI / body | Display (optional) | Mono (ids, code) | VI | Fits |
|---|---|---|---|---|---|
| Neutral precise | Inter | Inter (tight tracking) | JetBrains Mono | yes | SaaS, admin, dashboards |
| Engineered, industrial | IBM Plex Sans | IBM Plex Sans | IBM Plex Mono | yes | operations, factories, enterprise |
| Vietnamese-native | Be Vietnam Pro | Be Vietnam Pro | JetBrains Mono | yes | VN products, public services |
| Warm friendly | Plus Jakarta Sans | Plus Jakarta Sans | JetBrains Mono | yes | consumer, booking, services |
| Geometric modern | Manrope | Manrope | JetBrains Mono | yes | fintech, modern tools |
| Technical sharp | Geist | Geist | Roboto Mono | yes | developer tools, data |
| Humanist calm | Source Sans 3 | Source Serif 4 | Source Code Pro | yes | content-heavy, documentation, health |
| Editorial | Public Sans | Newsreader / Fraunces | IBM Plex Mono | yes | publishing, marketing with long reads |
| Characterful display | Hanken Grotesk | Bricolage Grotesque | JetBrains Mono | yes | marketing, brand-forward products |
| Readable (accessibility) | Lexend | Lexend | Roboto Mono | yes | education, broad audiences |
| Compact data | Archivo (wdth 87-100) | Archivo (wdth 112-125) | IBM Plex Mono | yes | dense tables, logistics |


**Latin-only (fine for English-only UIs, not for Vietnamese text):** DM Sans, Instrument Sans, Figtree, Sora, Outfit, Spline Sans, Bodoni Moda.

| Personality (English-only UIs) | UI / body | Display | Mono |
|---|---|---|---|
| Friendly geometric | DM Sans | DM Sans / Fraunces | JetBrains Mono |
| Crisp modern | Instrument Sans | Instrument Serif / Instrument Sans | JetBrains Mono |
| Soft rounded | Figtree | Figtree | JetBrains Mono |
| Tech display | Outfit / Sora | Sora | Space Mono |
| Fashion | Instrument Sans | Bodoni Moda | JetBrains Mono |


Language rule: if any UI text is Vietnamese (`lang: vi`, or a bilingual VI/EN product), every family that renders that text must ship `vietnamese`. English-only products may use any family above. Each style guide has `fonts` (Vietnamese-capable) and `fontsLatin` (English-only alternative): `dfw style apply <id> --fonts latin`.

Rules
- One family for UI is usually enough; a second only for display or long-form reading.
- Scale with real steps (ratio ~1.2-1.25 for product UI, 1.333+ for marketing).
- Tabular numbers for data (`font-variant-numeric: tabular-nums`); mono only for identifiers, never for labels.
- Tight tracking (-0.01 to -0.02em) only at 20px+; never on small text.
