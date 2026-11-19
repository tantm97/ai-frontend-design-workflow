// Golden tests of the dfw binary (rust-core design decision 4). Each case runs the binary on a
// copy of a fixture project and compares what a user can observe (exit code, stdout, stderr and
// every file written under .dfw/ and design/) with the result recorded in tests/golden/<suite>/.
// The recorded results were made by the Node CLI, the reference implementation of 0.5.2, while it
// still existed; they are the contract now.
//
//   node --test tests/golden/                     check the binary against the recorded results
//   DFW_GOLDEN_UPDATE=1 node --test tests/golden/  record what the binary does now (for a behaviour
//                                                  change: review the diff of tests/golden/ by hand)
//
// The binary is DFW_BIN, else native/target/debug/dfw. Without a binary the suites skip with the
// reason, unless CI or DFW_REQUIRE_BIN=1 is set: then it is an error.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test as nodeTest } from 'node:test';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
export const GOLDEN = path.join(REPO, 'tests', 'golden');
/** The clock the binary sees (`DFW_NOW`), so written timestamps are the recorded ones. */
export const NOW = '2026-01-02T03:04:05.678Z';
const DEFAULT_PROJECT = path.join(REPO, 'tests', 'fixtures', 'penpot', 'project');
const WATCHED = ['.dfw', 'design'];
const UPDATE = process.env.DFW_GOLDEN_UPDATE === '1';

/** What `dfw init --product Fixture` writes at NOW: the default `.dfw/state.json` of a run. */
export const MINIMAL_STATE = {
  schema: 1,
  product: 'Fixture',
  mode: 'new-product',
  phase: 'discovery',
  active: true,
  createdAt: NOW,
  updatedAt: NOW,
  design: { version: 1, status: 'draft', locked: false, approved: [] },
  gates: { G1: null, DS: null, G2: null, G3: null, G4: null },
  waived: {},
  history: [{ at: NOW, event: 'init', mode: 'new-product' }],
};

/** Path of the binary: `DFW_BIN`, else the debug build. */
export function nativeBinary() {
  const exe = process.platform === 'win32' ? 'dfw.exe' : 'dfw';
  return path.resolve(process.env.DFW_BIN || path.join(REPO, 'native', 'target', 'debug', exe));
}

/**
 * A reason to skip (no binary), or false. Pass it as `{ skip: skipReason() }` to `describe`.
 * When the binary is required (`CI` or `DFW_REQUIRE_BIN=1`), a missing binary is an error
 * instead: a green run must mean the cases ran.
 */
export function skipReason() {
  const bin = nativeBinary();
  if (fs.existsSync(bin)) return false;
  const why = `no dfw binary at ${bin}: build it (cd native && cargo build) or set DFW_BIN`;
  if (process.env.CI || process.env.DFW_REQUIRE_BIN === '1') throw new Error(`${why} (the binary is required: CI or DFW_REQUIRE_BIN=1 is set)`);
  return why;
}

/* ---------------------------------------------------------------- recorded results per test */

/** The test that is running: its recorded results and how many were used. */
let current = null;

function caseFile(suite, fullName) {
  const slug = fullName
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 72)
    .replace(/-$/, '');
  const hash = crypto.createHash('sha256').update(fullName).digest('hex').slice(0, 8);
  return path.join(GOLDEN, suite, `${slug}-${hash}.json`);
}

/**
 * `test` of node:test bound to the recorded results of one suite (`tests/golden/<suite>/`). Tests
 * of a file run one after the other, so one "current" test is enough.
 */
export function suite(name) {
  return (title, ...rest) => {
    const fn = rest.pop();
    const opts = rest[0] ?? {};
    return nodeTest(title, opts, async (t) => {
      const file = caseFile(name, t.fullName ?? title);
      const recorded = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')).results : null;
      current = { file, fullName: t.fullName ?? title, results: UPDATE ? [] : recorded, next: 0 };
      if (!UPDATE && !recorded) assert.fail(`no recorded results for "${current.fullName}" (${path.relative(REPO, file)}): run once with DFW_GOLDEN_UPDATE=1 and review the new file`);
      try {
        await fn(t);
        if (UPDATE) {
          fs.mkdirSync(path.dirname(file), { recursive: true });
          fs.writeFileSync(file, `${JSON.stringify({ test: current.fullName, results: current.results }, null, 2)}\n`);
        } else if (current.next !== current.results.length) {
          assert.fail(`"${current.fullName}" used ${current.next} of its ${current.results.length} recorded results: the test changed; run it with DFW_GOLDEN_UPDATE=1 and review the diff`);
        }
      } finally {
        current = null;
      }
    });
  };
}

/** A value as JSON stores it; refuses what JSON would change (undefined, NaN, buffers...). */
function asJson(value) {
  const copy = JSON.parse(JSON.stringify(value) ?? 'null');
  assert.deepEqual(copy, value, 'a recorded result must be plain JSON (no undefined, NaN, Buffer or class instance)');
  return copy;
}

/**
 * The next recorded result of this test. With DFW_GOLDEN_UPDATE=1, `actual` (what the binary
 * did, in the same shape) is recorded instead and returned.
 */
export function expected(actual) {
  if (!current) throw new Error('expected() outside a test made by suite()');
  if (UPDATE) {
    current.results.push(asJson(actual));
    return actual;
  }
  if (current.next >= current.results.length) assert.fail(`"${current.fullName}" has no recorded result #${current.next + 1}: the test changed; run it with DFW_GOLDEN_UPDATE=1 and review the diff`);
  return current.results[current.next++];
}

/* ---------------------------------------------------------------- one command line */

function snapshot(root) {
  const files = new Map();
  const walk = (dir) => {
    for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
      const abs = path.join(dir, ent.name);
      if (ent.isDirectory()) walk(abs);
      else if (ent.isFile()) files.set(path.relative(root, abs).split(path.sep).join('/'), fs.readFileSync(abs));
    }
  };
  for (const top of WATCHED) if (fs.existsSync(path.join(root, top))) walk(path.join(root, top));
  return files;
}

/** Text as it is, anything else as base64, so a binary file still compares and prints. */
function printable(buf) {
  const text = buf.toString('utf8');
  return Buffer.from(text, 'utf8').equals(buf) ? text : `base64:${buf.toString('base64')}`;
}

/** The version this checkout builds (core/manifest.json), masked in every output. */
const VERSION = JSON.parse(fs.readFileSync(path.join(REPO, 'core', 'manifest.json'), 'utf8')).version;

/**
 * Run `dfw <args>` with the given program on a copy of a fixture project; what a user observes,
 * with the temporary paths, the runtime name and the version masked.
 * opts: stdin (text), project (fixture folder to copy; default tests/fixtures/penpot/project),
 * state (object for .dfw/state.json; default MINIMAL_STATE; null = no workflow), setup(root)
 * (prepare the copy before the run), env (extra variables), normalize(text) (mask ids or other
 * values that legitimately differ per run; applied to file names as well), timeout (ms).
 */
export function runWith(cmd, cmdArgs, args, opts = {}) {
  const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-')));
  const root = path.join(tmp, 'project');
  try {
    fs.cpSync(opts.project || DEFAULT_PROJECT, root, { recursive: true });
    if (opts.state !== null) {
      fs.mkdirSync(path.join(root, '.dfw'), { recursive: true });
      fs.writeFileSync(path.join(root, '.dfw', 'state.json'), JSON.stringify(opts.state || MINIMAL_STATE, null, 2) + '\n');
    }
    opts.setup?.(root);
    const before = snapshot(root);
    const env = { ...process.env };
    for (const k of ['DFW_NATIVE', 'DFW_FORWARDED', 'DFW_NO_BINARY', 'DFW_VERSION', 'XDG_CONFIG_HOME', 'CLAUDE_CONFIG_DIR']) delete env[k];
    Object.assign(env, {
      DFW_HOME: path.join(tmp, 'home'),
      DFW_NOW: NOW,
      DFW_PROJECT_DIR: root,
      CLAUDE_PROJECT_DIR: root,
      DFW_PLUGIN_ROOT: path.join(REPO, 'core'),
      ...opts.env,
    });
    const r = spawnSync(cmd, [...cmdArgs, ...args], { cwd: root, env, input: opts.stdin ?? '', encoding: 'utf8', timeout: opts.timeout ?? 180_000 });
    if (r.error) throw r.error;

    // Temp paths differ per run and the checkout differs per machine; JSON output carries them
    // escaped (Windows backslashes).
    const escaped = (p) => JSON.stringify(p).slice(1, -1);
    const normalize = (text) => {
      let t = text;
      for (const [from, to] of [[root, '<root>'], [tmp, '<tmp>'], [REPO, '<repo>']]) t = t.split(escaped(from)).join(to).split(from).join(to);
      // The one line that names the runtime: "dfw 0.5.1 (node 22.1.0)" / "(binary)" / "(native)".
      t = t.replace(/^(dfw \S+) \((?:node [^)]+|binary|native)\)$/m, '$1 (<runtime>)');
      t = maskVersion(t);
      return opts.normalize ? opts.normalize(t) : t;
    };
    // opts.normalize masks file names too (a review session is named after the real second).
    const name = (rel) => (opts.normalize ? opts.normalize(rel) : rel);
    const files = {};
    const after = snapshot(root);
    for (const [rel, buf] of after) if (!before.get(rel)?.equals(buf)) files[name(rel)] = normalize(printable(buf));
    for (const rel of before.keys()) if (!after.has(rel)) files[name(rel)] = null; // deleted
    return { code: r.status, stdout: normalize(r.stdout), stderr: normalize(r.stderr), files: Object.fromEntries(Object.entries(files).sort(([a], [b]) => (a < b ? -1 : 1))) };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

/** The release version in output ("dfw 0.6.0", `"version": "0.6.0"`) as `<version>`. */
export function maskVersion(text) {
  return String(text)
    .split(`dfw ${VERSION}`)
    .join('dfw <version>')
    .split(`"version": "${VERSION}"`)
    .join('"version": "<version>"')
    .split(`"version":"${VERSION}"`)
    .join('"version":"<version>"')
    .split(`"service.version","value":{"stringValue":"${VERSION}"}`)
    .join('"service.version","value":{"stringValue":"<version>"}');
}

/**
 * Run `dfw <args>` with the binary (options: see `runWith`). Returns { expected, native }, each
 * { code, stdout, stderr, files }; files maps a relative path under .dfw/ or design/ to its new
 * content (null = deleted) for everything the command changed.
 */
export function run(args, opts = {}) {
  const native = runWith(nativeBinary(), [], args, opts);
  return { expected: expected(native), native };
}

function firstDifference(label, a, b) {
  const [x, y] = [String(a).split('\n'), String(b).split('\n')];
  let i = 0;
  while (i < x.length && i < y.length && x[i] === y[i]) i++;
  const show = (lines, mark) => lines.slice(i, i + 6).map((l) => `    ${mark} ${JSON.stringify(l)}`).join('\n') || `    ${mark} (nothing)`;
  return `${label}: first difference at line ${i + 1}\n${show(x, 'expected')}\n${show(y, 'binary  ')}`;
}

/** Fail with a readable diff unless exit code, stdout and written files are the recorded ones. */
export function assertSame({ expected: want, native }, { stderr = false } = {}) {
  const problems = [];
  if (want.code !== native.code) problems.push(`exit code: expected ${want.code}, binary ${native.code}\n    expected stderr: ${JSON.stringify(want.stderr.slice(0, 400))}\n    binary stderr:   ${JSON.stringify(native.stderr.slice(0, 400))}`);
  if (want.stdout !== native.stdout) problems.push(firstDifference('stdout', want.stdout, native.stdout));
  if (stderr && want.stderr !== native.stderr) problems.push(firstDifference('stderr', want.stderr, native.stderr));
  for (const rel of new Set([...Object.keys(want.files), ...Object.keys(native.files)])) {
    const [a, b] = [want.files[rel], native.files[rel]];
    if (a === b) continue;
    const state = (v) => (v === undefined ? 'not written' : v === null ? 'deleted' : 'written');
    if (typeof a !== 'string' || typeof b !== 'string') problems.push(`file ${rel}: expected ${state(a)}, binary ${state(b)}`);
    else problems.push(firstDifference(`file ${rel}`, a, b));
  }
  if (problems.length) assert.fail(`the binary differs from the recorded result:\n${problems.join('\n')}`);
}
