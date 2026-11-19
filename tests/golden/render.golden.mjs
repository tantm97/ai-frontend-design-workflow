// Render checks of the binary: `layout check`, the axe check, `tokens extract` and the layout
// check of `hook stop`, compared with the results the Node CLI of 0.5.2 recorded in
// tests/golden/render/. Run: node --test tests/golden/
//
// The tests that render need a real browser and run only with DFW_TEST_CHROME=1. Every run gets
// its own TMPDIR, which must hold no browser profile afterwards. Nothing is downloaded: axe-core
// is copied from the dfw home of this machine when it is there (else only the "not downloaded"
// answer is compared), and `tokens extract` reads a local file.
//
// Node pushed findings in the order its six tabs finished, which changed from run to run; the
// binary reports them in job order. So findings are compared as sorted lists.
import { describe } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { REPO, NOW, MINIMAL_STATE, nativeBinary, skipReason, assertSame, run, runWith, suite, expected } from './harness.mjs';

const test = suite('render');

const CHROME = process.env.DFW_TEST_CHROME === '1' ? false : 'needs Chrome: set DFW_TEST_CHROME=1';
const HOSTS = [[], ['--host', 'copilot'], ['--host', 'claude']];
const CONCEPTS = { ...MINIMAL_STATE, phase: 'concepts' };
/** The axe-core version the binary pins (AXE_VERSION in native/src/browser/a11y.rs). */
const AXE_VERSION = '4.14.0';
const AXE_HOME = path.join(process.env.DFW_HOME || path.join(process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config'), 'dfw'), 'tools', 'axe-core', AXE_VERSION);
const HAS_AXE = fs.existsSync(path.join(AXE_HOME, 'axe.min.js'));

const write = (root, rel, data) => {
  const file = path.join(root, rel);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, typeof data === 'string' ? data : JSON.stringify(data, null, 2) + '\n');
};

/** Mark the design of a project as checked (the layout cache), as `markChecked` of 0.5.2 did. */
const markChecked = (root) => {
  const r = spawnSync(nativeBinary(), ['__debug', 'mark-checked', root], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
};

/** Findings in a stable order: Node's order depended on which tab finished first. */
const sorted = (findings) => [...findings].sort((a, b) => (JSON.stringify(a) < JSON.stringify(b) ? -1 : 1));

/** stdout of `layout check` (text or --json) and of `hook stop`, with the finding lines sorted. */
function stable(text) {
  if (text.startsWith('Layout: ')) {
    const [head, ...lines] = text.trimEnd().split('\n');
    return [head, ...lines.sort()].join('\n') + '\n';
  }
  let v;
  try {
    v = JSON.parse(text);
  } catch {
    return text;
  }
  if (Array.isArray(v?.findings) && v.status) return JSON.stringify({ ...v, findings: sorted(v.findings) }, null, 2) + '\n';
  const sortBlock = (s) => {
    if (typeof s !== 'string' || !s.startsWith('dfw layout check after your edits')) return s;
    const lines = s.split('\n');
    return [lines[0], ...lines.slice(1, -1).sort(), lines.at(-1)].join('\n');
  };
  if (v && typeof v === 'object' && !Array.isArray(v)) return JSON.stringify(Object.fromEntries(Object.entries(v).map(([k, x]) => [k, sortBlock(x)])));
  return text;
}

/** A TMPDIR for one run; `done()` fails when a browser or its profile is left behind. */
function scratch() {
  const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-render-')));
  return {
    dir,
    done() {
      const profiles = fs.readdirSync(dir).filter((f) => f.startsWith('dfw-browser-'));
      const browsers = spawnSync('pgrep', ['-f', `user-data-dir=${dir}/dfw-browser-`], { encoding: 'utf8' }).stdout.trim();
      fs.rmSync(dir, { recursive: true, force: true });
      assert.deepEqual([profiles, browsers], [[], ''], 'a browser or its profile was left behind');
    },
  };
}

/** What assertSame compares without stderr: exit code, stdout and the files written. */
const shown = (r) => ({ code: r.code, stdout: r.stdout, files: r.files });

/** `dfw <args>` with the binary; its exit code, stdout and files are the recorded ones. */
function same(args, opts) {
  const native = runWith(nativeBinary(), [], args, opts);
  const now = shown(native);
  assert.deepEqual(now, expected(now));
  return native;
}

/** `same` with a private TMPDIR, sorted findings and the leftover check. */
function render(args, opts = {}) {
  const tmp = scratch();
  try {
    return same(args, { timeout: 120_000, normalize: stable, ...opts, env: { TMPDIR: tmp.dir, ...opts.env } });
  } finally {
    tmp.done();
  }
}

const page = (body, head = '') => `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>T</title><style>body{margin:0;font:16px/1.4 sans-serif}${head}</style></head><body>${body}</body></html>`;

/** Small hand-written screens, each with one known problem. */
const PROBLEMS = {
  overlap: page('<main><p data-dfw-id="first" style="position:absolute;left:20px;top:20px;width:200px;height:40px;margin:0">First block of text</p><p data-dfw-id="second" style="position:absolute;left:60px;top:30px;width:200px;height:40px;margin:0">Second block of text</p><div style="height:700px;background:#eee"></div></main>'),
  overflow: page('<main><div data-dfw-id="wide" style="width:2000px;height:720px;background:#ccd">Wider than any screen</div></main>'),
  anchor: page('<main><div style="height:1500px;background:#eee">Long content</div><button data-dfw-anchor="save" data-dfw-id="save" style="height:48px;width:160px">Save</button></main>'),
  void: page('<main><p data-dfw-id="top" style="margin:0;height:60px;background:#dde">Top</p><div style="height:420px"></div><p data-dfw-id="bottom" style="margin:0;height:400px;background:#dde">Bottom</p></main>'),
  touch: page('<main><button data-dfw-id="tiny" style="height:30px;width:120px">Small</button><div style="height:760px;background:#eee">Body</div></main>'),
};

/** A project of plain screens (no masters, so no Design System page). */
const problemProject = (ids) => (root) => {
  fs.rmSync(path.join(root, 'design'), { recursive: true, force: true });
  write(root, 'design/design.json', {
    schema: 1,
    product: 'Fixture',
    breakpoints: [{ id: 'desktop', width: 1280, height: 800 }, { id: 'mobile', width: 390, height: 700 }],
    states: ['empty'],
    screens: ids.map((id) => ({ id, variants: [{ id: 'a', file: `screens/${id}/a.html` }] })),
  });
  for (const id of ids) write(root, `design/screens/${id}/a.html`, PROBLEMS[id]);
};

describe('render: layout check', { skip: skipReason() }, () => {
  test('usage, and --changed with nothing changed (no browser needed)', () => {
    // `__debug mark-checked` writes the layout cache markChecked of 0.5.2 wrote.
    const cache = (() => {
      const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-mark-')));
      try {
        fs.cpSync(path.join(REPO, 'tests', 'fixtures', 'penpot', 'project'), dir, { recursive: true });
        markChecked(dir);
        return fs.readFileSync(path.join(dir, '.dfw', 'layout-cache.json'), 'utf8');
      } finally {
        fs.rmSync(dir, { recursive: true, force: true });
      }
    })();
    assert.equal(cache, expected(cache));
    const usage = run(['layout', 'measure']);
    assertSame(usage, { stderr: true });
    assert.equal(usage.native.code, 1);
    const r = same(['layout', 'check', '--changed'], { setup: (root) => markChecked(root) });
    assert.deepEqual([r.stdout, r.files], ['Layout: nothing changed since the last check.\n', {}]);
  });

  test('the fixture project: text, --json, --states, --no-stress, --screen, --bp, --no-system, unknown flags', { skip: CHROME }, () => {
    const text = render(['layout', 'check']);
    assert.match(text.stdout, /^Layout: 6 render\(s\), 0 must, \d+ should\.\n/);
    assert.match(text.stdout, /\(touch-target\) items\/a @ mobile: 2 tappable/);
    assert.ok(text.files['.dfw/layout-cache.json'] && text.files['.dfw/build/design/system/index.html'], 'cache and composed pages are written');
    const json = JSON.parse(render(['layout', 'check', '--json']).stdout);
    assert.deepEqual([json.status, json.renders, Object.keys(json.findings[0])], ['ran', 6, ['level', 'rule', 'file', 'line', 'message', 'source']]);
    assert.match(render(['layout', 'check', '--no-stress', '--no-system']).stdout, /^Layout: 2 render/);
    assert.match(render(['layout', 'check', '--bp', 'mobile', '--no-system', '--json']).stdout, /"renders": 3/);
    assert.match(render(['layout', 'check', '--screen', 'nope,other']).stdout, /^Layout: 1 render/);
    assert.match(render(['layout', '--bp', 'nope', '--no-system']).stdout, /^Layout: 1 render/, 'only the reflow render is left');
    // Flags the command does not know (--stress is the default, --page does not exist) change nothing.
    assert.match(render(['layout', 'check', '--stress', '--page', 'stock', '--states']).stdout, /^Layout: 6 render/);
    // With masters but no breakpoint left, Node failed on the Design System job (`r.bp.id`); so does the binary.
    const none = runWith(nativeBinary(), [], ['layout', 'check', '--bp', 'nope'], { timeout: 120_000 });
    const noneSeen = { code: none.code, stdout: none.stdout };
    assert.deepEqual(noneSeen, expected(noneSeen));
    assert.deepEqual([none.code, none.stdout, none.stderr.split('\n')[0]], [1, '', "Error: Cannot read properties of undefined (reading 'id')"]);
    const states = (root) => write(root, 'design/design.json', { ...JSON.parse(fs.readFileSync(path.join(root, 'design/design.json'), 'utf8')), states: ['empty', 'error'] });
    assert.match(render(['layout', 'check', '--states', '--json'], { setup: states }).stdout, /"renders": 10/);
  });

  test('--changed: first everything, then nothing, then the edited screen only', { skip: CHROME }, () => {
    const first = render(['layout', 'check', '--changed']);
    assert.match(first.stdout, /^Layout: 6 render/);
    const edited = (root) => {
      markChecked(root);
      fs.appendFileSync(path.join(root, 'design/screens/items/a.html'), '<!-- edited -->\n');
    };
    const again = render(['layout', 'check', '--changed'], { setup: edited });
    assert.match(again.stdout, /^Layout: 5 render/, 'no Design System page: no master changed');
    const master = (root) => {
      markChecked(root);
      fs.appendFileSync(path.join(root, 'design/system/components/button.html'), '\n');
    };
    assert.match(render(['layout', 'check', '--changed', '--json'], { setup: master }).stdout, /"renders": 6/);
  });

  test('hand-written screens: overlap, overflow, off-screen anchor, void, tiny touch target', { skip: CHROME }, () => {
    const ids = Object.keys(PROBLEMS);
    const r = render(['layout', 'check', '--json', '--states'], { setup: problemProject(ids) });
    assert.equal(r.code, 2);
    assert.deepEqual(Object.keys(r.files).filter((f) => f.endsWith('layout-cache.json')), [], 'MUST findings: the screens are not marked as checked');
    const found = JSON.parse(r.stdout).findings.map((f) => `${f.level} ${f.rule} ${f.message.split(':')[0]}`);
    for (const want of [
      'error overlap overlap/a @ desktop',
      'error overlap overlap/a @ mobile [empty]',
      'error h-overflow overflow/a @ desktop',
      'warn reflow overflow/a @ 320px, text 125%',
      'error anchor anchor/a @ desktop',
      'error anchor anchor/a @ mobile [stress',
      'warn void void/a @ desktop',
      'warn touch-target touch/a @ mobile',
    ]) assert.ok(found.includes(want), `${want}\n${found.join('\n')}`);
    assert.ok(!found.includes('warn touch-target touch/a @ desktop'));
    const text = render(['layout', 'check', '--screen', 'overlap,touch'], { setup: problemProject(ids) });
    assert.equal(text.code, 2);
    assert.match(text.stdout, /^Layout: 10 render\(s\), 4 must, \d+ should\.\n- \[MUST\] \(overlap\) overlap\/a @ desktop \[stress: long lists\]: "first" overlaps "second" \(\d+px²\)\.\n/);
    // A screen whose file is missing is a render finding, and a clean design is marked as checked.
    const missing = render(['layout', 'check', '--no-stress'], { setup: (root) => { problemProject(['touch'])(root); fs.rmSync(path.join(root, 'design/screens/touch/a.html')); } });
    assert.match(missing.stdout, /\(render\) touch\/a @ desktop: could not render \(The "path" argument must be of type string\. Received undefined\)/);
    assert.deepEqual([missing.code, Object.keys(missing.files)], [0, ['.dfw/layout-cache.json']]);
  });
});

describe('render: hook stop', { skip: skipReason() }, () => {
  /** `hook stop` for one host; the project is found through the environment the harness sets. */
  const hook = (host, opts = {}, chrome = false) => {
    const args = ['hook', 'stop', ...host];
    const o = { stdin: JSON.stringify({ session_id: 's1', hook_event_name: 'Stop', stop_hook_active: false }), state: CONCEPTS, ...opts };
    return chrome ? render(args, o) : same(args, o);
  };

  test('nothing changed, comments pending, layoutOnStop false (no browser needed)', () => {
    const comment = { id: 'fb_001', status: 'open', text: 'Make the title bigger', screen: 'items', variant: 'a', createdAt: NOW };
    for (const host of HOSTS) {
      assert.deepEqual([hook(host, { setup: (root) => markChecked(root) }).stdout, hook(host, { setup: (root) => markChecked(root) }).files], ['', {}]);
      const pending = hook(host, { setup: (root) => write(root, '.dfw/reviews/rv_1.json', { schema: 1, id: 'rv_1', startedAt: NOW, designVersion: 'v1', comments: [comment] }) });
      assert.match(pending.stdout, /new Studio comment/);
      assert.ok(!('.dfw/layout-cache.json' in pending.files), 'comments first: no layout check in the same stop');
      const off = hook(host, { setup: (root) => write(root, '.dfw/config.json', { checks: { layoutOnStop: false } }) });
      assert.deepEqual([off.stdout, off.files], ['', {}]);
    }
  });

  test('a changed screen with a MUST finding blocks and is marked; a changed master re-renders the Design System page', { skip: CHROME }, () => {
    const broken = (root) => {
      markChecked(root);
      const file = path.join(root, 'design/screens/items/a.html');
      fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replace('<section class="panel"', '<div data-dfw-id="items.wide" style="width:2400px;height:40px;background:#ccd">Too wide</div>\n    <section class="panel"'));
    };
    for (const host of HOSTS) {
      const r = hook(host, { setup: broken }, true);
      const reason = Object.values(JSON.parse(r.stdout)).find((v) => typeof v === 'string' && v.startsWith('dfw layout check after your edits'));
      assert.match(reason, /^dfw layout check after your edits \(1 screen\(s\)\):\n- \[MUST\] \(h-overflow\) items\/a @ desktop/);
      assert.match(reason, /\nFix these in the design system \(masters, page layout\) where possible, re-run "dfw layout check --changed", then finish\. See references\/layout-patterns\.md\.$/);
      assert.ok(r.files['.dfw/layout-cache.json'], 'reported once: the design is marked as checked');
    }
    const master = (root) => {
      markChecked(root);
      fs.appendFileSync(path.join(root, 'design/system/components/button.html'), '\n');
    };
    const system = hook([], { setup: master, state: { ...CONCEPTS, phase: 'system' } }, true);
    assert.ok(system.files['.dfw/layout-cache.json'] && system.files['.dfw/build/design/system/index.html'], 'a changed master re-renders the Design System page');
  });

  // On a machine with Chrome in /Applications (or a browser in /usr/bin) no variable hides it:
  // DFW_BROWSER at a missing path only skips the first of three lookups. The case runs where
  // the doctor of the binary finds no browser with an empty home and PATH.
  const bare = (tmp) => ({ DFW_BROWSER: path.join(tmp, 'missing-browser'), CHROME_PATH: '', HOME: tmp, PATH: path.dirname(nativeBinary()), PROGRAMFILES: '', 'PROGRAMFILES(X86)': '', LOCALAPPDATA: '' });
  const probe = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-probe-'));
  const doctor = spawnSync(nativeBinary(), ['doctor'], { cwd: probe, env: { ...bare(probe), DFW_HOME: probe }, encoding: 'utf8' }).stdout || '';
  const found = /^ok +browser for screenshots and layout check - (.*)$/m.exec(doctor)?.[1] ?? '';
  fs.rmSync(probe, { recursive: true, force: true });

  test('no browser installed: the stop passes and nothing is marked', { skip: found ? `a browser is installed where no variable hides it (${found})` : false }, () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-bare-'));
    try {
      for (const host of HOSTS) {
        const r = hook(host, { env: bare(tmp) });
        assert.deepEqual([r.stdout, r.files], ['', {}]);
      }
      const layout = same(['layout', 'check'], { env: bare(tmp) });
      assert.match(layout.stdout, /^Layout check unavailable: no Chrome, Edge or Chromium found/);
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });
});

describe('render: tokens extract', { skip: skipReason() }, () => {
  const SITE = page('<header style="background:#0d6b78;color:#fff;padding:16px 24px"><h1 style="font:600 24px Georgia,serif;margin:0">Shop</h1></header><main style="padding:24px;display:grid;gap:12px"><p style="color:#1f2937">Plain text in <a href="#" style="color:#0d6b78">a link</a>.</p><button style="border:1px solid #cbd5e1;border-radius:6px;padding:8px 12px;background:#f8fafc;font:14px \'Helvetica Neue\',Arial,sans-serif">Buy</button><div style="border-radius:12px;background:#f8fafd;padding:8px;height:40px">Card</div></main>');
  const site = (root) => write(root, 'site/index.html', SITE);
  const url = (root) => pathToFileURL(path.join(root, 'site/index.html')).href;

  test('usage without --url', () => {
    const r = run(['tokens', 'extract']);
    assertSame(r, { stderr: true });
    assert.equal(r.native.code, 1);
  });

  test('a local page: the seed file and the summary line', { skip: CHROME }, () => {
    // The URL holds the project folder of the run, so the page is addressed by its own path
    // and the folder is masked in what is compared.
    const tmp = scratch();
    try {
      for (const out of [[], ['--out', 'seed/tokens.json']]) {
        const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-native-')));
        let native;
        try {
          site(dir);
          const args = ['tokens', 'extract', '--url', url(dir), '--root', dir, ...out];
          const env = { ...process.env, DFW_HOME: path.join(dir, 'home'), DFW_NOW: NOW, DFW_PLUGIN_ROOT: path.join(REPO, 'core'), TMPDIR: tmp.dir };
          for (const k of ['DFW_PROJECT_DIR', 'CLAUDE_PROJECT_DIR']) delete env[k];
          const r = spawnSync(nativeBinary(), args, { cwd: dir, env, encoding: 'utf8', timeout: 120_000 });
          const file = path.join(dir, out[1] || 'design/system/extracted.tokens.json');
          native = { code: r.status, stdout: r.stdout, stderr: r.stderr.split(dir).join('<root>'), file: fs.existsSync(file) ? fs.readFileSync(file, 'utf8').split(dir).join('<root>') : null };
        } finally {
          fs.rmSync(dir, { recursive: true, force: true });
        }
        assert.deepEqual(native, expected(native));
        assert.equal(native.code, 0, native.stderr);
        assert.match(native.stdout, new RegExp(`^Wrote ${out[1] || 'design/system/extracted.tokens.json'}: \\d+ colours, 3 font families, \\d+ font sizes, 2 radii, \\d+ spacing values\\. It is a seed`));
        const seed = JSON.parse(native.file);
        assert.match(seed.$description, /^Extracted from file:\/\/<root>\/site\/index\.html on 2026-01-02 by "dfw tokens extract"/);
        assert.ok(Object.values(seed.font.family).some((f) => JSON.stringify(f.$value) === '["Helvetica Neue","Arial","sans-serif"]'), 'quotes are stripped from family names');
        assert.ok(Object.values(seed.color).some((c) => c.$value?.hex === '#0d6b78'));
      }
    } finally {
      tmp.done();
    }
  });
});

describe('render: accessibility check (axe)', { skip: skipReason() }, () => {
  const BAD = page('<main><h1>Orders</h1><img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" width="40" height="40"><form><input type="text" name="q"></form><p style="color:#bbb;background:#fff">Hard to read</p><button aria-pressed="false">Dead toggle</button><button aria-pressed="false" onclick="this.setAttribute(\'aria-pressed\', \'true\')">Live toggle</button></main>');

  /** The a11y check of the binary (`__debug a11y`) as JSON, on its own project copy and dfw home; it is the recorded one. */
  function axe(opts, { withAxe = HAS_AXE, project = path.join(REPO, 'tests/fixtures/penpot/project') } = {}) {
    const tmp = scratch();
    let native;
    try {
      const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-native-')));
      try {
        const root = path.join(dir, 'project');
        fs.cpSync(project, root, { recursive: true });
        write(root, 'site/bad.html', BAD);
        if (withAxe) fs.cpSync(AXE_HOME, path.join(dir, 'home', 'tools', 'axe-core', AXE_VERSION), { recursive: true });
        const o = { ...opts, ...(opts.url ? { url: pathToFileURL(path.join(root, opts.url)).href } : {}) };
        const flags = [...(o.url ? ['--url', o.url] : []), ...(o.states ? ['--states'] : []), ...(o.themes ? ['--themes'] : []), ...(o.screens ? ['--screen', o.screens.join(',')] : [])];
        const env = { ...process.env, DFW_HOME: path.join(dir, 'home'), DFW_NOW: NOW, DFW_PLUGIN_ROOT: path.join(REPO, 'core'), TMPDIR: tmp.dir };
        for (const k of ['DFW_PROJECT_DIR', 'CLAUDE_PROJECT_DIR']) delete env[k];
        const r = spawnSync(nativeBinary(), ['__debug', 'a11y', '--root', root, ...flags], { cwd: root, env, encoding: 'utf8', timeout: 120_000 });
        assert.equal(r.status, 0, r.stderr);
        const res = JSON.parse(r.stdout.split(dir).join('<tmp>'));
        native = { ...res, findings: sorted(res.findings) };
      } finally {
        fs.rmSync(dir, { recursive: true, force: true });
      }
    } finally {
      tmp.done();
    }
    assert.deepEqual(native, expected(native));
    return native;
  }

  test('axe not downloaded: unverifiable, never a pass (no page is opened)', () => {
    const r = axe({ url: 'site/bad.html' }, { withAxe: false });
    assert.deepEqual([r.tool, r.status, r.findings], ['axe', 'unverifiable', []]);
  });

  test('a live page with known violations, and dead controls', { skip: CHROME || (HAS_AXE ? false : `axe-core ${AXE_VERSION} is not in ${AXE_HOME} (dfw setup --a11y)`) }, () => {
    const r = axe({ url: 'site/bad.html' });
    assert.equal(r.status, 'ran');
    const rules = r.findings.map((f) => `${f.level} ${f.rule}`);
    for (const want of ['error axe-image-alt', 'error axe-label', 'error axe-color-contrast', 'warn dead-control']) assert.ok(rules.includes(want), `${want}\n${rules.join('\n')}`);
    assert.deepEqual(Object.keys(r.findings[0]), ['level', 'rule', 'file', 'line', 'source', 'message']);
    const dead = r.findings.filter((f) => f.rule === 'dead-control');
    assert.equal(dead.length, 1);
    assert.match(dead[0].message, /control "Dead toggle" announces a state/);
  });

  test('the design screens: every state and theme, one screen, no screen', { skip: CHROME || (HAS_AXE ? false : 'axe-core is not downloaded') }, () => {
    assert.equal(axe({}).status, 'ran');
    assert.equal(axe({ states: true, themes: true }).status, 'ran');
    assert.deepEqual(axe({ screens: ['nope'] }), { tool: 'axe', status: 'skipped', findings: [] });
  });
});
