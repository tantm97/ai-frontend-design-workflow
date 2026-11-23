// The import graph of the Studio browser code (core/studio) has no cycle, and no module imports
// across folders with `../` (docs/conventions/javascript.md J14). A call from a lower module to a
// higher one goes through the `ui` table of its state.js, wired at start-up (app.js,
// overlay/main.js). The generated bundles and the Penpot plugin are left out.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const STUDIO = path.join(REPO, 'core/studio');
const SKIP = new Set(['overlay.js', 'live.js']);

/** Every module of the Studio code, as paths relative to core/studio. */
function modules() {
  const out = [];
  const walk = (dir) => {
    for (const e of fs.readdirSync(path.join(STUDIO, dir), { withFileTypes: true })) {
      const rel = dir ? `${dir}/${e.name}` : e.name;
      if (e.isDirectory()) {
        if (e.name !== 'penpot') walk(rel);
      } else if (e.name.endsWith('.js') && !SKIP.has(rel)) out.push(rel);
    }
  };
  walk('');
  return out.sort();
}

/**
 * The import edges of one module's source, resolved to core/studio-relative paths.
 * @param {string} from module path relative to core/studio
 * @param {string} text its source
 */
export function importsOf(from, text) {
  const edges = [];
  for (const m of text.matchAll(/^\s*(?:import|export)\s[^;]*?from\s+'([^']+)'/gms)) {
    const spec = m[1];
    if (spec.startsWith('#studio/')) edges.push(spec.slice('#studio/'.length));
    else if (spec.startsWith('.')) edges.push(path.posix.normalize(path.posix.join(path.posix.dirname(from), spec)));
  }
  return edges;
}

/** The cycles of a graph (strongly connected components of more than one module). */
export function cycles(graph) {
  let index = 0;
  const idx = new Map();
  const low = new Map();
  const stack = [];
  const on = new Set();
  const out = [];
  const visit = (v) => {
    idx.set(v, index);
    low.set(v, index++);
    stack.push(v);
    on.add(v);
    for (const w of graph.get(v) || []) {
      if (!idx.has(w)) {
        visit(w);
        low.set(v, Math.min(low.get(v), low.get(w)));
      } else if (on.has(w)) low.set(v, Math.min(low.get(v), idx.get(w)));
    }
    if (low.get(v) === idx.get(v)) {
      const c = [];
      let w;
      do {
        w = stack.pop();
        on.delete(w);
        c.push(w);
      } while (w !== v);
      if (c.length > 1 || (graph.get(v) || []).includes(v)) out.push(c.sort());
    }
  };
  for (const v of graph.keys()) if (!idx.has(v)) visit(v);
  return out;
}

test('imports: the checker finds a cycle and resolves #studio/ and ./ imports', () => {
  assert.deepEqual(importsOf('app/a.js', "import { x } from './b.js';\nimport { y } from '#studio/shared/c.js';"), [
    'app/b.js',
    'shared/c.js',
  ]);
  const g = new Map([
    ['a', ['b']],
    ['b', ['c']],
    ['c', ['a']],
    ['d', ['a']],
  ]);
  assert.deepEqual(cycles(g), [['a', 'b', 'c']]);
});

test('imports: the Studio code has no import cycle', () => {
  const graph = new Map();
  for (const m of modules()) graph.set(m, importsOf(m, fs.readFileSync(path.join(STUDIO, m), 'utf8')));
  const found = cycles(graph);
  assert.deepEqual(
    found,
    [],
    `import cycles (call the higher module through ui.<name> of state.js instead):\n${found.map((c) => c.join(' -> ')).join('\n')}`,
  );
});

test('imports: no ../ imports in the Studio code', () => {
  const bad = [];
  for (const m of modules()) {
    const text = fs.readFileSync(path.join(STUDIO, m), 'utf8');
    for (const x of text.matchAll(/from\s+'(\.\.\/[^']*)'/g)) bad.push(`${m}: ${x[1]}`);
  }
  assert.deepEqual(bad, [], 'use #studio/<folder>/<file>.js');
});
