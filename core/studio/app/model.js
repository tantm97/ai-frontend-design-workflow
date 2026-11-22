// @ts-check
// Pure model of the Studio canvas: which frames a page shows, in which rows, and the small text
// helpers around them. Functions of values only (javascript.md J3): no DOM, no Studio state, so
// tests/unit/studio-app-model.test.mjs imports them in Node.

/**
 * @typedef {{ id: string, label?: string, short?: string, width: number, height: number }} Breakpoint
 * @typedef {{ id: string, label?: string, file: string, recommended?: boolean }} Variant
 * @typedef {{ id: string, title?: string, description?: string, variants?: Variant[], states?: string[] }} Screen
 * @typedef {{ breakpoints: Breakpoint[], screens?: Screen[], states?: string[], flows?: { from: string, to: string, label?: string }[], themes?: string[] }} Manifest
 * @typedef {{ title?: string, items?: string[] }} NoteGroup
 * @typedef {{ id: string, title: string, description?: string, system?: boolean, flow?: boolean, screens?: string[],
 *   notes?: NoteGroup[], sections?: { kind?: string, title?: string, breakpoint?: string, screens?: string[], states?: string[], notes?: NoteGroup[] }[] }} StudioPage
 * @typedef {{ manifest: Manifest, activeBps: string[] | null, activeStates: string[] | null, fullHeight: boolean, theme: string }} ViewContext
 *   What the toolbar shows: shown breakpoints and states (null = all), full height, theme.
 * @typedef {{ key: string, system?: boolean, screen: Screen | null, variant?: Variant, bp: Breakpoint | null, state?: string | null,
 *   file?: string, title: string, src: string, w: number, h: number, auto: boolean }} FrameSpec
 * @typedef {{ title?: string | null, sub?: string | null, notes?: NoteGroup[] | null, frames: FrameSpec[] }} RowModel
 * @typedef {{ id: string, title: string, description: string, rows: RowModel[], flow?: boolean }} PageModel
 */

/** HTML-escape a value for innerHTML (javascript.md J4). */
export const esc = (/** @type {unknown} */ s) =>
  String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

/** One-letter breakpoint tag of a frame title ("D", "T", "M"). */
export const bpShort = (/** @type {Breakpoint | null} */ bp) =>
  bp
    ? bp.short ||
      String(bp.label || bp.id)
        .charAt(0)
        .toUpperCase()
    : '';
/** The recommended variant of a screen, else its first. */
export const primaryVariant = (/** @type {Screen} */ s) => (s.variants || []).find((v) => v.recommended) || (s.variants || [])[0];
export const bpShown = (/** @type {ViewContext} */ ctx, /** @type {string} */ id) => !ctx.activeBps || ctx.activeBps.includes(id);
export const activeBps = (/** @type {ViewContext} */ ctx) => ctx.manifest.breakpoints.filter((b) => bpShown(ctx, b.id));
/** States of a screen: its own list, else the manifest's. */
export const statesOf = (/** @type {Manifest} */ manifest, /** @type {Screen} */ s) =>
  s.states && s.states.length ? s.states : manifest.states || [];
export const shownStates = (/** @type {ViewContext} */ ctx, /** @type {string[]} */ list) =>
  ctx.activeStates ? list.filter((x) => ctx.activeStates.includes(x)) : list;
/**
 * The states, out of `list`, that a variant designs (the server reports `designedStates`: a
 * .when-/.unless- block or a data-state rule in the screen or its masters). An undesigned state
 * would draw the default screen again, so it gets no frame. Without the report: all of them.
 */
export const designedOf = (/** @type {Variant & { designedStates?: string[] }} */ v, /** @type {string[]} */ list) =>
  Array.isArray(v?.designedStates) ? list.filter((x) => v.designedStates?.includes(x)) : list;
/** Every state named by the manifest or a screen, once. */
export const allStates = (/** @type {Manifest} */ manifest) => [
  ...new Set([...(manifest.states || []), ...(manifest.screens || []).flatMap((s) => s.states || [])]),
];
export const screenById = (/** @type {Manifest} */ manifest, /** @type {string} */ id) => (manifest.screens || []).find((s) => s.id === id);

/**
 * URL of a design file in a frame, with the theme and state as query parameters.
 * @param {string} file
 * @param {string | null | undefined} state
 * @param {string} theme
 */
export function frameSrc(file, state, theme) {
  const q = new URLSearchParams();
  if (theme) q.set('theme', theme);
  if (state) q.set('state', state);
  const qs = q.toString();
  return `/design/${file}${qs ? `?${qs}` : ''}`;
}

/**
 * @param {ViewContext} ctx
 * @param {Screen} s
 * @param {Variant} v
 * @param {Breakpoint} bp
 * @param {string | null} state
 * @returns {FrameSpec}
 */
export function makeSpec(ctx, s, v, bp, state) {
  const multi = (s.variants || []).length > 1;
  return {
    key: [s.id, v.id, bp.id, state].filter(Boolean).join('::'),
    screen: s,
    variant: v,
    bp,
    state: state || null,
    file: v.file,
    title: [bpShort(bp), s.title || s.id, multi ? v.label || v.id : null, state].filter(Boolean).join(' · '),
    src: frameSrc(v.file, state, ctx.theme),
    w: bp.width,
    h: bp.height,
    auto: ctx.fullHeight,
  };
}

/** @returns {FrameSpec} */
export function systemSpec() {
  return {
    key: 'system',
    system: true,
    screen: null,
    title: 'Design System',
    src: '/__dfw/system.html',
    w: 1440,
    h: 900,
    auto: true,
    bp: null,
  };
}

/**
 * Resolve "screen" or "screen/variant" refs.
 * @param {Manifest} manifest
 * @param {string[] | undefined} list
 */
export function refs(manifest, list) {
  return (list || [])
    .map((r) => {
      const [sid, vid] = String(r).split('/');
      const s = screenById(manifest, sid);
      if (!s) return null;
      return { s, v: vid ? (s.variants || []).find((x) => x.id === vid) || null : null };
    })
    .filter(Boolean);
}

/**
 * Page model: { id, title, description, rows: [{ title, sub, notes, frames: [spec] }] }. A canvas page is
 * laid out like a design file: a page header, then ROWS of frames (one per breakpoint, a variants row, a
 * states row, or manifest-declared sections). A row may carry an annotations panel (spec notes).
 * @param {ViewContext} ctx
 * @param {StudioPage} page
 * @returns {PageModel}
 */
export function pageModel(ctx, page) {
  const m = ctx.manifest;
  if (page.system) {
    return {
      id: page.id,
      title: page.title,
      description: page.description || 'Foundations and components, generated from design/tokens and design/system/components.',
      rows: [{ title: null, notes: page.notes || null, frames: [systemSpec()] }],
    };
  }
  const bps = activeBps(ctx);
  const primary = bps[0];
  const screens = page.screens.map((id) => screenById(m, id)).filter(Boolean);
  /** @type {RowModel[]} */
  const rows = [];
  if (page.flow && primary) {
    // Rows of 4 at the first shown breakpoint, in link order (sources before targets).
    const specs = screens.map((s) => makeSpec(ctx, s, primaryVariant(s), primary, null));
    for (let i = 0; i < specs.length; i += 4)
      rows.push({
        title: i ? null : `Flow at ${primary.label || primary.id} ${primary.width}`,
        sub: i ? null : `${(m.flows || []).length} link(s)`,
        frames: specs.slice(i, i + 4),
      });
    return { id: page.id, title: page.title, description: page.description || '', rows, flow: true };
  }
  if (page.sections && page.sections.length) {
    for (const sec of page.sections) {
      const kind = sec.kind || (sec.breakpoint ? 'breakpoint' : sec.states ? 'states' : 'custom');
      const bpDecl = sec.breakpoint && m.breakpoints.find((b) => b.id === sec.breakpoint);
      if (kind === 'breakpoint' && bpDecl && !bpShown(ctx, bpDecl.id)) continue;
      const bp = bpDecl || primary;
      if (!bp) continue;
      const list = sec.screens ? refs(m, sec.screens) : screens.map((s) => ({ s, v: null }));
      const frames = [];
      for (const { s, v } of list) {
        if (kind === 'variants') {
          const vs = v ? [v] : (s.variants || []).filter((x) => x !== primaryVariant(s));
          for (const x of vs) frames.push(makeSpec(ctx, s, x, bp, null));
        } else if (kind === 'states') {
          const sv = v || primaryVariant(s);
          for (const st of designedOf(sv, shownStates(ctx, sec.states || statesOf(m, s)))) frames.push(makeSpec(ctx, s, sv, bp, st));
        } else frames.push(makeSpec(ctx, s, v || primaryVariant(s), bp, null));
      }
      const defTitle = { breakpoint: `${bp.label || bp.id} ${bp.width}`, variants: 'Variants', states: 'States' }[kind] || '';
      rows.push({
        title: sec.title || defTitle,
        sub: kind === 'breakpoint' ? `${bp.width} × ${bp.height}` : `at ${bp.label || bp.id} ${bp.width}`,
        notes: sec.notes || null,
        frames,
      });
    }
  } else if (primary) {
    for (const bp of bps) {
      rows.push({
        title: `${bp.label || bp.id} ${bp.width}`,
        sub: `${bp.width} × ${bp.height}`,
        frames: screens.map((s) => makeSpec(ctx, s, primaryVariant(s), bp, null)),
      });
    }
    const alt = screens.flatMap((s) =>
      (s.variants || []).filter((v) => v !== primaryVariant(s)).map((v) => makeSpec(ctx, s, v, primary, null)),
    );
    if (alt.length) rows.push({ title: 'Variants', sub: `Alternatives at ${primary.label || primary.id} ${primary.width}`, frames: alt });
    if (allStates(m).length) {
      const st = screens.flatMap((s) =>
        designedOf(primaryVariant(s), shownStates(ctx, statesOf(m, s))).map((x) => makeSpec(ctx, s, primaryVariant(s), primary, x)),
      );
      const names = [...new Set(st.map((f) => f.state))];
      if (st.length)
        rows.push({ title: `States (${names.join(' · ')})`, sub: `at ${primary.label || primary.id} ${primary.width}`, frames: st });
    }
  }
  if (page.notes?.length) {
    if (!rows.length) rows.push({ title: null, frames: [] });
    rows[0].notes = [...page.notes, ...(rows[0].notes || [])];
  }
  return {
    id: page.id,
    title: page.title,
    description: page.description || '',
    rows: rows.filter((r) => r.frames.length || r.notes?.length),
  };
}

/** Number of frames a page shows. */
export const frameCount = (/** @type {ViewContext} */ ctx, /** @type {StudioPage} */ page) =>
  pageModel(ctx, page).rows.reduce((n, r) => n + r.frames.length, 0);

/**
 * Annotations panel of a row.
 * @param {string | null | undefined} title
 * @param {string | null | undefined} sub
 * @param {NoteGroup[]} notes
 */
export function notesHtml(title, sub, notes) {
  return `<div class="notes">${title ? `<h3>${esc(title)}</h3>` : ''}${sub ? `<p class="nsub">${esc(sub)}</p>` : ''}${notes
    .map(
      (g) =>
        `<div class="ng">${g.title ? `<h4>${esc(g.title)}</h4>` : ''}<ul>${(g.items || []).map((i) => `<li>${esc(i)}</li>`).join('')}</ul></div>`,
    )
    .join('')}</div>`;
}

/**
 * Layout edit scope of a frame: resizing in the widest breakpoint writes the base rule; in a narrower
 * frame it writes inside @media (max-width: <frame width>px), so it applies there and below.
 * @param {Breakpoint[] | undefined} breakpoints
 * @param {{ bp?: Breakpoint | null } | null | undefined} f
 */
export function layoutScope(breakpoints, f) {
  const widest = Math.max(0, ...(breakpoints || []).map((b) => b.width || 0));
  if (!f?.bp || f.bp.width >= widest) return { maxWidth: 0, scope: '' };
  return { maxWidth: f.bp.width, scope: `≤${f.bp.width}px` };
}

/** Comment statuses that wait for the agent. */
export const ACTIVE = ['open', 'acknowledged', 'reopened'];

/**
 * The value of an own key of a table, else undefined: a key from data never reaches an inherited
 * property such as "constructor" (security-privacy.md S4).
 * @template T
 * @param {Record<string, T>} table
 * @param {unknown} key
 * @returns {T | undefined}
 */
export const own = (table, key) => (typeof key === 'string' && Object.hasOwn(table, key) ? table[key] : undefined);

const STATUS_LABEL = {
  open: 'Open · waiting for Claude',
  acknowledged: 'Claude is working',
  reopened: 'Reopened · waiting for Claude',
  fixed: 'Fixed · please verify',
  resolved: 'Resolved',
  dismissed: 'Dismissed',
};

/** Words for a comment status (plain text: esc() it for innerHTML); an unknown status is shown as it is. */
export function statusLabel(/** @type {unknown} */ s) {
  return own(STATUS_LABEL, s) ?? String(s ?? '');
}

/** Does a comment show under a filter tab ("active", "fixed", "closed", "all")? */
export const inFilter = (/** @type {string} */ filter, /** @type {{ status: string }} */ c) =>
  filter === 'active'
    ? ACTIVE.includes(c.status)
    : filter === 'fixed'
      ? c.status === 'fixed'
      : filter === 'closed'
        ? ['resolved', 'dismissed'].includes(c.status)
        : true;

/** Pin statuses per Pins setting: 'active' (open + to verify), 'all' (also closed) or 'off'. */
export const PIN_STATUSES = {
  active: ['open', 'acknowledged', 'reopened', 'fixed'],
  all: ['open', 'acknowledged', 'reopened', 'fixed', 'resolved'],
  off: [],
};

/**
 * The comments pinned on a frame, as the overlay needs them to place pins.
 * @param {{ id: string, status: string, screen?: string, variant?: string, state?: string | null, target?: { selector?: string, dfwId?: string } }[]} comments
 * @param {{ system?: boolean, screen?: { id: string } | null, variant?: { id: string } | null, state?: string | null }} f
 * @param {string[]} show statuses to show
 */
export function pinsOf(comments, f, show) {
  return comments
    .filter(
      (c) =>
        show.includes(c.status) &&
        (f.system
          ? c.screen === 'system'
          : f.screen && c.screen === f.screen.id && c.variant === f.variant?.id && (c.state || null) === (f.state || null)),
    )
    .map((c) => ({ id: c.id, n: Number(c.id.replace('fb_', '')), selector: c.target?.selector, dfwId: c.target?.dfwId, status: c.status }));
}

/** Child -> parent layer index map of a layer tree (null for top-level layers). */
export function layerParents(/** @type {{ i: number, children: any[] }[]} */ tree) {
  /** @type {Map<number, number | null>} */
  const parents = new Map();
  const walk = (/** @type {{ i: number, children: any[] }[]} */ nodes, /** @type {number | null} */ p) =>
    nodes.forEach((n) => (parents.set(n.i, p), walk(n.children, n.i)));
  walk(tree, null);
  return parents;
}

/**
 * PNG file name of a screenshot: screen, variant, breakpoint, theme, state.
 * @param {{ system?: boolean, screen?: { id: string } | null, file?: string | null, variant?: { id: string } | null, bp?: { id: string } | null, state?: string | null }} f
 * @param {string} theme
 */
export const shotName = (f, theme) =>
  f.system
    ? 'design-system'
    : [f.screen?.id || f.file?.replace(/\.html$/, ''), f.variant?.id, f.bp?.id, theme, f.state]
        .filter(Boolean)
        .join('__')
        .replace(/\//g, '__');
