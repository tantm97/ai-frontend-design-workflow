// The Studio's page model (core/studio/app/model.js): which frames a page shows, in which rows,
// for the toolbar's breakpoint and state choices; and the small filters around comments and pins.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  esc,
  frameCount,
  frameSrc,
  inFilter,
  layerParents,
  layoutScope,
  pageModel,
  pinsOf,
  shotName,
  statusLabel,
} from '../../core/studio/app/model.js';

const manifest = {
  breakpoints: [
    { id: 'desktop', label: 'Desktop', width: 1440, height: 900 },
    { id: 'mobile', label: 'Mobile', width: 390, height: 844 },
  ],
  states: ['empty', 'error'],
  screens: [
    {
      id: 'home',
      title: 'Home',
      variants: [
        { id: 'a', label: 'A', file: 'screens/home/a.html' },
        { id: 'b', label: 'B', file: 'screens/home/b.html' },
      ],
    },
    { id: 'detail', title: 'Detail', variants: [{ id: 'a', file: 'screens/detail/a.html' }], states: ['loading'] },
  ],
  flows: [{ from: 'home', to: 'detail' }],
};
const ctx = (over = {}) => ({ manifest, activeBps: null, activeStates: null, fullHeight: true, theme: '', ...over });
const page = { id: 'screens', title: 'Screens', screens: ['home', 'detail'] };
const titles = (m) => m.rows.map((r) => r.title);
const keys = (r) => r.frames.map((f) => f.key);

test('studio-app-model: auto rows: one per breakpoint, then variants, then states', () => {
  const m = pageModel(ctx(), page);
  assert.deepEqual(titles(m), ['Desktop 1440', 'Mobile 390', 'Variants', 'States (empty · error · loading)']);
  assert.deepEqual(keys(m.rows[0]), ['home::a::desktop', 'detail::a::desktop']);
  assert.deepEqual(keys(m.rows[2]), ['home::b::desktop']);
  assert.deepEqual(keys(m.rows[3]), ['home::a::desktop::empty', 'home::a::desktop::error', 'detail::a::desktop::loading']);
  assert.equal(m.rows[0].frames[0].title, 'D · Home · A');
  assert.equal(frameCount(ctx(), page), 8);
});

test('studio-app-model: the toolbar choices filter breakpoints and states', () => {
  const m = pageModel(ctx({ activeBps: ['mobile'], activeStates: ['error'] }), page);
  assert.deepEqual(titles(m), ['Mobile 390', 'Variants', 'States (error)']);
  assert.deepEqual(keys(m.rows[2]), ['home::a::mobile::error']);
});

test('studio-app-model: the Flow page lays screens out in rows of 4 at the first breakpoint', () => {
  const m = pageModel(ctx(), { ...page, id: 'flow', title: 'Flow', flow: true });
  assert.equal(m.flow, true);
  assert.deepEqual(titles(m), ['Flow at Desktop 1440']);
  assert.equal(m.rows[0].sub, '1 link(s)');
});

test('studio-app-model: declared sections, refs and page notes', () => {
  const m = pageModel(ctx(), {
    ...page,
    notes: [{ title: 'Why', items: ['Because'] }],
    sections: [
      { kind: 'variants', screens: ['home/b'], title: 'Options' },
      { states: ['empty'], screens: ['home'] },
      { breakpoint: 'mobile' },
    ],
  });
  assert.deepEqual(titles(m), ['Options', 'States', 'Mobile 390']);
  assert.deepEqual(keys(m.rows[0]), ['home::b::desktop']);
  assert.deepEqual(keys(m.rows[1]), ['home::a::desktop::empty']);
  assert.equal(m.rows[0].notes[0].title, 'Why', 'page notes go on the first row');
});

test('studio-app-model: the system page is one Design System frame', () => {
  const m = pageModel(ctx(), { id: 'system', title: 'Design System', system: true });
  assert.equal(m.rows[0].frames[0].src, '/__dfw/system.html');
});

test('studio-app-model: frame URLs carry the theme and state', () => {
  assert.equal(frameSrc('screens/home/a.html', null, ''), '/design/screens/home/a.html');
  assert.equal(frameSrc('screens/home/a.html', 'empty', 'dark'), '/design/screens/home/a.html?theme=dark&state=empty');
});

test('studio-app-model: layout edits at the widest breakpoint are the base rule, narrower ones a media rule', () => {
  assert.deepEqual(layoutScope(manifest.breakpoints, { bp: manifest.breakpoints[0] }), { maxWidth: 0, scope: '' });
  assert.deepEqual(layoutScope(manifest.breakpoints, { bp: manifest.breakpoints[1] }), { maxWidth: 390, scope: '≤390px' });
  assert.deepEqual(layoutScope(manifest.breakpoints, null), { maxWidth: 0, scope: '' });
});

test('studio-app-model: comment filters, labels and the pins of a frame', () => {
  const comments = [
    { id: 'fb_001', status: 'open', screen: 'home', variant: 'a', target: { dfwId: 'home.cta' } },
    { id: 'fb_002', status: 'resolved', screen: 'home', variant: 'a' },
    { id: 'fb_003', status: 'fixed', screen: 'home', variant: 'a', state: 'empty' },
    { id: 'fb_004', status: 'open', screen: 'system' },
  ];
  assert.deepEqual(
    comments.filter((c) => inFilter('active', c)).map((c) => c.id),
    ['fb_001', 'fb_004'],
  );
  assert.deepEqual(
    comments.filter((c) => inFilter('closed', c)).map((c) => c.id),
    ['fb_002'],
  );
  assert.equal(statusLabel('fixed'), 'Fixed · please verify');
  const home = { screen: { id: 'home' }, variant: { id: 'a' }, state: null };
  assert.deepEqual(pinsOf(comments, home, ['open', 'fixed']), [
    { id: 'fb_001', n: 1, selector: undefined, dfwId: 'home.cta', status: 'open' },
  ]);
  assert.deepEqual(
    pinsOf(comments, { system: true }, ['open']).map((p) => p.id),
    ['fb_004'],
  );
});

test('studio-app-model: layer parents, shot names, escaping', () => {
  const tree = [
    { i: 0, children: [{ i: 1, children: [{ i: 2, children: [] }] }] },
    { i: 3, children: [] },
  ];
  assert.deepEqual(
    [...layerParents(tree)],
    [
      [0, null],
      [1, 0],
      [2, 1],
      [3, null],
    ],
  );
  const f = { screen: { id: 'home' }, variant: { id: 'a' }, bp: { id: 'desktop' }, state: 'empty' };
  assert.equal(shotName(f, 'dark'), 'home__a__desktop__dark__empty');
  assert.equal(shotName({ system: true }, ''), 'design-system');
  assert.equal(shotName({ file: 'screens/x.html' }, ''), 'screens__x');
  assert.equal(esc(`<img src=x onerror="a('b')">&`), '&lt;img src=x onerror=&quot;a(&#39;b&#39;)&quot;&gt;&amp;');
});
