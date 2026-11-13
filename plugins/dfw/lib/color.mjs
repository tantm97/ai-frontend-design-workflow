// Color system the way a senior designer builds one: perceptual (OKLCH) ramps from a few
// decisions (brand hue, neutral temperature, harmony), roles picked by contrast targets instead
// of by eye, status colours harmonised to the palette and kept semantically distinct from the
// accent, and an audit that explains every problem.

// ------------------------------------------------------------------ conversions
const toLin = (v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const toGam = (v) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055);

export function hexToRgb(hex) {
  const h = hex.replace('#', '');
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
}
export function rgbToHex(rgb) {
  return '#' + rgb.map((v) => Math.round(Math.min(1, Math.max(0, v)) * 255).toString(16).padStart(2, '0')).join('');
}

export function hexToOklch(hex) {
  const [r, g, b] = hexToRgb(hex).map(toLin);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return { l: L, c: Math.hypot(A, B), h: ((Math.atan2(B, A) * 180) / Math.PI + 360) % 360 };
}

function oklchToLinear({ l, c, h }) {
  const a = c * Math.cos((h * Math.PI) / 180);
  const b = c * Math.sin((h * Math.PI) / 180);
  const L_ = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const M_ = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const S_ = (l - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [4.0767416621 * L_ - 3.3077115913 * M_ + 0.2309699292 * S_, -1.2684380046 * L_ + 2.6097574011 * M_ - 0.3413193965 * S_, -0.0041960863 * L_ - 0.7034186147 * M_ + 1.707614701 * S_];
}

/** OKLCH -> hex, reducing chroma until the colour fits sRGB (keeps lightness and hue). */
export function oklchToHex({ l, c, h }) {
  let lo = 0;
  let hi = c;
  const inGamut = (cc) => oklchToLinear({ l, c: cc, h }).every((v) => v >= -0.0005 && v <= 1.0005);
  if (inGamut(c)) return rgbToHex(oklchToLinear({ l, c, h }).map(toGam));
  for (let i = 0; i < 24; i++) {
    const mid = (lo + hi) / 2;
    if (inGamut(mid)) lo = mid;
    else hi = mid;
  }
  return rgbToHex(oklchToLinear({ l, c: lo, h }).map(toGam));
}

export function luminance(hex) {
  const [r, g, b] = hexToRgb(hex).map(toLin);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
export function contrast(a, b) {
  const [x, y] = [luminance(a), luminance(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}
export const hueDistance = (a, b) => {
  const d = Math.abs(a - b) % 360;
  return Math.min(d, 360 - d);
};

// ------------------------------------------------------------------ ramps
export const STEPS = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950];
// Perceptually even lightness targets (OKLCH L). Light steps are close together because
// the eye separates light tints less; dark steps hold text contrast.
const L_TARGET = { 50: 0.977, 100: 0.951, 200: 0.903, 300: 0.836, 400: 0.748, 500: 0.655, 600: 0.566, 700: 0.484, 800: 0.405, 900: 0.33, 950: 0.255 };
// Chroma envelope: tints and shades carry less chroma than the middle (avoids neon tints and muddy darks).
const C_SHAPE = { 50: 0.12, 100: 0.24, 200: 0.42, 300: 0.62, 400: 0.82, 500: 0.96, 600: 1, 700: 0.94, 800: 0.82, 900: 0.66, 950: 0.5 };

/**
 * A ramp of 11 steps around a hue. `chroma` = peak chroma (0.02 tinted neutral ... 0.2 vivid).
 * `hueShift` rotates darker steps slightly (warm darks look richer: positive towards red/yellow).
 */
export function ramp({ hue, chroma, hueShift = 0, lightness = L_TARGET }) {
  const out = {};
  for (const s of STEPS) {
    const t = (STEPS.indexOf(s) - 5) / 5; // -1 .. 1
    out[s] = oklchToHex({ l: lightness[s], c: chroma * C_SHAPE[s], h: (hue + hueShift * t + 360) % 360 });
  }
  return out;
}

/** Neutral ramp tinted toward a hue (paper, steel, linen...). tint 0.004 (almost grey) - 0.02 (clearly warm/cool). */
export function neutralRamp({ hue, tint = 0.01 }) {
  const out = {};
  for (const s of STEPS) out[s] = oklchToHex({ l: L_TARGET[s], c: tint * (0.6 + 0.4 * C_SHAPE[s]), h: hue });
  return out;
}

export const HARMONY = { analogous: 30, 'analogous-wide': 60, triadic: 120, 'split-complementary': 150, complementary: 180 };

// Semantic status hues (OKLCH): green, blue, amber, red. They are shifted (never more than 18°)
// toward the palette's temperature so they belong to it, and away from the accent when too close.
const STATUS_HUE = { ok: 150, info: 250, warn: 75, err: 27 };

/** Pick the lightest step of a ramp that reaches `min` contrast on `bg` (keeps hierarchy light, still legible). */
export function stepFor(r, bg, min, { from = 'light' } = {}) {
  const order = from === 'light' ? STEPS : [...STEPS].reverse();
  for (const s of order) if (contrast(r[s], bg) >= min) return s;
  return from === 'light' ? 950 : 50;
}

/**
 * Build a full palette + roles + status from a few decisions.
 * seeds: { brand: {hue, chroma} | {hex}, neutral: {hue?, tint}, signature?: {hue|harmony, chroma}, accentFrom?: 'brand'|'signature', dark?: bool }
 * names: { brand: 'son', neutral: 'giay', signature: 'kim' } (palette names in the product's world)
 */
export function buildPalette(seeds, names = {}) {
  const brandSeed = seeds.brand.hex ? hexToOklch(seeds.brand.hex) : { h: seeds.brand.hue, c: seeds.brand.chroma };
  const brandHue = brandSeed.h;
  const brandChroma = Math.min(0.24, Math.max(0.04, seeds.brand.chroma ?? brandSeed.c));
  const nHue = seeds.neutral?.hue ?? brandHue;
  const tint = seeds.neutral?.tint ?? 0.01;
  const sigSeed = seeds.signature || null;
  const sigHue = sigSeed ? (sigSeed.hue ?? (brandHue + (HARMONY[sigSeed.harmony] ?? 180)) % 360) : null;

  const n = { brand: names.brand || 'brand', neutral: names.neutral || 'neutral', signature: names.signature || 'signature' };
  const ramps = {
    [n.brand]: ramp({ hue: brandHue, chroma: brandChroma, hueShift: seeds.brand.hueShift ?? 0 }),
    [n.neutral]: neutralRamp({ hue: nHue, tint }),
  };
  if (sigSeed) ramps[n.signature] = ramp({ hue: sigHue, chroma: sigSeed.chroma ?? brandChroma, hueShift: sigSeed.hueShift ?? 0 });

  // Accent source: brand ramp (default), signature ramp, or the neutral ramp (ink buttons: luxury, editorial,
  // or whenever the brand hue collides with a status meaning, e.g. a red brand).
  const accentName = seeds.accentFrom === 'signature' && sigSeed ? n.signature : seeds.accentFrom === 'neutral' ? n.neutral : n.brand;
  const accentRamp = ramps[accentName];
  const accentHue = accentName === n.signature ? sigHue : accentName === n.neutral ? nHue : brandHue;
  const accentIsChromatic = accentName !== n.neutral;
  // Status: semantic hues stay recognisable (red is red); only a small temperature pull (≤ 6°) toward
  // the neutrals so they belong to the palette. Collisions with the accent are reported by the audit,
  // not hidden by bending the status hue.
  const statusRamps = {};
  const statusNotes = [];
  for (const [k, base] of Object.entries(STATUS_HUE)) {
    const diff = ((nHue - base + 540) % 360) - 180;
    const pull = Math.max(-6, Math.min(6, diff * Math.min(1, tint / 0.02) * 0.1));
    statusRamps[k] = ramp({ hue: (base + pull + 360) % 360, chroma: k === 'warn' ? 0.16 : Math.min(0.19, Math.max(0.13, brandChroma)) });
  }
  // Info = accent when they are close (a second blue next to the accent is noise).
  if (accentIsChromatic && hueDistance(STATUS_HUE.info, accentHue) < 40) {
    statusRamps.info = accentRamp;
    statusNotes.push('info uses the accent ramp (the accent is blue; a second blue would compete).');
  }

  const N = ramps[n.neutral];
  const B = accentRamp;
  const dark = Boolean(seeds.dark);
  const roles = {};
  const ref = (rampName, step) => `${rampName}-${step}`;
  if (!dark) {
    const surface = '#ffffff';
    roles.canvas = ref(n.neutral, 50);
    roles.surface = seeds.warmSurface ? ref(n.neutral, 50) : surface;
    if (seeds.warmSurface) roles.canvas = ref(n.neutral, 100);
    const surf = roles.surface.startsWith('#') ? roles.surface : N[50];
    roles['surface-sunken'] = ref(n.neutral, 100);
    roles['surface-hover'] = ref(n.neutral, 100);
    roles.rule = ref(n.neutral, 200);
    roles.border = ref(n.neutral, stepFor(N, surf, 3));
    roles['border-strong'] = ref(n.neutral, stepFor(N, surf, 4.5));
    roles.subtle = ref(n.neutral, stepFor(N, surf, 4.5));
    roles.muted = ref(n.neutral, stepFor(N, N[100], 5.2));
    if (roles.muted === roles.subtle) roles.muted = ref(n.neutral, STEPS[Math.min(10, STEPS.indexOf(Number(roles.subtle.split('-').pop())) + 1)]);
    roles.ink = ref(n.neutral, stepFor(N, N[100], 13) === 950 ? 950 : Math.max(900, stepFor(N, N[100], 13)));
    const acc = seeds.accentStep ?? (accentIsChromatic ? stepFor(B, surf, 4.5) : 900);
    roles.accent = ref(accentName, acc);
    roles['accent-hover'] = ref(accentName, STEPS[Math.min(10, STEPS.indexOf(acc) + 1)]);
    if (accentIsChromatic) {
      roles['accent-soft'] = ref(accentName, 50);
      roles['accent-text'] = ref(accentName, stepFor(B, B[50], 4.5) < 700 ? 700 : stepFor(B, B[50], 4.5));
      roles['focus-ring'] = ref(accentName, 200);
    } else {
      // Ink accent: selection must still be visible on the surface, so step down from the surface tone.
      roles['accent-soft'] = ref(n.neutral, seeds.warmSurface ? 200 : 100);
      roles['accent-text'] = ref(n.neutral, 900);
      roles['focus-ring'] = ref(n.neutral, 300);
    }
    roles['on-accent'] = contrast('#ffffff', B[acc]) >= 4.5 ? '#ffffff' : ref(n.neutral, 950);
    roles.focus = roles.accent;
    roles.brand = ref(n.brand, seeds.brandStep ?? 800);
    roles['on-brand'] = ref(sigSeed && seeds.onBrandSignature ? n.signature : n.neutral, sigSeed && seeds.onBrandSignature ? 100 : 50);
    roles.signature = sigSeed ? ref(n.signature, seeds.signatureStep ?? 400) : ref(n.brand, 400);
  } else {
    roles.canvas = ref(n.neutral, 950);
    roles.surface = ref(n.neutral, 900);
    roles['surface-sunken'] = ref(n.neutral, 800);
    roles['surface-hover'] = ref(n.neutral, 800);
    roles.rule = ref(n.neutral, 700);
    roles.border = ref(n.neutral, stepFor(N, N[900], 3, { from: 'dark' }));
    roles['border-strong'] = ref(n.neutral, 400);
    roles.subtle = ref(n.neutral, stepFor(N, N[900], 4.5, { from: 'dark' }));
    roles.muted = ref(n.neutral, stepFor(N, N[900], 7, { from: 'dark' }));
    roles.ink = ref(n.neutral, 50);
    const acc = seeds.accentStep ?? stepFor(B, N[900], 6, { from: 'dark' });
    roles.accent = ref(accentName, acc);
    roles['accent-hover'] = ref(accentName, STEPS[Math.max(0, STEPS.indexOf(acc) - 1)]);
    roles['accent-soft'] = ref(accentName, 950);
    roles['accent-text'] = ref(accentName, 300);
    roles['on-accent'] = ref(n.neutral, 950);
    roles.focus = roles.accent;
    roles['focus-ring'] = ref(accentName, 800);
    roles.brand = ref(n.neutral, 800);
    roles['on-brand'] = ref(n.neutral, 50);
    roles.signature = sigSeed ? ref(n.signature, 400) : ref(n.brand, 400);
  }

  const status = {};
  for (const [k, r] of Object.entries(statusRamps)) {
    if (!dark) {
      status[k] = k === 'warn' ? r[300] : r[stepFor(r, '#ffffff', 4.6)];
      status[`${k}-tint`] = r[50];
      status[`${k}-text`] = r[stepFor(r, r[50], 6) < 700 ? 700 : stepFor(r, r[50], 6)];
    } else {
      status[k] = r[400];
      status[`${k}-tint`] = r[950];
      status[`${k}-text`] = r[300];
    }
  }
  return { ramps, roles, status, names: n, notes: statusNotes, seeds: { brandHue, brandChroma, nHue, tint, sigHue } };
}

// ------------------------------------------------------------------ audit
const ROLE_PAIRS = [
  ['ink', 'surface', 7, 'MUST', 'Body text'], ['ink', 'canvas', 7, 'MUST', 'Body text on canvas'], ['muted', 'surface', 4.5, 'MUST', 'Secondary text'],
  ['muted', 'canvas', 4.5, 'MUST', 'Secondary text on canvas'], ['muted', 'surface-sunken', 4.5, 'MUST', 'Secondary text in table headers'],
  ['subtle', 'surface', 4.5, 'SHOULD', 'Placeholder text'], ['border', 'surface', 3, 'MUST', 'Control boundary (WCAG 1.4.11)'],
  ['accent', 'surface', 4.5, 'MUST', 'Links and accent text'], ['on-accent', 'accent', 4.5, 'MUST', 'Primary button label'], ['on-accent', 'accent-hover', 4.5, 'MUST', 'Primary button label (hover)'],
  ['accent-text', 'accent-soft', 4.5, 'MUST', 'Selected item text'], ['focus', 'surface', 3, 'MUST', 'Focus indicator'], ['on-brand', 'brand', 4.5, 'MUST', 'Text on brand surfaces'],
];

/**
 * Audit resolved role colours (hex) like a senior reviewer. Returns [{level: 'error'|'warn'|'info', rule, message}].
 * input: { roles: {role: hex}, status: {ok, err, warn, info, *-tint, *-text}, onFill, typeSizes?: {body, displays: []}, families?: [], radii?: [], surface?: 'product'|'marketing', exceptions?: [rule] }
 */
export function auditColors({ roles, status, onFill = '#ffffff' }, { exceptions = [] } = {}) {
  const out = [];
  const add = (level, rule, message) => out.push({ level: exceptions.includes(rule) && level === 'error' ? 'info' : level, rule, message: exceptions.includes(rule) ? `${message} (recorded exception)` : message });
  for (const [fg, bg, min, lvl, what] of ROLE_PAIRS) {
    if (!roles[fg] || !roles[bg]) continue;
    const r = contrast(roles[fg], roles[bg]);
    if (r < min) add(lvl === 'MUST' ? 'error' : 'warn', 'contrast', `${what}: ${fg} on ${bg} is ${r.toFixed(2)}:1, needs ${min}:1.`);
  }
  for (const s of ['ok', 'err']) if (status[s] && contrast(onFill, status[s]) < 4.5) add('error', 'contrast', `Text on status.${s} fill is ${contrast(onFill, status[s]).toFixed(2)}:1.`);
  for (const s of ['ok', 'info', 'warn', 'err']) if (status[`${s}-text`] && contrast(status[`${s}-text`], status[`${s}-tint`]) < 4.5) add('error', 'contrast', `status.${s}-text on its tint is ${contrast(status[`${s}-text`], status[`${s}-tint`]).toFixed(2)}:1.`);

  const acc = hexToOklch(roles.accent);
  for (const s of ['err', 'ok']) {
    if (!status[s]) continue;
    const st = hexToOklch(status[s]);
    const d = hueDistance(acc.h, st.h);
    if (acc.c > 0.05 && d < 25) add('error', 'accent-vs-status', `Accent (${roles.accent}) and status.${s} (${status[s]}) are only ${d.toFixed(0)}° apart: users will read the primary action as ${s === 'err' ? 'an error/destructive' : 'a success'} signal. Move the accent, or make the brand hue a surface/ornament colour and use a different accent.`);
    else if (acc.c > 0.05 && d < 40) add('warn', 'accent-vs-status', `Accent and status.${s} are ${d.toFixed(0)}° apart; keep icons + words on every ${s} message.`);
  }
  // Neutrals: one temperature.
  const neutralRoles = ['canvas', 'surface-sunken', 'rule', 'border', 'muted', 'ink'].filter((r) => roles[r]);
  const tinted = neutralRoles.map((r) => ({ r, ...hexToOklch(roles[r]) })).filter((x) => x.c > 0.006);
  for (let i = 1; i < tinted.length; i++) {
    if (hueDistance(tinted[0].h, tinted[i].h) > 40) {
      add('warn', 'neutral-temperature', `Neutrals mix temperatures: ${tinted[0].r} hue ${tinted[0].h.toFixed(0)}° vs ${tinted[i].r} ${tinted[i].h.toFixed(0)}°. Tint all neutrals toward one hue.`);
      break;
    }
  }
  if (roles.brand && status.err) {
    const br = hexToOklch(roles.brand);
    if (br.c > 0.05 && hueDistance(br.h, hexToOklch(status.err).h) < 20) add('info', 'brand-vs-error', 'The brand hue shares the error hue: never place error messages on brand surfaces, and keep errors iconified with words.');
  }
  const canvas = hexToOklch(roles.canvas);
  if (canvas.c > 0.035) add('warn', 'canvas-chroma', `Canvas chroma ${canvas.c.toFixed(3)} is high for a large surface (tires the eye, shifts every colour on it). Keep ≤ 0.03.`);
  // Hue budget.
  const chromatic = Object.entries(roles)
    .filter(([k]) => !['signature'].includes(k))
    .map(([, v]) => hexToOklch(v))
    .filter((x) => x.c > 0.05);
  const hues = [];
  for (const c of chromatic) if (!hues.some((h) => hueDistance(h, c.h) < 25)) hues.push(c.h);
  if (hues.length > 2) add('warn', 'hue-budget', `${hues.length} distinct chromatic hues in UI roles (besides status). One accent hue (+ one brand/signature) reads as designed; more reads as decoration.`);
  return out;
}

/**
 * Modular type scale. UI steps (2xs..2xl) use `ratio` around the base; display steps use a larger
 * `displayRatio` starting at `displayFrom`. 3xl/4xl are aliases of display-sm/md so components that
 * use them stay on the same scale.
 */
export function typeScale({ base = 16, ratio = 1.2, displayRatio = 1.333, displayFrom }) {
  const r = (n) => Math.round(n);
  const ui = { '2xs': Math.max(11, r(base / ratio ** 2.5)), xs: Math.max(12, r(base / ratio ** 2)), sm: r(base / ratio), md: base, lg: r(base * ratio), xl: r(base * ratio ** 2), '2xl': r(base * ratio ** 3) };
  if (ui.xs <= ui['2xs']) ui.xs = ui['2xs'] + 1;
  if (ui.sm <= ui.xs) ui.sm = ui.xs + 1;
  const d0 = Math.max(displayFrom || r(base * ratio ** 4), ui['2xl'] + 2);
  const display = { 'display-sm': d0, 'display-md': r(d0 * displayRatio), 'display-lg': r(d0 * displayRatio ** 2), 'display-xl': r(d0 * displayRatio ** 3) };
  ui['3xl'] = display['display-sm'];
  ui['4xl'] = display['display-md'];
  return { ui, display, ratio, displayRatio, base };
}

/**
 * Check a type scale for rhythm and hierarchy. scale = { ui: [px], display: [px] } (or a flat array = ui).
 */
export function auditTypeScale(scale, { body = 16, surface = 'product' } = {}) {
  const out = [];
  const ui = [...new Set(Array.isArray(scale) ? scale : scale.ui)].sort((a, b) => a - b);
  const display = [...new Set(Array.isArray(scale) ? [] : scale.display || [])].sort((a, b) => a - b);
  if (display.length >= 3) {
    const rr = display.slice(1).map((x, i) => x / display[i]);
    const mean = rr.reduce((a, b) => a + b, 0) / rr.length;
    const dev = Math.max(...rr.map((x) => Math.abs(x - mean)));
    if (dev > 0.08) out.push({ level: 'warn', rule: 'type-ratio', message: `Display steps ${display.join('/')} do not follow one ratio (${rr.map((x) => x.toFixed(2)).join(', ')}). Use a modular scale (dfw type scale).` });
  }
  const top = (display.length ? display : ui).at(-1);
  const need = surface === 'marketing' ? 3 : 1.8;
  if (top / body < need) out.push({ level: 'warn', rule: 'type-contrast', message: `Largest size ${top}px is only ${(top / body).toFixed(1)}× body; ${surface} UIs need ≥ ${need}× for clear hierarchy.` });
  const all = [...new Set([...ui, ...display])].sort((a, b) => a - b);
  // Below ~20px the eye cannot separate sizes less than ~8% apart; above, 6%.
  const close = all.slice(1).filter((x, i) => x / all[i] < (all[i] < 20 ? 1.08 : 1.06)).length;
  if (close >= 2) out.push({ level: 'warn', rule: 'type-steps', message: `${close} adjacent sizes are too close (e.g. 14/15): they look like mistakes, not hierarchy. Merge them or use a modular scale.` });
  return out;
}
