// Cross-implementation harness (rust-core): run one dfw command line against the Node CLI and
// the native binary, each on its own copy of a fixture project, and compare what a user can
// observe: exit code, stdout and every file written under .dfw/ and design/.
// These tests exist only while both implementations exist; afterwards they become golden tests.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
/** The clock both implementations see (`DFW_NOW`), so written timestamps are equal. */
export const NOW = '2026-01-02T03:04:05.678Z';
const NODE_CLI = path.join(REPO, 'core', 'bin', 'dfw.mjs');
const DEFAULT_PROJECT = path.join(REPO, 'tests', 'fixtures', 'penpot', 'project');
const WATCHED = ['.dfw', 'design'];

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

/** Path of the native binary: `DFW_BIN`, else the debug build. */
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
  const why = `no native binary at ${bin}: build it (cd native && cargo build) or set DFW_BIN`;
  if (process.env.CI || process.env.DFW_REQUIRE_BIN === '1') throw new Error(`${why} (the binary is required: CI or DFW_REQUIRE_BIN=1 is set)`);
  return why;
}

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

function runOne(impl, args, opts) {
  const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), `dfw-conformance-${impl}-`)));
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
    const [cmd, argv] = impl === 'node' ? [process.execPath, [NODE_CLI, ...args]] : [nativeBinary(), args];
    const r = spawnSync(cmd, argv, { cwd: root, env, input: opts.stdin ?? '', encoding: 'utf8', timeout: opts.timeout ?? 60_000 });
    if (r.error) throw r.error;

    // Temp paths differ per run; JSON output carries them escaped (Windows backslashes).
    const escaped = (p) => JSON.stringify(p).slice(1, -1);
    const normalize = (text) => {
      let t = text;
      for (const [from, to] of [[root, '<root>'], [tmp, '<tmp>']]) t = t.split(escaped(from)).join(to).split(from).join(to);
      // The one line that names the runtime: "dfw 0.5.1 (node 22.1.0)" / "(binary)" / "(native)".
      t = t.replace(/^(dfw \S+) \((?:node [^)]+|binary|native)\)$/m, '$1 (<runtime>)');
      return opts.normalize ? opts.normalize(t) : t;
    };
    const files = {};
    const after = snapshot(root);
    for (const [rel, buf] of after) if (!before.get(rel)?.equals(buf)) files[rel] = normalize(printable(buf));
    for (const rel of before.keys()) if (!after.has(rel)) files[rel] = null; // deleted
    return { code: r.status, stdout: normalize(r.stdout), stderr: normalize(r.stderr), files: Object.fromEntries(Object.entries(files).sort(([a], [b]) => (a < b ? -1 : 1))) };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

/**
 * Run `dfw <args>` with both implementations.
 * opts: stdin (text), project (fixture folder to copy; default tests/fixtures/penpot/project),
 * state (object for .dfw/state.json; default MINIMAL_STATE; null = no workflow), setup(root)
 * (prepare the copy before the run), env (extra variables), normalize(text) (mask ids or other
 * values that legitimately differ per run), timeout (ms).
 * Returns { node, native }, each { code, stdout, stderr, files }; files maps a relative path under
 * .dfw/ or design/ to its new content (null = deleted) for everything the command changed.
 */
export function both(args, opts = {}) {
  return { node: runOne('node', args, opts), native: runOne('native', args, opts) };
}

function firstDifference(label, a, b) {
  const [x, y] = [String(a).split('\n'), String(b).split('\n')];
  let i = 0;
  while (i < x.length && i < y.length && x[i] === y[i]) i++;
  const show = (lines, mark) => lines.slice(i, i + 6).map((l) => `    ${mark} ${JSON.stringify(l)}`).join('\n') || `    ${mark} (nothing)`;
  return `${label}: first difference at line ${i + 1}\n${show(x, 'node  ')}\n${show(y, 'native')}`;
}

/** Fail with a readable diff unless exit code, stdout and written files are the same. */
export function assertSame({ node, native }, { stderr = false } = {}) {
  const problems = [];
  if (node.code !== native.code) problems.push(`exit code: node ${node.code}, native ${native.code}\n    node stderr:   ${JSON.stringify(node.stderr.slice(0, 400))}\n    native stderr: ${JSON.stringify(native.stderr.slice(0, 400))}`);
  if (node.stdout !== native.stdout) problems.push(firstDifference('stdout', node.stdout, native.stdout));
  if (stderr && node.stderr !== native.stderr) problems.push(firstDifference('stderr', node.stderr, native.stderr));
  for (const rel of new Set([...Object.keys(node.files), ...Object.keys(native.files)])) {
    const [a, b] = [node.files[rel], native.files[rel]];
    if (a === b) continue;
    const state = (v) => (v === undefined ? 'not written' : v === null ? 'deleted' : 'written');
    if (typeof a !== 'string' || typeof b !== 'string') problems.push(`file ${rel}: node ${state(a)}, native ${state(b)}`);
    else problems.push(firstDifference(`file ${rel}`, a, b));
  }
  if (problems.length) assert.fail(`node and native differ:\n${problems.join('\n')}`);
}
