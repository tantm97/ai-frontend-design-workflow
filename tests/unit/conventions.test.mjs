// docs/conventions/git-and-review.md G15 and javascript.md J14: a suppression carries its reason.
// `eslint-disable…` must end with ` -- <reason>`; `@ts-ignore` / `@ts-expect-error` must be
// followed by text that says why. The generated bundles (core/studio/overlay.js, live.js) are
// skipped: their sources are checked.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const ROOTS = ['core/studio', 'scripts', 'tests'];
const SKIP = new Set(['core/studio/overlay.js', 'core/studio/live.js']);

/** Every .js / .mjs / .html file under the roots, as repo-relative paths. */
function files() {
  const out = [];
  const walk = (dir) => {
    for (const e of fs.readdirSync(path.join(REPO, dir), { withFileTypes: true })) {
      const rel = `${dir}/${e.name}`;
      if (e.isDirectory()) {
        if (e.name !== 'node_modules' && !rel.startsWith('tests/golden/')) walk(rel);
        // Golden recordings are data; the golden test files themselves are checked below.
      } else if (/\.(m?js|html)$/.test(e.name) && !SKIP.has(rel)) out.push(rel);
    }
  };
  for (const r of ROOTS) walk(r);
  for (const e of fs.readdirSync(path.join(REPO, 'tests/golden'))) if (e.endsWith('.mjs')) out.push(`tests/golden/${e}`);
  return out;
}

/** The suppressions of a text that give no reason, as `line: text`. */
export function bareSuppressions(text) {
  const bad = [];
  text.split('\n').forEach((line, i) => {
    const eslint = /eslint-disable(?:-next-line|-line)?\b(.*)$/.exec(line);
    if (eslint && !/\s--\s+\S/.test(eslint[1])) bad.push(`${i + 1}: ${line.trim()}`);
    const ts = /@ts-(?:ignore|expect-error)\b(.*)$/.exec(line);
    if (ts && !/\S/.test(ts[1].replace(/\*\/\s*$/, ''))) bad.push(`${i + 1}: ${line.trim()}`);
  });
  return bad;
}

test('conventions: the reason check itself', () => {
  assert.deepEqual(bareSuppressions('// eslint-disable-next-line no-console -- prints the report'), []);
  assert.equal(bareSuppressions('// eslint-disable-next-line no-console').length, 1);
  assert.equal(bareSuppressions('/* eslint-disable */').length, 1);
  assert.deepEqual(bareSuppressions('// @ts-expect-error the DOM type lacks it'), []);
  assert.equal(bareSuppressions('// @ts-ignore').length, 1);
});

test('conventions: every eslint-disable and @ts-ignore in the code says why', () => {
  const found = [];
  for (const rel of files()) {
    // This file names the patterns in its own test strings.
    if (rel === 'tests/unit/conventions.test.mjs') continue;
    for (const b of bareSuppressions(fs.readFileSync(path.join(REPO, rel), 'utf8'))) found.push(`${rel}:${b}`);
  }
  assert.deepEqual(found, [], `add " -- <reason>" (eslint) or a reason after @ts-…:\n${found.join('\n')}`);
});

/** Lines over this many characters in the Studio's own JavaScript (javascript.md J13). */
const MAX_LINE = 200;

/** `file:line (length)` of every line over MAX_LINE in `text`. */
function longLines(rel, text) {
  return text
    .split('\n')
    .map((l, i) => [l.length, i + 1])
    .filter(([n]) => n > MAX_LINE)
    .map(([n, line]) => `${rel}:${line} (${n})`);
}

test('conventions: the long-line check itself', () => {
  assert.deepEqual(longLines('a.js', `${'x'.repeat(MAX_LINE)}\n${'y'.repeat(MAX_LINE + 1)}`), ['a.js:2 (201)']);
});

test('conventions: no line of the Studio JavaScript is over 200 characters (J13)', () => {
  const studio = files().filter((f) => f.startsWith('core/studio/') && f.endsWith('.js'));
  assert.ok(studio.length > 10, 'the Studio sources are found');
  const long = studio.flatMap((f) => longLines(f, fs.readFileSync(path.join(REPO, f), 'utf8')));
  assert.deepEqual(long, [], 'split these lines (the generated overlay.js and live.js are skipped)');
});

// javascript.md J6 (review row 34): any window can post to the Studio, and a page another origin
// frames passes `e.source === parent`. Every `message` listener is written inline and its first
// statement is `if (<a test that reads .origin>) return;`, so nothing runs before the sender's
// origin is compared; every postMessage names a target origin other than '*'.

/** Index just past the bracket that closes the one at `open` (strings and comments skipped). */
function closing(text, open) {
  /** @type {Record<string, string>} */
  const pairs = { '(': ')', '{': '}', '[': ']' };
  const stack = [];
  for (let i = open; i < text.length; i++) {
    const c = text[i];
    if (c === '/' && text[i + 1] === '/') i = text.indexOf('\n', i) >>> 0;
    else if (c === '/' && text[i + 1] === '*') i = text.indexOf('*/', i) + 1;
    else if (c === "'" || c === '"' || c === '`') {
      for (i++; i < text.length && text[i] !== c; i++) if (text[i] === '\\') i++;
    } else if (Object.hasOwn(pairs, c)) stack.push(pairs[c]);
    else if (c === stack.at(-1)) {
      stack.pop();
      if (!stack.length) return i + 1;
    }
  }
  return text.length;
}

/** The text without its leading blanks and comments. */
const stripLead = (s) => s.replace(/^(?:\s+|\/\/[^\n]*|\/\*[\s\S]*?\*\/)*/, '');

/** `line: why` for every message listener of `text` that acts before comparing `.origin`. */
export function uncheckedListeners(text) {
  const bad = [];
  const line = (i) => text.slice(0, i).split('\n').length;
  for (const m of text.matchAll(/addEventListener\(\s*(['"`])message\1\s*,|\bonmessage\s*=/g)) {
    const at = m.index + m[0].length;
    const head = /^\s*(?:async\s*)?(?:function\s*\w*\s*\([^)]*\)|\(?[\w\s,{}]*\)?\s*=>)\s*\{/.exec(text.slice(at));
    if (!head) {
      bad.push(`${line(m.index)}: the handler is not an inline function with a body`);
      continue;
    }
    const body = stripLead(text.slice(at + head[0].length));
    const first = /^if\s*\(/.exec(body);
    const open = first ? first[0].length - 1 : 0;
    const cond = first ? body.slice(open, closing(body, open)) : '';
    const rest = first ? stripLead(body.slice(open + cond.length)) : '';
    if (!first || !/\.origin\b/.test(cond) || !/^return\b/.test(rest))
      bad.push(`${line(m.index)}: the first statement is not \`if (… e.origin …) return;\``);
  }
  for (const m of text.matchAll(/\.postMessage\(/g)) {
    const open = m.index + m[0].length - 1;
    const args = text.slice(open + 1, closing(text, open) - 1);
    if (/,\s*(['"`])\*\1\s*$/.test(args)) bad.push(`${line(m.index)}: postMessage to '*'`);
    else if (!/,/.test(args)) bad.push(`${line(m.index)}: postMessage without a target origin`);
  }
  return bad;
}

test('conventions: the message-listener check itself', () => {
  const ok = [
    "addEventListener('message', (e) => {\n  // J6\n  if (e.source !== parent || e.origin !== location.origin) return;\n  run(e.data);\n});",
    "window.addEventListener('message', async (e) => {\n  if (!allowed.includes(e.origin)) return;\n});",
    "parent.postMessage({ type: 'x' }, '/');",
    'f.contentWindow.postMessage(msg, o);',
  ];
  for (const t of ok) assert.deepEqual(uncheckedListeners(t), [], t);
  const bad = [
    "addEventListener('message', (e) => {\n  if (e.source !== parent) return;\n  run(e.data);\n});",
    "addEventListener('message', (e) => {\n  const d = e.data;\n  if (e.origin !== location.origin) return;\n});",
    "addEventListener('message', (e) => {\n  if (e.origin === location.origin) run(e.data);\n});",
    "addEventListener('message', onMessage);",
    'window.onmessage = (e) => { run(e.data); };',
    "parent.postMessage(msg, '*');",
    'parent.postMessage(msg);',
  ];
  for (const t of bad) assert.equal(uncheckedListeners(t).length, 1, t);
});

test('conventions: every message listener in the Studio compares e.origin first (J6)', () => {
  const studio = files().filter((f) => f.startsWith('core/studio/'));
  const read = (f) => fs.readFileSync(path.join(REPO, f), 'utf8');
  const found = studio.flatMap((f) => uncheckedListeners(read(f)).map((b) => `${f}:${b}`));
  const listeners = studio.filter((f) => /addEventListener\(\s*'message'/.test(read(f)));
  assert.ok(listeners.length >= 3, `the listeners are found (${listeners.join(', ')})`);
  assert.deepEqual(found, [], found.join('\n'));
});

// Review row 2: fs.watch and its wrappers miss events on some platforms and file systems
// (docs/conventions/README.md §0); a watcher polls with a deadline instead.
const WATCH = /\bfs(?:\.promises)?\.watch(?:File)?\s*\(|\bwatchFile\s*\(|chokidar/;

test('conventions: no tracked JavaScript uses fs.watch, fs.watchFile or chokidar', () => {
  assert.ok(WATCH.test('fs.watch(dir, cb)') && WATCH.test("import c from 'chokidar'") && !WATCH.test('watcher.close()'));
  const tracked = execFileSync('git', ['ls-files', '-z', '--', '*.js', '*.mjs'], { cwd: REPO, encoding: 'utf8' })
    .split('\0')
    .filter((f) => f && f !== 'tests/unit/conventions.test.mjs');
  assert.ok(tracked.length > 50, 'the tracked scripts are found');
  const hits = tracked.filter((f) => fs.existsSync(path.join(REPO, f)) && WATCH.test(fs.readFileSync(path.join(REPO, f), 'utf8')));
  assert.deepEqual(hits, [], 'poll with a deadline instead (docs/conventions/README.md §0)');
});
