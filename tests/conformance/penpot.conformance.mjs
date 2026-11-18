// Penpot bridge of the native port: `tokens export --format penpot`, `tokens import`,
// `penpot import | plugin | export`, each run by the Node CLI and by the native binary on copies
// of a fixture project and compared (stdout, exit code, every file written). buildExport and the
// live comment loop (syncThreads) are compared function by function through the binary's hidden
// `__debug penpot-build-export` and `__debug penpot-sync` commands.
// Run: DFW_BIN=native/target/debug/dfw node --test tests/conformance/
// `penpot export` renders in a real browser: it runs only with DFW_TEST_CHROME=1.
import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { both, assertSame, nativeBinary, skipReason, REPO, NOW, MINIMAL_STATE } from './harness.mjs';
import { buildExport, exportStructure, syncThreads } from '../../core/lib/penpot.mjs';
import { updateComment, allComments } from '../../core/lib/comments.mjs';
import { loadState } from '../../core/lib/state.mjs';
import { buildTokens } from '../../core/lib/tokens.mjs';

const FIX = path.join(REPO, 'tests', 'fixtures', 'penpot');
const CHROME = process.env.DFW_TEST_CHROME === '1' ? false : 'needs Chrome: set DFW_TEST_CHROME=1';
const read = (f) => JSON.parse(fs.readFileSync(path.join(FIX, f), 'utf8'));
const write = (root, rel, data) => {
  fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  fs.writeFileSync(path.join(root, rel), typeof data === 'string' ? data : JSON.stringify(data));
};

// A review session is named after the real second it starts in, and a dry run that cannot read
// the plugin's tokens leaves its temporary file (named after the process) behind, as in Node.
const mask = (text) => text.replace(/rv_\d{8}T\d{6}/g, 'rv_<now>').replace(/\.tokens-\d+\.json/g, '.tokens-<pid>.json');
const masked = (r) => ({ ...r, stdout: mask(r.stdout), stderr: mask(r.stderr), files: Object.fromEntries(Object.entries(r.files).map(([k, v]) => [mask(k), typeof v === 'string' ? mask(v) : v])) });

/** `both` of the harness with the names above masked. */
function run(args, opts) {
  const r = both(args, opts);
  return { node: masked(r.node), native: masked(r.native) };
}

/** Same command on both, same stdout, exit code and files; returns the native result. */
function same(args, opts, check = {}) {
  const r = run(args, opts);
  assertSame(r, check);
  return r.native;
}

function native(args, { input = '', env = {} } = {}) {
  const e = { ...process.env, DFW_PLUGIN_ROOT: path.join(REPO, 'core'), ...env };
  for (const k of ['DFW_NATIVE', 'DFW_FORWARDED']) delete e[k];
  const r = spawnSync(nativeBinary(), args, { env: e, input, encoding: 'utf8', timeout: 60_000, maxBuffer: 64 * 1024 * 1024 });
  if (r.error) throw r.error;
  return r;
}

/** The tokens of the token fixtures (every type the bridge maps, a theme) in a project copy. */
const tokenProject = (root) => {
  fs.copyFileSync(path.join(FIX, 'dfw-tokens.json'), path.join(root, 'design/tokens/tokens.json'));
  fs.mkdirSync(path.join(root, 'design/tokens/themes'), { recursive: true });
  fs.copyFileSync(path.join(FIX, 'dfw-theme-dark.json'), path.join(root, 'design/tokens/themes/dark.json'));
};
const EDITED = path.join(FIX, 'penpot-edited.tokens.json');
const FROZEN = { ...MINIMAL_STATE, design: { version: 2, status: 'approved', locked: true, approved: [] } };

describe('penpot tokens', { skip: skipReason() }, () => {
  test('tokens export --format penpot writes the same token file', () => {
    const r = same(['tokens', 'export', '--format', 'penpot'], { setup: tokenProject });
    assert.equal(r.code, 0);
    assert.match(r.stdout, /^Wrote \.dfw\/penpot\/tokens\.penpot\.json: \d+ token\(s\) in Penpot format \(sets: base, themes\/dark\)\./);
    assert.match(r.stdout, /Not exported: base easing\.out \(cubicBezier\)/);
    assert.match(r.stdout, /Lossy: /);
    assert.deepEqual(Object.keys(r.files), ['.dfw/penpot/tokens.penpot.json']);
    assert.deepEqual(Object.keys(JSON.parse(r.files['.dfw/penpot/tokens.penpot.json'])), ['base', 'themes/dark', '$themes', '$metadata']);
    // Without --format, to another file, as JSON.
    assert.deepEqual(Object.keys(same(['tokens', 'export', '--out', '.dfw/out/pp.json'], { setup: tokenProject }).files), ['.dfw/out/pp.json']);
    const json = same(['tokens', 'export', '--json'], { setup: tokenProject });
    assert.equal(JSON.parse(json.stdout).file, '<root>/.dfw/penpot/tokens.penpot.json');
    // The fixture project's own tokens (no theme).
    same(['tokens', 'export', '--format', 'penpot'], {});
  });

  test('tokens export: option themes, key order, values that have no Penpot form', () => {
    const odd = {
      color: { $type: 'color', 10: { $value: '#000' }, 2: { $value: { colorSpace: 'srgb', components: [0.12345, 0.5, 2], alpha: 0.25 } }, lab: { $value: { colorSpace: 'lab', components: [50, 0, 0] } }, p3: { $value: { colorSpace: 'display-p3', components: [1, 0, 0], hex: '#FF0000', alpha: 0.5 } }, ok: { $value: { colorSpace: 'oklch', components: [0.7, 0.1, 30], alpha: 0.3 } }, bad: { $value: 5 }, nul: { $value: null } },
      space: { $type: 'dimension', calc: { $value: 'calc({space.a} * 2)' }, a: { $value: { value: 1.23456789, unit: 'rem' } }, n: { $value: 12 }, odd: { $value: [1] } },
      motion: { $type: 'duration', a: { $value: { value: 0.25, unit: 's' } }, b: { $value: '{motion.a}' }, c: { $value: null } },
      text: { $type: 'string', a: { $value: 'line-through' }, b: { $value: 'none' }, c: { $value: 'free' } },
      type: { $type: 'typography', a: { $value: { fontFamily: ['A', 'B'], fontSize: '{space.n}', fontWeight: 600, lineHeight: { value: 1.5, unit: 'rem' }, textCase: 'uppercase' } }, b: { $value: 'x' } },
      shadow: { $type: 'shadow', a: { $value: { color: '#000', offsetX: { value: 1, unit: 'px' }, offsetY: 2, blur: '{space.n}' } }, b: { $value: [null] } },
      weird: { $type: 7, a: { $value: 1, $description: 'Seven' } },
      'a.b': { c: { $type: 'number', $value: 1e21 } },
    };
    const setup = (root) => {
      write(root, 'design/tokens/tokens.json', odd);
      write(root, 'design/tokens/themes/opt-a.json', { color: { $type: 'color', x: { $value: '#fff' } } });
      write(root, 'design/tokens/themes/2.json', { color: { $type: 'color', x: { $value: '#111' } } });
      write(root, 'design/tokens/themes/dark.json', { color: { $type: 'color', x: { $value: '#222' } } });
    };
    const r = same(['tokens', 'export', '--format', 'penpot'], { setup });
    assert.match(r.stdout, /Left out option themes: opt-a/);
    assert.match(r.stdout, /sets: base, themes\/2, themes\/dark/);
    assert.match(same(['tokens', 'export', '--opt-themes', '--json'], { setup }).stdout, /themes\/opt-a/);
  });

  test('tokens export and import: usage errors', () => {
    for (const args of [['tokens', 'export', '--format', 'figma'], ['tokens', 'export', '--format'], ['tokens', 'import'], ['tokens', 'import', EDITED, '--from', 'figma'], ['tokens', 'import', 'missing.json'], ['tokens', 'import', path.join(FIX, 'export.structure.txt')]]) {
      const r = run(args, {});
      assert.equal(r.native.code, 1, args.join(' '));
      assert.equal(r.native.code, r.node.code);
      assert.deepEqual(r.native.files, r.node.files);
    }
    assertSame(run(['tokens', 'export', '--format', 'figma'], {}), { stderr: true });
    assertSame(run(['tokens', 'import'], {}), { stderr: true });
    assertSame(run(['tokens', 'import', 'missing.json'], {}), { stderr: true });
    assertSame(run(['tokens', 'export'], { setup: (root) => fs.rmSync(path.join(root, 'design/tokens/tokens.json')) }), { stderr: true });
  });

  test('tokens import of a Penpot-edited file: dry run, real, prune, json', () => {
    const dry = same(['tokens', 'import', EDITED, '--dry-run'], { setup: tokenProject });
    assert.match(dry.stdout, /color\.blue\.600: #0066ff -> #0055dd[\s\S]*Dry run: nothing written\.\n$/);
    assert.deepEqual(dry.files, {});
    const real = same(['tokens', 'import', EDITED], { setup: tokenProject });
    assert.match(real.stdout, /Wrote design\/tokens\/tokens\.json, design\/tokens\/themes\/dark\.json; tokens\.css rebuilt\./);
    assert.deepEqual(Object.keys(real.files), ['design/system/components.css', 'design/tokens/themes/dark.json', 'design/tokens/tokens.css', 'design/tokens/tokens.json']);
    const pruned = same(['tokens', 'import', EDITED, '--prune', '--from', 'penpot'], { setup: tokenProject });
    assert.match(pruned.stdout, /  - color\.veil\n/);
    same(['tokens', 'import', EDITED, '--json'], { setup: tokenProject });
    same(['tokens', 'import', EDITED, '--dry-run', '--json', '--prune'], { setup: tokenProject });
    // Into the fixture project's own tokens, and into a project without tokens.
    same(['tokens', 'import', EDITED], {});
    same(['tokens', 'import', EDITED], { setup: (root) => fs.rmSync(path.join(root, 'design/tokens/tokens.json')) });
  });

  test('tokens import: the export of the same project changes nothing', () => {
    const roundTrip = (root) => {
      tokenProject(root);
      const r = spawnSync(process.execPath, [path.join(REPO, 'core/bin/dfw.mjs'), 'tokens', 'export', '--out', 'own.json'], { cwd: root, env: { ...process.env, DFW_HOME: path.join(root, '.home'), DFW_NO_BINARY: '1', DFW_PROJECT_DIR: root, CLAUDE_PROJECT_DIR: root }, encoding: 'utf8' });
      assert.equal(r.status, 0, r.stderr);
    };
    const r = same(['tokens', 'import', 'own.json'], { setup: roundTrip });
    assert.match(r.stdout, /^base: no changes\nthemes\/dark: no changes\nNothing to write\.\n$/);
    assert.deepEqual(r.files, {});
  });

  test('tokens import is refused while frozen or review-only; a dry run is allowed', () => {
    const frozen = run(['tokens', 'import', EDITED], { setup: tokenProject, state: FROZEN });
    assertSame(frozen, { stderr: true });
    assert.equal(frozen.native.code, 1);
    assert.match(frozen.native.stderr, /Design v2 is approved and frozen \(G2\)[\s\S]*dfw reopen/);
    assert.deepEqual(frozen.native.files, {});
    assert.match(same(['tokens', 'import', EDITED, '--dry-run'], { setup: tokenProject, state: FROZEN }).stdout, /Dry run/);
    const reviewOnly = run(['tokens', 'import', EDITED], { setup: tokenProject, state: { ...MINIMAL_STATE, mode: 'review-only' } });
    assertSame(reviewOnly, { stderr: true });
    assert.equal(reviewOnly.native.code, 1);
    // No workflow at all: the import runs.
    assert.equal(same(['tokens', 'import', EDITED], { setup: tokenProject, state: null }).code, 0);
  });

  test('tokens import: reserved names, a file with only a theme, hostile shapes', () => {
    const base = (root) => write(root, 'design/tokens/tokens.json', { color: { $type: 'color', a: { $value: '#ffffff' } } });
    // Raw text: JSON.parse makes "__proto__" an own key, which is what a hostile file relies on.
    const proto = '{"__proto__":{"zzz":{"$value":"#000000","$type":"color"}},"color":{"a":{"$value":"#ffffff","$type":"color"},"constructor":{"prototype":{"yyy":{"$value":"#111111","$type":"color"}}}}}';
    const r = same(['tokens', 'import', 'in.json'], { setup: (root) => (base(root), write(root, 'in.json', proto)) });
    assert.match(r.stdout, /^in: no changes\nNot imported: in __proto__ \(undefined\): reserved name \(__proto__, constructor, prototype\)\nNot imported: in color\.constructor \(undefined\): reserved name/);
    assert.deepEqual(r.files, {});
    same(['tokens', 'import', 'in.json', '--json'], { setup: (root) => (base(root), write(root, 'in.json', proto)) });

    const onlyTheme = { 'themes/dark': { color: { a: { $value: '#000000', $type: 'color' } } }, $themes: [], $metadata: {} };
    const theme = run(['tokens', 'import', 'dark.json'], { setup: (root) => (base(root), write(root, 'dark.json', onlyTheme)) });
    assertSame(theme, { stderr: true });
    assert.equal(theme.native.code, 1);
    assert.match(theme.native.stderr, /No base token set found/);
    assert.deepEqual(theme.native.files, {}, 'tokens.json untouched');

    const hostile = {
      // Theme names that would leave themes/ or are not names; sets that are not objects.
      sets: { base: { color: { a: { $value: '#000000', $type: 'color' } } }, 'themes/../../x': { c: { $value: '#000', $type: 'color' } }, 'themes/a/b': {}, 'theme/ok.name-1': { c: { $value: '#123', $type: 'color' } }, 'themes/': {}, 5: { c: { $value: '1', $type: 'number' } }, 'themes/n': null, 'themes/s': 'text' },
      // Every Penpot type, values of the wrong shape, legacy value/type keys, dotted keys.
      types: {
        base: {
          c: { hex3: { $value: '#AbC', $type: 'color' }, hex4: { $value: '#abcd', $type: 'color' }, rgb: { $value: 'rgb(300 1.5 0 / 50%)', $type: 'color' }, hsl: { $value: 'hsla(120deg, 100%, 25%, .5)', $type: 'color' }, expr: { $value: 'darken({c.hex3}, 10)', $type: 'color' }, num: { $value: 5, $type: 'color' }, alias: { $value: ' {c.hex3} ', $type: 'color' } },
          d: { a: { $value: '1.5REM', $type: 'sizing' }, b: { $value: -2, $type: 'spacing' }, c: { $value: 'round({d.a} / 3)', $type: 'dimension' }, d: { $value: '12 px', $type: 'borderRadius' }, e: { $value: 'auto', $type: 'borderWidth' }, f: { $value: null, $type: 'strokeWidth' } },
          n: { a: { $value: '50%', $type: 'opacity' }, b: { $value: '-.5', $type: 'rotation' }, c: { $value: 'x', $type: 'number' }, d: { $value: '{n.a} + 1', $type: 'number' }, 'delay.in': { $value: '2 * 100', $type: 'number' } },
          w: { a: { $value: 'Extra-Bold Italic', $type: 'fontWeights' }, b: { $value: 'italic', $type: 'fontWeight' }, c: { $value: 'chunky', $type: 'fontWeights' }, d: { $value: 650, $type: 'fontWeights' } },
          f: { a: { $value: ["'A B'", 'c', '', 7], $type: 'fontFamilies' }, b: { $value: 3, $type: 'fontFamily' } },
          s: { a: { $value: 'inset 0 1px #000', $type: 'shadow' }, b: { $value: [{ color: 'rgba(0,0,0,.5)', x: 1, y: '2px', blur: '{d.b}', spread: 'x', type: 'innerShadow' }, 'raw', 5], $type: 'boxShadow' }, c: { $value: { offsetX: '1rem', inset: true }, $type: 'shadow' } },
          t: { a: { $value: { 'font-family': 'A, "B C"', fontSizes: '{d.a}', 'font-weight': 'bold', letterSpacing: '1%', lineHeights: 'AUTO', textCase: 'none', textDecoration: null }, $type: 'typography' }, b: { $value: 'x', $type: 'typography' }, c: { $value: ['x'], $type: 'typography' } },
          x: { a: { $value: 'underline', $type: 'textDecoration' }, b: { $value: 'true', $type: 'boolean' }, c: { $value: 'x' }, d: { $value: '{c.hex3}', $type: 'other' }, e: { $value: '{d.a}', $type: 'sizing', $description: 'Aliased' } },
          legacy: { type: 'color', one: { value: '#010203', description: 'Old style' }, two: { value: '#040506', type: 'color' } },
          'dot.ted': { 'key.name': { $value: '#fff', $type: 'color' } },
          prototype: { $value: '#fff', $type: 'color' },
          arr: [{ $value: '#fff', $type: 'color' }],
        },
        $themes: 'x',
        $metadata: { tokenSetOrder: ['nope', 'base', 7, 'base'] },
      },
      // The same names as existing dfw tokens, with other types and group/token clashes.
      clash: { base: { color: { $type: 'spacing', a: { $value: '4' } }, space: { 2: { 'deep.er': { $value: '1', $type: 'number' } }, $value: 'x' }, radius: { md: { $value: '6px', $type: 'borderRadius', $description: 'Now described' } }, font: { size: { md: { $value: '16', $type: 'fontSizes' } } }, 10: { $value: '1', $type: 'number' }, 9: { $value: '2', $type: 'number' } }, $themes: [] },
    };
    for (const [name, doc] of Object.entries(hostile)) {
      const file = name === 'sets' ? { ...doc, $themes: [], $metadata: {} } : doc;
      for (const flags of [['--dry-run'], [], ['--json'], ['--prune']]) {
        const r = run(['tokens', 'import', 'in.json', ...flags], { setup: (root) => write(root, 'in.json', file) });
        assertSame(r);
        // "types" holds a colour that is a number: imported as it is, and the rebuild of
        // tokens.css then refuses it, in both.
        assert.equal(r.native.code, name === 'types' && flags[0] !== '--dry-run' ? 1 : 0, `${name} ${flags}: ${r.native.stderr}`);
      }
    }
    // Shapes Node refuses or cannot read: same exit code, nothing written.
    for (const text of ['[]', '5', 'null', '"x"', '{"base":{},"$metadata":{"tokenSetOrder":5}}', '{"base":{"s":{"$value":null,"$type":"shadow"}},"$themes":[]}', '{"a":']) {
      const r = run(['tokens', 'import', 'in.json'], { setup: (root) => write(root, 'in.json', text) });
      assert.equal(r.native.code, r.node.code, text);
      assert.equal(r.native.stdout, r.node.stdout, text);
      assert.deepEqual(r.native.files, r.node.files, text);
    }
  });

  test('tokens import: a folder of set files', () => {
    const folder = (root) => {
      tokenProject(root);
      const edited = read('penpot-edited.tokens.json');
      write(root, 'pp/base.json', edited.base);
      write(root, 'pp/themes/dark.json', edited['themes/dark']);
      write(root, 'pp/Brands/A.json', edited['Brands/A']);
      write(root, 'pp/$themes.json', edited.$themes);
      write(root, 'pp/$metadata.json', edited.$metadata);
      write(root, 'pp/notes.txt', 'not a set');
    };
    const r = same(['tokens', 'import', 'pp'], { setup: folder });
    assert.match(r.stdout, /Skipped Penpot sets .*: Brands\/A/);
    same(['tokens', 'import', 'pp', '--dry-run', '--json'], { setup: folder });
  });
});

/* ------------------------------------------------------------------ screens */

const tokensOf = (root) => ({ base: JSON.parse(fs.readFileSync(path.join(root, 'design/tokens/tokens.json'), 'utf8')), themes: {} });
const fixtureExport = (generatedAt = '2026-11-16T10:00:00.000Z') => buildExport({ renders: read('renders.json'), tokens: tokensOf(path.join(FIX, 'project')), product: 'Fixture', generatedAt });

/** Put the fixture export and a change file built from it in a project copy. */
const withExport = (changesFile, edit = (c) => c) => (root) => {
  const doc = fixtureExport();
  write(root, `.dfw/penpot/exports/${doc.id}.json`, doc);
  write(root, '.dfw/penpot/export.json', doc);
  write(root, 'changes.json', edit({ ...read(changesFile), exportId: doc.id }, doc));
};

describe('penpot screens', { skip: skipReason() }, () => {
  test('__debug penpot-build-export builds the same document, id included', () => {
    const tokens = path.join(FIX, 'project/design/tokens/tokens.json');
    for (const at of ['2026-11-16T10:00:00.000Z', NOW]) {
      const r = native(['__debug', 'penpot-build-export', path.join(FIX, 'renders.json'), tokens, at]);
      assert.equal(r.status, 0, r.stderr);
      const doc = fixtureExport(at);
      assert.equal(r.stdout, `${JSON.stringify(doc)}\n`);
      assert.match(doc.id, /^px_\d{14}_[0-9a-f]{8}$/);
    }
    // Without tokens: no token links, the same hash.
    const bare = native(['__debug', 'penpot-build-export', path.join(FIX, 'renders.json'), '-']);
    assert.equal(bare.stdout, `${JSON.stringify(buildExport({ renders: read('renders.json'), tokens: null, product: 'Fixture', generatedAt: '2026-11-16T10:00:00.000Z' }))}\n`);
    // Other tokens (aliases, a theme, lossy colours) and renders with every layer kind.
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-conformance-penpot-'));
    try {
      const renders = read('renders.json');
      const extra = structuredClone(renders[0]);
      extra.screen = 'detail';
      extra.page = { id: 'more' };
      extra.breakpoint = { id: 'mobile', width: 390, height: 700 };
      extra.snapshot.height = 812.4;
      extra.snapshot.children.push(
        { kind: 'image', name: 'img', tag: 'img', dfwId: null, anchor: null, x: 1, y: 2, w: 30, h: 40, style: { radius: [6, 6, 6, 6], opacity: 0.5, clip: true }, src: 'https://example.test/a.png', alt: 'A' },
        { kind: 'image', name: 'img', tag: 'img', dfwId: 'pic', anchor: 'pic', x: 1, y: 2, w: 30, h: 40, style: {}, src: 'file:///tmp/x.png', alt: '' },
        { kind: 'svg', name: 'svg.icon', tag: 'svg', dfwId: null, anchor: null, x: 0.005, y: 1.005, w: 16, h: 16, style: {}, svg: '<svg xmlns="http://www.w3.org/2000/svg"></svg>', icon: 'check' },
        { kind: 'frame', name: 'div.card', tag: 'div', dfwId: null, anchor: null, x: 10.123, y: 20.456, w: 100, h: 50, style: { fill: { hex: '#0066ff', opacity: 0.5 }, borders: [{ w: 1, color: { hex: '#e4e4e7', opacity: 1 }, style: 'dashed' }, null, { w: 2, color: { hex: '#1c1b1a', opacity: 1 }, style: 'solid' }, null], radius: [6, 0, 6, 0], shadows: [{ inset: false, x: 0, y: 1, blur: 2, spread: 0, color: null }, { inset: true, x: 0, y: 0, blur: 0, spread: 1, color: { hex: '#000000', opacity: 0.1 } }], backgroundImage: `linear-gradient(${'x'.repeat(80)})` }, component: { name: 'card', variant: null, size: 'lg' }, children: [{ kind: 'text', name: 'text', dfwId: null, anchor: null, x: 12, y: 22, w: 40, h: 12, characters: 'Tiếng Việt "quoted"\n ', text: { fontFamily: 'Arial', fontSize: 12, color: { hex: '#1c1b1a', opacity: 1 } }, placeholder: true }, { kind: 'frame', name: 'div', tag: 'div', dfwId: null, anchor: null, x: 0, y: 0, w: 0, h: 0, style: {}, children: [{ kind: 'frame', name: 'div.inner', tag: 'div', dfwId: 'inner', anchor: 'inner', x: 13, y: 23, w: 5, h: 5, style: { borders: [1, 2, 3, 4].map(() => ({ w: 1, color: { hex: '#e4e4e7', opacity: 1 }, style: 'dotted' })) }, children: [] }] }] },
      );
      fs.writeFileSync(path.join(dir, 'renders.json'), JSON.stringify([...renders, extra]));
      const base = { ...read('dfw-tokens.json'), color: { ...read('dfw-tokens.json').color, loop: { $value: '{color.loop}' }, far: { $value: '{color.accent}' }, 'on-accent': { $value: '{color.ink}' } }, radius: { $type: 'dimension', md: { $value: { value: 6, unit: 'px' } }, 0: { $value: { value: 0, unit: 'px' } } } };
      fs.writeFileSync(path.join(dir, 'tokens.json'), JSON.stringify(base));
      const r = native(['__debug', 'penpot-build-export', path.join(dir, 'renders.json'), path.join(dir, 'tokens.json')]);
      assert.equal(r.status, 0, r.stderr);
      const doc = buildExport({ renders: JSON.parse(fs.readFileSync(path.join(dir, 'renders.json'), 'utf8')), tokens: { base, themes: {} }, product: 'Fixture', generatedAt: '2026-11-16T10:00:00.000Z' });
      assert.equal(r.stdout, `${JSON.stringify(doc)}\n`);
      assert.equal(doc.pages.length, 2);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  test('penpot import of the change fixtures: dry run and real', () => {
    for (const file of ['changes.json', 'changes-components.json']) {
      const dry = same(['penpot', 'import', 'changes.json', '--dry-run'], { setup: withExport(file) });
      assert.equal(dry.code, 0, dry.stderr);
      assert.match(dry.stdout, /Dry run: no comments written\.\n$/);
      assert.deepEqual(dry.files, {}, 'a dry run writes nothing');
      const real = same(['penpot', 'import', 'changes.json'], { setup: withExport(file) });
      assert.match(real.stdout, /Comments are in the review loop/);
      const written = Object.keys(real.files);
      assert.ok(written.includes('.dfw/penpot/imported.json') && written.some((f) => f.startsWith('.dfw/penpot/changes/')) && written.some((f) => f.startsWith('.dfw/reviews/')), written.join(', '));
      assert.ok(!written.some((f) => f.startsWith('design/')), 'nothing in design/ changes');
      same(['penpot', 'import', 'changes.json', '--json'], { setup: withExport(file) });
      same(['penpot', 'import', 'changes.json', '--dry-run', '--json'], { setup: withExport(file) });
      // A frozen design: the same comments with the gate noted.
      const frozen = same(['penpot', 'import', 'changes.json'], { setup: withExport(file), state: FROZEN });
      assert.match(frozen.stdout, /The design is frozen \(G2\)/);
    }
    const tokens = same(['penpot', 'import', 'changes.json'], { setup: withExport('changes.json') });
    assert.ok('.dfw/penpot/tokens.from-penpot.json' in tokens.files);
    assert.match(tokens.stdout, /Token sets from Penpot: 1 token change\(s\); file \.dfw\/penpot\/tokens\.from-penpot\.json/);
  });

  test('penpot import twice: nothing is imported again; a reply continues its comment', () => {
    const twice = (edit) => (root) => {
      withExport('changes.json')(root);
      const first = spawnSync(process.execPath, [path.join(REPO, 'core/bin/dfw.mjs'), 'penpot', 'import', 'changes.json'], { cwd: root, env: { ...process.env, DFW_HOME: path.join(root, '.home'), DFW_NOW: NOW, DFW_NO_BINARY: '1', DFW_PROJECT_DIR: root, CLAUDE_PROJECT_DIR: root }, encoding: 'utf8' });
      assert.equal(first.status, 0, first.stderr);
      const changes = JSON.parse(fs.readFileSync(path.join(root, 'changes.json'), 'utf8'));
      write(root, 'changes.json', edit(changes));
    };
    const again = same(['penpot', 'import', 'changes.json'], { setup: twice((c) => c) });
    assert.match(again.stdout, /skipped \(already imported, or a resolved Penpot thread\)\.[\s\S]*No new comments\.\n$/);
    const moved = same(['penpot', 'import', 'changes.json'], {
      setup: twice((c) => {
        for (const b of c.boards) for (const l of b.layers) l.x += 3;
        for (const t of c.comments) t.x += 3;
        c.comments[0].comments.push({ content: 'Agreed.', user: 'Designer' });
        c.comments[1].resolved = false;
        c.pushedAt = '2026-11-16T12:30:00.000Z';
        return c;
      }),
    });
    assert.match(moved.stdout, /Moved by 3, 0 px/);
    assert.ok(Object.keys(moved.files).some((f) => f.startsWith('.dfw/reviews/')));
  });

  test('penpot import: untrusted change files', () => {
    const variants = {
      'exportId is a path': (c) => ({ ...c, exportId: '../../../outside' }),
      'exportId is not on disk': (c) => ({ ...c, exportId: 'px_20200101000000_deadbeef' }),
      'exportId is not a string': (c) => ({ ...c, exportId: 7 }),
      'pushedAt is a number': (c) => ({ ...c, pushedAt: 1731750000000 }),
      'pushedAt is a path': (c) => ({ ...c, pushedAt: '../../x' }),
      'pushedAt is missing': ({ pushedAt, ...c }) => c,
      'reserved names in the plugin tokens': (c) => ({ ...c, tokens: { sets: [{ name: 'base', active: true, tokens: [{ name: '__proto__.polluted', type: 'color', value: '#fff' }, { name: 'color.ok', type: 'color', value: '#000' }, { name: 'constructor', type: 'color', value: '#000' }] }, { name: '__proto__', tokens: [] }, { name: 'prototype', tokens: [{ name: 'a', type: 'color', value: '#000' }] }], themes: [{ name: 'T', sets: ['base', '__proto__'] }] } }),
      'a token file instead of plugin tokens': ({ tokens, ...c }) => ({ ...c, tokenFile: read('penpot-edited.tokens.json') }),
      'tokens only as a theme': (c) => ({ ...c, tokens: { sets: [{ name: 'themes/dark', tokens: [{ name: 'color.ink', type: 'color', value: '#000' }] }] } }),
      'unknown layers and boards': (c) => {
        c.boards[0].layers.push({ path: '9.9', name: 'ghost', kind: 'rect', x: 20, y: 140, w: 4, h: 4 }, { path: 5, name: 'num' }, { name: 'nopath', x: 1 });
        c.boards.push({ key: 'nope|a|desktop', name: 'Gone', layers: [] }, { key: 'items|a|desktop', layers: [], added: [{ name: 'again', type: 'ellipse', x: 0, y: 0 }] });
        c.missingBoards = ['items|a|desktop', 'nope'];
        return c;
      },
      'values of the wrong type': (c) => ({
        ...c,
        boards: [{ key: 'items|a|desktop', name: 'b', layers: [{ path: '0', x: '5', y: null, w: {}, h: [], fills: 'red', name: 7 }, { path: '0.1', x: 168.03, y: 35.94, w: 97.14, h: 40, fills: [{ color: 255, opacity: '0.5' }], name: '' }, { path: '0.1.0', characters: 5, x: 0, y: 0 }, { path: '0.3', fills: [{}], x: 16, y: 111.88, w: 992, h: 127 }], added: [{ x: 'a' }, {}] }],
        comments: [{ seq: 0, boardKey: 'items|a|desktop', x: 20, y: 40, comments: [{ content: '[DFW] not mine' }, { content: 0 }, { user: 'U' }] }, { boardKey: 'nope', comments: [{ content: 'Lost' }] }, { seq: 'x', componentKey: 'button|primary|', resolved: 1, comments: [{ content: 'Done?' }] }, { seq: 4, componentKey: 'button|primary|', comments: [] }],
        components: [{ key: 'button|primary|', name: '  button /   primary ', layers: [{ root: 1, x: 0, y: 0, w: 200, h: 40, fills: [], strokes: [{ color: '#FF0000', width: 2.5 }], radius: [null, '8', 6, 6] }, { path: 'main:x#0.1.0', kind: 'frame', x: 60, y: 11, w: 5, h: 5, name: 'renamed', fills: [{ color: '#fff', opacity: 0.5 }] }, { path: 'zzz' }], added: [{ name: 'a', x: 500, y: 500 }] }, { key: 'button|ghost|', layers: [{ root: true, x: 0, y: 0, w: 80.25, h: 40, strokes: [], fills: [{ color: '#e4e4e7' }], radius: [1, 2, 3, 4] }] }, { key: 'card||' }],
        missingComponents: ['button|primary|', 'button|ghost|x', 'nope'],
        tokens: { sets: [{ name: 'base', tokens: [{ name: 'radius.md', type: 'borderRadius', value: '{radius.md}' }, { type: 'color', value: '#000' }, { name: 'space.4', type: 'spacing', value: { a: 1 } }] }] },
      }),
    };
    for (const [name, edit] of Object.entries(variants)) {
      for (const flags of [['--dry-run'], [], ['--json']]) {
        const r = run(['penpot', 'import', 'changes.json', ...flags], { setup: (root) => (withExport('changes.json', edit)(root), write(root, 'outside.json', { ...fixtureExport(), id: 'evil' })) });
        assertSame(r);
        assert.equal(r.native.code, 0, `${name} ${flags}: ${r.native.stderr}`);
        if (name === 'exportId is a path' && !flags.length) assert.match(r.native.stdout, /^Compared with export px_20261116100000_/, 'falls back to the latest export');
        if (name === 'pushedAt is a number' && !flags.length) assert.ok('.dfw/penpot/changes/1731750000000.json' in r.native.files);
        if (name === 'pushedAt is a path' && !flags.length) assert.ok('.dfw/penpot/changes/changes.json' in r.native.files);
        if (name === 'unknown layers and boards' && !flags.length) assert.deepEqual(JSON.parse(r.native.files['.dfw/penpot/changes/20261116T110000000Z.json']).boards[0].added.map((a) => a.name), ['Last audit', 'ghost', 'num', 'nopath'], 'unknown layers are archived as added');
      }
    }
  });

  test('penpot import and plugin: usage and missing files', () => {
    for (const args of [['penpot'], ['penpot', 'nope'], ['penpot', 'import'], ['penpot', 'import', 'missing.json'], ['penpot', 'import', 'changes.json']]) assertSame(run(args, {}), { stderr: true });
    assertSame(run(['penpot', 'import', 'changes.json'], { setup: (root) => write(root, 'changes.json', { schema: 'x' }) }), { stderr: true });
    assertSame(run(['penpot', 'import', 'changes.json'], { setup: withExport('changes.json', (c) => ({ ...c, schema: 'dfw-penpot-changes/2' })) }), { stderr: true });
    for (const text of ['0', 'null', '""']) assertSame(run(['penpot', 'import', 'changes.json'], { setup: (root) => write(root, 'changes.json', text) }), { stderr: true });
    const plugin = same(['penpot', 'plugin'], {});
    assert.match(plugin.stdout, /^The Studio is not running[^\n]*\nPenpot plugin manifest: http:\/\/127\.0\.0\.1:4600\/__dfw\/penpot\/manifest\.json\n/);
    same(['penpot', 'plugin'], { setup: (root) => write(root, '.dfw/config.json', { studio: { port: 4711 } }) });
    // A Studio that is recorded but gone, and one that is alive (this process).
    same(['penpot', 'plugin'], { setup: (root) => write(root, '.dfw/studio.json', { pid: 2 ** 22 + 12345, url: 'http://127.0.0.1:4999/' }) });
    const alive = same(['penpot', 'plugin'], { setup: (root) => write(root, '.dfw/studio.json', { pid: process.pid, url: 'http://127.0.0.1:4999/' }) });
    assert.match(alive.stdout, /^Penpot plugin manifest: http:\/\/127\.0\.0\.1:4999\/__dfw\/penpot\/manifest\.json\n/);
  });

  test('penpot export needs screens and a known breakpoint', { skip: CHROME }, () => {
    for (const args of [['penpot', 'export', '--bp', 'tv'], ['penpot', 'export', '--page', 'nope'], ['penpot', 'export', '--bp'], ['penpot', 'export', '--page']]) assertSame(run(args, {}), { stderr: true });
    assertSame(run(['penpot', 'export'], { setup: (root) => fs.rmSync(path.join(root, 'design/design.json')) }), { stderr: true });
    assertSame(run(['penpot', 'export'], { setup: (root) => write(root, 'design/design.json', { screens: [] }) }), { stderr: true });
  });

  test('penpot export renders the same boards (needs Chrome)', { skip: CHROME }, () => {
    for (const args of [['penpot', 'export', '--bp', 'desktop'], ['penpot', 'export'], ['penpot', 'export', '--page', 'stock', '--json', '--out', '.dfw/out/e.json']]) {
      // tokens.css is built first, as a project has it: the screens are rendered with their styles.
      const r = run(args, { timeout: 180_000, setup: (root) => buildTokens(root) });
      assert.equal(r.node.code, 0, r.node.stderr);
      assert.equal(r.native.code, 0, r.native.stderr);
      const file = args.includes('--out') ? '.dfw/out/e.json' : '.dfw/penpot/export.json';
      const [a, b] = [JSON.parse(r.node.files[file]), JSON.parse(r.native.files[file])];
      assert.deepEqual(a.errors, []);
      assert.deepEqual(b.errors, []);
      assert.deepEqual(exportStructure(b), exportStructure(a));
      if (args.length === 4) assert.deepEqual(exportStructure(b), fs.readFileSync(path.join(FIX, 'export.structure.txt'), 'utf8').trimEnd().split('\n'));
      // Same machine, same browser, same fonts: the two renders are the same document.
      assertSame(r);
    }
  });
});

/* ------------------------------------------------------------------ live comments */

describe('penpot live comments', { skip: skipReason() }, () => {
  /** Two copies of the fixture project with the same export; `generatedAt` decides staleness. */
  function pair(generatedAt) {
    const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-conformance-live-')));
    const roots = ['node', 'native'].map((name) => {
      const root = path.join(tmp, name);
      fs.cpSync(path.join(FIX, 'project'), root, { recursive: true });
      write(root, '.dfw/state.json', `${JSON.stringify(MINIMAL_STATE, null, 2)}\n`);
      const doc = fixtureExport(generatedAt);
      write(root, `.dfw/penpot/exports/${doc.id}.json`, doc);
      write(root, '.dfw/penpot/export.json', doc);
      // An open session, so no session named after the real second is started.
      write(root, '.dfw/reviews/rv_1.json', { schema: 1, id: 'rv_1', startedAt: NOW, designVersion: 'v1', comments: [] });
      return root;
    });
    return { tmp, node: roots[0], native: roots[1], exportId: fixtureExport(generatedAt).id };
  }
  const tree = (root) => {
    const out = {};
    const walk = (dir) => {
      for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
        const abs = path.join(dir, e.name);
        if (e.isDirectory()) walk(abs);
        else out[path.relative(root, abs)] = fs.readFileSync(abs, 'utf8');
      }
    };
    walk(path.join(root, '.dfw'));
    return out;
  };
  /** One check of the plugin on both copies: same answer, same files. */
  function sync(p, payload, at) {
    process.env.DFW_NOW = at;
    let node;
    try {
      node = syncThreads(p.node, structuredClone(payload), { state: loadState(p.node) });
    } finally {
      delete process.env.DFW_NOW;
    }
    const r = native(['__debug', 'penpot-sync', p.native], { input: JSON.stringify(payload), env: { DFW_NOW: at } });
    assert.equal(r.status, 0, r.stderr);
    assert.equal(r.stdout, `${JSON.stringify(node)}\n`);
    assert.deepEqual(tree(p.native), tree(p.node));
    return node;
  }
  /** What the agent does between two checks, on both copies. */
  function agent(p, id, patch, at) {
    process.env.DFW_NOW = at;
    try {
      for (const root of [p.node, p.native]) updateComment(root, id, patch);
    } finally {
      delete process.env.DFW_NOW;
    }
  }
  const thread = (seq, messages, extra = {}) => ({ seq, resolved: false, page: 'Stock', boardKey: 'items|a|desktop', x: 120, y: 175, comments: messages.map((content) => ({ content, user: 'Designer' })), ...extra });
  const at = (minute) => `2026-01-02T03:${String(minute).padStart(2, '0')}:00.000Z`;

  test('__debug penpot-sync: new thread, ack, fixed, reply, resolve, un-resolve', () => {
    const p = pair('2099-01-01T00:00:00.000Z');
    try {
      const base = { fileId: 'file-1', exportId: p.exportId };
      // New threads: one open, one resolved before dfw saw it, one on a main component.
      const first = sync(p, { ...base, threads: [thread(1, ['Make low stock more visible']), thread(2, ['Old note'], { resolved: true }), thread(3, ['B']), { seq: 4, componentKey: 'button|primary|', x: 1, y: 1, comments: [{ content: 'Heavier', user: 'Lead', date: '2026-11-16T10:30:00Z' }] }] }, at(1));
      assert.equal(first.comments.length, 3);
      assert.equal(first.stale, false);
      const [a, b] = allComments(p.node).map((c) => c.id);
      const open = [thread(1, ['Make low stock more visible']), thread(3, ['B'])];
      assert.deepEqual(sync(p, { ...base, threads: open }, at(2)).comments, []);
      // Ack: one reply, offered until the plugin confirms it.
      agent(p, a, { status: 'acknowledged' }, at(3));
      const ack = sync(p, { ...base, threads: open }, at(4)).replies;
      assert.equal(ack.length, 1);
      assert.equal(sync(p, { ...base, threads: open }, at(5)).replies.length, 1);
      assert.equal(sync(p, { ...base, threads: open, posted: [{ seq: 1, key: ack[0].key }, { seq: 9, key: 'k' }, { seq: 1 }, null] }, at(6)).replies.length, 0);
      // A question from the agent, then the fix.
      agent(p, a, { message: 'Which rows exactly?' }, at(7));
      agent(p, a, { status: 'fixed', message: 'Low stock rows now have a warning badge.' }, at(8));
      const fixed = sync(p, { ...base, threads: open }, at(9)).replies;
      assert.equal(fixed.length, 2);
      const posted = fixed.map(({ seq, key }) => ({ seq, key }));
      const withReplies = [thread(1, ['Make low stock more visible', ...fixed.map((f) => f.text)]), thread(3, ['B'])];
      assert.deepEqual(sync(p, { ...base, threads: withReplies, posted }, at(10)).updated, []);
      // The designer replies: reopened. Fixed again, then the thread is resolved: accepted.
      const replied = [thread(1, ['Make low stock more visible', ...fixed.map((f) => f.text), 'Still too small']), thread(3, ['B'])];
      assert.deepEqual(sync(p, { ...base, threads: replied }, at(11)).updated, [{ id: a, status: 'reopened' }]);
      agent(p, a, { status: 'fixed', message: 'Bigger now.' }, at(12));
      const resolved = replied.map((t) => ({ ...t, resolved: true }));
      assert.deepEqual(sync(p, { ...base, threads: resolved }, at(13)).updated, [{ id: a, status: 'resolved' }, { id: b, status: 'dismissed' }]);
      assert.deepEqual(sync(p, { ...base, threads: resolved }, at(14)).updated, []);
      // Un-resolve: reopened; and a thread whose dfw comment cannot change says so.
      assert.deepEqual(sync(p, { ...base, threads: [{ ...replied[0], resolved: false }, resolved[1]] }, at(15)).updated, [{ id: a, status: 'reopened' }]);
      // Another file, and payloads of the wrong shape.
      assert.deepEqual(sync(p, { fileId: 'file-2', exportId: p.exportId, threads: [] }, at(16)).replies, []);
      sync(p, { fileId: 7, exportId: '../x', threads: 'x', posted: 'y' }, at(17));
      sync(p, { fileId: 'f'.repeat(200), threads: [{ seq: 1, comments: [{ content: 'No board' }] }, { seq: null, boardKey: 'items|a|desktop', x: 0, y: 0, comments: [{ content: 'No number' }] }] }, at(18));
      sync(p, {}, at(19));
    } finally {
      fs.rmSync(p.tmp, { recursive: true, force: true });
    }
  });

  test('__debug penpot-sync: boards are behind once a design file is newer than the export', () => {
    const p = pair('2020-01-01T00:00:00.000Z');
    try {
      assert.equal(sync(p, { fileId: 'f', exportId: p.exportId, threads: [] }, at(1)).stale, true);
    } finally {
      fs.rmSync(p.tmp, { recursive: true, force: true });
    }
  });
});
