// The Studio's registries hold their values (docs/conventions/javascript.md J14): colours live in
// CSS variables of app.css, HTTP methods in shared/paths.js, and the text of innerHTML templates
// in shared/text.js. The review bar of live/ draws in the user's app, where the Studio's CSS
// variables do not exist, so its stylesheet is left out.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const STUDIO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'core/studio');
const SKIP = new Set(['overlay.js', 'live.js']);

/** The Studio's browser modules (app/, overlay/, live/, shared/, app.js), relative paths. */
function modules() {
  const out = ['app.js'];
  for (const dir of ['app', 'overlay', 'live', 'shared'])
    for (const f of fs.readdirSync(path.join(STUDIO, dir))) if (f.endsWith('.js') && !SKIP.has(f)) out.push(`${dir}/${f}`);
  return out;
}
const read = (/** @type {string} */ rel) => fs.readFileSync(path.join(STUDIO, rel), 'utf8');

/** Hex colours of a stylesheet outside custom property definitions, as `line: text`. */
export function hexOutsideVars(css) {
  const bad = [];
  css.split('\n').forEach((line, i) => {
    if (/^\s*--[\w-]+\s*:/.test(line)) return;
    if (/#[0-9a-fA-F]{3,8}\b/.test(line)) bad.push(`${i + 1}: ${line.trim()}`);
  });
  return bad;
}

test('registries: the checker finds a hex colour outside a variable', () => {
  assert.deepEqual(hexOutsideVars(':root {\n  --ink: #111;\n}\n.a { color: #fff; }'), ['4: .a { color: #fff; }']);
});

test('registries: app.css colours are CSS variables', () => {
  assert.deepEqual(hexOutsideVars(read('app.css')), [], 'define the colour as a --variable in :root and use var()');
});

test('registries: no hex colour in the Studio JavaScript (live/ stylesheet aside)', () => {
  const bad = [];
  for (const m of modules()) {
    if (m.startsWith('live/')) continue;
    read(m)
      .split('\n')
      .forEach((line, i) => {
        if (/['"`]#[0-9a-fA-F]{3,8}['"`]/.test(line)) bad.push(`${m}:${i + 1}: ${line.trim()}`);
      });
  }
  assert.deepEqual(bad, [], 'use a CSS variable of app.css (var(--...))');
});

test('registries: HTTP methods come from shared/paths.js METHOD', () => {
  const bad = [];
  for (const m of modules()) {
    if (m === 'shared/paths.js') continue;
    read(m)
      .split('\n')
      .forEach((line, i) => {
        if (/['"](GET|POST|PATCH|PUT|DELETE)['"]/.test(line)) bad.push(`${m}:${i + 1}: ${line.trim()}`);
      });
  }
  assert.deepEqual(bad, [], 'use METHOD.<name>');
});

/** The static text of a template literal assigned to innerHTML: tags, ${} and entities removed. */
export function templateWords(src) {
  const out = [];
  for (const m of src.matchAll(/innerHTML\s*=\s*(`(?:[^`\\]|\\.)*`|'[^']*')/g)) {
    const text = m[1]
      .slice(1, -1)
      .replace(/\$\{[^}]*\}/g, ' ')
      .replace(/<[^>]*>/g, ' ')
      .replace(/&\w+;/g, ' ');
    for (const w of text.match(/[A-Za-z]{3,}(?:\s+[A-Za-z]{2,})+/g) || []) out.push(w.trim());
  }
  return out;
}

test('registries: the checker finds words in an innerHTML template', () => {
  assert.deepEqual(templateWords('el.innerHTML = `<p>${TEXT.x}</p><b>Click here</b>`;'), ['Click here']);
  assert.deepEqual(templateWords('el.innerHTML = \'<li class="a b">\' + x;'), []);
});

test('registries: innerHTML templates take their words from shared/text.js', () => {
  const bad = [];
  for (const m of modules()) {
    if (m.startsWith('shared/')) continue;
    for (const w of templateWords(read(m))) bad.push(`${m}: ${w}`);
  }
  assert.deepEqual(bad, [], 'move the words into TEXT (shared/text.js)');
});
