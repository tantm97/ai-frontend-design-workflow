// Shared helpers for the dfw CLI, hooks and studio. Node >= 20, no dependencies.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

export const PLUGIN_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const MARKETPLACE_NAME = 'dfw-marketplace';

export function readJson(file, fallback = undefined) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (err) {
    if (fallback !== undefined && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) return fallback;
    if (fallback !== undefined && err instanceof SyntaxError) {
      throw new Error(`Invalid JSON in ${file}: ${err.message}`);
    }
    throw err;
  }
}

export function writeJson(file, data) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(data, null, 2) + '\n');
  fs.renameSync(tmp, file);
}

export function appendJsonl(file, entry) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.appendFileSync(file, JSON.stringify(entry) + '\n');
}

export function readJsonl(file) {
  if (!fs.existsSync(file)) return [];
  return fs
    .readFileSync(file, 'utf8')
    .split('\n')
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l));
}

export function nowIso() {
  return new Date().toISOString();
}

export function sha256(buf) {
  return crypto.createHash('sha256').update(buf).digest('hex');
}

/** Global dfw config directory (survives plugin uninstall). */
export function globalDir() {
  if (process.env.DFW_HOME) return process.env.DFW_HOME;
  const xdg = process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config');
  return path.join(xdg, 'dfw');
}

/** Claude Code config directory (respects CLAUDE_CONFIG_DIR). */
export function claudeDir() {
  return process.env.CLAUDE_CONFIG_DIR || path.join(os.homedir(), '.claude');
}

/**
 * Find the project root: the nearest ancestor containing .dfw/, else the
 * nearest containing .git, else the start directory.
 */
export function findProjectRoot(start = process.env.CLAUDE_PROJECT_DIR || process.cwd()) {
  let dir = path.resolve(start);
  let gitRoot = null;
  for (;;) {
    if (fs.existsSync(path.join(dir, '.dfw', 'state.json'))) return dir;
    if (!gitRoot && fs.existsSync(path.join(dir, '.git'))) gitRoot = dir;
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return gitRoot || path.resolve(start);
}

export function projectPaths(root) {
  return {
    root,
    dfw: path.join(root, '.dfw'),
    state: path.join(root, '.dfw', 'state.json'),
    config: path.join(root, '.dfw', 'config.json'),
    configLocal: path.join(root, '.dfw', 'config.local.json'),
    decisions: path.join(root, '.dfw', 'decisions.jsonl'),
    reviews: path.join(root, '.dfw', 'reviews'),
    stack: path.join(root, '.dfw', 'stack.json'),
    studio: path.join(root, '.dfw', 'studio.json'),
    checks: path.join(root, '.dfw', 'checks'),
    design: path.join(root, 'design'),
    manifest: path.join(root, 'design', 'design.json'),
    tokens: path.join(root, 'design', 'tokens'),
    approved: path.join(root, 'design', 'approved'),
    handoff: path.join(root, 'design', 'handoff'),
  };
}

/** Recursively list files under dir (relative paths, posix separators). */
export function listFiles(dir, { exclude = [] } = {}) {
  const out = [];
  if (!fs.existsSync(dir)) return out;
  const walk = (abs, rel) => {
    for (const ent of fs.readdirSync(abs, { withFileTypes: true })) {
      const r = rel ? `${rel}/${ent.name}` : ent.name;
      if (exclude.some((e) => r === e || r.startsWith(`${e}/`))) continue;
      if (ent.isDirectory()) walk(path.join(abs, ent.name), r);
      else if (ent.isFile()) out.push(r);
    }
  };
  walk(dir, '');
  return out.sort();
}

export function copyDir(src, dest, { exclude = [] } = {}) {
  for (const rel of listFiles(src, { exclude })) {
    const to = path.join(dest, rel);
    fs.mkdirSync(path.dirname(to), { recursive: true });
    fs.copyFileSync(path.join(src, rel), to);
    fs.chmodSync(to, fs.statSync(path.join(src, rel)).mode);
  }
}

/** Minimal glob matcher supporting **, * and ? on posix-style paths. */
export function globToRegExp(glob) {
  let re = '';
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === '*') {
      if (glob[i + 1] === '*') {
        i++;
        if (glob[i + 1] === '/') {
          i++;
          re += '(?:.*/)?';
        } else re += '.*';
      } else re += '[^/]*';
    } else if (c === '?') re += '[^/]';
    else re += c.replace(/[.+^${}()|[\]\\]/g, '\\$&');
  }
  return new RegExp(`^${re}$`);
}

export function matchesAny(rel, globs = []) {
  return globs.some((g) => globToRegExp(g).test(rel));
}

export function toPosix(p) {
  return p.split(path.sep).join('/');
}

/** Deep merge for config layering: objects merge, arrays and scalars replace. */
export function deepMerge(base, over) {
  if (over === undefined) return base;
  if (Array.isArray(over) || typeof over !== 'object' || over === null) return over;
  const out = { ...(typeof base === 'object' && base && !Array.isArray(base) ? base : {}) };
  for (const [k, v] of Object.entries(over)) out[k] = deepMerge(out[k], v);
  return out;
}

export function getPath(obj, dotted) {
  return dotted.split('.').reduce((o, k) => (o == null ? undefined : o[k]), obj);
}

export function setPath(obj, dotted, value) {
  const keys = dotted.split('.');
  let o = obj;
  for (const k of keys.slice(0, -1)) {
    if (typeof o[k] !== 'object' || o[k] === null) o[k] = {};
    o = o[k];
  }
  o[keys.at(-1)] = value;
  return obj;
}

/** Parse a CLI value: JSON when it parses, else the raw string. */
export function parseValue(v) {
  try {
    return JSON.parse(v);
  } catch {
    return v;
  }
}

export function parseArgs(argv) {
  const args = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith('--')) {
      const [k, inline] = a.slice(2).split(/=(.*)/s);
      if (inline !== undefined) args[k] = inline;
      else if (argv[i + 1] !== undefined && !argv[i + 1].startsWith('--')) args[k] = argv[++i];
      else args[k] = true;
    } else args._.push(a);
  }
  return args;
}

export async function readStdin() {
  if (process.stdin.isTTY) return '';
  const chunks = [];
  for await (const c of process.stdin) chunks.push(c);
  return Buffer.concat(chunks).toString('utf8');
}

export class DfwError extends Error {
  constructor(message, code = 1) {
    super(message);
    this.exitCode = code;
  }
}
