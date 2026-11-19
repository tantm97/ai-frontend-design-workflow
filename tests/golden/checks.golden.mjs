// Checks: design lint, the design-system audit, the check runner with its external tools, the
// DS gate and the PostToolUse hook that runs them. Commands run with the binary on copies of a
// fixture project; the lint functions are also checked one by one through the binary's hidden
// `__debug lint` command, on every fixture, kit and template file and on hand-written hostile
// inputs. The recorded results in tests/golden/checks/ are what the Node CLI and lint module of
// 0.5.2 printed, in the shape each case compares.
//
// No test starts the real Impeccable engine, Hallmark's sloplint or the Taste copy-lint, and
// none uses the network: every run that can reach an external tool points DFW_PLUGIN_ROOT at a
// temporary marketplace of stand-in scripts and moves HOME, the Claude config folder and the
// engine cache into the run's temporary folder.
// Run: node --test tests/golden/
import { describe, before, after } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { assertSame, expected, nativeBinary, runWith, skipReason, suite, REPO, NOW, MINIMAL_STATE } from './harness.mjs';

const test = suite('checks');
const CORE = path.join(REPO, 'core');
const KIT = path.join(CORE, 'kits', 'base', 'components');
const FIXTURE = path.join(REPO, 'tests', 'fixtures', 'penpot', 'project');
const SCREEN = 'design/screens/items/a.html';

const POSIX_ONLY = process.platform === 'win32' ? 'the stand-in tools are sh scripts' : false;

const write = (root, rel, data) => {
  const file = path.join(root, rel);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, typeof data === 'string' || Buffer.isBuffer(data) ? data : JSON.stringify(data, null, 2) + '\n');
  return file;
};
const all = (...setups) => (root) => setups.forEach((s) => s?.(root));
const st = (over = {}) => structuredClone({ ...MINIMAL_STATE, ...over });

/**
 * Run the binary in a project while it is prepared (system init, icons add, build), as the Node
 * functions of the same names did in-process when the results were recorded.
 */
function dfwIn(root, args) {
  const env = { ...process.env };
  for (const k of ['DFW_NO_BINARY', 'DFW_VERSION', 'XDG_CONFIG_HOME', 'CLAUDE_CONFIG_DIR']) delete env[k];
  Object.assign(env, { DFW_HOME: path.join(path.dirname(root), 'setup-home'), DFW_NOW: NOW, DFW_PROJECT_DIR: root, CLAUDE_PROJECT_DIR: root, DFW_PLUGIN_ROOT: CORE });
  const r = spawnSync(nativeBinary(), args, { cwd: root, env, encoding: 'utf8', timeout: 180_000 });
  if (r.error) throw r.error;
  assert.equal(r.status, 0, `dfw ${args.join(' ')}: ${r.stderr}`);
}

/** The words of a JSON parser differ between V8 and serde: findings that quote them are cut after the rule. */
const cutParserWords = (line) => line.replace(/(\(dfw\/(?:manifest|token)-json\)) .*/g, '$1 <parser words>');
function parserWords(text) {
  const fix = (v) => {
    if (typeof v === 'string') return v.split('\n').map(cutParserWords).join('\n');
    if (Array.isArray(v)) return v.map(fix);
    if (v && typeof v === 'object') {
      const o = Object.fromEntries(Object.entries(v).map(([k, x]) => [k, fix(x)]));
      if (o.rule === 'manifest-json' || o.rule === 'token-json') o.message = '<parser words>';
      return o;
    }
    return v;
  };
  try {
    return JSON.stringify(fix(JSON.parse(text)), null, 1);
  } catch {
    return fix(text);
  }
}

/** One run of the binary on its own copy of the project. */
const raw = (args, opts = {}) => runWith(nativeBinary(), [], args, opts);
/** What `assertSame` compares of one result: exit code, stdout, written files, and stderr when asked. */
const seen = (r, { stderr = false } = {}) => ({ code: r.code, stdout: r.stdout, ...(stderr ? { stderr: r.stderr } : {}), files: r.files });
/** Where Node failed (or died) in its own words, only the exit code and stdout are compared. */
const codeAndStdout = (r) => ({ code: r.code, stdout: r.stdout });

/** Run the binary, parser words masked. */
function run(args, opts = {}) {
  opts.normalize ??= parserWords; // set on the object itself: `stdin` may be a getter (see hookOpts)
  return raw(args, opts);
}

/** The native result against the next recorded one (exit code, stdout, files; stderr when asked). Returns it. */
function check(native, { stderr = false } = {}) {
  const want = expected(seen(native, { stderr }));
  assertSame({ expected: { stderr: '', ...want }, native }, { stderr });
  return native;
}

/** Same stdout, exit code and files as recorded. Returns the native result. */
const same = (args, opts) => check(run(args, opts));

/** As `same`, also the same stderr (an error a user caused: its message is part of the contract). */
const sameWithStderr = (args, opts) => check(run(args, opts), { stderr: true });

/**
 * `--json` of lint and check: compared without the "unverified" array (the binary must print
 * it). Returns the native object.
 */
function sameJson(args, opts) {
  return jsonSame(run(args, opts));
}
function jsonSame(native) {
  const data = JSON.parse(native.stdout);
  assert.ok(Array.isArray(data.unverified), 'native --json lists the tools that did not run');
  const { unverified, ...rest } = data;
  check({ ...native, stdout: JSON.stringify(rest) });
  return data;
}

/**
 * Arguments that need the path of the run's own project copy: `argsOf(root)` is evaluated after
 * the harness called `setup`, before it builds the command line.
 */
function withRoot(argsOf, opts = {}) {
  const args = [];
  return [args, { ...opts, setup: (root) => (opts.setup?.(root), args.splice(0, args.length, ...argsOf(root))) }];
}

// ---------------------------------------------------------------- projects

const BAD_SCREEN = `<!doctype html>
<html>
<head><style>.x { color: #6366f1; z-index: 40 } input { font-size: 12px }</style></head>
<body>
<main data-dfw-id="Main Area"><h1>🚀 Lorem ipsum</h1>
<img src="https://picsum.photos/200">
<button style="color:red" data-dfw-id="go">Click</button><button data-dfw-id="go">Again...</button>
<a href="#">click here</a><p>Seamless and 10x faster. Oops, something went wrong</p>
<table></table><form></form>
<dfw-use c="button" variant="nope" id="b1">Go</dfw-use><dfw-use c="missing">x</dfw-use>
</main>
</body>
</html>
`;
const RATIONALE = `# Design\n\n## Rationale\n${['Accent is signal blue because red, green and yellow already mean stop, go and caution.', 'Neutrals are cool steel because the counter sits under fluorescent light.', 'Base 16 with a 1.2 UI ratio because operators read from one metre.', 'Large density because users wear gloves at the counter.', 'Hazard tape motif only on the scan zone because it marks where to act.'].map((x) => `- ${x}`).join('\n')}\n`;

/** The fixture with every kind of finding: a bad screen, a stylesheet, a theme, a broken token file. */
const messy = (root) => {
  write(root, SCREEN, BAD_SCREEN);
  write(root, 'design/styles/site.css', '/* #fff */\n.a { color: #fff; transition: all .2s }\n.b { outline: none; border-left: 3px solid red; border-radius: 4px }\n.c { background: linear-gradient(#3b82f6, #8b5cf6) }\n');
  write(root, 'design/tokens/themes/dark.json', { color: { $type: 'color', ink: { $value: '{color.nope}' }, a: { $value: '#4F46E5' }, b: { $value: '#4f46e6' } }, 'bad.name': { $value: 1 } });
  write(root, 'design/tokens/themes/broken.json', '{ "color": ');
  write(root, 'design/approved/v1/screens/items/a.html', '<img>');
  write(root, 'design/system/head.html', '<img>');
  write(root, 'README.md', '# not a design file\n');
  const master = path.join(root, 'design/system/components/button.html');
  fs.writeFileSync(master, fs.readFileSync(master, 'utf8').replace('variants="primary ghost"', 'variants="primary ghost" replaces="button"'));
};

const ONLY_NESTED = 'design/screens/only/a.html';
/** A second project inside the copy: `--root nested` makes targets relative to the working directory differ from project-relative ones. */
const nested = (root) => {
  fs.cpSync(path.join(root, 'design'), path.join(root, 'nested', 'design'), { recursive: true });
  write(root, 'nested/.dfw/state.json', MINIMAL_STATE);
  write(root, `nested/${SCREEN}`, BAD_SCREEN);
  write(root, `nested/${ONLY_NESTED}`, BAD_SCREEN);
};

// ---------------------------------------------------------------- lint

describe('checks: lint', { skip: skipReason() }, () => {
  test('whole design: text and --json, clean fixture and one with every kind of finding', () => {
    for (const setup of [undefined, messy]) {
      const text = same(['lint'], { setup });
      assert.match(text.stdout, /^Checks: \d+ must, \d+ should, \d+ nice\. Tools: dfw-lint=ran, dfw-ds-audit=ran\n/);
      const data = sameJson(['lint', '--json'], { setup });
      assert.deepEqual(data.results.map((r) => r.tool), ['dfw-lint', 'dfw-ds-audit']);
      assert.deepEqual(data.unverified, []);
      assert.equal(text.code, data.tally.error ? 2 : 0);
    }
    const found = sameJson(['lint', '--json'], { setup: messy }).results[0].findings.map((f) => f.rule);
    for (const rule of ['html-lang', 'tokens-link', 'dfw-id-format', 'dfw-id-duplicate', 'dfw-id-missing', 'img-alt', 'inline-style', 'hardcoded-color', 'ai-default-color', 'input-font-size', 'z-index-raw', 'emoji-icon', 'filler-copy', 'copy-tell', 'vague-error', 'ellipsis-char', 'link-text', 'invented-metric', 'placeholder-image-host', 'component-unknown', 'screen-visual-style', 'use-component', 'state-coverage', 'transition-all', 'left-border-card', 'ai-gradient', 'focus-removed', 'token-alias', 'token-name', 'token-type', 'near-duplicate-color', 'token-json', 'blueprint']) {
      assert.ok(found.includes(rule), `${rule} is exercised`);
    }
  });

  test('--max caps the text, exit code 2 follows MUST findings', () => {
    const r = same(['lint', '--max', '3'], { setup: messy });
    assert.equal(r.code, 2);
    assert.match(r.stdout, /\n- \.\.\. \d+ more \(run "dfw check --json" for all\)\.$/m);
    same(['lint', '--max', 'nope'], { setup: messy });
    same(['lint', '--max=0'], { setup: messy });
  });

  test('each target form: relative, ./relative, absolute, relative to the working directory', () => {
    const rules = (data) => data.results.flatMap((r) => r.findings).map((f) => `${f.file}:${f.line}:${f.rule}`);
    const want = rules(sameJson(['lint', SCREEN, '--json'], { setup: messy }));
    assert.ok(want.some((r) => r.endsWith(':img-alt')));
    assert.deepEqual(rules(sameJson(['lint', `./${SCREEN}`, '--json'], { setup: messy })), want);
    assert.deepEqual(rules(sameJson(...withRoot((root) => ['lint', path.join(root, SCREEN), '--json'], { setup: messy }))), want);
    // The working directory is the outer project; the linted project is nested in it.
    const inner = all(messy, nested);
    assert.deepEqual(rules(sameJson(['lint', `nested/${SCREEN}`, '--root', 'nested', '--json'], { setup: inner })), want);
    assert.deepEqual(rules(sameJson(['lint', ONLY_NESTED, '--root', 'nested', '--json'], { setup: inner })), want.map((r) => r.replace(SCREEN, ONLY_NESTED)).filter((r) => !r.endsWith(':state-coverage')));
    // A path that also exists relative to the working directory is taken from there (as in Node): outside the linted project.
    assert.deepEqual(rules(sameJson(['lint', SCREEN, '--root', 'nested', '--json'], { setup: inner })), []);
    for (const target of [SCREEN, `./${SCREEN}`, `design/../${SCREEN}`]) assert.equal(same(['lint', target], { setup: messy }).code, 2);
    same(...withRoot((root) => ['lint', path.join(root, SCREEN)], { setup: messy }));
    same(['lint', `nested/${SCREEN}`, '--root', 'nested'], { setup: inner });
  });

  test('other targets: a folder, several files, tokens (audit), the manifest, files that are not design files', () => {
    for (const targets of [
      ['design/screens'],
      ['design'],
      ['design/tokens/tokens.json'],
      ['design/tokens/themes/dark.json', 'design/tokens/themes/broken.json'],
      ['design/system/components/button.html'],
      ['design/design.json'],
      ['design/styles/site.css', SCREEN, 'design/styles/site.css'],
      ['design/approved/v1/screens/items/a.html'],
      ['design/system/head.html'],
      ['design/missing.html'],
      ['README.md'],
      ['../outside.html'],
      ['https://example.com/page.html'],
      ['design/screens/items'],
    ]) {
      same(['lint', ...targets], { setup: messy });
      sameJson(['lint', ...targets, '--json'], { setup: messy });
    }
    // "--json <file>": the parser takes the file for the flag's value, so the whole design is linted (as in Node).
    assert.equal(sameJson(['lint', '--json', SCREEN], { setup: messy }).results.length, 2);
  });

  test('projects without a design, without a workflow, with broken files', () => {
    same(['lint'], { setup: (root) => fs.rmSync(path.join(root, 'design'), { recursive: true }) });
    same(['lint'], { state: null });
    const r = same(['lint'], { setup: (root) => write(root, 'design/design.json', '{ "screens": [') });
    assert.match(r.stdout, /\(dfw\/manifest-json\) <parser words>/);
    sameJson(['lint', '--json'], { setup: (root) => write(root, 'design/design.json', '{ "screens": [') });
    same(['lint'], { setup: (root) => write(root, 'design/design.json', 'null') });
    same(['lint'], { setup: (root) => write(root, 'design/tokens/tokens.json', '{}') });
    // A config or token file the commands cannot read ends them the same way; the parser's own words differ.
    for (const [rel, text] of [['.dfw/config.json', '{ nope'], ['design/tokens/tokens.json', '{ nope']]) {
      const broken = raw(['lint'], { setup: (root) => write(root, rel, text) });
      assert.deepEqual(codeAndStdout(broken), expected(codeAndStdout(broken)), rel);
      assert.equal(broken.code, 1);
    }
  });

  test('design.json of the wrong shape: the same findings, or the same failure where Node dies on a TypeError', () => {
    const variant = { id: 'a', file: 'screens/items/a.html' };
    const shapes = [
      '5', '"text"', 'true', '[]', '[1]', '{"screens":{}}', '{"screens":"ab"}', '{"screens":5}', '{"screens":[null]}', '{"screens":[5]}', '{"screens":["x"]}', '{"screens":0,"pages":0}',
      { screens: [{ id: 'a', variants: {} }] },
      { screens: [{ id: 'a', variants: 'x' }] },
      { screens: [{ id: 'a', variants: [null] }] },
      { screens: [{ id: 'a', variants: [{ file: 5 }] }] },
      { screens: [{ id: 'a', variants: [{ file: '/etc/hosts' }, { file: '../design/design.json', id: 1 }, { file: '' }, { file: 0 }] }] },
      { screens: [{ id: 'items', variants: [variant] }, null] },
      { screens: [{ id: 'items', variants: [variant, null] }, { id: 'b', variants: 'x' }] },
      { screens: [], pages: {} },
      { screens: [], pages: 'ab' },
      { screens: [], pages: 7 },
      { screens: [], pages: [null] },
      { screens: [], pages: ['x', 5, []] },
      { screens: [{ id: 'a' }, { id: 'b' }], pages: [{ screens: 'abc' }] },
      { screens: [], pages: [{ screens: {} }] },
      { screens: [{ id: 1 }, { id: null }, { id: true }], pages: [{ id: 1, screens: [null, {}, [1], 1.0, true, 'x'] }] },
      { screens: [{ id: 0, variants: [variant] }, { id: 0.0, blueprint: [], variants: [variant] }, { id: '', blueprint: { goal: 0, layout: '', aboveFold: [0], data: {}, feedback: false } }, { id: null }, { id: ['_x'] }, { id: { a: 1 } }] },
      { screens: [{ id: 'items', states: 'loading', variants: [variant] }], states: 'x' },
      { screens: [{ id: 'items', states: '', variants: [variant] }], states: 'x' },
      { screens: [{ id: 'items', states: [], variants: [variant] }], states: { a: 1 } },
      { screens: [{ id: 'items', states: { length: 1 }, variants: [variant] }] },
      { screens: [{ id: 'items', states: { length: 0 }, variants: [variant] }], states: 0 },
      { screens: [{ id: 'items', states: [1, null, 'ERROR', { a: 1 }, 'Loading'], variants: [variant] }] },
      { screens: [{ id: '_items', variants: [variant] }], states: 5 },
    ];
    let died = 0;
    for (const shape of shapes) {
      const setup = (root) => {
        write(root, 'design/design.json', typeof shape === 'string' ? shape : JSON.stringify(shape));
        fs.appendFileSync(path.join(root, SCREEN), '<table></table>');
      };
      for (const args of [['lint', '--json'], ['lint', SCREEN, '--json'], ['lint', 'design/design.json', '--json']]) {
        const r = run(args, { setup });
        const label = `${args.join(' ')} on ${JSON.stringify(shape)}`;
        if (r.code === 1) {
          died++;
          assert.deepEqual(codeAndStdout(r), expected(codeAndStdout(r)), label);
          assert.deepEqual([r.code, r.stdout], [1, ''], label);
          assert.match(r.stderr, /^Error: Malformed design\/design\.json: /, label);
        } else jsonSame(r);
      }
    }
    assert.ok(died > 20, `shapes Node dies on are covered (${died})`);
  });

  test('byte-identical --json', () => {
    for (const args of [['lint', '--json'], ['lint', SCREEN, '--json']]) {
      check(raw(args, { setup: messy, normalize: parserWords }));
    }
  });
});

// ---------------------------------------------------------------- system audit

describe('checks: system audit', { skip: skipReason() }, () => {
  /** A project made by `system init` with the kit masters copied in (a fixture only: the CLI never copies them). */
  const kit = (preset, names) => (root) => {
    fs.rmSync(path.join(root, 'design', 'tokens', 'tokens.json'));
    dfwIn(root, ['system', 'init', '--preset', preset]);
    const dir = path.join(root, 'design/system/components');
    fs.mkdirSync(dir, { recursive: true });
    for (const f of fs.readdirSync(KIT)) if (!names || names.includes(f.replace('.html', ''))) fs.copyFileSync(path.join(KIT, f), path.join(dir, f));
    dfwIn(root, ['build']);
  };
  const audit = (setup) => {
    const text = same(['system', 'audit'], { setup });
    const data = JSON.parse(same(['system', 'audit', '--json'], { setup }).stdout);
    assert.equal(text.code, data.some((f) => f.level === 'error') ? 2 : 0);
    return data;
  };

  test('fixture tokens, every preset, kit copies, a written rationale', () => {
    assert.ok(audit().some((f) => f.rule === 'rationale'));
    for (const preset of ['crisp', 'soft', 'industrial']) audit(kit(preset, []));
    const copied = audit(kit('crisp', ['button', 'card']));
    assert.equal(copied.filter((f) => f.rule === 'kit-copy').length, 2);
    const reasoned = audit((root) => write(root, 'DESIGN.md', RATIONALE));
    assert.ok(!reasoned.some((f) => f.rule === 'rationale'));
    audit((root) => write(root, 'DESIGN.md', RATIONALE.replace('## Rationale', '## Decisions and rationale  ').replaceAll('\n', '\r\n')));
    audit((root) => write(root, 'DESIGN.md', '## Rationale\n- too short\n* 12345678901234567890 exactly long enough\n  - indented and long enough to count as one\n\n## Other\n- this one is under another heading, not counted\n'));
  });

  test('token shapes: short hex, aliases, objects, off-grid spacing, shadows, families, radii', () => {
    const dim = (value) => ({ $value: { value, unit: 'px' } });
    const trees = [
      { color: { $type: 'color', ink: { $value: '#ccc' }, surface: { $value: '#fff' }, canvas: { $value: '#fff' }, accent: { $value: '#ddd' }, muted: { $value: '#eee' } } },
      { color: { $type: 'color', palette: { n9: { $value: '#111111', $description: 'ink only' }, n0: { $value: { hex: '#f4ede0', components: [0.9, 0.9, 0.8] } } }, ink: { $value: '{color.palette.n9}' }, surface: { $value: '{color.palette.n0}' }, canvas: { $value: '{color.palette.n0}' }, accent: { $value: '#2453c7ff' }, loop: { $value: '{color.loop}' }, status: { ok: { $value: '#1a7f37' }, err: { $value: '#cf222e' } } } },
      {
        color: { $type: 'color', canvas: { $value: '#f4ede0' } },
        space: { $type: 'dimension', 1: dim(4), 2: dim(6), 3: dim(10), 4: dim(2), 5: dim(7.5), rem: { $value: { value: 3, unit: 'rem' } } },
        radius: { $type: 'dimension', a: dim(1), b: dim(2), c: dim(3), d: dim(4), e: dim(5), f: dim(6), pill: dim(999) },
        font: { $type: 'fontFamily', a: { $value: ['A', 'x'] }, b: { $value: 'B' }, c: { $value: 'C' }, d: { $value: 'D' }, e: { $value: 'A' }, size: { $type: 'dimension', md: dim(15), lg: dim(15.5), xl: dim(40), 'display-md': dim(48) } },
        shadow: { $type: 'shadow', none: { $value: [] }, sm: { $value: { color: { components: [0, 0, 0.01] } } }, md: { $value: [{ color: { components: [0.2, 0.1, 0] } }] } },
        $extensions: { dfw: { exceptions: ['contrast:muted'] } },
      },
      {},
      [],
      { color: { $value: '#123456', $type: 'color' } },
    ];
    for (const tree of trees) audit((root) => write(root, 'design/tokens/tokens.json', tree));
    audit((root) => all((r) => write(r, 'design/tokens/tokens.json', trees[2]), (r) => write(r, 'design/design.json', { surface: 'marketing', screens: [] }))(root));
    audit((root) => write(root, 'design/design.json', '{ broken'));
    for (const manifest of ['5', '"text"', '[]', '{"surface":7}', '{"surface":""}']) audit((root) => write(root, 'design/design.json', manifest));
  });

  test('token values of the wrong type: the same findings, or the same failure where Node dies on a TypeError', () => {
    const color = { $type: 'color', canvas: { $value: '#f4ede0' } };
    const shadow = (value) => ({ color, shadow: { $type: 'shadow', a: { $value: value } } });
    const trees = [
      shadow(null),
      shadow([null]),
      shadow([{ color: { components: [0, 0, 0] } }, null]),
      shadow([{ color: { components: [1, 0, 0] } }, null]),
      shadow({ color: { components: 5 } }),
      shadow({ color: { components: true } }),
      shadow({ color: { components: {} } }),
      shadow({ color: { components: '000' } }),
      shadow({ color: { components: 'abc' } }),
      shadow({ color: { components: '' } }),
      shadow({ color: { components: [] } }),
      shadow({ color: { components: [null, false, '0.01'] } }),
      shadow({ color: { components: ['x'] } }),
      shadow({ color: null }),
      shadow('0 1px 2px #000'),
      shadow(7),
      { color: { $type: 'color', canvas: { $value: { hex: 5 } } } },
      { color: { $type: 'color', canvas: { $value: { hex: ['#fff'] } } } },
      { color: { $type: 'color', canvas: { $value: { hex: '' } } } },
      { color: { $type: 'color', canvas: { $value: 5 } } },
      { color: { $type: 'color', canvas: { $value: '{color.canvas}' } } },
      { space: { a: { $value: { unit: 'px', value: '10' } }, b: { $value: { unit: 'px', value: null } }, c: { $value: { unit: 'px' } }, d: { $value: { unit: 'px', value: [10] } }, e: { $value: { unit: 'px', value: '6' } }, f: { $value: null }, g: { $value: 'px' } } },
      { radius: { a: { $value: null }, b: { $value: 1 }, c: { $value: { value: null } }, d: { $value: { value: 1 } }, e: { $value: { value: 1.0 } }, f: { $value: { value: '1' } }, g: { $value: { value: {} } }, h: { $value: { value: {} } }, i: { $value: [1] } } },
      { font: { $type: 'fontFamily', a: { $value: [] }, b: { $value: null }, c: { $value: [null] }, d: { $value: 5 }, e: { $value: { a: 1 } }, f: { $value: [['x', 'y']] }, size: { md: { $value: { value: '15' } }, lg: { $value: { value: 0 } }, xl: { $value: 20 } } } },
      { $extensions: { dfw: { exceptions: ['contrast', 7, null] } }, color: { $type: 'color', canvas: { $value: '#fff' }, ink: { $value: '#111' }, surface: { $value: '#fff' }, accent: { $value: '#00f' } } },
    ];
    let died = 0;
    for (const tree of trees) {
      const r = run(['system', 'audit', '--json'], { setup: (root) => write(root, 'design/tokens/tokens.json', JSON.stringify(tree)) });
      if (r.code === 1) {
        died++;
        assert.deepEqual(codeAndStdout(r), expected(codeAndStdout(r)), JSON.stringify(tree));
        assert.deepEqual([r.code, r.stdout], [1, ''], JSON.stringify(tree));
      } else check(r);
    }
    assert.ok(died >= 6, `trees Node dies on are covered (${died})`);
  });

  test('no token file: no findings; a broken one: the command fails the same way', () => {
    const none = same(['system', 'audit'], { setup: (root) => fs.rmSync(path.join(root, 'design/tokens/tokens.json')) });
    assert.equal(none.stdout, 'Design system audit: no findings.\n');
    assert.deepEqual(JSON.parse(same(['system', 'audit', '--json'], { setup: (root) => fs.rmSync(path.join(root, 'design/tokens/tokens.json')) }).stdout), []);
    const broken = raw(['system', 'audit'], { setup: (root) => write(root, 'design/tokens/tokens.json', '{ nope') });
    assert.deepEqual(codeAndStdout(broken), expected(codeAndStdout(broken)));
    assert.deepEqual([broken.code, broken.stdout], [1, '']);
  });
});

// ---------------------------------------------------------------- stand-in tools

const ENGINE_VERSION = '9.9.9';
/** Stand-in for the Impeccable engine: one finding per file it was given, and what it saw of the environment. */
const ENGINE = `#!/bin/sh
# Test stand-in, not the Impeccable engine.
shift 2
sep=''
printf 'note: stand-in\\n['
for f in "$@"; do
  printf '%s{"antipattern":"stub","name":"Stub","description":"checked with api=%s track=%s","severity":"%s","file":"%s","line":3,"snippet":"<main>"}' "$sep" "\${IMPECCABLE_API_URL:-unset}" "\${DO_NOT_TRACK:-unset}" "\${STUB_SEVERITY:-warning}" "$f"
  sep=','
done
printf ']'
`;
/** Stand-in for the upstream sh launcher: what the Node code on this branch starts. It only hands over to the cached engine. */
const LAUNCHER = `#!/bin/sh\n# Test stand-in, not the Impeccable launcher.\nexec "$IMPECCABLE_HOME/bin/${ENGINE_VERSION}/impeccable" "$@"\n`;
const SLOPLINT = `// Test stand-in, not Hallmark's sloplint.
const args = process.argv.slice(2);
const files = args.slice(0, args.indexOf('--json'));
const scope = args[args.indexOf('--scope') + 1];
process.stdout.write(JSON.stringify({ findings: files.flatMap((file, i) => [
  { gate: String(i + 1), grade: 'FAIL', file, line: 2, evidence: 'scope ' + scope, fix: 'rewrite the hero' },
  { gate: 20, grade: 'FAIL', file, evidence: 'stamp' },
  { gate: 'r', grade: 'REFLEX', evidence: 'no file' },
]) }));
`;
const COPY_LINT = `// Test stand-in, not the Taste copy-lint.
const files = process.argv.slice(2).filter((a) => a !== '--json');
process.stdout.write('copy-lint\\n' + JSON.stringify({ files: files.map(() => ({ findings: [{ id: 'T1', sev: 'FAIL', msg: 'vague headline', fix: 'be specific' }, { id: 'T2', sev: 'INFO' }, { id: 'T3', sev: 'WARN' }] })) }));
`;
const sha256 = (text) => crypto.createHash('sha256').update(text).digest('hex');
const engineAsset = () => `impeccable-${{ darwin: 'darwin', linux: 'linux', win32: 'windows' }[process.platform]}-${process.arch}`;

/**
 * Options for a run that may reach external tools: a marketplace of stand-ins next to the
 * project copy, and every place a real tool could be found moved into the run's own folder.
 * tools: impeccable ('ready' | 'not-downloaded' | 'tampered' | false), hallmark, taste (booleans).
 */
function withTools(tools = {}, opts = {}) {
  const env = { ...opts.env };
  const setup = (root) => {
    const tmp = path.dirname(root);
    const market = path.join(tmp, 'market');
    fs.mkdirSync(market, { recursive: true });
    fs.symlinkSync(CORE, path.join(market, 'dfw'), 'dir');
    const plugin = (name) => write(market, `${name}/.claude-plugin/plugin.json`, { name });
    const impHome = path.join(tmp, 'impeccable-home');
    if (tools.impeccable) {
      plugin('impeccable');
      const scripts = `impeccable/skills/impeccable/scripts`;
      fs.chmodSync(write(market, `${scripts}/impeccable`, LAUNCHER), 0o755);
      write(market, `${scripts}/VERSION`, `${ENGINE_VERSION}\n`);
      write(market, `${scripts}/engine-checksums.txt`, `# stand-in\n${ENGINE_VERSION} ${engineAsset()} ${sha256(ENGINE)}\n`);
      if (tools.impeccable !== 'not-downloaded') {
        const cached = write(impHome, `bin/${ENGINE_VERSION}/impeccable`, tools.impeccable === 'tampered' ? `${ENGINE}# changed\n` : ENGINE);
        fs.chmodSync(cached, 0o755);
      }
    }
    if (tools.hallmark) {
      plugin('hallmark');
      write(market, 'hallmark/skills/hallmark/scripts/sloplint.mjs', SLOPLINT);
    }
    if (tools.taste) {
      plugin('taste-reference');
      write(market, 'taste-reference/skills/taste-reference/scripts/copy-lint.mjs', COPY_LINT);
    }
    Object.assign(env, {
      DFW_PLUGIN_ROOT: path.join(market, 'dfw'),
      HOME: path.join(tmp, 'user-home'),
      USERPROFILE: path.join(tmp, 'user-home'),
      CLAUDE_CONFIG_DIR: path.join(tmp, 'claude'),
      COPILOT_HOME: path.join(tmp, 'copilot'),
      IMPECCABLE_HOME: impHome,
      IMPECCABLE_BIN: '',
      ...(typeof opts.env === 'function' ? opts.env(root, tmp) : opts.env),
    });
    opts.setup?.(root);
  };
  return { ...opts, env, setup };
}
const ALL_TOOLS = { impeccable: 'ready', hallmark: true, taste: true };
const tasteOn = (root) => write(root, '.dfw/config.json', { checks: { tasteCopyLint: true } });
const PLAIN = 'design/pages/plain.html';
const plainPage = (root) => write(root, PLAIN, '<!doctype html><html lang="en"><head><link rel="stylesheet" href="../tokens/tokens.css"></head><body><main data-dfw-id="m">x</main></body></html>');

// ---------------------------------------------------------------- check

describe('checks: check', { skip: skipReason() || POSIX_ONLY }, () => {
  test('files with the dfw tool only (external tools switched off)', () => {
    const off = (root) => write(root, '.dfw/config.json', { checks: { impeccable: false, sloplint: false } });
    for (const targets of [[SCREEN], ['design/tokens/tokens.json'], [SCREEN, 'design/styles/site.css', 'README.md']]) {
      for (const setup of [off, all(off, messy)]) {
        const text = same(['check', ...targets], withTools({}, { setup }));
        assert.match(text.stdout, /^Checks: .* Tools: dfw-lint=ran(, dfw-ds-audit=ran)?\n/);
        const data = sameJson(['check', ...targets, '--json'], withTools({}, { setup }));
        assert.deepEqual(data.unverified, []);
        assert.equal(text.code, data.tally.error ? 2 : 0);
      }
    }
    same(...withRoot((root) => ['check', path.join(root, SCREEN)], withTools({}, { setup: all(off, messy) })));
    same(['check', '--no-a11y'], withTools({}, { setup: all(off, messy) }));
  });

  test('stand-in external tools: composed copies go in, findings come back on the sources', () => {
    const setup = all(tasteOn, plainPage);
    const r = same(['check', SCREEN, PLAIN, 'design/notes.md'], withTools(ALL_TOOLS, { setup }));
    assert.match(r.stdout, /Tools: dfw-lint=ran, impeccable=ran, sloplint=ran, taste-copy-lint=ran\n/);
    // The screen was handed over as .dfw/build/..., and every finding names the design file.
    assert.match(r.stdout, new RegExp(`\\[SHOULD\\] ${SCREEN}:3 \\(impeccable/impeccable-stub\\) Stub: checked with api=unset track=1 \\[<main>\\]`));
    assert.match(r.stdout, new RegExp(`\\[SHOULD\\] ${SCREEN}:2 \\(sloplint/hallmark-1\\) scope app \\| fix: rewrite the hero`));
    assert.match(r.stdout, new RegExp(`\\[SHOULD\\] ${SCREEN}:1 \\(taste-copy-lint/taste-T1\\) vague headline \\| fix: be specific`));
    assert.doesNotMatch(r.stdout, /\.dfw\/build/);
    assert.ok(r.files['.dfw/build/design/screens/items/a.html'], 'the composed copy was written');
    const data = sameJson(['check', SCREEN, PLAIN, '--json'], withTools(ALL_TOOLS, { setup }));
    assert.deepEqual(data.results.map((x) => `${x.tool}=${x.status}`), ['dfw-lint=ran', 'impeccable=ran', 'sloplint=ran', 'taste-copy-lint=ran']);
    assert.deepEqual(data.results.slice(1).map((x) => x.findings.length), [2, 4, 4]);
    assert.deepEqual([...new Set(data.results.slice(1).flatMap((x) => x.findings.map((f) => f.file)))].sort(), ['', PLAIN, SCREEN]);
    assert.deepEqual(data.unverified, []);
    // The whole design without the rendered accessibility check; a marketing surface makes Hallmark's FAIL a MUST.
    same(['check', '--no-a11y'], withTools(ALL_TOOLS, { setup }));
    const marketing = all(setup, (root) => write(root, 'design/design.json', { ...JSON.parse(fs.readFileSync(path.join(root, 'design/design.json'), 'utf8')), surface: 'marketing' }));
    const page = same(['check', PLAIN, '--max', '99'], withTools(ALL_TOOLS, { setup: marketing }));
    assert.equal(page.code, 2);
    assert.match(page.stdout, /\[MUST\] design\/pages\/plain\.html:2 \(sloplint\/hallmark-1\) scope page/);
    // A MUST from the engine, and tools picked by the config.
    assert.equal(same(['check', PLAIN], withTools(ALL_TOOLS, { setup, env: { STUB_SEVERITY: 'critical' } })).code, 2);
    const some = (root) => write(root, '.dfw/config.json', { checks: { sloplint: false, tasteCopyLint: false } });
    assert.match(same(['check', PLAIN], withTools(ALL_TOOLS, { setup: all(plainPage, some) })).stdout, /Tools: dfw-lint=ran, impeccable=ran\n/);
  });

  test('tools that are not installed, or whose engine is not downloaded, are NOT VERIFIED', () => {
    const none = same(['check', PLAIN], withTools({}, { setup: plainPage }));
    assert.match(none.stdout, /Tools: dfw-lint=ran, impeccable=unavailable, sloplint=unavailable\nNOT VERIFIED: impeccable \(unavailable\); sloplint \(unavailable\)\./);
    assert.equal(none.code, 0);
    const waiting = same(['check', PLAIN], withTools({ impeccable: 'not-downloaded', hallmark: true }, { setup: plainPage }));
    assert.match(waiting.stdout, /NOT VERIFIED: impeccable \(Engine not downloaded yet; run "dfw check --setup" once\.\)\./);
    assert.deepEqual(sameJson(['check', PLAIN, '--json'], withTools({ impeccable: 'not-downloaded' }, { setup: plainPage })).unverified, ['impeccable', 'sloplint']);
    // Nothing for a tool to read: skipped, which is not "did not run".
    assert.match(same(['check', 'README.md'], withTools(ALL_TOOLS)).stdout, /impeccable=skipped, sloplint=skipped\n$/);
  });

  test('--setup: a verified engine is ready', () => {
    const ready = same(['check', '--setup'], withTools({ impeccable: 'ready' }));
    assert.equal(ready.stdout, 'Impeccable engine ready.\n');
    assert.ok(ready.files['.dfw/checks/setup.html']);
    same(['lint', '--setup'], withTools({ impeccable: 'ready' }));
  });

  test('usage errors', () => {
    const r = sameWithStderr(['check', SCREEN, '--lighthouse'], withTools({}));
    assert.deepEqual([r.code, r.stderr], [1, '--lighthouse needs --url <running app> (verify phase).\n']);
  });

  // ---- the hardened runner (0.5.2): what the binary does, and that it is what Node did.

  /** Two tests for one case (as when both implementations ran): what the binary does, and the recorded result. */
  const hardened = (name, args, opts, expectNative) => {
    test(`hardened runner: ${name}`, () => expectNative(check(raw(args, opts()))));
    test(`hardened runner, same as Node: ${name}`, () => check(raw(args, opts())));
  };

  // The hardened runner writes the page for the launcher before it looks for the plugin.
  hardened('--setup without the plugin says so', ['check', '--setup'], () => withTools({}), (r) => assert.deepEqual([r.code, r.stdout, r.stderr], [1, '', 'Impeccable plugin not found.\n']));
  hardened('--json lists the tools that did not run', ['check', PLAIN, '--json'], () => withTools({ impeccable: 'not-downloaded', hallmark: true }, { setup: plainPage }), (r) => {
    assert.deepEqual(JSON.parse(r.stdout).unverified, ['impeccable']);
    assert.equal(r.code, 0);
  });
  hardened('--strict exits 3 when a tool did not run', ['check', PLAIN, '--strict'], () => withTools({ impeccable: 'not-downloaded', hallmark: true }, { setup: plainPage }), (r) => assert.equal(r.code, 3));
  hardened('--strict <file> keeps the file as a target', ['check', '--strict', PLAIN, '--json'], () => withTools({}, { setup: plainPage }), (r) => {
    assert.equal(r.code, 3);
    assert.deepEqual(JSON.parse(r.stdout).results.map((x) => x.tool), ['dfw-lint', 'impeccable', 'sloplint']);
  });
  hardened('--strict with every tool run exits 0', ['check', PLAIN, '--strict'], () => withTools(ALL_TOOLS, { setup: plainPage }), (r) => assert.equal(r.code, 0));
  hardened('MUST findings keep exit 2 under --strict', ['check', SCREEN, '--strict'], () => withTools({}, { setup: messy }), (r) => assert.equal(r.code, 2));
  hardened('a cached engine with another checksum is not run', ['check', SCREEN], () => withTools({ impeccable: 'tampered' }), (r) => {
    assert.match(r.stdout, /impeccable=not-installed/);
    assert.match(r.stdout, /NOT VERIFIED: impeccable \(Engine at .*impeccable does not match the pinned checksum; delete it, then run "dfw check --setup" once\.\)/);
  });
  const ownBuild = (root, tmp) => {
    const bin = write(tmp, 'own/impeccable', ENGINE.replace('"Stub"', '"Own build"'));
    fs.chmodSync(bin, 0o755);
    return bin;
  };
  hardened('IMPECCABLE_BIN is not run without the global opt-in', ['check', SCREEN], () => withTools({ impeccable: 'not-downloaded' }, { env: (root, tmp) => ({ IMPECCABLE_BIN: ownBuild(root, tmp) }) }), (r) => {
    assert.match(r.stdout, /impeccable=not-installed/);
    assert.doesNotMatch(r.stdout, /Own build/);
  });
  hardened('IMPECCABLE_BIN runs with impeccable.allowUnverifiedEngine in the global config, not in the project config', ['check', SCREEN], () => withTools({ impeccable: 'not-downloaded' }, {
    env: (root, tmp) => ({ IMPECCABLE_BIN: ownBuild(root, tmp) }),
    setup: (root) => write(path.dirname(root), 'home/config.json', { impeccable: { allowUnverifiedEngine: true } }),
  }), (r) => assert.match(r.stdout, /\(impeccable\/impeccable-stub\) Own build: checked/));
  hardened('a project config cannot grant the opt-in', ['check', SCREEN], () => withTools({ impeccable: 'not-downloaded' }, {
    env: (root, tmp) => ({ IMPECCABLE_BIN: ownBuild(root, tmp) }),
    setup: (root) => write(root, '.dfw/config.json', { impeccable: { allowUnverifiedEngine: true } }),
  }), (r) => assert.match(r.stdout, /impeccable=not-installed/));
  hardened('a project config cannot point the engine elsewhere or switch tracking on', ['check', SCREEN], () => withTools({ impeccable: 'ready' }, {
    setup: (root) => write(root, '.dfw/config.json', { env: { IMPECCABLE_API_URL: 'http://evil.example', DO_NOT_TRACK: '0', NO_COLOR: '1' } }),
  }), (r) => {
    assert.match(r.stdout, /checked with api=unset track=1/);
    assert.match(r.stderr, /dfw: ignored env\.IMPECCABLE_API_URL in .*config\.json \(a project config cannot set it; use the global config: dfw config set env\.IMPECCABLE_API_URL <value> --global\)/);
    assert.match(r.stderr, /dfw: ignored env\.DO_NOT_TRACK /);
    assert.doesNotMatch(r.stderr, /NO_COLOR/);
  });
  hardened('the global config may set any variable', ['check', SCREEN], () => withTools({ impeccable: 'ready' }, {
    setup: (root) => write(path.dirname(root), 'home/config.json', { env: { IMPECCABLE_API_URL: 'http://mirror.internal', DO_NOT_TRACK: '0' } }),
  }), (r) => assert.match(r.stdout, /checked with api=http:\/\/mirror\.internal track=0/));
  hardened('--setup does not start the launcher when it would run an engine dfw cannot verify', ['check', '--setup'], () => withTools({ impeccable: 'not-downloaded' }, { env: (root, tmp) => ({ IMPECCABLE_BIN: ownBuild(root, tmp) }) }), (r) => {
    // 0.5.2: a setup that was refused is an error (exit 1, the reason on stderr).
    assert.match(r.stderr, /^Impeccable setup stopped: the launcher would run an engine dfw cannot verify \(.*own\/impeccable\)\. Remove it or unset IMPECCABLE_BIN, or allow it: dfw config set impeccable\.allowUnverifiedEngine true --global\n$/);
    assert.deepEqual([r.code, r.stdout], [1, '']);
  });
});

// ---------------------------------------------------------------- hook

describe('checks: hook post-tool-use', { skip: skipReason() }, () => {
  /** Options for a hook run whose payload needs the project's path (the harness reads `stdin` after `setup`). */
  function hookOpts(payload, opts = {}) {
    const o = { normalize: parserWords, ...opts };
    let root = null;
    o.setup = (r) => {
      root = r;
      opts.setup?.(r);
    };
    Object.defineProperty(o, 'stdin', { enumerable: true, get: () => JSON.stringify(payload(root)) });
    return o;
  }
  // The same edit as each host sends it.
  const HOSTS = {
    claude: [[], (rel) => (root) => ({ session_id: 's1', hook_event_name: 'PostToolUse', cwd: root, tool_name: 'Edit', tool_input: { file_path: path.join(root, rel) }, tool_response: { success: true } })],
    copilot: [['--host', 'copilot'], (rel) => (root) => ({ sessionId: 's1', hookEventName: 'postToolUse', cwd: root, toolName: 'str_replace_editor', toolArgs: JSON.stringify({ path: rel }) })],
  };
  const context = (host, r) => (r.stdout ? (host === 'copilot' ? JSON.parse(r.stdout).additionalContext : JSON.parse(r.stdout).hookSpecificOutput.additionalContext) : null);
  const comment = { id: 'fb_001', createdAt: NOW, designVersion: 'v1', status: 'open', screen: 'items', variant: 'a', message: 'Make the title bigger', author: 'user' };
  const session = (root) => write(root, '.dfw/reviews/rv_1.json', { schema: 1, id: 'rv_1', startedAt: NOW, designVersion: 'v1', comments: [comment] });

  for (const [host, [flags, edit]] of Object.entries(HOSTS)) {
    const post = (rel, opts) => same(['hook', 'post-tool-use', ...flags], hookOpts(edit(rel), opts));

    test(`${host}: an edited screen is linted; a clean page says nothing`, () => {
      assert.equal(post(PLAIN, { setup: plainPage }).stdout, '');
      post(SCREEN);
      const r = post(SCREEN, { setup: messy });
      const text = context(host, r);
      assert.match(text, new RegExp(`^dfw checks on ${SCREEN}:\nChecks: \\d+ must, \\d+ should, \\d+ nice\\. Tools: dfw-lint=ran\n`));
      assert.match(text, /\n- \.\.\. \d+ more \(run "dfw check --json" for all\)\.\nFix MUST items before moving on; SHOULD items unless the brief or a recorded decision says otherwise\.$/);
      assert.equal(text.split('\n').filter((l) => l.startsWith('- [')).length, 12, 'checks.maxFindings defaults to 12');
      assert.deepEqual(r.files, {});
      post(SCREEN, { setup: all(messy, (root) => write(root, '.dfw/config.json', { checks: { maxFindings: 3 } })) });
    });

    test(`${host}: an edited master rebuilds components.css and is linted with the design-system audit`, () => {
      const master = 'design/system/components/button.html';
      const changed = (root) => fs.appendFileSync(path.join(root, master), '\n<dfw-component name="chip"><style>.chip { color: #fff; padding: 12px }</style><template><button class="chip"><dfw-slot></dfw-slot></button></template></dfw-component>\n');
      const r = post(master, { setup: changed });
      assert.match(r.files['design/system/components.css'], /\.chip/);
      assert.match(context(host, r), /Tools: dfw-lint=ran, dfw-ds-audit=ran\n/);
      assert.match(context(host, r), /\(dfw\/hardcoded-color\) Hard-coded color "#fff" in a component/);
    });

    test(`${host}: an edited token file rebuilds tokens.css; a broken one is a finding`, () => {
      const tokens = 'design/tokens/tokens.json';
      const recolor = (root) => {
        const tree = JSON.parse(fs.readFileSync(path.join(root, tokens), 'utf8'));
        tree.color.promo = { $value: '#6366f1' };
        write(root, tokens, tree);
      };
      const r = post(tokens, { setup: recolor });
      assert.match(r.files['design/tokens/tokens.css'], /--color-promo:\s*#6366f1/);
      assert.match(context(host, r), /\(dfw\/ai-default-color\)/);
      const theme = post('design/tokens/themes/dark.json', { setup: (root) => write(root, 'design/tokens/themes/dark.json', { color: { $type: 'color', ink: { $value: '{color.nope}' } } }) });
      assert.match(theme.files['design/tokens/tokens.css'], /\[data-theme="dark"\]/);
      const brokenTheme = post('design/tokens/themes/dark.json', { setup: (root) => write(root, 'design/tokens/themes/dark.json', '{ "color": ') });
      assert.match(context(host, brokenTheme), /\(dfw\/token-json\) <parser words>/);
      assert.deepEqual(brokenTheme.files, {});
      // A broken tokens.json stops the design-system audit, and with it the whole hook call,
      // which fails open: nothing is said (as in Node).
      const broken = post(tokens, { setup: (root) => write(root, tokens, '{ "color": ') });
      assert.deepEqual([broken.code, broken.stdout, broken.files], [0, '', {}]);
    });

    test(`${host}: a file that is not UI says nothing; new Studio comments ride along where the host needs it`, () => {
      for (const rel of ['src/server.ts', 'README.md', 'design/approved/v1/screens/items/a.html', 'design/notes.txt']) assert.deepEqual([post(rel).stdout, post(rel).files], ['', {}], rel);
      // UI files outside design/ are checked too: lint has nothing to say about them.
      for (const rel of ['src/App.tsx', 'src/styles.css', 'design/design.json']) post(rel, { setup: messy });
      const r = post('src/server.ts', { setup: session });
      const combined = post(SCREEN, { setup: all(messy, session) });
      if (host === 'copilot') {
        assert.match(context(host, r), /^1 new Studio comment\(s\) from the user:/);
        assert.match(context(host, combined), /^dfw checks on [^]*\n\n1 new Studio comment\(s\) from the user:/);
        assert.deepEqual(Object.keys(combined.files), ['.dfw/reviews/.delivered.json']);
      } else {
        assert.equal(r.stdout, '');
        assert.doesNotMatch(context(host, combined), /Studio comment/);
      }
    });

    test(`${host}: checks.onEdit off, "full" with stand-in tools, a paused workflow`, { skip: POSIX_ONLY }, () => {
      const config = (onEdit) => (root) => write(root, '.dfw/config.json', { checks: { onEdit } });
      assert.equal(post(SCREEN, { setup: all(messy, config(false)) }).stdout, '');
      assert.equal(post(SCREEN, { setup: messy, state: st({ active: false }) }).stdout, '');
      assert.equal(post(SCREEN, { setup: messy, state: null }).stdout, '');
      post(SCREEN, { setup: all(messy, config('fast')) });
      post(SCREEN, { setup: all(messy, config('anything else')) });
      for (const onEdit of ['full', true]) {
        const r = same(['hook', 'post-tool-use', ...flags], hookOpts(edit(SCREEN), withTools({ impeccable: 'ready', hallmark: true }, { setup: all(messy, config(onEdit)) })));
        assert.match(context(host, r), /Tools: dfw-lint=ran, impeccable=ran, sloplint=ran\n/);
      }
    });
  }
});

// ---------------------------------------------------------------- approve DS

describe('checks: approve DS', { skip: skipReason() }, () => {
  const system = st({ phase: 'system', gates: { ...MINIMAL_STATE.gates, G1: { approvedAt: NOW, note: 'Direction A' } } });
  const inventory = (root) => write(root, 'design/system/inventory.json', { components: [{ name: 'button', purpose: 'Confirm the issue at the counter', usedBy: ['items'] }] });
  const rationale = (root) => write(root, 'DESIGN.md', RATIONALE);
  const ds = (flags, opts) => sameWithStderr(['approve', 'DS', ...flags], { state: system, ...opts });

  test('blocked without masters, by MUST findings and by undecided preset options; approved otherwise', () => {
    const none = ds(['--note', 'x'], { setup: (root) => fs.rmSync(path.join(root, 'design/system/components'), { recursive: true }) });
    assert.deepEqual([none.code, none.files], [1, {}]);
    assert.match(none.stderr, /^DS needs a design system: component masters in design\/system\/components\//);

    const blocked = ds(['--note', 'x']);
    assert.equal(blocked.code, 1);
    assert.match(blocked.stderr, /^DS blocked: \d+ MUST finding\(s\) in the design system\.\nChecks: /);
    assert.match(blocked.stderr, /\[MUST\] DESIGN\.md:1 \(dfw-ds\/rationale\)/);
    assert.match(blocked.stderr, /\[MUST\] [^\n]* \(dfw-ds\/inventory-missing\)/);
    assert.ok(!('.dfw/state.json' in blocked.files), 'a blocked gate records nothing');

    const accepted = ds(['--note', 'accepted as is', '--accept-findings']);
    assert.equal(accepted.code, 0);
    assert.match(accepted.stdout, /^DS approved: /);
    assert.equal(JSON.parse(accepted.files['.dfw/state.json']).gates.DS.note, 'accepted as is');

    const ready = all(inventory, rationale);
    check(raw(['approve', 'DS', '--note', 'Button, tokens'], { state: system, setup: ready }), { stderr: true });
    // Whatever the fixture's own findings are, the binary decides as Node did; with them
    // accepted the gate is recorded.
    const options = all(ready, (root) => write(root, 'design/tokens/themes/opt-soft.json', { color: { $type: 'color', ink: { $value: '#222222' } } }));
    const undecided = ds(['--note', 'x', '--accept-findings'], { setup: options });
    assert.deepEqual([undecided.code, undecided.stderr], [1, 'Preset options are still active (opt-* themes). Promote the chosen one first: dfw system choose <preset>.\n']);
    assert.equal(ds(['--note', 'x', '--accept-findings', '--force'], { setup: options }).code, 0);
    assert.equal(ds(['--note', 'x', '--accept-findings'], { setup: all(ready, (root) => write(root, 'design/tokens/themes/dark.json', { color: { $type: 'color', ink: { $value: '#222222' } } })) }).code, 0);
  });

  test('a clean design system is approved without accepting anything', () => {
    const clean = all(inventory, rationale, (root) => {
      fs.rmSync(path.join(root, 'design/tokens/tokens.json'));
      dfwIn(root, ['system', 'init', '--preset', 'crisp']);
      write(root, 'design/system/components/button.html', '<dfw-component name="button" variants="primary"><style>.b { min-height: var(--size-control); color: var(--color-on-accent); background: var(--color-accent); }\n.b:hover { background: var(--color-accent-hover); }\n.b:disabled { opacity: var(--opacity-disabled); }\n.b:focus-visible { outline: var(--focus-width) solid var(--color-focus); }</style><template><button class="b" type="button"><dfw-slot>Go</dfw-slot></button></template><dfw-examples><dfw-use c="button">Issue tool</dfw-use></dfw-examples></dfw-component>');
    });
    const r = check(raw(['approve', 'DS', '--note', 'Crisp preset, button'], { state: system, setup: clean }), { stderr: true });
    assert.equal(r.code, 0, r.stderr);
    assert.match(r.stdout, /^DS approved: /);
    assert.ok(r.files['design/system/components.css'], 'the gate rebuilds components.css first');
    assert.ok(r.files['.dfw/decisions.jsonl'].includes('gate:DS'));
  });

  test('missing note, no workflow, other letter case', () => {
    assert.match(ds([]).stderr, /^Approvals record what the user approved/);
    assert.equal(ds(['--note', 'x'], { state: null }).code, 1);
    assert.equal(sameWithStderr(['approve', 'ds', '--note', 'x', '--accept-findings'], { state: system }).code, 0);
  });
});

// ---------------------------------------------------------------- the lint functions, one by one

/** A value JSON.stringify prints as Node printed the function's result. */
const J = (v) => JSON.stringify(v);

describe('checks: lint functions on a corpus (__debug lint)', { skip: skipReason() }, () => {
  let tmp;
  let root;
  let n = 0;
  before(() => {
    tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-lint-')));
    root = path.join(tmp, 'project');
    fs.cpSync(FIXTURE, root, { recursive: true });
    write(root, '.dfw/state.json', '{}');
    fs.rmSync(path.join(root, 'design/tokens/tokens.json'));
    dfwIn(root, ['system', 'init', '--preset', 'crisp']);
    const dir = path.join(root, 'design/system/components');
    fs.renameSync(path.join(dir, 'button.html'), path.join(dir, 'fixture-button.html'));
    fs.writeFileSync(path.join(dir, 'fixture-button.html'), fs.readFileSync(path.join(dir, 'fixture-button.html'), 'utf8').replace('name="button"', 'name="fixture-button"'));
    for (const f of fs.readdirSync(KIT)) fs.copyFileSync(path.join(KIT, f), path.join(dir, f));
    dfwIn(root, ['icons', 'add', 'chevron-down', 'triangle-alert', 'search', 'x', 'check', 'package', 'house', 'file-clock', 'settings', 'layout-dashboard', 'circle-check', 'circle-x', 'circle-alert', 'octagon-alert', 'package-open', 'ellipsis', 'wrench', 'log-in', 'log-out', 'plus']);
    dfwIn(root, ['build']);
    write(root, 'design/design.json', { states: ['loading'], screens: [{ id: 'hostile', states: ['Empty'], variants: [{ id: 'a', file: 'screens/hostile/a.html' }] }, { id: '_shared', variants: [{ id: 'a', file: 'screens/_shared/a.html' }] }, { id: 7, variants: [{ id: 'a', file: 'screens/seven/a.html' }] }] });
  });
  after(() => fs.rmSync(tmp, { recursive: true, force: true }));

  const native = (args) => {
    const env = { ...process.env, DFW_PLUGIN_ROOT: CORE };
    const r = spawnSync(nativeBinary(), ['__debug', 'lint', ...args], { env, encoding: 'utf8', timeout: 60_000, maxBuffer: 64 * 1024 * 1024 });
    if (r.error) throw r.error;
    assert.equal(r.status, 0, r.stderr);
    return r.stdout.replace(/\n$/, '');
  };
  /** Write `input` (text or bytes) to a file; returns [file]. */
  const input = (data, ext = 'html') => {
    const file = path.join(tmp, `in-${n++}.${ext}`);
    fs.writeFileSync(file, data);
    return [file];
  };
  const label = (data) => (Buffer.isBuffer(data) ? data.toString('latin1') : String(data)).slice(0, 70);
  /** The temporary folder of the corpus project, masked in a recorded result. */
  const mask = (text) => text.split(JSON.stringify(tmp).slice(1, -1)).join('<tmp>').split(tmp).join('<tmp>');
  /** The binary's result of one lint function against the recorded one (Node's, as JSON text). Returns it. */
  const compare = (args, what) => {
    const text = mask(native(args));
    assert.equal(text, expected(text), what);
    return text;
  };

  const sameHtml = (data, rel = 'design/pages/x.html', what = label(data)) => {
    const [file] = input(data);
    return compare([file, '--as', 'html', '--rel', rel], `lintHtml: ${what}`);
  };
  const sameComponent = (data, rel = 'design/system/components/x.html', what = label(data)) => {
    const [file] = input(data);
    return compare([file, '--as', 'component', '--rel', rel], `lintComponentFile: ${what}`);
  };
  const sameCss = (data, rel = 'design/styles/x.css', what = label(data)) => {
    const [file] = input(data, 'css');
    return compare([file, '--as', 'css', '--rel', rel], `lintCss: ${what}`);
  };
  const sameScreen = (data, rel = 'design/screens/hostile/a.html', what = label(data)) => {
    const [file] = input(data);
    return compare([file, '--as', 'screen', '--rel', rel, '--root', root], `lintScreen: ${what}`);
  };
  const sameStates = (data, rel, what = label(data)) => {
    const [file] = input(data);
    return compare([file, '--as', 'states', '--rel', rel, '--root', root], `stateCoverage ${rel}: ${what}`);
  };
  const sameTokens = (tree, base, what = J(tree).slice(0, 70)) => {
    const [file] = input(typeof tree === 'string' ? tree : J(tree), 'json');
    const baseFile = base === undefined ? [] : ['--base', input(J(base), 'json')[0]];
    return compare([file, '--as', 'tokens', '--rel', 'design/tokens/t.json', ...baseFile], `lintTokens: ${what}`);
  };

  const files = (dirs, ext) => {
    const out = [];
    const walk = (dir) => {
      for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
        if (e.isDirectory()) walk(path.join(dir, e.name));
        else if (e.name.endsWith(ext)) out.push(path.join(dir, e.name));
      }
    };
    for (const d of dirs) walk(path.join(REPO, d));
    return out;
  };
  const SHIPPED = ['tests/fixtures', 'core/kits', 'core/templates'];

  test('every fixture, kit and template HTML file: as a page, a master and a screen', () => {
    const html = files(SHIPPED, '.html');
    assert.ok(html.length >= 30);
    let findings = 0;
    for (const f of html) {
      const text = fs.readFileSync(f);
      const rel = path.relative(REPO, f);
      findings += JSON.parse(sameHtml(text, `design/${rel}`, rel)).length;
      sameComponent(text, `design/system/components/${path.basename(f)}`, rel);
      sameScreen(text, 'design/screens/hostile/a.html', rel);
      sameScreen(text, 'design/screens/_shared/a.html', rel);
      sameStates(text, 'design/screens/hostile/a.html', rel);
    }
    assert.ok(findings > 30, 'the corpus raises findings');
  });

  test('every shipped stylesheet and token file, and the files a project gets from system init', () => {
    for (const f of [...files(SHIPPED, '.css'), path.join(root, 'design/system/components.css'), path.join(root, 'design/tokens/tokens.css')]) sameCss(fs.readFileSync(f), 'design/styles/x.css', f);
    sameCss('.a{color:#fff}', 'design/tokens/tokens.css');
    const tokens = files(SHIPPED, '.json').filter((f) => fs.readFileSync(f, 'utf8').includes('"$value"'));
    assert.ok(tokens.length >= 7);
    const base = JSON.parse(fs.readFileSync(path.join(root, 'design/tokens/tokens.json'), 'utf8'));
    for (const f of tokens) {
      sameTokens(fs.readFileSync(f, 'utf8'), undefined, f);
      sameTokens(fs.readFileSync(f, 'utf8'), base, `${f} on a base`);
    }
  });

  // Hand-written inputs the expressions of the Node lint read in surprising ways. The binary
  // must read them the same way: these are the findings users see today.
  const HOSTILE = [
    // comments
    '<html lang="en"><body>\n<!-- <img src="x.png"> <button>old</button> -->\n<main data-dfw-id="m">x</main></body></html>',
    '<body><!-- unterminated <h1>🚀</h1> <p>Lorem ipsum</p>',
    '<!--><body><img>--><a href="#">click here</a>',
    '<body><style>/* </style> */ .a{color:#fff}</style><p>after</p></body>',
    '<!-- <style>.a{color:#6366f1}</style> --><body></body>',
    '<body><!-- data-dfw-id="dup" --><p data-dfw-id="dup">x</p><p data-dfw-id="dup">y</p></body>',
    // quotes
    "<body><p data-dfw-id='single'>x</p><p data-dfw-id='single'>y</p><img alt='' src='a'><button style='color:red'>z</button></body>",
    '<body><img title="a > b" alt="x"><img title="a > b"><button data-x="</button>" style="x">y</button></body>',
    '<body><p data-dfw-id="unterminated>x</p><p data-dfw-id="b">y</p><img alt=x></body>',
    '<body><p data-dfw-id = "spaced id" data-dfw-id\n=\n"nl">x</p><p data-dfw-id="">e</p><p data-dfw-id=" \t">w</p><p data-dfw-id=bare>b</p></body>',
    '<body><p data-dfw-id="Ünï">x</p><p data-dfw-id=".dot">x</p><p data-dfw-id="a..b_c-9">x</p><p data-dfw-id="-a">x</p></body>',
    '<body><span class="icon big">🚀</span><span class=\'icon\'>🚀</span><span class="xicon">🚀</span><span class="icon-lg">⭐</span><span class="a icon"\n data-x=">">\n\n 🎉x</span><i class="icon">abcd🚀</i><i class="icon">abc🚀</i><i class="icon">🚀🚀class="icon">🚀</i></body>',
    // unclosed and nested
    '<body><h1>🚀 never closed<h2>⭐ closed</h2><li>one 🎉<li>two</li><label>ok</label><button>🚀</BUTTON><h3>x</h3 ></body>',
    '<body><h1><h1>🚀</h1></h1><li><li>⭐</li></li><button><button>plain</button>🎉</button></body>',
    '<body><main><section><nav><header><footer><aside><form><table><dialog><ul><ol><label><select><textarea><input><a><h1><h2><h3>',
    '<body><a\nhref="#"\n>\n here \n</a><a href="#">Here</a><A HREF="#">LEARN MORE</A><a href="#">here </a x><a>read more<b></b></a><ab>here</ab></body>',
    '<body><style>.a{color:red',
    '<body><style>.a{color:#fff}</style><style></style><style media="x">.a{color:#fff}</style><STYLE>.b{color:#ABCDEF12}</STYLE><style>.c{color:#fff}</style ></body>',
    '<style title="x">x</style><style data-a="a{color:#fff}">a{color:#fff}</style><style\n\n>\n\n.d{color:#fff}</style>',
    '<body><script>var s = "<h1>🚀</h1> lorem ipsum... seamless <style>.x{color:#fff}</style>";</script><p>ok</p><SCRIPT>oops</SCRIPT><script>x</script ><p>Lorem ipsum</p></body>',
    // case, white space, line ends
    '<HTML LANG="en"><HEAD><DFW-HEAD></DFW-HEAD></HEAD><BODY><IMG SRC=x ALT=y><BUTTON STYLE="x">a</BUTTON><H1>🚀</H1></BODY></HTML>',
    '<html\tlang="en"><body><html><img\talt\t=\t"x"><img alt-text="x"><img data-alt="x"><img xalt="y"><button data-style="x">a</button><button style-x="1">b</button></body>',
    '\uFEFF<html><head>\r\n<link href="tokens.css">\r\n</head>\r\n<body>\r\n<img>\r\n<p>Loading...</p>\r\n</body>\r\n</html>\r\n',
    '<html><body>\r<img>\r<h1>🚀</h1>\r<p>Oops</p>\u2028<img>\u2029<img>\u0085<img></body></html>',
    '',
    ' \n\t ',
    '<body',
    '<',
    // copy rules
    '<body><p>seamless Seamlessly SEAMLESS seamlessness \u017Feamless effortless-ly unlock the power unlock potential unlockthe power game changer game-changer gamechanger revolutionize revolutionise empowers you empowering teams empower them in today\'s fast-paced in today’s fast paced</p></body>',
    '<body><p>Oops! OOPS oopsie an error has occurred An error occurred something went wrong. Something\nwent wrong something  went wrong</p><p>a...b ....c .. . ... …... ...</p><p>...</p>...</body>',
    '<body><p>10x faster 10 x faster 10x  faster 10xfaster 3.5× more 2 X better 2x\u00A0cheaper .5x productive 1.x faster x faster 99.9% uptime 99.99 % uptime 99.% uptime 199.9%uptime 99.9% uptimes</p></body>',
    '<body><p>lorem ipsum Lorem\nipsum lorem  ipsum Feature One feature two featured three feature four placeholder text Sample Content your headline here your tagline here your title here</p><input placeholder="placeholder text"></body>',
    '<body><img alt="" src="//images.unsplash.com/a"><img alt="" src="https://PLACEHOLD.CO/1"><img alt="" src="x.placeholder.com"><img alt="" src="notplacehold.co"><img alt="" src="placehold.com"><img alt="" src="via.placeholder.com/1"></body>',
    '<title>Lorem ipsum seamless 10x faster ... 🚀</title><template><h1>🚀 in a template</h1><p>Lorem ipsum</p></template><body><p>seamless</p></body>',
    // colours and CSS inside pages
    '<body><p style="color:#6366F1">a</p><p>#4f46e5 #4F46E5x #4338ca- #d97757, #F4F1EA. #8b5cf6f #6366f #a855f7</p></body>',
    `<style>:root{--a:#fff; --b: rgb(0 0 0)}\n.x{color:#abc;\n background:oklch(50% 0 0)}\n.y{--local:1px solid #123456}\n.z{color: var(--a, #fff); border-color: #fff}</style>`,
    `<style>.x{--v: ${'a'.repeat(80)} #fff}.y{--v: ${'a'.repeat(70)} #fff}.z{--v:${'🚀'.repeat(40)}#fff}.w{--v:${'🚀'.repeat(38)}#fff}.q{--v:a${'🚀'.repeat(39)}#fff}</style>`,
    '<style>.a{color:#ff}.b{color:#ffff}.c{color:#fffff}.d{color:#ffffff}.e{color:#fffffff}.f{color:#ffffffff}.g{color:#fffffffff}.h{color:#ggg}.i{color:rgba(0,0,0,.5)}.j{color:hsl(0 0% 0%)}.k{color:color(srgb 0 0 0)}.l{color:mycolor(1)}.m{background:url(lab(1).png)}.n{color:RGB(0,0,0)}</style>',
    '<style>input{font-size:15.99px}select,.x{font-size:1px}.textarea{font-size:1px}textarea-x{font-size:1px}my-input{font-size:1px}.a>input{font-size:16px}:is(input){font-size:0px}INPUT{font-size:1px}input{font-size:15.px}input{font-size:15PX}input{font-size:1em}</style>',
    '<style>.a{z-index:2}.b{z-index:-2}.c{z-index:1}.d{z-index:0}.e{z-index:-1}.f{z-index: 99999999999999999999999}.g{z-index:var(--z)}.h{z-index:1.5}.i{z-index:2;z-index:1}@media (x){.j{z-index:5}}</style>',
    '<style>.a{background:linear-gradient(#3b82f6,#8b5cf6)}.b{background:linear-gradient(to right,#3b82f6 0%,#3b83f6 100%)}.c{background:linear-gradient(#f00,#00f)}.d{background:LINEAR-GRADIENT(90deg,#06b6d4,#8b5cf6,#fff)}.e{background:linear-gradient(#3b82f6,#8b5cf6),linear-gradient(#000,#111)}.f{background:linear-gradient(#3b82f6,\n#8b5cf6\n)}.g{background:linear-gradient(#36f,#85f)}.h{background:linear-gradient(#3b82f6,#8b5cf6</style>',
    '<style>.a{border-left:2px solid red;border-radius:4px}.b{border-left:1px solid;border-radius:4px}.c{border-left-width:10px;border-radius:0}.d{border-left:solid 12px;border-top-left-radius:1px}.e{border-left:var(--w) solid;border-radius:4px}.f{border-left: 0.2px;border-radius:1px}.g{border-radius:1px;border-left:calc(1px + 2px)}.h{border-left:x2px;border-radius:1px}</style>',
    // bytes that are not UTF-8, astral characters
    Buffer.concat([Buffer.from('<body><h1>'), Buffer.from([0xff, 0xfe, 0xf0, 0x9f]), Buffer.from('</h1><p data-dfw-id="'), Buffer.from([0xc3]), Buffer.from('">Lorem ipsum</p><img></body>')]),
    '<body><h1>𝒜 plain astral</h1><h2>©️ ™ ↔ ✔</h2><li>1️⃣</li><label>#️⃣ *️⃣</label><button>🇻🇳</button><h3>Tiếng Việt: đẹp “quá”</h3></body>',
    // many
    `<body>${Array.from({ length: 300 }, (_, i) => `<button data-dfw-id="b.${i % 50}">B${i}</button>${i % 9 ? '' : '\n'}`).join('')}</body>`,
    `${'<div>'.repeat(400)}<img>${'</div>'.repeat(400)}<body>${'<li>'.repeat(200)}🚀</body>`,
  ];

  test(`hostile pages (${HOSTILE.length}): lintHtml and, as a master, lintComponentFile`, () => {
    assert.ok(HOSTILE.length >= 30);
    let findings = 0;
    for (const data of HOSTILE) {
      findings += JSON.parse(sameHtml(data)).length;
      sameComponent(data);
    }
    assert.ok(findings > 150, `the hostile corpus raises findings (${findings})`);
  });

  test('hostile masters', () => {
    const m = (attrs, css, tpl, rest = '') => `<dfw-component ${attrs}>\n<style>\n${css}\n</style>\n<template>\n${tpl}\n</template>\n${rest}</dfw-component>\n`;
    const inputs = [
      m('name="ok"', '.a{color:var(--c)}', '<div class="a"></div>'),
      m("name='single'", '', '<div></div>'),
      m('name="Upper"', '', '<div></div>'),
      m('name="9lives"', '', '<div></div>'),
      m('name = "spaced"\n  group="x"', '', '<div></div>'),
      m('data-name="x"', '', '<div></div>'),
      m('title="a > b" name="after"', '', '<div></div>'),
      '<dfw-component name="a"><dfw-component><DFW-COMPONENT name="c"><dfw-components name="d"><dfw-component\nname="e">',
      '<!-- <dfw-component name="x"><template></template></dfw-component> -->',
      '<dfw-component name="x"><TEMPLATE><button>x</button></TEMPLATE></dfw-component>',
      '<dfw-component name="x"><template id="a"></template><template><button>second</button></template></dfw-component>',
      '<dfw-component name="x"><template><button>unclosed template</dfw-component>',
      m('name="x"', '.b{padding:12px;margin:2px 3px;gap:2.5px;font-size:1rem;border-radius:calc(4px + 1px);padding-inline-start:var(--s, 8px);margin:-8px;border-radius:50%;paddings:9px;row-gap:9px;font-size-adjust:9px}', '<i></i>'),
      m('name="x"', '.b{padding:var(--a)var(--b) 9px;margin:var(--a 9px;gap:9PX;gap:9px}\n.c{padding:\n  9px\n  10px}.d{padding:9px', '<i></i>'),
      m('name="x"', '.b{color:red;color:#fff;--c:#fff;background:rgb(1 2 3)}', '<i></i>'),
      m('name="x"', '', '<button>x</button>'),
      m('name="x"', '.b:hover{}', '<button>x</button>'),
      m('name="x"', '.b:hover{} .b[disabled]{}', '\n\n  <BUTTON class="b">x</BUTTON>'),
      m('name="x"', '.b:hover{} .b[aria-disabled="true"]{}', '<button>x</button>'),
      m('name="x"', '', '<a href="#">x</a>'),
      m('name="x"', '', '<article><a href="#">x</a><button>y</button></article>'),
      m('name="x"', '', '<div role="button">x</div>'),
      m('name="x"', '', '<div class="a" role="tab" tabindex="0">x</div>'),
      m('name="x"', '', "<div role='button'>x</div>"),
      m('name="x"', '', '<div><span role="button">x</span></div>'),
      m('name="x"', '', '<div data-x=">" role="switch">x</div>'),
      m('name="x"', 'input{appearance:none}', '<input class="i">'),
      m('name="x"', 'input{-webkit-appearance: none}', '<select></select>'),
      m('name="x"', 'input{appearance:auto}', '<textarea></textarea>'),
      m('name="x"', '.a{outline:none}', '<div></div>'),
      m('name="x"', '.a{outline: 0}.a:focus-within{outline:1px solid}', '<div></div>'),
      m('name="x"', '.a{outline:0px}.b{outline:none!important}.c{outline-offset:0}', '<div></div>'),
      m('name="x"', '.a:focus-visible{outline:none}', '<button disabled>x</button>'),
      m('name="x"', '', '<!-- note --><button>x</button>'),
      m('name="x"', '', 'text first <button>x</button>'),
      m('name="x"', '', '<my-button>x</my-button>'),
      m('name="x"', '.a{color:#6366f1}', '<h2>🚀 {{title}}</h2><p>Lorem ipsum</p>', '<dfw-examples><dfw-use c="x">Seamless</dfw-use></dfw-examples>'),
      '<dfw-component name="x"><style>.a{color:#fff}</style><style>.a:hover{}</style><template><button>x</button></template><style>.a:disabled{}</style></dfw-component>',
    ];
    for (const text of inputs) {
      sameComponent(text);
      sameHtml(text, 'design/system/index.html');
    }
  });

  test('hostile stylesheets', () => {
    const inputs = [
      '',
      '/* only a comment #fff */',
      '/* unterminated .a{color:#fff}',
      '.a{color:#fff}/* c */.b{color:#000}/**/.c{color:#111}/*/ .d{color:#222} /*/.e{color:#333}',
      '/*\n\n#fff\n*/\n.a{\n  color:\n    #fff;\n}\n',
      ':root{--a:#fff;--b:#000}\n.a{color:#fff;--c:#000;background:#111}',
      '.a{color:#fff}\n.b{--c:  #000 }\n.c{\n  --d\n  :\n  #000;\n  color: #000}',
      '--a:#fff;.a{}',
      '.a{x:y;--a:url(#fff);fill:url(#grad)}#fff{}#abc.d{}a#fed{}',
      '@media (min-width:1px){.a{color:#fff}}@supports (color: color(display-p3 1 1 1)){.b{color:oklab(1 0 0)}}',
      '.a{transition:all .2s}.b{transition: all}.c{transition :all}.d{transition:allx}.e{transition:opacity,all}.f{TRANSITION:ALL}',
      '.a{transition:\nall}',
      '.a{outline:none}',
      '.a{outline:none}.a:focus{outline:1px}',
      '.a{outline:none}.a:focus-visible{}',
      '/* :focus-visible */.a{outline:0}',
      '.a{outline:0px}.b{outline:nonex}.c{outline-style:none}.d{outline : none}',
      'input{font-size:12px}\n\n\n.a,\ninput[type=text]{font-size:12px}\n}\n}}{{\nselect{font-size:12px}',
      '.a{z-index:10}\n.b{color:red}.c{z-index:20}{z-index:30}',
      '.a{background:linear-gradient(#3b82f6,#8b5cf6);border-left:4px solid;border-radius:2px}',
      '.a{border-left:4px solid}\n.b{border-radius:2px}\n.c{border-left:44px;x:{border-radius:1px}}',
      `.a{color:#fff}${'\n'.repeat(500)}.b{color:#000}`,
      '.a{color:#fff}\r\n.b{color:#000}\r.c{color:#111}\u2028.d{color:#222}',
      '.tiếng-việt{color:#fff}/* 🚀 */.b::after{content:"🚀 #fff";color:#000}',
      Buffer.concat([Buffer.from('.a{color:'), Buffer.from([0xff, 0xc3]), Buffer.from('#fff}/*'), Buffer.from([0xe2, 0x82]), Buffer.from('*/.b{color:#000}')]),
      Array.from({ length: 400 }, (_, i) => `.c${i}{color:#${(i * 2654435761 % 0xffffff).toString(16).padStart(6, '0')};z-index:${i % 5}}`).join('\n'),
    ];
    for (const text of inputs) {
      sameCss(text);
      sameCss(text, 'tokens.css');
      sameHtml(Buffer.concat([Buffer.from('<html lang="en"><head><dfw-head></dfw-head><style>'), Buffer.from(text), Buffer.from('</style></head><body></body></html>')]), 'design/pages/x.html', `in <style>: ${label(text)}`);
    }
    sameCss('.a{color:#fff}', 'design/tokens/tokens.css');
    sameCss('.a{color:#fff}', 'tokens/tokens.css.bak');
  });

  test('hostile screens: styling, raw elements, instances, state coverage', () => {
    const page = (head, body) => `<!doctype html>\n<html lang="en">\n<head><dfw-head></dfw-head>${head}</head>\n<body>\n${body}\n</body>\n</html>\n`;
    const inputs = [
      page('', '<main data-dfw-id="m"><dfw-use c="page"><dfw-use c="app-shell" current="items"><dfw-use c="button" variant="primary" id="b">Go</dfw-use></dfw-use></dfw-use></main>'),
      page('<style>.x { display: grid; color: red; gap: 8px } .y{font-size:14px}</style>', '<main data-dfw-id="m"><button data-dfw-id="b">Raw</button><span style="background: #fff">x</span></main>'),
      page('<style>\n/* color: red;\n background: blue */\n.x{COLOR:red;Color:red;-color:red;background-color : red;border-top-color:red;border-top:1px;font:inherit;fonts:x;fill:x;stroke-width:1;--color:red}\n.y{text-decoration-color:red;text-decoration:none;text-decoration-line:x;outline-color:red;outline:0}</style>', '<p>x</p>'),
      page('<style>.x{color:red}</style><style>a:hover{color:red}.b{}color:red;{color:red}\n\n\ncolor\n:\nred</style>', ''),
      page('<style>@media (color:1) { .a { margin: 0 } }\n.b:is(a,b){ font : x; line-height:1;letter-spacing:0;text-transform:none;box-shadow:none;border-radius:0 }</style>', ''),
      page('', '<p style="color:red;;COLOR : blue; --color:x;background:url(a:b);margin:0">x</p><p style=\'color:red\'>y</p><p\tstyle\t=\t"fill:x">z</p><p data-style="color:red">q</p><p style="">e</p><pstyle="color:red">f</p>'),
      page('', '<button>raw</button>\n<dfw-use c="tabs"> <button>accepted</button><input><select></select></dfw-use><dfw-use c="tabs"><button>first child</button></dfw-use>\n<BUTTON>raw too</BUTTON><buttons>x</buttons><button-x>y</button-x>\n<dfw-use c="segmented"\n label="x">\n<button>one</button>\n<button>two</button>\n</dfw-use><input type="text"><select></select><textarea></textarea><table></table><nav></nav><dialog></dialog>'),
      `<html><head><button>in head</button><select></select></head>\n<body><dfw-use c="button">a</dfw-use><input></body><button>after body</button>`,
      '<button>no body at all</button><dfw-use c="button">x</dfw-use><select>',
      page('', Array.from({ length: 4 }, (_, i) => `<dfw-use c="button" variant="primary" id="p.${i}">P</dfw-use>`).join('') + '<dfw-use c="button" variant=\'primary\'>q</dfw-use><p>variant="primary"</p>'),
      page('', '<dfw-use c="button" variant="primary">1</dfw-use><dfw-use c="button" variant="primary">2</dfw-use>'),
      page('', '<main>no instances</main>'),
      '<BODY><main>upper body, no instances</main></BODY>',
      'no body, no instances',
      page('', '<dfw-use c="nope">x</dfw-use><dfw-use>y</dfw-use><dfw-icon name="nope"></dfw-icon><dfw-use c="button" variant="nope" size="huge">z</dfw-use>'),
      page('', '<dfw-use c="button">never closed'),
      page('', '<dfw-use c="page"><dfw-use c="button">a</dfw-use></dfw-use>'),
      page('', '<dfw-use c="app-shell" current="x"><dfw-use c="button">a</dfw-use></dfw-use>'),
      page('', '<dfw-use c="data-table"></dfw-use>'),
      page('', '<table></table>'),
      page('', '<TABLE></TABLE><FORM></FORM>'),
      page('', '<form></form>'),
      page('', '<dfw-use c="field" label="x"></dfw-use><dfw-use id="f" c="x-form-y"></dfw-use>'),
      page('', '<dfw-use id="l" variant="x" c="my-list"></dfw-use>'),
      page('', '<dfw-use c="nav-list"\n></dfw-use><dfw-use c=\'grid\'></dfw-use><dfw-use c="feed" ></dfw-use><tablet></tablet><forms></forms>'),
      page('', '<dfw-use c="stat-grid">x</dfw-use>'),
      page('', '<dfw-used c="table"></dfw-used><p c="table">x</p>'),
      // Comments: the lint rules read through them, compose does not expand what is inside.
      '<body>\n<!-- <dfw-use c="nope">x</dfw-use> <button>old</button> <table> -->\n<main data-dfw-id="m"><dfw-use c="button">a</dfw-use></main></body>',
      page('<!-- <style>.x{color:red}</style> -->', '<!-- style="color:red" variant="primary" variant="primary" variant="primary" --><dfw-use c="tabs"><!-- --><button>x</button></dfw-use>'),
    ];
    for (const text of inputs) {
      for (const rel of ['design/screens/hostile/a.html', 'design/screens/_shared/a.html', 'design/screens/seven/a.html', 'design/screens/unlisted/a.html']) {
        sameScreen(text, rel);
        sameStates(text, rel);
      }
      sameHtml(text, 'design/screens/hostile/a.html');
    }
  });

  test('screens the tokenizer composes differently than the Node expressions did (by design)', () => {
    // design/compose reads tags with the tokenizer: an instance inside a quoted attribute or a
    // script is not an instance, so lintScreen does not report it. Node read every one of these
    // differently; the rules of lint itself still agree.
    const rel = 'design/screens/unlisted/a.html';
    const cases = [
      ['<body><p title="<dfw-use c=\'nope\'>">x</p><dfw-use c="button">a</dfw-use></body>', ['page-layout', 'app-shell']],
      ['<body><script>const s = \'<dfw-use c="nope">x</dfw-use>\';</script><main>x</main></body>', ['no-components']],
    ];
    for (const [text, want] of cases) {
      const [file] = input(text);
      const found = native([file, '--as', 'screen', '--rel', rel, '--root', root]);
      assert.deepEqual(JSON.parse(found).map((f) => f.rule), want, text);
      // Node differed here by design: the recorded result is the binary's, which the rule lists above pin.
      assert.equal(mask(found), expected(mask(found)), text);
      sameHtml(text, rel);
    }
  });

  test('hostile token trees', () => {
    const c = (v) => ({ $value: v });
    const trees = [
      {},
      [],
      { $value: '#123456', $type: 'color' },
      { a: c('#fff') },
      { color: { $type: 'color', a: c('#f4f4f5'), b: c('#f4f4f6'), c: c('#F4F4F5'), d: c('#f4f4f7'), e: c({ hex: '#f4f4f5' }), f: c({ hex: 5 }), g: c(['#f4f4f6']), h: c(null), i: c('#fff'), j: c('#f4f4f5ff'), k: c('f4f4f5') } },
      { color: { $type: 'color', 2: c('#111111'), 1: c('#111112'), b: c('#111113'), a: c('#111114'), '01': c('#111115'), '-1': c('#111116') } },
      { 'a.b': { 'c{d}': c('{a.b.c{d}}'), $type: 'x' }, '{': c(1), '}': {}, '.': [] },
      { a: { $type: '', b: c(1), c: { $type: 0, d: c(2) }, e: { $type: false, f: c(3) }, g: { $type: 'x', h: c(4), i: { $type: null, j: c(5) } } } },
      { a: { $type: 'x', b: [c('{a.b.0}'), c('{a.b.1}'), c('{a.b.2}'), [c('{a.b.3.0}')]], c: 'text', d: 5, e: null, f: true } },
      { a: { $type: 'x', b: c('{a.c}'), c: c('{a.b} {a.d}{a.e}'), d: c('{}'), e: c('{{a.b}}'), f: c('{a.b'), g: c('a.b}'), h: c({ x: ['{a.nope}', { y: '{a.b}', z: 3 }], w: '{a.zip}' }), i: c('{a. b}') } },
      { $description: 'x', $extensions: { a: c('{nope}') }, a: { $description: c('#6366f1'), $type: 'color', b: { ...c('#6366f1'), $extensions: { c: c(1) } } } },
      { color: { $type: 'color', a: c('#6366F1'), b: c(' #4f46e5 '), c: c(['#8b5cf6', { d: '#D97757' }]), e: c('#6366f1ff'), f: c('constructor'), g: c('__proto__'), h: c('toString'), i: c('Constructor '), j: c('hasOwnProperty') } },
      { a: { $type: 'color', b: c('#000000') }, 'a.b': { $type: 'color', ...c('#000001') } },
      { a: { $type: 'x', ...c(1), b: c(2) }, c: { ...c(undefined), $type: 'y' } },
      { __proto__x: c(1), constructor: { $type: 'x', prototype: c('{constructor.prototype}') }, toString: c('{toString}') },
      JSON.stringify({ a: { $type: 'x', b: { $value: 1 } } }).replace('"a"', '"__proto__"'),
      '{"a":{"$type":"x","b":{"$value":1},"b":{"$value":"{a.nope}"}},"a":{"c":{"$value":2}}}',
      '{"color":{"$type":"color","é":{"$value":"#101010"},"🚀":{"$value":"#101011"},"e\\u0301":{"$value":"{color.é}"}}}',
      (() => {
        let t = c('{deep}');
        for (let i = 0; i < 60; i++) t = { [`g${i}`]: t, $type: i % 2 ? 'x' : '' };
        return t;
      })(),
      { color: Object.fromEntries([['$type', 'color'], ...Array.from({ length: 120 }, (_, i) => [`c${i}`, c(`#${(0x101010 + i * 0x010000 + (i % 3)).toString(16)}`)])]) },
    ];
    for (const tree of trees) {
      sameTokens(tree);
      sameTokens(tree, { a: { b: c(1), nope: c(2) }, deep: c(0), color: { é: c(1) } });
    }
    sameTokens({ a: c('{b.c}') }, [{ c: c(1) }]);
    sameTokens({ a: c('{0.c}') }, [{ c: c(1) }]);
    sameTokens({ a: c('{}') }, { $value: 1 });
    sameTokens({ a: c('{b}') }, 'text');
    // Text that is not JSON never reaches lintTokens.
    const [file] = input('{ nope', 'json');
    assert.equal(native([file, '--as', 'tokens']), '{"error":true}');
  });
});
