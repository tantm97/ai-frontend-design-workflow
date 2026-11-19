#!/usr/bin/env node
// Check benchmark: screens with one seeded, labelled defect each (tests/bench/fixtures, "<!-- expect: ... -->")
// plus clean screens. Measures, per rule and per checker: detection (seeded defects found), false positives
// (tracked rules firing where they were not seeded) and time. Rendered checks need Chrome; axe needs
// "dfw setup --a11y"; missing ones are reported as not measured, never as passing.
//   npm run bench                 run and compare with tests/bench/baseline.json
//   npm run bench -- --save       also store this run as the new baseline
//   npm run bench -- --bin <dfw>  benchmark another dfw binary (e.g. the previous release); default
//                                 DFW_BIN, else native/target/debug/dfw
// Never edit a fixture to make a checker pass; fix the checker or add a dated note in the fixture.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, '..', '..');
const BIN = process.argv.includes('--bin')
  ? path.resolve(process.argv[process.argv.indexOf('--bin') + 1])
  : process.env.DFW_BIN || path.join(REPO, 'native', 'target', 'debug', process.platform === 'win32' ? 'dfw.exe' : 'dfw');
/** `dfw <args>` on the benchmark project; its stdout parsed as JSON, or null when it did not run. */
const dfwJson = (args) => {
  try {
    return JSON.parse(
      execFileSync(BIN, [...args, '--root', root], {
        cwd: root,
        env: { ...process.env, CLAUDE_PROJECT_DIR: '', DFW_TELEMETRY: 'off' },
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'ignore'],
        maxBuffer: 64 << 20,
      }),
    );
  } catch (e) {
    // lint and layout exit 2 when they found errors; their JSON is still the result.
    try {
      return JSON.parse(e.stdout);
    } catch {
      return null;
    }
  }
};

const fixtures = fs
  .readdirSync(path.join(HERE, 'fixtures'))
  .filter((f) => f.endsWith('.html'))
  .sort();
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-bench-'));
execFileSync(BIN, ['init', '--root', root], { cwd: root, env: { ...process.env, CLAUDE_PROJECT_DIR: '', DFW_TELEMETRY: 'off' } });
fs.mkdirSync(path.join(root, 'design/screens/bench'), { recursive: true });
const expect = new Map();
const states = new Map();
for (const f of fixtures) {
  const html = fs.readFileSync(path.join(HERE, 'fixtures', f), 'utf8');
  fs.writeFileSync(path.join(root, 'design/screens/bench', f), html);
  const m = /<!-- expect: ([^>]*?) -->/.exec(html);
  expect.set(`design/screens/bench/${f}`, m && m[1] !== 'none' ? m[1].split(',').map((x) => x.trim()) : []);
  states.set(
    f,
    (/<!-- states: ([^>]*?) -->/.exec(html)?.[1] || '')
      .split(',')
      .map((x) => x.trim())
      .filter(Boolean),
  );
}
const manifestFile = path.join(root, 'design/design.json');
const manifest = JSON.parse(fs.readFileSync(manifestFile, 'utf8'));
manifest.screens = fixtures.map((f) => ({
  id: f.replace('.html', ''),
  title: f,
  ...(states.get(f).length ? { states: states.get(f) } : {}),
  variants: [{ id: 'a', file: `screens/bench/${f}` }],
}));
fs.writeFileSync(manifestFile, JSON.stringify(manifest, null, 2));

const checkers = [];
const time = async (name, fn) => {
  const t = Date.now();
  const r = await fn();
  checkers.push({ name, status: r.status, ms: Date.now() - t, findings: r.findings });
};
const missing = { status: 'not in this version', findings: [] };
await time('lint', () => {
  const r = dfwJson(['lint', '--json'])?.results?.find((x) => x.tool === 'dfw-lint');
  return r ? { status: r.status, findings: r.findings } : missing;
});
await time('layout', () => dfwJson(['layout', 'check', '--json', '--no-system']) || missing);
await time('axe', () => dfwJson(['__debug', 'a11y']) || missing);
const found = new Map();
for (const c of checkers)
  for (const f of c.findings || []) {
    if (!found.has(f.file)) found.set(f.file, new Set());
    found.get(f.file).add(f.rule);
  }

const tracked = new Set([...expect.values()].flat());
const ran = new Set(checkers.filter((c) => c.status === 'ran').map((c) => c.name));
const checkerOf = (rule) =>
  rule.startsWith('axe-') ? 'axe' : ['h-overflow', 'overlap', 'clipped', 'reflow', 'anchor', 'void'].includes(rule) ? 'layout' : 'lint';
const perRule = {};
let falsePositives = 0;
const fpList = [];
for (const [file, rules] of expect) {
  const got = found.get(file) || new Set();
  for (const r of rules) {
    perRule[r] = perRule[r] || { seeded: 0, found: 0, measured: ran.has(checkerOf(r)) };
    perRule[r].seeded++;
    if (got.has(r)) perRule[r].found++;
  }
  for (const r of got)
    if (tracked.has(r) && !rules.includes(r)) {
      falsePositives++;
      fpList.push(`${path.basename(file)}: ${r}`);
    }
}
const measured = Object.values(perRule).filter((x) => x.measured);
const result = {
  at: new Date().toISOString(),
  version: execFileSync(BIN, ['version'], { encoding: 'utf8' }).split(' ')[1] || '?',
  fixtures: fixtures.length,
  detection:
    measured.reduce((n, x) => n + x.found, 0) /
    Math.max(
      1,
      measured.reduce((n, x) => n + x.seeded, 0),
    ),
  coverage:
    Object.values(perRule).reduce((n, x) => n + x.found, 0) /
    Math.max(
      1,
      Object.values(perRule).reduce((n, x) => n + x.seeded, 0),
    ),
  falsePositives,
  checkers: checkers.map(({ name, status, ms, findings }) => ({ name, status, ms, findings: findings?.length || 0 })),
  rules: perRule,
};
fs.rmSync(root, { recursive: true, force: true });

const pct = (x) => `${Math.round(x * 100)}%`;
console.log(`dfw check benchmark (v${result.version}, ${result.fixtures} fixtures)`);
for (const c of result.checkers)
  console.log(`  ${c.name.padEnd(7)} ${c.status.padEnd(13)} ${String(c.ms).padStart(6)} ms  ${c.findings} finding(s)`);
console.log(
  `  detection ${pct(result.detection)} of measured seeded defects · coverage ${pct(result.coverage)} of all seeded defects · false positives ${falsePositives}`,
);
for (const [r, x] of Object.entries(perRule).sort())
  console.log(`  ${x.measured ? (x.found === x.seeded ? 'ok  ' : 'MISS') : 'n/m '} ${r.padEnd(24)} ${x.found}/${x.seeded}`);
if (fpList.length) console.log(`  false positives: ${fpList.join('; ')}`);

const baseFile = path.join(HERE, 'baseline.json');
if (fs.existsSync(baseFile)) {
  const b = JSON.parse(fs.readFileSync(baseFile, 'utf8'));
  console.log(
    `  vs baseline v${b.version}: detection ${pct(b.detection)} -> ${pct(result.detection)}, false positives ${b.falsePositives} -> ${falsePositives}`,
  );
  if (result.detection < b.detection || falsePositives > b.falsePositives) process.exitCode = 1;
}
if (process.argv.includes('--save')) fs.writeFileSync(baseFile, `${JSON.stringify(result, null, 2)}\n`);
