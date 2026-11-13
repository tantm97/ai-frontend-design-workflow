#!/usr/bin/env node
// Generates the curated style-guide library (core/kits/styles/<id>.json).
// A style guide is a visual concept, not a theme: a metaphor, a named palette with usage rules,
// a type pairing with real display contrast, shape/elevation/density, 1-2 signature motifs,
// grid, component notes, do/don't. `dfw style apply <id>` turns it into DTCG tokens on top of
// the kit contract. Every role pair is contrast-checked before writing (WCAG 2.2 AA).
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const OUT = path.join(ROOT, 'core', 'kits', 'styles');

import { buildPalette, typeScale, auditColors, auditTypeScale, contrast, hexToOklch } from '../core/lib/color.mjs';

// p = palette { name: [hex, usage] }, r = roles { contractRole: paletteName | hex }
const GUIDES = [
  {
    id: 'heritage-signboard',
    seeds: { brand: { hue: 30, chroma: 0.15 }, neutral: { hue: 80, tint: 0.014 }, signature: { hue: 82, chroma: 0.12 }, accentFrom: 'neutral', warmSurface: true, brandStep: 800, onBrandSignature: true, signatureStep: 500 },
    names: { brand: 'son', neutral: 'giay', signature: 'kim' },
    usage: { son: 'Lacquer red: signboard surfaces and brand moments only (never small text, never next to errors).', giay: 'Aged paper to ink: every neutral, tinted warm so the paper and the ink belong together.', kim: 'Gold leaf: ornament, rules and text on lacquer; never body text on paper.' },
    type: { base: 16, ratio: 1.2, displayRatio: 1.414, displayFrom: 30, eyebrow: { size: 12, weight: 600, tracking: '0.16em', upper: true } },
    axes: { formality: 4, energy: 2, era: 1, warmth: 5, density: 2, richness: 5, seriousness: 4 },
    rationale: ["Accent = ink (giay-900), not lacquer red: red is the error hue; an ink button keeps the primary action calm and lets red mean 'signboard' (brand surfaces) instead of 'danger'.", "Neutrals are tinted toward paper (hue 80) at the same temperature as the gold, so paper, ink and gold read as one material family.", "Display scale 30/42/60/85 at √2 ratio: carved-signboard titles need big jumps; UI text stays at a calm 1.2 ratio.", "Gold is the signature (ornament and text on lacquer) because gold on paper cannot reach 4.5:1 for small text."],
    name: 'Heritage signboard',
    tags: ['heritage', 'culture', 'vietnam', 'retail', 'luxury', 'craft', 'editorial', 'serif', 'warm', 'storefront'],
    concept: 'Old-quarter shop signboards: lacquer-red boards with gold-leaf lettering on aged paper; ink-black text; serif display set large like carved characters.',
    fits: ['heritage retail', 'jewellery, tea, crafts', 'culture and tourism', 'premium F&B'],
    avoid: ['dense back-office tools', 'products that must feel neutral'],
    base: 'soft',
    fonts: { display: 'Noto Serif Display', body: 'Be Vietnam Pro', mono: 'JetBrains Mono', displayWeights: '400;500', note: 'Noto Serif Display at 400-500; never bold.' },
    fontsLatin: { display: 'Cormorant Garamond', body: 'DM Sans', mono: 'JetBrains Mono', displayWeights: '500;600' },
    radius: { sm: 2, md: 4, lg: 6, xl: 8 },
    density: 'regular',
    motifs: [
      { name: 'signboard-frame', use: 'Hero blocks and brand panels: double frame (gold inner rule inside a lacquer board). One per screen.', css: '.motif--signboard-frame { background: var(--color-brand); color: var(--color-on-brand); padding: var(--space-8); outline: var(--border-width-strong) solid var(--color-signature); outline-offset: calc(var(--space-2) * -1); }' },
      { name: 'gold-rule', use: 'Section dividers: short gold rule with a centred diamond. Never under every heading.', css: '.motif--gold-rule { display: flex; align-items: center; gap: var(--space-2); color: var(--color-signature); } .motif--gold-rule::before, .motif--gold-rule::after { content: ""; height: 1px; width: var(--space-12); background: currentColor; } .motif--gold-rule > span { width: var(--space-1-5); height: var(--space-1-5); background: currentColor; transform: rotate(45deg); }' },
    ],
    layout: 'Desktop 12 columns, content 1200, gutters 24, generous top whitespace above display titles; mobile 2-column product grid, margins 16.',
    components: ['Buttons 14/600, radius 2-4: ink primary, outline secondary; lacquer only on brand bands with gold text.', 'Eyebrow caps 12/600 tracking 0.16em above display titles (ink on paper; gold only on lacquer).', 'Product names in display serif 22, exactly two lines.', 'Price in display serif, never "0 ₫" (show "Liên hệ").'],
    do: ['Set display titles big (42-85) and let them breathe.', 'Use gold only as ornament and thin rules.', 'Use real product photography on paper backgrounds.'],
    dont: ['Gold text on paper below 18px.', 'Gradients, glass, glows.', 'More than one framed signboard block per view.'],
  },
  {
    id: 'industrial-utility',
    seeds: { brand: { hue: 250, chroma: 0.12 }, neutral: { hue: 240, tint: 0.008 }, signature: { hue: 95, chroma: 0.17 }, signatureStep: 300 },
    names: { brand: 'xanh-thep', neutral: 'thep', signature: 'vang' },
    usage: { 'xanh-thep': 'Signal blue: primary actions, current step, selection (also the info colour).', thep: 'Steel greys, slightly cool: surfaces, borders, text.', vang: 'Safety yellow: hazard tape, attention edges, label tape; always with ink text, never as text.' },
    type: { base: 16, ratio: 1.2, displayRatio: 1.333, displayFrom: 28, eyebrow: { size: 12, weight: 600, tracking: '0.12em', upper: true } },
    axes: { formality: 3, energy: 3, era: 3, warmth: 2, density: 4, richness: 2, seriousness: 5 },
    rationale: ["Accent = signal blue: the only hue that is not already a safety/status meaning on a shop floor (red stop, green go, yellow caution).", "Info merges into the accent (both blue) so the screen has one blue, not two competing ones.", "Yellow is a signature, not a status: tape and edges carry it; text on it is always ink (9:1).", "Large density and a 1.333 display ratio: read from a metre away with gloves on, big feedback beats subtle hierarchy."],
    name: 'Industrial utility',
    tags: ['industrial', 'factory', 'warehouse', 'operations', 'counter', 'logistics', 'safety', 'condensed', 'utility', 'b2b', 'internal'],
    concept: 'Tool-crib and shop-floor signage: steel greys, safety-yellow hazard tape, stencilled numbers and condensed labels you can read from a metre away.',
    fits: ['counters, kiosks, factory floors', 'warehouse and asset tracking', 'maintenance and field ops'],
    avoid: ['consumer lifestyle brands', 'luxury'],
    base: 'industrial',
    fonts: { display: 'Barlow Condensed', body: 'IBM Plex Sans', mono: 'IBM Plex Mono', displayWeights: '500;600;700', note: 'Barlow Condensed 600 for titles and big numbers, uppercase only for short labels.' },
    fontsLatin: { display: 'Barlow Condensed', body: 'IBM Plex Sans', mono: 'IBM Plex Mono', displayWeights: '600;700' },
    radius: { sm: 2, md: 3, lg: 4, xl: 6 },
    density: 'large',
    motifs: [
      { name: 'hazard-tape', use: 'Thin top/bottom edge of attention zones (scan area, warnings). Max one per view.', css: '.motif--hazard-tape { height: var(--space-2); background: repeating-linear-gradient(-45deg, var(--color-signature) 0 var(--space-3), var(--color-brand) var(--space-3) var(--space-6)); }' },
      { name: 'label-tape', use: 'Asset IDs and bin locations as printed label tape: yellow band, ink mono text.', css: '.motif--label-tape { display: inline-flex; align-items: center; padding: var(--space-half) var(--space-2); background: var(--color-signature); color: var(--color-brand); font: var(--font-weight-medium) var(--font-size-sm) / 1.2 var(--font-family-mono); letter-spacing: var(--font-tracking-wide); }' },
    ],
    layout: 'Fixed mode rail + work area; one centred work object; feedback band at the bottom; 8px grid; big numbers in condensed display.',
    components: ['Primary actions 56px at the counter, 44px elsewhere.', 'Status = symbol + word + colour; big feedback band 88-96px.', 'IDs always mono or label-tape.'],
    do: ['Use condensed display for numbers and screen titles.', 'Keep surfaces flat; one raised object.', 'Use yellow only as tape/edges.'],
    dont: ['Rounded pill everything.', 'Yellow text.', 'Thin light-grey text on steel.'],
  },
  {
    id: 'swiss-grid',
    seeds: { brand: { hue: 265, chroma: 0.2 }, neutral: { hue: 265, tint: 0.003 } },
    names: { brand: 'coban', neutral: 'xam' },
    usage: { coban: 'Cobalt: the single accent (actions, links, selection, info).', xam: 'Near-neutral greys (a hint of cobalt so they do not look dirty next to it).' },
    type: { base: 15, ratio: 1.2, displayRatio: 1.414, displayFrom: 28, eyebrow: { size: 11, weight: 600, tracking: '0.08em', upper: true } },
    axes: { formality: 4, energy: 2, era: 5, warmth: 2, density: 4, richness: 1, seriousness: 4 },
    rationale: ["One accent hue only; everything else is grey and type. Swiss style gets its character from grid and scale, not colour.", "Greys carry 0.003 chroma of the accent hue: pure greys look dirty next to a saturated blue.", "15px base for dense productivity screens; display jumps at √2 for oversized numerals."],
    name: 'Swiss grid',
    tags: ['minimal', 'swiss', 'grid', 'saas', 'admin', 'precise', 'neutral', 'productivity', 'b2b', 'dashboard'],
    concept: 'International typographic style: strict grid, hairline rules, black type on white, one cobalt accent, oversized numerals and flush-left everything.',
    fits: ['SaaS and admin', 'analytics', 'developer and design tools'],
    avoid: ['warm consumer brands', 'playful products'],
    base: 'crisp',
    fonts: { display: 'Inter', body: 'Inter', mono: 'JetBrains Mono', displayWeights: '500;600', note: 'Display = Inter 600 with -0.03em tracking at 40+.' },
    fontsLatin: { display: 'Instrument Sans', body: 'Instrument Sans', mono: 'JetBrains Mono', displayWeights: '500;600' },
    radius: { sm: 2, md: 4, lg: 6, xl: 8 },
    density: 'compact',
    motifs: [
      { name: 'hairline-grid', use: 'Section headers aligned to a visible 1px rule; numbers in tabular display.', css: '.motif--hairline-grid { border-top: 1px solid var(--color-ink); padding-top: var(--space-3); }' },
      { name: 'index-number', use: 'Oversized section index (01, 02) in display weight, muted.', css: '.motif--index-number { font: var(--font-weight-semibold) var(--font-size-display-md) / 1 var(--font-family-display); letter-spacing: -0.03em; color: var(--color-rule); }' },
    ],
    layout: '12 columns, 24 gutters, everything flush-left on column lines; whitespace instead of boxes; rules instead of cards.',
    components: ['Tables without vertical lines, 1px rules, 13-14 text.', 'Cards rare: prefer rules and spacing.', 'Buttons 36px, radius 4.'],
    do: ['Replace boxes with alignment and rules.', 'Use one accent only.', 'Big tabular numbers for key metrics.'],
    dont: ['Shadows on everything.', 'Coloured icon tiles.', 'Centred layouts.'],
  },
  {
    id: 'warm-craft',
    seeds: { brand: { hue: 42, chroma: 0.13 }, neutral: { hue: 70, tint: 0.016 }, signature: { hue: 118, chroma: 0.08 }, accentFrom: 'neutral', warmSurface: true, brandStep: 700 },
    names: { brand: 'dat-nung', neutral: 'lanh', signature: 'oliu' },
    usage: { 'dat-nung': 'Terracotta: illustrations, brand bands, seals, tags; not for buttons (too close to error red).', lanh: 'Linen to dark brown: all neutrals and the ink-brown primary buttons.', oliu: 'Olive: secondary tags, notes, leaf details.' },
    type: { base: 16, ratio: 1.2, displayRatio: 1.414, displayFrom: 30, eyebrow: { size: 12, weight: 600, tracking: '0.14em', upper: true } },
    axes: { formality: 2, energy: 3, era: 2, warmth: 5, density: 2, richness: 4, seriousness: 2 },
    rationale: ["Primary buttons are dark brown ink, terracotta is brand: terracotta sits 15° from error red and would make every CTA look like a warning.", "Linen neutrals (hue 70) and olive signature are analogous-warm, so the palette feels hand-made rather than digital.", "Serif display at √2 jumps gives the menu/editorial voice; UI text stays sans."],
    name: 'Warm craft',
    tags: ['warm', 'craft', 'food', 'hospitality', 'cafe', 'handmade', 'serif', 'friendly', 'consumer', 'booking'],
    concept: 'Linen, terracotta and stitched paper bags: a hand-made, editorial warmth with a soft serif for headings and a stamp-like seal.',
    fits: ['F&B, cafes, bakeries', 'hotels and homestays', 'handmade goods'],
    avoid: ['enterprise tools', 'finance'],
    base: 'soft',
    fonts: { display: 'Fraunces', body: 'Be Vietnam Pro', mono: 'JetBrains Mono', displayWeights: '400;500;600', note: 'Fraunces 500 with soft optical size; italic for small flourishes only.' },
    fontsLatin: { display: 'Fraunces', body: 'DM Sans', mono: 'JetBrains Mono', displayWeights: '400;500;600' },
    radius: { sm: 6, md: 10, lg: 16, xl: 22 },
    density: 'regular',
    motifs: [
      { name: 'stitch', use: 'Dashed "stitched" border on coupons, notes, empty states.', css: '.motif--stitch { border: var(--border-width-strong) dashed var(--color-rule); border-radius: var(--radius-lg); padding: var(--space-5); }' },
      { name: 'seal', use: 'Round stamp seal with the brand initial; one per page, near the logo or a guarantee.', css: '.motif--seal { width: var(--space-16); height: var(--space-16); border-radius: var(--radius-pill); display: grid; place-items: center; border: var(--border-width-strong) solid var(--color-accent); color: var(--color-accent); font: italic var(--font-weight-medium) var(--font-size-xl) / 1 var(--font-family-display); transform: rotate(-8deg); }' },
    ],
    layout: 'Editorial columns with generous margins; photography large; asymmetric hero (text left 5 cols, image right 7).',
    components: ['Buttons 40px, radius 10, dark-brown ink primary; terracotta for seals, tags and brand bands.', 'Menu/product cards: photo first, name in serif 22.', 'Prices tabular, small caps currency.'],
    do: ['Real food/product photos.', 'Warm neutrals instead of grey.', 'Serif headings, sans UI.'],
    dont: ['Cold blue accents.', 'Emoji as icons.', 'Thick drop shadows.'],
  },
  {
    id: 'clinical-calm',
    seeds: { brand: { hue: 200, chroma: 0.1 }, neutral: { hue: 210, tint: 0.006 }, signature: { hue: 35, chroma: 0.07 }, signatureStep: 200 },
    names: { brand: 'xanh-ngoc', neutral: 'may', signature: 'hong' },
    usage: { 'xanh-ngoc': 'Calm teal: actions, links, progress.', may: 'Cloud greys with a hint of teal: surfaces and text.', hong: 'Soft coral: human moments (avatars, empty states, care notes); never status.' },
    type: { base: 17, ratio: 1.2, displayRatio: 1.25, displayFrom: 28, eyebrow: { size: 12, weight: 600, tracking: '0.08em', upper: true } },
    axes: { formality: 3, energy: 1, era: 4, warmth: 3, density: 2, richness: 2, seriousness: 4 },
    rationale: ["Teal is 50° from both the success green and the info blue, so actions never read as status.", "17px base: patients and older users read forms; the display ratio is modest (1.25) to stay calm.", "A warm coral signature balances the clinical teal (complementary temperature) for human moments only."],
    name: 'Clinical calm',
    tags: ['health', 'medical', 'clinic', 'calm', 'trust', 'accessible', 'public-service', 'booking', 'forms'],
    concept: 'A quiet clinic: clean white rooms, a calm teal, humanist type and very clear forms; reassurance through clarity.',
    fits: ['healthcare and pharmacy', 'insurance', 'appointments and forms-heavy services'],
    avoid: ['entertainment', 'brands needing drama'],
    base: 'soft',
    fonts: { display: 'Source Serif 4', body: 'Source Sans 3', mono: 'Source Code Pro', displayWeights: '500;600', note: 'Body 16-17 for readability; serif display only for page titles.' },
    fontsLatin: { display: 'Source Serif 4', body: 'Figtree', mono: 'Source Code Pro', displayWeights: '500;600' },
    radius: { sm: 6, md: 8, lg: 12, xl: 16 },
    density: 'regular',
    motifs: [{ name: 'calm-panel', use: 'Reassurance panels (what happens next, privacy): soft teal background, no border.', css: '.motif--calm-panel { background: var(--color-accent-soft); border-radius: var(--radius-lg); padding: var(--space-5) var(--space-6); }' }],
    layout: 'Single-column forms max 640; steps on top; help text beside on desktop.',
    components: ['Inputs 44-48px, labels always visible, errors in words.', 'One primary per step.', 'Large readable body 16-17.'],
    do: ['Explain next steps.', 'Use plain language.', 'Show progress.'],
    dont: ['Red for anything that is not an error.', 'Dense tables for patients.', 'Stock photos of smiling doctors.'],
  },
  {
    id: 'ledger-finance',
    seeds: { brand: { hue: 160, chroma: 0.09 }, neutral: { hue: 100, tint: 0.008 }, signature: { hue: 85, chroma: 0.11 }, accentFrom: 'neutral', brandStep: 800 },
    names: { brand: 'xanh-ngan-hang', neutral: 'so-cai', signature: 'vang' },
    usage: { 'xanh-ngan-hang': 'Bank green: header bands, brand moments; not for actions (green means positive/success in finance).', 'so-cai': 'Ledger paper to ink: neutrals and the ink primary buttons.', vang: 'Gold: double rules under totals, seals.' },
    type: { base: 15, ratio: 1.2, displayRatio: 1.333, displayFrom: 28, eyebrow: { size: 11, weight: 700, tracking: '0.12em', upper: true } },
    axes: { formality: 5, energy: 1, era: 2, warmth: 3, density: 4, richness: 2, seriousness: 5 },
    rationale: ["Buttons are ink, not green: in finance green means 'positive amount' and success; an action must not look like a gain.", "Paper neutrals slightly warm (hue 100) so the ledger feels like paper, not a spreadsheet.", "Numbers carry the hierarchy: tabular mono, 1.333 display ratio for balances."],
    name: 'Ledger',
    tags: ['finance', 'accounting', 'fintech', 'banking', 'ledger', 'numbers', 'trust', 'serious', 'b2b'],
    concept: 'A bound ledger: cream pages, deep bank green, gold double rules under totals, and numbers that line up perfectly.',
    fits: ['accounting and invoicing', 'banking and payments', 'investment reports'],
    avoid: ['playful consumer apps'],
    base: 'crisp',
    fonts: { display: 'Newsreader', body: 'Manrope', mono: 'JetBrains Mono', displayWeights: '500;600', note: 'Numbers in JetBrains Mono or Manrope tabular; serif only for headings.' },
    fontsLatin: { display: 'Newsreader', body: 'Figtree', mono: 'JetBrains Mono', displayWeights: '500;600' },
    radius: { sm: 2, md: 4, lg: 6, xl: 8 },
    density: 'compact',
    motifs: [{ name: 'double-rule', use: 'Totals and closing balances: double gold rule above the figure.', css: '.motif--double-rule { border-top: 3px double var(--color-signature); padding-top: var(--space-2); }' }],
    layout: 'Right-aligned numeric columns, decimal alignment, sticky totals row; 12 columns.',
    components: ['Amounts tabular mono, negatives in parentheses + err text.', 'Tables zebra-free, ledger lines only.', 'Buttons 36px, radius 4.'],
    do: ['Align every number on the decimal.', 'Green is brand (bands, headers); positive amounts use the ok colour with a sign, actions are ink.', 'Show currency once per column.'],
    dont: ['Charts without numbers.', 'Neon success colours.', 'Rounded card soup.'],
  },
  {
    id: 'civic-public',
    seeds: { brand: { hue: 255, chroma: 0.13 }, neutral: { hue: 255, tint: 0.005 }, signature: { hue: 85, chroma: 0.16 }, signatureStep: 300 },
    names: { brand: 'xanh-cong', neutral: 'xam', signature: 'vang' },
    usage: { 'xanh-cong': 'Civic blue: actions, links, official header; also info.', xam: 'Neutral greys at the same temperature.', vang: 'Official notice band (dark text).' },
    type: { base: 17, ratio: 1.2, displayRatio: 1.25, displayFrom: 28, eyebrow: { size: 13, weight: 700, tracking: '0.04em', upper: false } },
    axes: { formality: 4, energy: 2, era: 3, warmth: 2, density: 2, richness: 1, seriousness: 5 },
    rationale: ["Blue signals trust and is safe for colour-vision deficiencies; it doubles as info so there is one blue.", "17px base and large targets: citizens of every age and device.", "Modest display ratio (1.25): clarity over drama."],
    name: 'Civic public',
    tags: ['government', 'public-service', 'civic', 'accessible', 'forms', 'education', 'trust', 'neutral'],
    concept: 'A public-service portal: plain, high-contrast, generous type, numbered steps and unmistakable actions; dignity through clarity.',
    fits: ['government and public services', 'education portals', 'NGOs'],
    avoid: ['luxury', 'brand-led marketing'],
    base: 'industrial',
    fonts: { display: 'Public Sans', body: 'Public Sans', mono: 'Roboto Mono', displayWeights: '600;700', note: 'Body 17; headings 700; never light weights.' },
    fontsLatin: { display: 'Public Sans', body: 'Public Sans', mono: 'Roboto Mono', displayWeights: '600;700' },
    radius: { sm: 2, md: 4, lg: 4, xl: 6 },
    density: 'large',
    motifs: [{ name: 'official-banner', use: 'Top strip stating the official site/service. Once, at the very top.', css: '.motif--official-banner { background: var(--color-brand); color: var(--color-on-brand); padding: var(--space-1) var(--space-4); font-size: var(--font-size-xs); }' }],
    layout: 'Single column 720 for tasks; numbered step list; left nav for long guides.',
    components: ['Buttons 48px, radius 4, full labels ("Nộp hồ sơ").', 'Inputs 48px, hint above field.', 'Error summary at top of form linking to fields.'],
    do: ['Number steps.', 'Write at plain-language level.', 'Make focus extra visible.'],
    dont: ['Icons without labels.', 'Grey text below 4.5:1.', 'Decorative imagery.'],
  },
  {
    id: 'playful-bold',
    seeds: { brand: { hue: 290, chroma: 0.2 }, neutral: { hue: 290, tint: 0.01 }, signature: { hue: 90, chroma: 0.17 }, signatureStep: 300 },
    names: { brand: 'tim', neutral: 'muc', signature: 'nang' },
    usage: { tim: 'Violet: actions and highlights.', muc: 'Violet-tinted neutrals and the ink used for hard shadows.', nang: 'Sunshine: stickers, rewards, delight; never text.' },
    type: { base: 16, ratio: 1.25, displayRatio: 1.414, displayFrom: 32, eyebrow: { size: 13, weight: 700, tracking: '0.02em', upper: false } },
    axes: { formality: 1, energy: 5, era: 5, warmth: 4, density: 2, richness: 4, seriousness: 1 },
    rationale: ["Violet + yellow are near-complementary: maximum energy with only two hues.", "Neutrals tinted violet so the hard ink shadows belong to the brand.", "Bigger UI ratio (1.25) and display from 32: playful products need bold hierarchy."],
    name: 'Playful bold',
    tags: ['playful', 'consumer', 'education', 'kids', 'social', 'bold', 'colorful', 'app', 'gamified'],
    concept: 'A sticker book: chunky shapes with hard offset shadows, a confident violet, sunshine yellow highlights and a characterful grotesque.',
    fits: ['learning apps', 'community and social', 'consumer onboarding'],
    avoid: ['finance back-office', 'medical'],
    base: 'soft',
    fonts: { display: 'Bricolage Grotesque', body: 'Hanken Grotesk', mono: 'JetBrains Mono', displayWeights: '600;700;800', note: 'Bricolage 700-800 tight for display; body Hanken 16.' },
    fontsLatin: { display: 'Bricolage Grotesque', body: 'Outfit', mono: 'JetBrains Mono', displayWeights: '600;700;800' },
    radius: { sm: 8, md: 12, lg: 20, xl: 28 },
    hardShadow: true,
    density: 'regular',
    motifs: [
      { name: 'sticker', use: 'Rotated badge sticker with hard shadow for new/rewards. Max two per view.', css: '.motif--sticker { display: inline-block; padding: var(--space-1) var(--space-3); background: var(--color-signature); color: var(--color-ink); border: var(--border-width-strong) solid var(--color-ink); border-radius: var(--radius-pill); box-shadow: 3px 3px 0 var(--color-ink); transform: rotate(-4deg); font-weight: var(--font-weight-bold); }' },
      { name: 'hard-card', use: 'Feature cards with 2px ink border and hard offset shadow.', css: '.motif--hard-card { border: var(--border-width-strong) solid var(--color-ink); border-radius: var(--radius-lg); box-shadow: 6px 6px 0 var(--color-ink); background: var(--color-surface); padding: var(--space-6); }' },
    ],
    layout: 'Big rounded sections, asymmetric illustration slots, 8px grid, mobile-first.',
    components: ['Buttons 44px, radius 12, 2px ink border on secondary.', 'Progress as chunky bars.', 'Empty states with a sticker + one line.'],
    do: ['Commit to the hard shadow style everywhere or nowhere.', 'Use yellow for delight moments only.'],
    dont: ['Soft blurry shadows mixed with hard ones.', 'Gradient blobs.', 'More than 3 accent colours per view.'],
  },
  {
    id: 'dark-console',
    seeds: { brand: { hue: 215, chroma: 0.12 }, neutral: { hue: 250, tint: 0.012 }, signature: { hue: 145, chroma: 0.16 }, dark: true },
    names: { brand: 'cyan', neutral: 'dem', signature: 'lan' },
    usage: { cyan: 'Cyan: actions, selection, focus.', dem: 'Night slate neutrals (blue-tinted, never pure black).', lan: 'Phosphor green: live/healthy indicators only (same meaning as success).' },
    type: { base: 14, ratio: 1.2, displayRatio: 1.333, displayFrom: 26, eyebrow: { size: 11, weight: 500, tracking: '0.1em', upper: true } },
    axes: { formality: 3, energy: 3, era: 5, warmth: 1, density: 5, richness: 2, seriousness: 4 },
    rationale: ["Accent is cyan, not green: in a monitoring console green means healthy; a green button would read as a status.", "Neutrals are blue-slate, never #000, to keep depth between panels.", "14px base: dense operator screens; mono carries data."],
    name: 'Dark console',
    tags: ['dark', 'developer', 'monitoring', 'devops', 'terminal', 'technical', 'data', 'night', 'security'],
    concept: 'A night-shift control console: deep slate panes, phosphor-green signals, monospace readouts and a dotted grid behind the canvas.',
    fits: ['developer tools', 'monitoring and incident consoles', 'security dashboards'],
    avoid: ['consumer retail', 'print-like editorial'],
    base: 'crisp',
    dark: true,
    fonts: { display: 'Geist', body: 'Geist', mono: 'Geist Mono', displayWeights: '500;600', note: 'Mono for every value and id; display Geist 600 tight.' },
    fontsLatin: { display: 'Geist', body: 'Geist', mono: 'Geist Mono', displayWeights: '500;600' },
    radius: { sm: 4, md: 6, lg: 8, xl: 10 },
    density: 'compact',
    motifs: [{ name: 'dot-grid', use: 'Background of canvases and empty areas (subtle dotted grid).', css: '.motif--dot-grid { background-image: radial-gradient(var(--color-rule) 1px, transparent 1px); background-size: var(--space-5) var(--space-5); }' }],
    layout: 'Dense panels with 1px dividers, resizable split views, status bar at bottom.',
    components: ['Values and timestamps in mono 12-13.', 'Status dots + labels; pulsing only for live incidents.', 'Buttons 32-36px.'],
    do: ['Green means healthy/live only; actions are cyan.', 'Use mono for data.', 'Avoid pure black.'],
    dont: ['Neon glows.', 'Purple-blue gradients.', 'Light grey text below 4.5:1.'],
  },
  {
    id: 'nature-organic',
    seeds: { brand: { hue: 135, chroma: 0.07 }, neutral: { hue: 90, tint: 0.014 }, signature: { hue: 50, chroma: 0.1 }, accentFrom: 'neutral', warmSurface: true, brandStep: 700 },
    names: { brand: 'xo-thom', neutral: 'yen', signature: 'dat-set' },
    usage: { 'xo-thom': 'Sage: brand bands, illustrations, charts; not actions (reads as success).', yen: 'Oat paper to bark: neutrals and the bark-ink buttons.', 'dat-set': 'Clay: tags and field-note marks.' },
    type: { base: 16, ratio: 1.2, displayRatio: 1.414, displayFrom: 28, eyebrow: { size: 12, weight: 700, tracking: '0.12em', upper: true } },
    axes: { formality: 3, energy: 1, era: 2, warmth: 4, density: 2, richness: 3, seriousness: 3 },
    rationale: ["Sage is the identity but not the action colour: green buttons would be confused with success states.", "Oat neutrals (hue 90) and clay signature are analogous earth tones; low chroma everywhere keeps it calm.", "Serif display at √2 jumps for an editorial, notebook voice."],
    name: 'Nature organic',
    tags: ['nature', 'wellness', 'agriculture', 'sustainability', 'organic', 'calm', 'serif', 'earthy', 'consumer'],
    concept: 'A field notebook: oat paper, sage and clay pigments, a literary serif and pressed-leaf line drawings.',
    fits: ['wellness and spa', 'agriculture and food origin', 'sustainability reporting'],
    avoid: ['technical consoles'],
    base: 'soft',
    fonts: { display: 'Newsreader', body: 'Nunito Sans', mono: 'JetBrains Mono', displayWeights: '400;500;600', note: 'Newsreader 500, italic for quotes; body Nunito Sans 16.' },
    fontsLatin: { display: 'Newsreader', body: 'DM Sans', mono: 'JetBrains Mono', displayWeights: '400;500;600' },
    radius: { sm: 4, md: 8, lg: 14, xl: 20 },
    density: 'regular',
    motifs: [{ name: 'field-note', use: 'Quoted notes and origin stories: italic serif with a clay left margin mark.', css: '.motif--field-note { font: italic var(--font-weight-regular) var(--font-size-lg) / 1.5 var(--font-family-display); color: var(--color-ink); padding-left: var(--space-5); border-left: var(--border-width) solid var(--color-signature); }' }],
    layout: 'Editorial 12 columns, wide margins, images with natural crops, text measure 60-70ch.',
    components: ['Buttons 40px radius 8.', 'Cards flat on paper with hairline.', 'Data with small sage charts.'],
    do: ['Natural photography.', 'Generous leading.', 'Muted pigments.'],
    dont: ['Neon greens.', 'Leaf emoji.', 'Organic blob shapes.'],
  },
  {
    id: 'ops-dense',
    seeds: { brand: { hue: 255, chroma: 0.17 }, neutral: { hue: 245, tint: 0.006 }, signature: { hue: 55, chroma: 0.17 }, signatureStep: 500 },
    names: { brand: 'lam', neutral: 'xam', signature: 'cam' },
    usage: { lam: 'Signal blue: actions and selection (also info).', xam: 'Cool greys for dense tables.', cam: 'Orange: in-transit/attention markers (strips, dots), never text below 18px.' },
    type: { base: 14, ratio: 1.125, displayRatio: 1.333, displayFrom: 24, eyebrow: { size: 11, weight: 600, tracking: '0.08em', upper: true } },
    axes: { formality: 3, energy: 2, era: 4, warmth: 2, density: 5, richness: 1, seriousness: 4 },
    rationale: ["14px base and a tight 1.125 UI ratio: 50 rows on screen without losing scanability.", "Orange is a signature for 'moving' states, 28° from error red; it is never used for errors and always has a label.", "One blue for actions + info."],
    name: 'Ops dense',
    tags: ['logistics', 'dense', 'dashboard', 'table', 'operations', 'supply-chain', 'condensed', 'data', 'b2b', 'internal'],
    concept: 'A dispatch board: compact condensed type, cool greys, signal colours used sparingly, everything aligned to columns so 50 rows fit on screen.',
    fits: ['logistics and dispatch', 'inventory and ERP screens', 'call centres'],
    avoid: ['marketing', 'consumer onboarding'],
    base: 'crisp',
    fonts: { display: 'Archivo', body: 'Archivo', mono: 'IBM Plex Mono', displayWeights: '600;700', note: 'Archivo at width 87-100 for UI; display at 600.' },
    fontsLatin: { display: 'Archivo', body: 'Archivo', mono: 'IBM Plex Mono', displayWeights: '600;700' },
    radius: { sm: 2, md: 3, lg: 4, xl: 6 },
    density: 'compact',
    motifs: [{ name: 'status-strip', use: 'A 3px left strip on table rows only for exceptional states (late, blocked).', css: '.motif--status-strip { box-shadow: inset 3px 0 0 var(--color-signature); }' }],
    layout: 'Full-width tables, sticky headers and first column, filters in one toolbar row, 4px grid.',
    components: ['Rows 36px, text 13, header 12.', 'Buttons 32px.', 'IDs mono 12.'],
    do: ['Fit more rows; trim chrome.', 'Right-align numbers.', 'Use colour only for exceptions.'],
    dont: ['Cards for table data.', 'Large empty headers.', 'Rainbow status chips.'],
  },
  {
    id: 'editorial-luxe',
    seeds: { brand: { hue: 10, chroma: 0.12 }, neutral: { hue: 75, tint: 0.01 }, accentFrom: 'neutral', warmSurface: true, brandStep: 700 },
    names: { brand: 'do-ruou', neutral: 'nga' },
    usage: { 'do-ruou': 'Burgundy: editorial accents, sale/limited tags with words, brand moments; not buttons.', nga: 'Ivory to black: neutrals and black buttons.' },
    type: { base: 16, ratio: 1.2, displayRatio: 1.5, displayFrom: 34, eyebrow: { size: 11, weight: 600, tracking: '0.24em', upper: true } },
    axes: { formality: 5, energy: 2, era: 2, warmth: 3, density: 1, richness: 3, seriousness: 4 },
    rationale: ["Black buttons, burgundy accents: burgundy is 17° from error red; luxury convention is black anyway.", "Display ratio 1.5 (34/51/77/115): fashion editorial needs extreme scale contrast.", "Ivory neutrals (hue 75) instead of white for a printed-page feel."],
    name: 'Editorial luxe',
    tags: ['luxury', 'fashion', 'editorial', 'premium', 'serif', 'minimal', 'magazine', 'storefront', 'beauty'],
    concept: 'A fashion magazine: ivory pages, black type, one burgundy accent, high-contrast Didone-like display, wide margins and thin rules.',
    fits: ['fashion and beauty', 'premium hospitality', 'galleries and publishing'],
    avoid: ['dense operational tools'],
    base: 'crisp',
    fonts: { display: 'Playfair Display', body: 'Hanken Grotesk', mono: 'JetBrains Mono', displayWeights: '400;500;600', note: 'Playfair 400 at 48+ only; body Hanken 15-16; small caps eyebrows.' },
    fontsLatin: { display: 'Bodoni Moda', body: 'Instrument Sans', mono: 'JetBrains Mono', displayWeights: '400;500' },
    radius: { sm: 0, md: 0, lg: 2, xl: 4 },
    density: 'regular',
    motifs: [{ name: 'thin-rule-caption', use: 'Image captions and section openers: thin rule + spaced small caps.', css: '.motif--thin-rule-caption { border-top: 1px solid var(--color-ink); padding-top: var(--space-2); font-size: var(--font-size-2xs); letter-spacing: 0.24em; text-transform: uppercase; }' }],
    layout: 'Asymmetric 12-column editorial grid, full-bleed imagery, huge display titles overlapping image edges sparingly.',
    components: ['Buttons square (radius 0), 44px, uppercase 12/600 tracking 0.16em.', 'Product titles serif 24.', 'Prices quiet, body size.'],
    do: ['Huge type, few words.', 'Lots of ivory space.', 'One accent.'],
    dont: ['Rounded corners.', 'Badges everywhere.', 'Icons in buttons.'],
  },
];

// ------------------------------------------------------------------ build + senior audit
const ROLE_USE = {
  canvas: 'page background', surface: 'cards, tables, inputs', 'surface-sunken': 'table headers, wells', 'surface-hover': 'hover', rule: 'dividers',
  border: 'control borders', 'border-strong': 'emphasised borders', subtle: 'placeholders', muted: 'secondary text', ink: 'body text',
  accent: 'primary actions, links', 'accent-hover': 'hover/pressed', 'accent-soft': 'selected backgrounds', 'accent-text': 'selected text',
  'on-accent': 'text on primary buttons', focus: 'focus ring', 'focus-ring': 'focus halo', brand: 'large brand surfaces', 'on-brand': 'text on brand surfaces', signature: 'signature motif',
};

let failed = 0;
fs.mkdirSync(OUT, { recursive: true });
for (const f of fs.readdirSync(OUT)) if (f.endsWith('.json')) fs.rmSync(path.join(OUT, f));
for (const g of GUIDES) {
  const pal = buildPalette(g.seeds, g.names);
  const hexOf = (v) => {
    if (v.startsWith('#')) return v;
    const i = v.lastIndexOf('-');
    return pal.ramps[v.slice(0, i)][v.slice(i + 1)];
  };
  // Palette = full ramps; each step says which roles use it, each ramp says what it is for.
  const usedBy = {};
  for (const [role, v] of Object.entries(pal.roles)) if (!v.startsWith('#')) (usedBy[v] ||= []).push(ROLE_USE[role] || role);
  const palette = {};
  for (const [name, r] of Object.entries(pal.ramps)) {
    for (const [step, hex] of Object.entries(r)) {
      const key = `${name}-${step}`;
      // Ramp intent once (on step 500) + the roles each step serves; unused steps stay available for states.
      palette[key] = { hex, usage: `${step === '500' ? `${g.usage[name] || ''} ` : ''}${usedBy[key] ? `Used for: ${usedBy[key].join(', ')}.` : step === '500' ? '' : 'Reserve step (hover/pressed/charts).'}`.trim() };
    }
  }
  const rolesHex = Object.fromEntries(Object.entries(pal.roles).map(([k, v]) => [k, hexOf(v)]));
  const onFill = g.seeds.dark ? pal.ramps[g.names.neutral][950] : '#ffffff';
  const audit = auditColors({ roles: rolesHex, status: pal.status, onFill });
  const ts = typeScale(g.type);
  audit.push(...auditTypeScale({ ui: ['2xs', 'xs', 'sm', 'md', 'lg', 'xl', '2xl'].map((k) => ts.ui[k]), display: Object.values(ts.display) }, { body: g.type.base, surface: g.tags.includes('storefront') || g.tags.includes('editorial') ? 'marketing' : 'product' }));
  for (const a of audit) {
    if (a.level === 'error') { console.error(`${g.id}: [${a.rule}] ${a.message}`); failed++; }
    else if (a.level === 'warn') console.warn(`${g.id}: warn [${a.rule}] ${a.message}`);
  }
  const contrasts = [['ink', 'surface'], ['muted', 'surface'], ['accent', 'surface'], ['on-accent', 'accent'], ['on-brand', 'brand'], ['border', 'surface']].map(([a, b]) => ({ fg: a, bg: b, ratio: Math.round(contrast(rolesHex[a], rolesHex[b]) * 100) / 100 }));
  const data = {
    ...g,
    type: { ...g.type, ui: ts.ui, display: [ts.display['display-xl'], ts.display['display-lg'], ts.display['display-md'], ts.display['display-sm']] },
    palette,
    roles: pal.roles,
    status: pal.status,
    onFill,
    shadowTint: pal.ramps[g.names.neutral][950],
    paletteNotes: pal.notes,
    contrasts,
    audit: audit.filter((a) => a.level !== 'error'),
    dark: Boolean(g.seeds.dark),
  };
  delete data.usage;
  fs.writeFileSync(path.join(OUT, `${g.id}.json`), JSON.stringify(data, null, 2) + '\n');
}
if (failed) {
  console.error(`${failed} senior-audit error(s).`);
  process.exit(1);
}
console.log(`Wrote ${GUIDES.length} style guides to ${path.relative(ROOT, OUT)} (palettes generated in OKLCH; contrast, semantic and type audits pass).`);
