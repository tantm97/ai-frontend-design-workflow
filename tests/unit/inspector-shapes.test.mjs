// Config shapes "dfw inspector setup" must not guess on: the patched file has to stay valid JavaScript
// and the plugin has to land in the bundler's own plugin list.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { patchConfig, inspectorSetup, INSPECTOR } from '../../core/lib/inspector.mjs';
import { newState, saveState, loadState, approveGate } from '../../core/lib/state.mjs';
import { writeJson } from '../../core/lib/util.mjs';

const FIX = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../fixtures/inspector');
const IMPORT = "import { codeInspectorPlugin } from 'code-inspector-plugin';\n";
const vite = (src) => patchConfig(src, { bundler: 'vite', file: 'vite.config.ts' });

test('patchConfig: an import with a trailing comment keeps the new import at the top of the file', () => {
  const src = `import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react' // fast refresh

export default defineConfig({
  plugins: [react()],
  build: {
    outDir: 'dist'
  },
})
`;
  const r = vite(src);
  assert.ok(r.text.includes(`import react from '@vitejs/plugin-react' // fast refresh\n${IMPORT}\nexport default`), r.text);
  assert.ok(r.text.includes("    outDir: 'dist'\n  },\n"), 'nothing inserted inside the config object');
  // Multi-line import, then a side-effect import: after the last one.
  const multi = vite(`import {\n  defineConfig,\n  loadEnv,\n} from 'vite';\nimport './env';\nexport default defineConfig({ plugins: [] });\n`);
  assert.ok(multi.text.includes(`import './env';\n${IMPORT}export default`), multi.text);
});

test('patchConfig: a commented-out plugins list is not the plugin list', () => {
  const r = vite(`import { defineConfig } from 'vite';
export default defineConfig({
  // plugins: [legacy()],
  /* plugins: [old()], */
  plugins: [react()],
});
`);
  assert.match(r.text, /^ {2}plugins: \[codeInspectorPlugin\(\{ bundler: 'vite'.*\}\), react\(\)\],$/m);
  assert.ok(r.text.includes('  // plugins: [legacy()],\n  /* plugins: [old()], */\n'), 'comments untouched');
});

test('patchConfig: nested plugin lists (postcss, resolve) are left alone; the config gets its own list', () => {
  const css = vite(`import { defineConfig } from 'vite';
export default defineConfig({
  css: { postcss: { plugins: [autoprefixer()] } },
});
`);
  assert.ok(css.text.includes('css: { postcss: { plugins: [autoprefixer()] } },'), css.text);
  assert.match(css.text, /defineConfig\(\{\n {2}plugins: \[codeInspectorPlugin\(\{ bundler: 'vite'/);
  const wp = patchConfig(`module.exports = {\n  entry: './src/index.ts',\n  resolve: { plugins: [new TsPaths()] },\n};\n`, { bundler: 'webpack', file: 'webpack.config.js' });
  assert.ok(wp.text.includes('resolve: { plugins: [new TsPaths()] },'), wp.text);
  assert.match(wp.text, /module\.exports = \{\n {2}plugins: \[codeInspectorPlugin\(\{ bundler: 'webpack'/);
});

test('patchConfig: a config function with a body is not edited (manual), never a statement in the body', () => {
  const src = `import { defineConfig, loadEnv } from 'vite';
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd());
  return { server: { port: Number(env.PORT) } };
});
`;
  const r = vite(src);
  assert.ok(r.manual, 'manual instructions');
  assert.equal(r.text, undefined);
});

test('inspector setup --write keeps the original config as <file>.dfw-backup, once', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-insp-backup-'));
  fs.cpSync(path.join(FIX, 'vite'), root, { recursive: true });
  process.env.DFW_HOME = path.join(root, '.home');
  const file = path.join(root, 'vite.config.ts');
  const original = fs.readFileSync(file, 'utf8');
  saveState(root, newState({ mode: 'existing-project' }));
  writeJson(path.join(root, '.dfw/stack.json'), { kind: 'javascript', dependencies: [{ name: INSPECTOR.package, version: INSPECTOR.version, dev: true }] });
  approveGate(root, loadState(root), 'G3', { note: 'ok' });
  inspectorSetup(root, { write: true });
  assert.equal(fs.readFileSync(`${file}.dfw-backup`, 'utf8'), original);
  // The user reverts by hand and runs setup again: the first backup is not replaced.
  fs.writeFileSync(file, original.replace('react()', 'react(), other()'));
  inspectorSetup(root, { write: true });
  assert.equal(fs.readFileSync(`${file}.dfw-backup`, 'utf8'), original);
});
