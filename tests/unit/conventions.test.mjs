// docs/conventions/git-and-review.md G15 and javascript.md J14: a suppression carries its reason.
// `eslint-disable…` must end with ` -- <reason>`; `@ts-ignore` / `@ts-expect-error` must be
// followed by text that says why. The generated bundles (core/studio/overlay.js, live.js) are
// skipped: their sources are checked.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

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
