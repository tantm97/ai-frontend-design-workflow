// Design-file foundation: tokens, colour, compose (on the tokenizer), icons, style guides,
// system, inventory, and the CLI commands built on them. Commands run on copies of a fixture
// project; compose and colour are also checked function by function through the binary's hidden
// `__debug compose|color` commands. The recorded results are what the Node CLI and modules of
// 0.5.2 printed for the same inputs (where Node had a known defect, the binary's own result).
// Run: node --test tests/golden/
import { describe, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { run, assertSame, nativeBinary, skipReason, REPO, suite, expected, MINIMAL_STATE } from './harness.mjs';

const test = suite('design1');

const CORE = path.join(REPO, 'core');

const KIT = path.join(CORE, 'kits', 'base', 'components');
const FIXTURE = path.join(REPO, 'tests', 'fixtures', 'penpot', 'project');

function native(args) {
  const env = { ...process.env, DFW_PLUGIN_ROOT: CORE };
  const r = spawnSync(nativeBinary(), args, { env, encoding: 'utf8', timeout: 180_000, maxBuffer: 64 * 1024 * 1024 });
  if (r.error) throw r.error;
  return r;
}

/** `dfw <args>` with the binary on a project (setup only: what a case compares is made after it). */
function cli(root, args) {
  const env = {
    ...process.env,
    DFW_PLUGIN_ROOT: CORE,
    DFW_PROJECT_DIR: root,
    CLAUDE_PROJECT_DIR: root,
    DFW_HOME: path.join(root, '..', 'home'),
  };
  const r = spawnSync(nativeBinary(), args, { cwd: root, env, encoding: 'utf8', timeout: 180_000 });
  if (r.error) throw r.error;
  assert.equal(r.status, 0, `dfw ${args.join(' ')}: ${r.stderr}`);
  return r;
}

/** The command gives the recorded stdout, exit code and files; returns the binary's result. */
function same(args, opts) {
  const r = run(args, opts);
  assertSame(r);
  return r.native;
}

const write = (root, rel, text) => {
  fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  fs.writeFileSync(path.join(root, rel), text);
};

describe('design commands', { skip: skipReason() }, () => {
  test('build and tokens build write tokens.css and components.css', () => {
    for (const args of [['build'], ['tokens', 'build']]) {
      const r = same(args);
      assert.equal(r.code, 0);
      assert.match(r.stdout, /^Wrote design\/tokens\/tokens\.css; 1 component\(s\) -> design\/system\/components\.css\n$/);
      assert.deepEqual(Object.keys(r.files), ['design/system/components.css', 'design/tokens/tokens.css']);
    }
    const themed = same(['build'], {
      setup: (root) =>
        write(
          root,
          'design/tokens/themes/dark.json',
          '{"color":{"$type":"color","ink":{"$value":"#fafafa"},"10":{"$value":"#000"},"2":{"$value":{"colorSpace":"oklch","components":[0.5,0.1,"none"],"alpha":0.25}}}}',
        ),
    });
    assert.match(themed.stdout, /\(themes: dark\)/);
    assert.match(
      themed.files['design/tokens/tokens.css'],
      /\[data-theme="dark"\] \{\n {2}--color-2: oklch\(0\.5 0\.1 none \/ 0\.25\);\n {2}--color-10: #000;\n {2}--color-ink: #fafafa;/,
    );
  });

  test('build errors: no tokens, a null value, a usage error', () => {
    const none = same(['build'], { setup: (root) => fs.rmSync(path.join(root, 'design/tokens/tokens.json')) });
    assert.equal(none.code, 1);
    assert.equal(none.stderr, 'design/tokens/tokens.json not found.\n');
    const nul = run(['build'], {
      setup: (root) => write(root, 'design/tokens/tokens.json', '{"space":{"$type":"dimension","a":{"$value":null}}}'),
    });
    assertSame(nul, { stderr: true });
    assert.equal(nul.native.code, 1);
    assertSame(run(['tokens'], {}), { stderr: true });
    assertSame(run(['tokens', 'nope'], {}), { stderr: true });
    // Broken JSON: same exit code and nothing written; the parser's own words differ (masked).
    const parserWords = (text) => text.replace(/^(design\/tokens\/tokens\.json: ).+$/m, '$1<parser message>');
    const broken = same(['build'], { setup: (root) => write(root, 'design/tokens/tokens.json', '{"a":'), normalize: parserWords });
    assert.equal(broken.code, 1);
    assert.match(broken.stderr, /^design\/tokens\/tokens\.json: /);
  });

  test('token types print the same CSS', () => {
    const tokens = {
      color: {
        $type: 'color',
        a: { $value: { colorSpace: 'srgb', components: [0.1, 0.25, 1], hex: '#1a40ff' } },
        b: { $value: { colorSpace: 'srgb', components: [0.12345, 'none', 1], alpha: 0.5 } },
        c: { $value: { colorSpace: 'display-p3', components: [1, 0.5, 0] } },
        d: { $value: { colorSpace: 'lab', components: [50.123456, -20, 30], alpha: 0.3 } },
        e: { $value: '{color.a}' },
        f: { $value: 'color-mix(in oklab, {color.a} 40%, {color.c})' },
        g: { $value: { colorSpace: 'oklch', components: [0.7, 0.1] } },
        h: { $value: { colorSpace: 'xyz', components: [0.1, 0.2, 0.3], hex: '#123' } },
      },
      space: {
        $type: 'dimension',
        half: { $value: { value: 2, unit: 'px' } },
        1: { $value: { value: 4, unit: 'px' } },
        '1-5': { $value: { value: 6.00004, unit: 'px' } },
        2: { $value: { value: 0.123456, unit: 'rem' } },
        raw: { $value: 12 },
        odd: { $value: [1, 2] },
      },
      duration: { $type: 'duration', fast: { $value: { value: 120, unit: 'ms' } } },
      n: { $type: 'number', a: { $value: 1.5 }, b: { $value: 1e21 }, c: { $value: 0.000001 }, d: { $value: 1e-7 } },
      w: { $type: 'fontWeight', bold: { $value: 700 } },
      fam: { $type: 'fontFamily', a: { $value: ['Be Vietnam Pro', 'system-ui', 'a,b'] }, b: { $value: 'Inter' }, c: { $value: [1, null] } },
      ease: { $type: 'cubicBezier', out: { $value: [0.2, 0, 0, 1.00001] } },
      shadow: {
        $type: 'shadow',
        a: {
          $value: [
            { color: '#000', offsetX: { value: 0, unit: 'px' }, offsetY: { value: 1, unit: 'px' }, blur: { value: 2, unit: 'px' } },
            {
              inset: true,
              color: { colorSpace: 'srgb', components: [0, 0, 0], alpha: 0.2 },
              offsetX: { value: 0, unit: 'px' },
              offsetY: { value: 4, unit: 'px' },
              spread: { value: -1, unit: 'px' },
            },
          ],
        },
      },
      border: { $type: 'border', a: { $value: { width: { value: 1, unit: 'px' }, style: 'solid', color: '{color.a}' } } },
      tr: {
        $type: 'transition',
        a: { $value: { duration: { value: 120, unit: 'ms' }, timingFunction: [0, 0, 1, 1] } },
        b: { $value: { duration: { value: 1, unit: 's' }, timingFunction: '{ease.out}', delay: { value: 10, unit: 'ms' } } },
      },
      stroke: { $type: 'strokeStyle', a: { $value: 'dotted' }, b: { $value: { dashArray: [1, 2] } } },
      grad: {
        $type: 'gradient',
        a: {
          $value: [
            { color: '#000', position: 0 },
            { color: '{color.a}', position: 0.333333 },
          ],
        },
      },
      type: {
        $type: 'typography',
        body: {
          $value: {
            fontFamily: ['Inter', 'sans-serif'],
            fontSize: { value: 16, unit: 'px' },
            fontWeight: 400,
            letterSpacing: { value: -0.01, unit: 'em' },
            lineHeight: 1.5,
          },
        },
      },
      other: {
        str: { $type: 'string', $value: 'uppercase' },
        bool: { $value: true },
        obj: { $type: 'custom', $value: { a: [1, 'x'] } },
        num: { $value: 3.25 },
      },
      list: [{ $type: 'number', $value: 1 }, { deep: { $value: 2 } }],
    };
    const r = same(['build'], { setup: (root) => write(root, 'design/tokens/tokens.json', JSON.stringify(tokens)) });
    assert.equal(r.code, 0);
    assert.match(
      r.files['design/tokens/tokens.css'],
      /--space-1: 4px;\n {2}--space-2: 0\.1235rem;\n {2}--space-half: 2px;\n {2}--space-1-5: 6px;/,
    );
  });

  test('system: presets, ref, list, inventory', () => {
    for (const args of [
      ['system', 'presets'],
      ['system', 'ref'],
      ['system', 'ref', '--json'],
      ['system', 'ref', 'button'],
      ['system'],
      ['system', 'list'],
      ['system', 'list', '--json'],
      ['system', 'inventory'],
      ['system', 'inventory', '--json'],
    ]) {
      assert.equal(same(args).code, 0, args.join(' '));
    }
    assertSame(run(['system', 'ref', 'nope']), { stderr: true });
    assertSame(run(['system', 'nope']), { stderr: true });
    assertSame(run(['system', 'init', '--only', 'button']), { stderr: true });
    const empty = same(['system', 'list'], { setup: (root) => fs.rmSync(path.join(root, 'design/system'), { recursive: true }) });
    assert.equal(empty.stdout, 'No components yet (dfw system init).\n');
  });

  test('system inventory: findings and exit code 2 on a kit copy', () => {
    const setup = (root) => {
      fs.copyFileSync(path.join(KIT, 'badge.html'), path.join(root, 'design/system/components/badge.html'));
      write(
        root,
        'design/system/inventory.json',
        JSON.stringify({ components: [{ name: 'button', purpose: 'Actions', usedBy: ['items'] }, { name: 'price' }, { purpose: 'x' }] }),
      );
    };
    const r = same(['system', 'inventory'], { setup });
    assert.equal(r.code, 2);
    assert.match(r.stdout, /\[MUST\] \(kit-copy\)/);
    assert.match(r.stdout, /\[SHOULD\] \(unused-master\)/);
    assert.equal(same(['system', 'inventory', '--json'], { setup }).code, 0);
    assertSame(run(['system', 'inventory'], { setup: (root) => write(root, 'design/system/inventory.json', '{"components":1}') }), {
      stderr: true,
    });
  });

  test('system init: preset, style, kept tokens, --force', () => {
    const placeholder = (root) => write(root, 'design/tokens/tokens.json', '{"$description":"Placeholder tokens created by dfw init."}');
    for (const args of [
      ['system', 'init'],
      ['system', 'init', '--preset', 'soft', '--force'],
      ['system', 'init', '--preset', 'industrial'],
    ]) {
      assert.equal(same(args, { setup: placeholder }).code, 0, args.join(' '));
    }
    const kept = same(['system', 'init']);
    assert.match(kept.stdout, /tokens kept \(pass --force to replace\); design\/system\/inventory\.json created/);
    assertSame(run(['system', 'init', '--preset', 'nope']), { stderr: true });
    assertSame(run(['system', 'init', '--style', 'swiss-grid']), { stderr: true });
    for (const style of ['swiss-grid', 'heritage-signboard', 'dark-console']) {
      const r = same(['system', 'init', '--style', style, '--force']);
      assert.equal(r.code, 0);
      assert.ok(r.files['design/system/style.json'] && r.files['design/tokens/tokens.json'] && r.files['design/system/head.html']);
    }
    assert.equal(same(['system', 'init', '--style', 'editorial-luxe', '--fonts', 'latin'], { setup: placeholder }).code, 0);
  });

  test('system options and choose (presets and style guides)', () => {
    const o = same(['system', 'options', 'crisp', 'soft', 'industrial-utility']);
    assert.equal(
      o.stdout,
      'Option themes: opt-crisp, opt-industrial-utility, opt-soft. Compare them in the studio (theme menu / "Themes" toggle), then: dfw system choose <preset>.\n',
    );
    assert.match(o.files['design/design.json'], /"themes": \[\n {4}"opt-crisp",\n {4}"opt-soft",\n {4}"opt-industrial-utility"\n {2}\]/);
    assertSame(run(['system', 'options', 'crisp']), { stderr: true });
    assertSame(run(['system', 'options', 'crisp', 'sotf']), { stderr: true });
    // A second run replaces the block in head.html and the option themes.
    const again = (root) => {
      write(root, 'design/system/head.html', '<link rel="x">\n<!-- dfw-options-begin -->\nold\n<!-- dfw-options-end -->\n<link rel="y">\n');
      write(root, 'design/tokens/themes/opt-old.json', '{}');
      write(root, 'design/tokens/themes/dark.json', '{}');
    };
    const second = same(['system', 'options', 'swiss-grid', 'warm-craft', '--fonts', 'latin'], { setup: again });
    assert.equal(second.files['design/tokens/themes/opt-old.json'], null);
    for (const name of ['soft', 'swiss-grid']) {
      const withOption = (root) => {
        write(root, `design/tokens/themes/opt-${name}.json`, '{"color":{"$type":"color","accent":{"$value":"#0d6b78"}}}');
        write(root, 'design/design.json', JSON.stringify({ schema: 1, themes: ['dark', `opt-${name}`], screens: [] }));
      };
      const c = same(['system', 'choose', name], { setup: withOption });
      assert.equal(c.code, 0);
      assert.match(c.files['design/tokens/tokens.css'], /--color-accent: #0d6b78/);
    }
    assert.equal(same(['system', 'choose', 'industrial']).code, 0);
    assertSame(run(['system', 'choose', 'nope']), { stderr: true });
    assertSame(run(['system', 'choose']), { stderr: true });
  });

  test('style: list, search, show, match, apply', () => {
    for (const args of [
      ['style'],
      ['style', 'list'],
      ['style', 'search', 'factory', 'counter'],
      ['style', 'search', 'dark,console', '--limit', '2'],
      ['style', 'search', 'zzzz'],
      ['style', 'search'],
      ['style', 'show', 'swiss-grid'],
      ['style', 'show', 'heritage-signboard', '--json'],
    ]) {
      assert.equal(same(args).code, 0, args.join(' '));
    }
    assertSame(run(['style', 'show', 'nope']), { stderr: true });
    assertSame(run(['style', 'show']), { stderr: true });
    assertSame(run(['style', 'nope']), { stderr: true });
    assertSame(run(['style', 'match']), { stderr: true });
    const brief = (root) =>
      write(
        root,
        'design/system/brief.json',
        JSON.stringify({
          personality: { formality: 3, energy: 2, era: 4, warmth: 2, density: 5, richness: 1, seriousness: 4, mood: 3 },
          weights: { density: 2 },
          constraints: { dark: false },
          keywords: ['Admin', 'b2b'],
        }),
      );
    for (const args of [
      ['style', 'match'],
      ['style', 'match', '--json'],
      ['style', 'match', '--limit', '12'],
      ['style', 'match', '--brief', 'design/system/brief.json', '--limit', '1'],
    ]) {
      assert.equal(same(args, { setup: brief }).code, 0, args.join(' '));
    }
    for (const id of fs
      .readdirSync(path.join(CORE, 'kits', 'styles'))
      .sort()
      .map((f) => f.replace('.json', ''))) {
      const r = same(['style', 'apply', id], {
        setup: (root) => fs.copyFileSync(path.join(CORE, 'templates', 'DESIGN.md'), path.join(root, 'DESIGN.md')),
      });
      assert.equal(r.code, 0, id);
      assert.ok(r.files['design/tokens/tokens.css'] && r.files['design/system/style.json'], id);
    }
    assert.equal(same(['style', 'apply', 'ops-dense', '--fonts', 'latin']).code, 0);
    assertSame(run(['style', 'apply', 'nope']), { stderr: true });
  });

  test('a project guide file: apply, options and choose', () => {
    // The project's own guide file (core/workflow/references/phase-direction.md), its id and
    // the option theme it makes.
    const SOURCE = 'design/system/style-source.json';
    const TWIN = 'design/system/twin.json';
    const ID = 'my-brand';
    const OPTION = `opt-${ID}`;
    const OPTION_FILE = `design/tokens/themes/${OPTION}.json`;
    // The guide format of `dfw style show <id> --json`, under the project's own id.
    const guide = (root, file = SOURCE, id = ID) => {
      const g = JSON.parse(fs.readFileSync(path.join(CORE, 'kits', 'styles', 'civic-public.json'), 'utf8'));
      write(root, file, JSON.stringify({ ...g, id }, null, 2));
    };
    const applied = same(['style', 'apply', SOURCE], { setup: guide });
    assert.equal(applied.code, 0, applied.stderr);
    assert.match(applied.stdout, new RegExp(`^Applied style "${ID}"`));
    const o = same(['system', 'options', SOURCE, 'crisp'], { setup: guide });
    assert.equal(o.code, 0, o.stderr);
    assert.ok(o.files[OPTION_FILE], Object.keys(o.files).join(', '));
    assert.ok(!Object.keys(o.files).some((f) => f.includes('opt-design')));
    const withOption = (root) => {
      guide(root);
      write(root, OPTION_FILE, '{"color":{"$type":"color","accent":{"$value":"#0d6b78"}}}');
      write(root, 'design/design.json', JSON.stringify({ schema: 1, themes: [OPTION], screens: [] }));
    };
    const c = same(['system', 'choose', SOURCE], { setup: withOption });
    assert.equal(c.code, 0, c.stderr);
    assert.match(c.files['design/tokens/tokens.css'], /--color-accent: #0d6b78/);
    assertSame(run(['system', 'choose', ID], { setup: withOption }), { stderr: true });
    const twins = (root) => {
      guide(root);
      guide(root, TWIN);
    };
    assertSame(run(['system', 'options', SOURCE, TWIN], { setup: twins }), { stderr: true });
  });

  test("style apply seeds DESIGN.md once and keeps the user's rationale", () => {
    // The harness watches .dfw/ and design/ only, so compare DESIGN.md by hand.
    const seeded = (text, second) => {
      const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-md-')));
      try {
        fs.cpSync(FIXTURE, root, { recursive: true });
        fs.mkdirSync(path.join(root, '.dfw'));
        fs.writeFileSync(path.join(root, '.dfw', 'state.json'), '{}');
        fs.writeFileSync(path.join(root, 'DESIGN.md'), text);
        for (const id of ['warm-craft', ...(second ? [second] : [])]) {
          const env = { ...process.env, DFW_PLUGIN_ROOT: CORE, CLAUDE_PROJECT_DIR: root };
          const r = spawnSync(nativeBinary(), ['style', 'apply', id], { cwd: root, env, encoding: 'utf8', timeout: 180_000 });
          assert.equal(r.status, 0, r.stderr);
        }
        const out = fs.readFileSync(path.join(root, 'DESIGN.md'), 'utf8');
        assert.equal(out, expected(out));
        return out;
      } finally {
        fs.rmSync(root, { recursive: true, force: true });
      }
    };
    const template = fs.readFileSync(path.join(CORE, 'templates', 'DESIGN.md'), 'utf8');
    assert.match(seeded(template), /Seeded from style guide "warm-craft"/);
    assert.match(seeded(template, 'swiss-grid'), /Seeded from style guide "swiss-grid"/);
    assert.match(seeded('# D\n\n## Rationale\n- mine: because\n\n## Next\n'), /- mine: because/);
    assert.match(seeded('# D\r\n\r\ntext\r\n'), /## Rationale\n<!-- Seeded/);
    assert.match(seeded('# D\n## Rationale\n\n## Next\n', 'swiss-grid'), /swiss-grid/);
  });

  test('type scale: print and --write', () => {
    for (const args of [
      ['type', 'scale'],
      ['type', 'scale', '--base', '15', '--ratio', '1.25', '--display-ratio', '1.414', '--from', '28'],
      ['type', 'scale', '--base', '10', '--ratio', '1.05'],
      ['type', 'scale', '--base', 'x', '--ratio', '0'],
      ['type', 'scale', '--write'],
      ['type', 'scale', '--base', '18', '--ratio', '1.333', '--write'],
    ]) {
      assert.equal(same(args).code, 0, args.join(' '));
    }
    assertSame(run(['type']), { stderr: true });
    const w = same(['type', 'scale', '--write'], {
      setup: (root) =>
        write(
          root,
          'design/tokens/tokens.json',
          '{"font":{"size":{"md":{"$value":{"value":1,"unit":"px"},"$extra":1},"2":{"$value":"x"},"zz":{"$value":"y"}}},"space":{"b":{"$value":1},"1":{"$value":2}}}',
        ),
    });
    assert.match(w.files['design/tokens/tokens.json'], /"size": \{\n {6}"2": /);
  });

  test('palette build: text and json, light and dark, every accent source', () => {
    const cases = [
      ['--brand', '#0b5cad'],
      [
        '--brand',
        '250',
        '--chroma',
        '0.12',
        '--neutral-hue',
        '240',
        '--tint',
        '0.008',
        '--signature',
        '95',
        '--signature-chroma',
        '0.17',
        '--names',
        'xanh,thep,vang',
      ],
      ['--brand', '28', '--chroma', '0.16', '--neutral-hue', '80', '--tint', '0.012', '--accent', 'neutral'],
      ['--brand', '28', '--harmony', 'triadic', '--accent', 'signature', '--warm-surface'],
      ['--brand', '#9a2f22', '--dark'],
      ['--brand', '145', '--harmony', 'nope', '--dark', '--accent', 'signature', '--names', 'a,a,b'],
      ['--brand', '#abc', '--chroma', '0.3', '--tint', '0.05'],
      ['--brand', '0', '--names', '2,1'],
      ['--brand', ''],
      ['--brand', '#zzzzzz'],
    ];
    for (const args of cases) {
      assert.equal(same(['palette', 'build', ...args]).code, 0, args.join(' '));
      assert.equal(same(['palette', 'build', ...args, '--json']).code, 0, `${args.join(' ')} --json`);
    }
    for (const args of [['palette'], ['palette', 'build'], ['palette', 'build', '--brand', 'blue']])
      assertSame(run(args), { stderr: true });
    for (let hue = 0; hue < 360; hue += 17)
      assertSame(run(['palette', 'build', '--brand', String(hue), '--chroma', String(0.05 + (hue % 5) * 0.04), '--json']));
  });

  test('icons: search, add, list', () => {
    for (const args of [
      ['icons'],
      ['icons', 'list'],
      ['icons', 'search', 'scan', 'barcode'],
      ['icons', 'search', 'arrow', '--limit', '60'],
      ['icons', 'search', 'a', '--limit', '400'],
      ['icons', 'search', 'Chevron,Down'],
      ['icons', 'search', 'zzzzqq'],
      ['icons', 'search'],
    ]) {
      assert.equal(same(args).code, 0, args.join(' '));
    }
    const added = same(['icons', 'add', 'search', 'x', 'house']);
    assert.deepEqual(Object.keys(added.files), [
      'design/system/icons/house.svg',
      'design/system/icons/search.svg',
      'design/system/icons/x.svg',
    ]);
    const miss = same(['icons', 'add', 'plus', 'chevron', 'zzzzqq', 'chevron']);
    assert.equal(miss.code, 2);
    assert.match(miss.stdout, /chevron: not found \(try: /);
    assert.equal(
      same(['icons', 'list'], {
        setup: (root) => write(root, 'design/system/icons/b.svg', '<svg/>') || write(root, 'design/system/icons/a.svg', '<svg/>'),
      }).stdout,
      'a b\n',
    );
    assertSame(run(['icons', 'nope']), { stderr: true });
  });

  test('blueprint: list, one screen, json', () => {
    const manifest = {
      screens: [
        {
          id: 'items',
          title: 'Items',
          blueprint: {
            layout: 'table',
            goal: 'Find an item',
            primaryAction: 'Add item',
            aboveFold: ['search', 'table'],
            regions: [
              { priority: 1, name: 'table', content: 'items' },
              { name: 'rail', content: ['a', 'b'] },
            ],
            data: { empty: 'CTA', typical: 40 },
            feedback: 'toast',
          },
        },
        { id: 'detail', blueprint: { goal: 'Read' } },
        { id: 'bare', title: 'Bare' },
        { id: '_explore', title: 'Hidden' },
      ],
    };
    const setup = (root) => write(root, 'design/design.json', JSON.stringify(manifest));
    for (const args of [
      ['blueprint'],
      ['blueprint', 'list'],
      ['blueprint', 'items'],
      ['blueprint', '_explore'],
      ['blueprint', 'nope'],
      ['blueprint', '--json'],
      ['blueprint', 'detail', '--json'],
    ]) {
      assert.equal(same(args, { setup }).code, 0, args.join(' '));
    }
    assert.equal(same(['blueprint']).stdout, 'items: NO BLUEPRINT\n');
  });

  test('screen add: placed after a sibling or at the end of its page, refusals, frozen design', () => {
    const shell = (current) =>
      `<!doctype html><html lang="vi"><body><dfw-use c="app-shell" variant="focus" current="${current}" id="shell"></dfw-use></body></html>\n`;
    const manifest = {
      schema: 1,
      product: 'Fixture',
      lang: 'vi',
      pages: [
        { id: 'stock', title: 'Stock', screens: ['items', 'detail'] },
        { id: 'settings', title: 'Settings', screens: [] },
      ],
      screens: [
        { id: 'items', title: 'Items', variants: [{ id: 'a', file: 'screens/items/a.html' }], blueprint: { layout: 'flow' } },
        { id: 'detail', title: 'Detail', variants: [{ id: 'a', file: 'screens/detail/a.html' }] },
      ],
    };
    const setup = (root) => {
      write(root, 'design/design.json', JSON.stringify(manifest, null, 2) + '\n');
      write(root, 'design/screens/items/a.html', shell('stock'));
      write(root, 'design/screens/detail/a.html', '<p></p>\n');
    };
    // Placed: between items and detail, at the end of a page, as JSON.
    for (const args of [
      [
        'screen',
        'add',
        'item-review',
        '--page',
        'stock',
        '--after',
        'items',
        '--from',
        'items,detail',
        '--title',
        'Review & save',
        '--route',
        '/items/review',
      ],
      ['screen', 'add', 'history', '--page', 'stock'],
      ['screen', 'add', 'billing', '--page', 'settings', '--json'],
    ]) {
      assert.equal(same(args, { setup }).code, 0, args.join(' '));
    }
    // Refused, nothing written: usage, ids, unknown page, --after elsewhere, unknown --from, an existing id.
    for (const args of [
      ['screen'],
      ['screen', 'add', 'item-review'],
      ['screen', 'add', '../escape', '--page', 'stock'],
      ['screen', 'add', '_explore', '--page', 'stock'],
      ['screen', 'add', 'item-review', '--page', 'billing'],
      ['screen', 'add', 'item-review', '--page', 'settings', '--after', 'items'],
      ['screen', 'add', 'item-review', '--page', 'stock', '--from', 'nope'],
      ['screen', 'add', 'items', '--page', 'stock'],
    ]) {
      const r = run(args, { setup });
      assertSame(r, { stderr: true });
      assert.equal(r.native.code, 1, args.join(' '));
    }
    // Approved and locked (G2): refused until reopened.
    const locked = { ...MINIMAL_STATE, design: { version: 1, status: 'approved', locked: true, approved: [1] } };
    const r = run(['screen', 'add', 'item-review', '--page', 'stock'], { setup, state: locked });
    assertSame(r, { stderr: true });
    assert.equal(r.native.code, 1);
  });

  test('fonts: usage error without a network call; the other subcommands answer the same', () => {
    assertSame(run(['fonts']), { stderr: true });
    assertSame(run(['fonts', 'check']), { stderr: true });
    assertSame(run(['tokens', 'extract']), { stderr: true });
    assertSame(run(['tokens', 'import']), { stderr: true });
    assertSame(run(['system', 'audit']));
  });
});

// ---------------------------------------------------------------- compose

describe('compose', { skip: skipReason() }, () => {
  let tmp;
  let root;
  let n = 0;
  before(() => {
    tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-compose-')));
    root = path.join(tmp, 'project');
    fs.cpSync(FIXTURE, root, { recursive: true });
    fs.mkdirSync(path.join(root, '.dfw'));
    fs.writeFileSync(path.join(root, '.dfw', 'state.json'), '{}');
    fs.rmSync(path.join(root, 'design/tokens/tokens.json'));
    // Made with the binary, as when the results were recorded.
    cli(root, ['system', 'init', '--preset', 'crisp']);
    const dir = path.join(root, 'design/system/components');
    fs.renameSync(path.join(dir, 'button.html'), path.join(dir, 'fixture-button.html'));
    fs.writeFileSync(
      path.join(dir, 'fixture-button.html'),
      fs.readFileSync(path.join(dir, 'fixture-button.html'), 'utf8').replace('name="button"', 'name="fixture-button"'),
    );
    for (const f of fs.readdirSync(KIT)) fs.copyFileSync(path.join(KIT, f), path.join(dir, f));
    cli(root, [
      'icons',
      'add',
      'chevron-down',
      'triangle-alert',
      'search',
      'x',
      'check',
      'package',
      'house',
      'file-clock',
      'settings',
      'layout-dashboard',
      'circle-check',
      'circle-x',
      'circle-alert',
      'octagon-alert',
      'package-open',
      'ellipsis',
      'wrench',
      'log-in',
      'log-out',
      'plus',
    ]);
    const m = (name, tpl, attrs = '') =>
      fs.writeFileSync(
        path.join(dir, `${name}.html`),
        `<dfw-component name="${name}"${attrs}>\n<template>\n${tpl}\n</template>\n</dfw-component>\n`,
      );
    m(
      't-box',
      '<section class="box"><header data-dfw-if="title">{{title}}</header><dfw-slot>empty</dfw-slot><footer><dfw-slot name="foot"/></footer></section>',
      ' props="title, tone=calm one"',
    );
    m(
      't-cond',
      `<div class="c c--{{ variant }}"><i data-dfw-if='a'>A</i><b data-dfw-if="!a">no a</b><u data-dfw-if=mode==x>X</u><s data-dfw-if="mode != x">not x</s><em data-dfw-if="">never</em>{{ mode==x ? is "x" & <y> }}{{base}}{{ nope }}</div>`,
      ' variants="one two" sizes="sm md" props="a mode=x"',
    );
    m('t-loop', '<div><dfw-use c="t-loop"></dfw-use></div>');
    m(
      't-deep',
      '<div class="deep"><dfw-use c="t-box" title="{{label}}"><dfw-use c="button" variant="ghost">{{label}}</dfw-use><dfw-fill slot="foot"><dfw-slot></dfw-slot></dfw-fill></dfw-use></div>',
      ' props="label"',
    );
    m('t-void', '<img class="pic" src="{{src}}" alt="">', ' props="src"');
    m('t-self', '<br class="gap"/>');
    m('t-text', 'just text {{variant}}');
    m('t-comment', '<!-- one --><!-- two -->\n<p class="p"><!-- inner <dfw-slot></dfw-slot> --><dfw-slot></dfw-slot></p>');
    m('t-amp', '<a class="a" href="?a=1&amp;b=2" title=\'it&#39;s &lt;ok&gt; &nbsp;\' data-e="" hidden>{{t}}</a>', ' props="t"');
    cli(root, ['build']);
  });
  after(() => fs.rmSync(tmp, { recursive: true, force: true }));

  /** Temporary and checkout paths as <tmp> and <repo>, so a result can be recorded. */
  const mask = (text) => text.split(tmp).join('<tmp>').split(REPO).join('<repo>');
  const nativeCompose = (text, base) => {
    const file = path.join(tmp, `in-${n++}.html`);
    fs.writeFileSync(file, text);
    const r = native(['__debug', 'compose', file, '--root', root, ...(base ? ['--base', base] : [])]);
    assert.equal(r.status, 0, r.stderr);
    return mask(r.stdout.replace(/\n$/, ''));
  };
  /** Compose with the binary, compare with the recorded result; returns it parsed. */
  const sameCompose = (text, base, label = text.slice(0, 80)) => {
    const b = nativeCompose(text, base);
    assert.equal(b, expected(b), label);
    return JSON.parse(b);
  };
  /** `__debug compose <file> --parse`: the masters of a file as parseComponentFile returned them. */
  const parse = (file) => {
    const r = native(['__debug', 'compose', file, '--parse']);
    assert.equal(r.status, 0, r.stderr);
    return mask(r.stdout);
  };

  test('every fixture screen and master, every kit master and its examples, the templates', () => {
    const files = [];
    const walk = (dir) => {
      for (const e of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
        if (e.isDirectory()) walk(path.join(dir, e.name));
        else if (e.name.endsWith('.html')) files.push(path.join(dir, e.name));
      }
    };
    for (const d of ['tests/fixtures', 'core/kits', 'core/templates']) walk(path.join(REPO, d));
    assert.ok(files.length >= 30);
    for (const f of files) {
      const text = fs.readFileSync(f, 'utf8');
      const rel = path.relative(REPO, f);
      sameCompose(text, '../../', rel);
      sameCompose(text, undefined, rel);
      const parsed = parse(f);
      assert.equal(parsed, expected(parsed), `parse ${rel}`);
    }
    // The masters of the project and their examples: every file in name order, a later master of
    // the same name replaces the earlier one in its place (loadComponents of 0.5.2).
    const lib = new Map();
    const dir = path.join(root, 'design/system/components');
    for (const f of fs
      .readdirSync(dir)
      .filter((x) => x.endsWith('.html'))
      .sort()) {
      for (const d of JSON.parse(parse(path.join(dir, f)))) lib.set(d.name, d.examples ?? null);
    }
    const masters = [...lib.entries()];
    assert.deepEqual(masters, expected(masters));
    for (const [name, examples] of masters) {
      if (name === 't-comment') continue; // differs by design: see the last test of this suite
      const r = sameCompose(examples || `<dfw-use c="${name}"></dfw-use>`, '/design/', `examples of ${name}`);
      if (!name.startsWith('t-')) assert.deepEqual(r.issues, [], name);
    }
  });

  test('generated files: the motif master of every style guide', () => {
    const project = path.join(tmp, 'motif-project');
    fs.cpSync(FIXTURE, project, { recursive: true });
    fs.mkdirSync(path.join(project, '.dfw'));
    fs.writeFileSync(path.join(project, '.dfw', 'state.json'), '{}');
    for (const id of fs
      .readdirSync(path.join(CORE, 'kits', 'styles'))
      .sort()
      .map((f) => f.replace('.json', ''))) {
      // The master as `dfw style apply` writes it (design/system/components/motif.html), then parsed.
      cli(project, ['style', 'apply', id]);
      const text = fs.readFileSync(path.join(project, 'design/system/components/motif.html'), 'utf8');
      assert.equal(text, expected(text), id);
      const file = path.join(tmp, `motif-${id}.html`);
      fs.writeFileSync(file, text);
      const parsed = parse(file);
      assert.equal(parsed, expected(parsed), id);
    }
  });

  test('hand-written inputs: props, slots, conditions, passthrough, quotes, line numbers', () => {
    const inputs = [
      '<dfw-use c="t-box" title="Hello &amp; &quot;you&quot;" id="b1" class="wide tall" aria-label="Box" data-x="" hidden>body<dfw-fill slot="foot">F</dfw-fill></dfw-use>',
      "<dfw-use c='t-box' title='single \"quoted\"' id='b2'>x</dfw-use>",
      '<dfw-use c=t-box title=bare id=b3 disabled>x</dfw-use>',
      '<dfw-use component="t-box"></dfw-use><dfw-use c="" component="t-box">  \n </dfw-use>',
      '<dfw-use c="t-box"><dfw-fill>default fill</dfw-fill>rest<dfw-fill slot="foot">1</dfw-fill><dfw-fill slot="foot">2</dfw-fill></dfw-use>',
      '<dfw-use c="t-box" title="t" title="u" TITLE="v"></dfw-use>',
      '<dfw-use c="t-cond"></dfw-use><dfw-use c="t-cond" a="1" mode="y" variant="two" size="md"></dfw-use><dfw-use c="t-cond" a="false" mode=""></dfw-use><dfw-use c="t-cond" a></dfw-use>',
      '<dfw-use c="t-cond" variant="three" size="xl" id="">x</dfw-use>',
      'line1\nline2\n<p>\n<dfw-use c="nope" id="n">kept <b>inner</b></dfw-use>\n<dfw-use>no name</dfw-use>\n<dfw-icon></dfw-icon>\n<dfw-icon name="nope" size="12"/>',
      '<dfw-icon name="search"></dfw-icon><dfw-icon name="x" size="16" label="Close" id="i.x" class="big" stroke-width="1.5"></dfw-icon><dfw-icon name="check" label=""/>',
      '<dfw-use c="t-loop"></dfw-use>',
      '<dfw-use c="t-deep" label="Deep &lt;label&gt;" id="d">slot <dfw-icon name="plus"></dfw-icon></dfw-use>',
      '<dfw-use c="t-void" src="{{base}}assets/a.png" id="v" class="k"></dfw-use><dfw-use c="t-self" id="s"/><dfw-use c="t-text" id="t">x</dfw-use>',
      '<dfw-use c="t-amp" t="a &amp; b &lt; c &apos;d&apos; &unknown;" id="amp" href="/x?y=1&amp;z=2" data-dfw-id="lost"></dfw-use>',
      '<DFW-USE c="t-box">upper</DFW-USE><dfw-use C="t-box">attr upper</dfw-use>',
      '<dfw-use c="button" variant="primary" icon="plus" kbd="Enter" id="go" disabled aria-busy="true">Go</dfw-use>',
      '<dfw-use c="app-shell" current="items"><dfw-fill slot="header"><dfw-use c="page-header" title="Inventory"><dfw-fill slot="actions"><dfw-use c="button">Add</dfw-use></dfw-fill></dfw-use></dfw-fill><main data-dfw-id="m">x</main></dfw-use>',
      '<html>\r\n<head>\r\n<dfw-head></dfw-head>\r\n</head>\r\n<body>\r\n<dfw-use c="button"\r\n  variant="nope"\r\n  id="crlf">Go</dfw-use>\r\n</body>\r\n</html>\r\n',
      'Tiếng Việt: <dfw-use c="t-box" title="Thêm vào giỏ — “đẹp”" id="vi">Nội dung 日本 🎉</dfw-use> kết thúc',
    ];
    for (const text of inputs) for (const base of [undefined, '../../', '/design/']) sameCompose(text, base);
  });

  test('head: with and without a closing tag, attributes, twice, missing', () => {
    for (const text of [
      '<head><dfw-head></dfw-head></head>',
      '<head>\n  <dfw-head>\n  </dfw-head>\n</head><dfw-use c="button" variant="x">a</dfw-use>',
      '<head><dfw-head/></head>',
      '<head><dfw-head data-x="1"> x</dfw-head></head>',
      '<dfw-head></dfw-head><dfw-head></dfw-head>',
      '<head></head>{{base}}x',
      '<dfw-head>\n\n\n</dfw-head>\n<dfw-icon name="zz"></dfw-icon>',
    ]) {
      for (const base of [undefined, '../', './']) sameCompose(text, base);
    }
  });

  test('hostile inputs both implementations read the same way', () => {
    const inputs = [
      // comments
      '<body>\n<!-- TODO <dfw-use c="button"> later -->\n<main>x</main>\n</body>',
      '<!-- a --><dfw-use c="button">a</dfw-use><!-- </dfw-use> --><!-- <dfw-icon name="x"> -->',
      '<dfw-use c="t-box">a<!-- </dfw-use> still inside --><dfw-fill slot="foot">f</dfw-fill></dfw-use>after',
      '<dfw-use c="t-box"><!-- <dfw-fill slot="foot">no</dfw-fill> -->body</dfw-use>',
      '<!-- unterminated <dfw-use c="button">x</dfw-use>',
      '<!--><dfw-use c="button">x</dfw-use>-->',
      // quotes
      '<dfw-use c="t-box" title="a > b" id="q1">x</dfw-use>',
      '<dfw-use c="t-box" title=\'a "b" > c\' id="q2">x</dfw-use>',
      '<dfw-use c="t-box" title="it\'s" id=\'q"3\'>x</dfw-use>',
      '<dfw-use c="t-box" title=a"b>x</dfw-use>',
      '<dfw-use c="t-box" title = "spaced"  id\n=\n"nl">x</dfw-use>',
      '<dfw-use c="t-box" title="unterminated>x</dfw-use><dfw-use c="button">later</dfw-use>',
      "<dfw-use c='t-box' title='unterminated>x</dfw-use>",
      '<dfw-use c="t-box" =oops ="x" a=`b` / >x</dfw-use>',
      // unclosed
      '<body>\n<dfw-use c="button">\n<main data-dfw-id="main"><h1>Title</h1></main>\n</body>',
      '<dfw-use c="button">a</dfw-use><dfw-use c="t-box">never closed <dfw-use c="button">b</dfw-use>',
      '<dfw-icon name="search"><dfw-use c="button">a</dfw-use>',
      '<dfw-use c="t-box"><dfw-fill slot="foot">never closed</dfw-use>',
      '<dfw-use c="t-box"><dfw-use c="button">inner never closed</dfw-use>',
      '<dfw-use c="button"',
      '<dfw-use',
      '<dfw-use c="button">x</dfw-use',
      '</dfw-use><dfw-use c="button">x</dfw-use></dfw-use>',
      // nesting
      `${'<dfw-use c="t-box" title="n">'.repeat(11)}core${'</dfw-use>'.repeat(11)}`,
      `${'<dfw-use c="t-box">'.repeat(14)}too deep${'</dfw-use>'.repeat(14)}`,
      `${'<div>'.repeat(300)}<dfw-use c="button">deep</dfw-use>${'</div>'.repeat(300)}`,
      '<dfw-use c="t-box"><dfw-use c="t-box"><dfw-fill slot="foot">inner</dfw-fill></dfw-use><dfw-fill slot="foot"><dfw-use c="t-box"><dfw-fill slot="foot">deepest</dfw-fill></dfw-use></dfw-fill></dfw-use>',
      // many
      Array.from(
        { length: 200 },
        (_, i) =>
          `<dfw-use c="button" variant="${['primary', 'ghost', 'nope'][i % 3]}" id="b.${i}">Button ${i}</dfw-use>${i % 7 ? '' : '\n'}`,
      ).join(''),
      Array.from(
        { length: 200 },
        (_, i) => `<dfw-icon name="${['search', 'x', 'missing'][i % 3]}" size="${12 + (i % 5)}"></dfw-icon>`,
      ).join('\n'),
      // odd text
      'a < b && c > d <dfw-use c="button">x</dfw-use> 1 << 2',
      '<!doctype html><?xml version="1.0"?><dfw-use c="button">x</dfw-use>',
      '<dfw-useless c="button">x</dfw-useless><dfw-icons name="x"></dfw-icons><dfw-use c="button">y</dfw-use>',
      '<dfw-use c="button">{{variant}} {{base}} {{ a==b?c }}</dfw-use>',
      '<dfw-use c="t-box" title="{{title}}">$& $1 $$ $`</dfw-use>',
      '',
      '﻿<dfw-use c="button">bom</dfw-use>',
    ];
    for (const text of inputs) for (const base of [undefined, '../../']) sameCompose(text, base);
  });

  test('inputs the tokenizer reads correctly where the regular expressions did not', () => {
    // Each case: input, what the native result must contain / not contain. Node differs on all.
    const cases = [
      [
        'a closing tag inside a quoted attribute does not close the instance',
        '<dfw-use c="t-box"><p title="</dfw-use>">x</p></dfw-use>',
        (r) => r.html.includes('<p title="</dfw-use>">x</p><footer>') && r.html.endsWith('</section>'),
      ],
      [
        'an instance inside a quoted attribute is not expanded',
        '<p title="<dfw-use c=\'button\'>x</dfw-use>">t</p>',
        (r) => r.instances.length === 0 && r.html.includes("<dfw-use c='button'>"),
      ],
      [
        'a fill inside a quoted attribute is not a fill',
        '<dfw-use c="t-box"><p title="<dfw-fill slot=\'foot\'>">x</p></dfw-use>',
        (r) => r.html.includes('<p title="<dfw-fill slot=\'foot\'>">x</p><footer></footer>'),
      ],
      [
        'an instance inside <script> is not expanded',
        '<script>const s = \'<dfw-use c="button">x</dfw-use>\';</script>',
        (r) => r.instances.length === 0,
      ],
      [
        'an icon inside <style> is not expanded',
        '<style>/* <dfw-icon name="nope"></dfw-icon> */</style>',
        (r) => r.issues.length === 0 && r.html.includes('<dfw-icon name="nope">'),
      ],
      [
        '<dfw-head> inside a comment is left alone',
        '<!-- <dfw-head></dfw-head> --><head><dfw-head></dfw-head></head>',
        (r) => r.html.startsWith('<!-- <dfw-head></dfw-head> --><head><link'),
        '../../',
      ],
      [
        'a slot inside a template comment is not filled',
        '<dfw-use c="t-comment">content</dfw-use>',
        (r) => r.html.includes('<!-- inner <dfw-slot></dfw-slot> -->content'),
      ],
      [
        'a longer element name does not nest in <dfw-use>',
        '<dfw-use c="t-box">a<dfw-use-x>b</dfw-use>c</dfw-use-x>',
        (r) => r.html.endsWith('c</dfw-use-x>') && r.issues.length === 0,
      ],
      [
        'a longer element name is not an instance',
        '<dfw-use-x c="t-box">x</dfw-use-x>',
        (r) => r.instances.length === 0 && r.html === '<dfw-use-x c="t-box">x</dfw-use-x>',
      ],
      [
        'attributes named like integers keep their place',
        '<dfw-use c="t-box" 2="b" 1="a" z="c">x</dfw-use>',
        (r) => r.html.startsWith('<section class="box" 2="b" 1="a" z="c"'),
      ],
    ];
    for (const [label, text, check, base] of cases) {
      // Node 0.5.2 read these wrongly, so the recorded result is the binary's own.
      const out = nativeCompose(text, base);
      assert.equal(out, expected(out), label);
      const r = JSON.parse(out);
      assert.ok(check(r), `${label}: ${JSON.stringify(r)}`);
    }
  });
});

// ---------------------------------------------------------------- colour

describe('colour', { skip: skipReason() }, () => {
  // Floats are compared at nine decimals (the binary prints them so): the last bits depend on the math library.
  const arg = (v) => (typeof v === 'string' ? v : JSON.stringify(v));
  /** Every call against its recorded result; returns the binary's results, parsed (inputs of later calls). */
  const check = (fn, calls) => {
    assert.ok(calls.length >= 1);
    // One process per batch would hide which input failed; the binary starts in a few ms.
    return calls.map((args) => {
      const r = native(['__debug', 'color', fn, ...args.map(arg)]);
      assert.equal(r.status, 0, r.stderr);
      assert.equal(r.stdout, expected(r.stdout), `${fn}(${args.map(arg).join(', ')})`);
      return JSON.parse(r.stdout);
    });
  };
  /** One result of the binary, parsed: the inputs of a case are made with it, as when recorded. */
  const nativeColor = (fn, ...args) => {
    const r = native(['__debug', 'color', fn, ...args.map(arg)]);
    assert.equal(r.status, 0, r.stderr);
    return JSON.parse(r.stdout);
  };
  // A deterministic spread of colours: every 3-digit shorthand step plus a pseudo-random walk.
  const hexes = [];
  let seed = 12345;
  const rnd = () => (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648;
  for (let i = 0; i < 230; i++)
    hexes.push(
      `#${Math.floor(rnd() * 0xffffff)
        .toString(16)
        .padStart(6, '0')}`,
    );
  hexes.push(
    '#000000',
    '#ffffff',
    '#fff',
    '#000',
    '#abc',
    '#abcd',
    '#aabbccdd',
    'ffffff',
    '#0b5cad',
    '#808080',
    '#010101',
    '#fefefe',
    '#ff0000',
    '#00ff00',
    '#0000ff',
    '#0a0a0a',
    '#0A0B0C',
    '#FFF',
    ' #123456',
    '#12',
    '#',
  );

  test('hexToRgb, hexToOklch, luminance over a few hundred colours', () => {
    check(
      'hexToRgb',
      hexes.map((h) => [h]),
    );
    check(
      'hexToOklch',
      hexes.map((h) => [h]),
    );
    check(
      'luminance',
      hexes.map((h) => [h]),
    );
  });

  test('rgbToHex and contrast', () => {
    const rgb = [];
    for (let i = 0; i < 200; i++) rgb.push([[rnd() * 1.2 - 0.1, rnd(), rnd()]]);
    rgb.push([[0, 0.5, 1]], [[0.49803, 0.50196, 0.00196]], [[1 / 510, 3 / 510, 5 / 510]], [[-1, 2, 0.999]]);
    check('rgbToHex', rgb);
    check(
      'contrast',
      hexes.map((h, i) => [h, hexes[(i * 7 + 3) % hexes.length]]),
    );
  });

  test('oklchToHex across lightness, chroma (in and out of gamut) and hue', () => {
    const calls = [];
    for (let l = 0; l <= 1.0001; l += 0.125)
      for (let c = 0; c <= 0.4; c += 0.08)
        for (let h = 0; h < 360; h += 45) calls.push([{ l: Number(l.toFixed(3)), c: Number(c.toFixed(2)), h }]);
    for (let i = 0; i < 60; i++) calls.push([{ l: rnd(), c: rnd() * 0.37, h: rnd() * 360 }]);
    assert.ok(calls.length > 400);
    check('oklchToHex', calls);
  });

  test('hueDistance, ramp, neutralRamp, stepFor', () => {
    const hd = [];
    for (let i = 0; i < 200; i++) hd.push([Number((rnd() * 720 - 180).toFixed(3)), Number((rnd() * 360).toFixed(3))]);
    check('hueDistance', hd);
    const ramps = [];
    for (let hue = 0; hue < 360; hue += 9)
      for (const chroma of [0.02, 0.08, 0.14, 0.2, 0.3]) ramps.push([{ hue, chroma, hueShift: hue % 4 ? 0 : (hue % 40) - 20 }]);
    check('ramp', ramps);
    const neutrals = [];
    for (let hue = 0; hue < 360; hue += 4.5) for (const tint of [0.003, 0.01, 0.02]) neutrals.push([{ hue, tint }]);
    check('neutralRamp', neutrals);
    const steps = [];
    for (let hue = 0; hue < 360; hue += 24) {
      const r = nativeColor('ramp', { hue, chroma: 0.14 });
      for (const min of [3, 4.5, 7, 13, 30])
        for (const bg of ['#ffffff', r[50], r[900]]) for (const from of ['light', 'dark']) steps.push([r, bg, min, { from }]);
    }
    assert.ok(steps.length > 400);
    check('stepFor', steps);
  });

  test('buildPalette, auditColors, typeScale, auditTypeScale', () => {
    const palettes = [];
    for (let hue = 0; hue < 360; hue += 15) {
      for (const [k, extra] of [
        [0, {}],
        [1, { dark: true }],
        [2, { accentFrom: 'neutral', warmSurface: true }],
        [3, { signature: { harmony: 'triadic' }, accentFrom: 'signature' }],
        [
          4,
          {
            signature: { hue: (hue + 95) % 360, chroma: 0.17, hueShift: 6 },
            onBrandSignature: true,
            accentStep: 700,
            brandStep: 900,
            signatureStep: 500,
          },
        ],
        [5, { signature: { harmony: 'complementary', chroma: 0.1 }, dark: true }],
        [6, { accentStep: 650 }],
        [7, { neutral: { tint: 0.02 } }],
      ]) {
        palettes.push([
          {
            brand: { hue, chroma: 0.06 + (k % 4) * 0.05, hueShift: k === 4 ? -8 : undefined },
            neutral: { hue: (hue + 30 * k) % 360, tint: 0.004 + k * 0.002 },
            ...extra,
          },
          k % 2 ? { brand: 'son', neutral: 'giay', signature: 'kim' } : {},
        ]);
      }
    }
    for (const hex of hexes.slice(0, 40)) palettes.push([{ brand: { hex } }]);
    palettes.push(
      [
        { brand: { hex: '#0b5cad', chroma: 0.5 }, neutral: {} },
        { brand: 'a', neutral: 'a' },
      ],
      [{ brand: { hue: 10, chroma: 0.1 } }, { brand: '2', neutral: '1', signature: '' }],
      [{ brand: { hue: 400, chroma: -1 }, neutral: null, signature: null }],
    );
    assert.ok(palettes.length > 200);
    const built = palettes.filter((p) => p[0].accentStep !== 650);
    const builtPalettes = check('buildPalette', built);
    const resolve = (pal) =>
      Object.fromEntries(
        Object.entries(pal.roles).map(([k, v]) => [
          k,
          v.startsWith('#') ? v : pal.ramps[v.slice(0, v.lastIndexOf('-'))][v.slice(v.lastIndexOf('-') + 1)],
        ]),
      );
    const audits = [];
    for (const [i, p] of built.entries()) {
      if (p[0].accentStep) continue;
      const pal = builtPalettes[i];
      audits.push(
        [{ roles: resolve(pal), status: pal.status }],
        [{ roles: resolve(pal), status: pal.status, onFill: '#111518' }, { exceptions: ['contrast', 'hue-budget'] }],
      );
    }
    audits.push(
      [
        {
          roles: {
            canvas: '#f3f5f8',
            surface: '#ffffff',
            'surface-sunken': '#eef1f5',
            rule: '#dde2e8',
            border: '#7c8590',
            muted: '#5a5048',
            ink: '#2a2118',
            accent: '#1d4ed8',
            'on-accent': '#ffffff',
            'accent-soft': '#e8effd',
            'accent-text': '#1e40af',
            focus: '#1d4ed8',
          },
          status: {},
        },
      ],
      [
        {
          roles: { accent: '#c00', canvas: '#fc9', brand: '#b00', signature: '#0f0', a: '#00f', b: '#0c0', c: '#cc0' },
          status: { err: '#d00', ok: '#c40', 'err-text': '#f88', 'err-tint': '#fee' },
        },
      ],
    );
    assert.ok(audits.length > 300);
    check('auditColors', audits);
    const scales = [];
    for (const base of [12, 13, 14, 15, 16, 17, 18, 20])
      for (const ratio of [1.067, 1.125, 1.2, 1.25, 1.333, 1.414, 1.5, 1.618])
        for (const displayRatio of [1.2, 1.333, 1.5])
          for (const displayFrom of [undefined, 28, 0]) scales.push([{ base, ratio, displayRatio, displayFrom }]);
    scales.push([{}], [{ base: 10, ratio: 1.25 }], [{ base: 2.5, ratio: 2 }]);
    assert.ok(scales.length > 300);
    const builtScales = check('typeScale', scales);
    const typeAudits = [];
    for (const [i, [opts]] of scales.slice(0, 220).entries()) {
      const ts = builtScales[i];
      typeAudits.push([
        { ui: Object.values(ts.ui), display: Object.values(ts.display) },
        { body: ts.base, surface: opts.base % 2 ? 'marketing' : 'product' },
      ]);
    }
    typeAudits.push(
      [[12, 13, 14, 15, 16]],
      [{ ui: [12, 13, 14, 15, 16], display: [22, 26, 34, 40] }, { body: 16 }],
      [{ ui: [16], display: [20, 30, 45, 67.5] }],
      [{ ui: [] }],
      [[16, 16, 24.5, 8]],
    );
    check('auditTypeScale', typeAudits);
  });
});
