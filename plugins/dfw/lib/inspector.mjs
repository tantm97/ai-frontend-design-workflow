// Source mapping for the running app: code-inspector-plugin (MIT, github.com/zh-lx/code-inspector)
// adds data-insp-path="<file>:<line>:<column>:<tag>" to every element in development builds only.
// dfw never adds it silently: it is a devDependency the user approves at G3 like any other.
// "dfw inspector setup" prints the change; "--write" applies it only after that approval.
import fs from 'node:fs';
import path from 'node:path';
import { readJson, projectPaths, DfwError, toPosix } from './util.mjs';
import { detectStack } from './stack.mjs';
import { loadState, gatePassed } from './state.mjs';

export const INSPECTOR = {
  package: 'code-inspector-plugin',
  version: '2.0.9', // pinned; exact version, no range
  license: 'MIT',
  repo: 'https://github.com/zh-lx/code-inspector',
  attribute: 'data-insp-path',
  purpose: 'dev-only source mapping (element -> file:line:col) for click-to-comment on the running app (dfw review)',
};

// Relative paths in the attribute; no hotkeys, switch, console tips or editor-launch server:
// dfw only reads the attribute. The plugin itself is inactive outside development
// (vite: apply on "serve" only; webpack: mode/NODE_ENV development; turbopack: NODE_ENV development).
const OPTIONS = "pathType: 'relative', hotKeys: false, showSwitch: false, hideConsole: true, server: 'close'";
const call = (bundler) => `codeInspectorPlugin({ bundler: '${bundler}', ${OPTIONS} })`;

/**
 * Parse a data-insp-path value ("src/Foo.tsx:42:7:div", "C:\\app\\src\\Foo.tsx:42:7:div").
 * Returns { file, line, column, tag, source: "file:line:col" } or null. Absolute paths inside
 * `root` become project-relative (POSIX separators).
 */
export function parseInspPath(value, root = null) {
  if (typeof value !== 'string' || !value.trim()) return null;
  const parts = value.trim().split(':');
  let tag = null;
  if (parts.length > 2 && !/^\d+$/.test(parts.at(-1))) tag = parts.pop();
  const nums = [];
  while (parts.length > 1 && nums.length < 2 && /^\d+$/.test(parts.at(-1))) nums.unshift(Number(parts.pop()));
  if (!nums.length) return null;
  let file = parts.join(':').replace(/\\/g, '/');
  if (!file) return null;
  const [line, column = null] = nums;
  if (root) {
    const isWin = /^[a-zA-Z]:\//.test(file);
    const lib = isWin ? path.win32 : path.posix;
    const base = isWin ? root.replace(/\//g, '\\') : toPosix(root);
    if (lib.isAbsolute(file)) {
      const rel = lib.relative(base, isWin ? file.replace(/\//g, '\\') : file);
      if (rel && !rel.startsWith('..') && !lib.isAbsolute(rel)) file = rel.replace(/\\/g, '/');
    }
  }
  file = file.replace(/^\.\//, '');
  return { file, line, column, tag, source: `${file}:${line}${column != null ? `:${column}` : ''}` };
}

const CONFIGS = {
  vite: ['vite.config.ts', 'vite.config.mts', 'vite.config.js', 'vite.config.mjs', 'vite.config.cjs', 'vite.config.cts'],
  webpack: ['webpack.config.js', 'webpack.config.cjs', 'webpack.config.mjs', 'webpack.config.ts', 'webpack.dev.js', 'webpack.config.dev.js'],
  next: ['next.config.ts', 'next.config.mjs', 'next.config.js', 'next.config.cjs'],
  nuxt: ['nuxt.config.ts', 'nuxt.config.js'],
  astro: ['astro.config.mjs', 'astro.config.ts', 'astro.config.js'],
  rsbuild: ['rsbuild.config.ts', 'rsbuild.config.js', 'rsbuild.config.mjs'],
  rspack: ['rspack.config.js', 'rspack.config.ts', 'rspack.config.mjs'],
  umi: ['.umirc.ts', '.umirc.js', 'config/config.ts', 'config/config.js'],
  'vue-cli': ['vue.config.js'],
};

const major = (v) => {
  const m = String(v || '').match(/(\d+)(?:\.(\d+))?/);
  return m ? [Number(m[1]), Number(m[2] || 0)] : null;
};

/** Which bundler serves the app in development, and its config file. */
export function detectBundler(root) {
  const pkg = readJson(path.join(root, 'package.json'), {});
  const deps = { ...pkg.dependencies, ...pkg.devDependencies };
  const has = (d) => Boolean(deps[d]);
  const order = [
    ['next', has('next')],
    ['nuxt', has('nuxt')],
    ['astro', has('astro')],
    ['umi', has('umi') || has('@umijs/max')],
    ['rsbuild', has('@rsbuild/core')],
    ['rspack', has('@rspack/cli') || has('@rspack/core')],
    ['vue-cli', has('@vue/cli-service')],
    ['vite', has('vite') || has('@sveltejs/kit')],
    ['webpack', has('webpack') || has('webpack-cli')],
    ['esbuild', has('esbuild')],
  ];
  const found = order.find(([, ok]) => ok);
  let bundler = found ? found[0] : null;
  // No dependency hint: fall back to config files present.
  if (!bundler) for (const [b, files] of Object.entries(CONFIGS)) if (files.some((f) => fs.existsSync(path.join(root, f)))) { bundler = b; break; }
  const configFile = bundler && CONFIGS[bundler] ? CONFIGS[bundler].find((f) => fs.existsSync(path.join(root, f))) || null : null;
  const out = { bundler, configFile, packageJson: fs.existsSync(path.join(root, 'package.json')) };
  if (bundler === 'next') {
    const v = major(deps.next);
    const dev = String(pkg.scripts?.dev || '');
    out.nextVersion = deps.next;
    // Next 16 develops with Turbopack by default; 15.3+ with --turbopack; otherwise webpack.
    out.nextMode = /--webpack\b/.test(dev) ? 'webpack' : v && (v[0] >= 16 || (/--turbo(pack)?\b/.test(dev) && (v[0] > 15 || (v[0] === 15 && v[1] >= 3)))) ? 'turbopack' : /--turbo(pack)?\b/.test(dev) ? 'turbo-experimental' : 'webpack';
  }
  return out;
}

const isEsm = (file, text) => /\.(mjs|mts|ts)$/.test(file) || /^\s*(import\s|export\s)/m.test(text);

function addImport(text, esm) {
  if (text.includes("'code-inspector-plugin'") || text.includes('"code-inspector-plugin"')) return text;
  if (!esm) {
    const line = "const { codeInspectorPlugin } = require('code-inspector-plugin');\n";
    const strict = text.match(/^(['"])use strict\1;?\s*\n/);
    return strict ? strict[0] + line + text.slice(strict[0].length) : line + text;
  }
  const line = "import { codeInspectorPlugin } from 'code-inspector-plugin';\n";
  // After the last top-level import statement (single- or multi-line, optional trailing // comment).
  // One statement per match: nothing before the module string may hold a quote or a semicolon.
  const re = /^import\b[^'";]*['"][^'"\n]+['"];?[ \t]*(?:\/\/[^\n]*)?\r?\n/gm;
  let end = 0;
  for (const m of text.matchAll(re)) end = m.index + m[0].length;
  return text.slice(0, end) + line + text.slice(end);
}

/** Brace depth at an index, or null when it sits inside a comment or a string (rough: good enough for config files). */
function depthAt(text, index) {
  let d = 0;
  let q = null;
  for (let i = 0; i < index; i++) {
    const c = text[i];
    if (q) {
      if (c === '\\') i++;
      else if (c === q) q = null;
    } else if (c === '"' || c === "'" || c === '`') q = c;
    else if (c === '/' && (text[i + 1] === '/' || text[i + 1] === '*')) {
      const line = text[i + 1] === '/';
      const end = line ? text.indexOf('\n', i) : text.indexOf('*/', i + 2);
      if (end === -1 || end >= index) return null;
      i = line ? end : end + 1;
    } else if (c === '{') d++;
    else if (c === '}') d--;
  }
  return q ? null : d;
}

/**
 * Insert `item` as the first element of the config's own `plugins: [` array; null when there is none.
 * Lists in comments and nested lists (css.postcss.plugins, resolve.plugins, rollupOptions.plugins) are not it.
 */
function addToPlugins(text, item) {
  const hits = [...text.matchAll(/\bplugins\s*:\s*\[/g)].map((m) => ({ at: m.index + m[0].length, d: depthAt(text, m.index) })).filter((h) => h.d !== null && h.d <= 1);
  if (!hits.length) return null;
  const best = hits.reduce((a, b) => (b.d < a.d ? b : a));
  const rest = text.slice(best.at);
  const ws = rest.match(/^\s*/)[0];
  if (rest.slice(ws.length).startsWith(']')) return `${text.slice(0, best.at)}${item}${rest}`; // empty list
  if (ws.includes('\n')) return `${text.slice(0, best.at)}${ws}${item},${rest}`; // one plugin per line
  return `${text.slice(0, best.at)}${item}, ${rest}`;
}

/** Insert `prop` as the first property of the exported config object; null when not found. */
function addProperty(text, prop) {
  const re = [
    /defineConfig\(\s*(?:(?:async\s*)?\([^)]*\)\s*=>\s*\(\s*)?\{/, // defineConfig({ / defineConfig(({ mode }) => ({ ; never a function body
    /(?:const|let|var)\s+nextConfig\s*(?::\s*[\w.]+\s*)?=\s*\{/,
    /module\.exports\s*=\s*(?:\([^)]*\)\s*=>\s*\(\s*)?\{/,
    /export\s+default\s*\{/,
  ];
  for (const r of re) {
    const m = text.match(r);
    if (m) {
      const at = m.index + m[0].length;
      return `${text.slice(0, at)}\n  ${prop},${text.slice(at)}`;
    }
  }
  return null;
}

/**
 * Patch a bundler config so it loads code-inspector-plugin. Returns { text, changed } or
 * { manual: <reason> } when the file's shape is not one dfw edits safely.
 */
export function patchConfig(text, { bundler, file = '', nextMode = 'webpack' }) {
  if (/code-inspector-plugin|codeInspectorPlugin/.test(text)) return { text, changed: false };
  const esm = isEsm(file, text);
  let body = null;
  if (bundler === 'vite') {
    body = addToPlugins(text, call('vite')) ?? addProperty(text, `plugins: [${call('vite')}]`);
  } else if (bundler === 'webpack') {
    body = addToPlugins(text, call('webpack')) ?? addProperty(text, `plugins: [${call('webpack')}]`);
  } else if (bundler === 'next') {
    if (nextMode === 'turbopack') {
      if (/\bturbopack\s*:/.test(text)) return { manual: 'next.config already has a "turbopack" key: add rules: codeInspectorPlugin(...) to it by hand.' };
      body = addProperty(text, `turbopack: { rules: ${call('turbopack')} }`);
    } else if (nextMode === 'webpack') {
      if (/\bwebpack\s*[:(]/.test(text)) return { manual: 'next.config already has a "webpack" function: push the plugin inside it, only when dev is true.' };
      body = addProperty(text, `webpack: (config, { dev }) => {\n    if (dev) config.plugins.push(${call('webpack')});\n    return config;\n  }`);
    } else return { manual: 'Next.js 15.0-15.2 with Turbopack: use experimental.turbo.rules = codeInspectorPlugin({ bundler: "turbopack", ... }).' };
  } else return { manual: `dfw does not edit ${bundler || 'this'} config files automatically.` };
  if (!body) return { manual: `could not find the plugin list or the exported config object in ${file || 'the config'}.` };
  return { text: addImport(body, esm), changed: true };
}

const MANUAL = {
  nuxt: `nuxt.config: vite: { plugins: [${call('vite')}] }`,
  astro: `astro.config: vite: { plugins: [${call('vite')}] }`,
  rsbuild: `rsbuild.config: tools: { rspack: { plugins: [${call('rspack')}] } }`,
  rspack: `rspack.config: plugins: [${call('rspack')}]`,
  umi: `.umirc / config: chainWebpack(memo) { memo.plugin('code-inspector-plugin').use(${call('webpack')}) } (or mako: { plugins: [${call('mako')}] })`,
  'vue-cli': `vue.config.js: chainWebpack: (config) => { config.plugin('code-inspector-plugin').use(${call('webpack')}) }`,
  esbuild: `your esbuild script: plugins: [codeInspectorPlugin({ bundler: 'esbuild', dev: () => <true only for the dev build>, ${OPTIONS} })]`,
};

const INSTALL = { npm: 'npm install', pnpm: 'pnpm install', yarn: 'yarn install', bun: 'bun install' };

/** What "dfw inspector setup" would do in this project. */
export function inspectorPlan(root) {
  const stack = detectStack(root);
  const b = detectBundler(root);
  const dep = `${INSPECTOR.package}@${INSPECTOR.version}`;
  const plan = { ...b, dependency: { name: INSPECTOR.package, version: INSPECTOR.version, dev: true, license: INSPECTOR.license, purpose: INSPECTOR.purpose }, framework: stack.framework?.name || null, packageManager: stack.packageManager || 'npm' };
  plan.install = `${INSTALL[plan.packageManager] || 'npm install'} (after adding "${INSPECTOR.package}": "${INSPECTOR.version}" to devDependencies)`;
  if (!b.packageJson) {
    plan.supported = false;
    plan.manual = 'No package.json: code-inspector-plugin needs a JavaScript bundler (Vite, webpack, Rspack, esbuild, Turbopack...). Live review still works without it (comments carry selector + text, no file:line).';
    return plan;
  }
  if (['vite', 'webpack', 'next'].includes(b.bundler) && b.configFile) {
    const file = path.join(root, b.configFile);
    const r = patchConfig(fs.readFileSync(file, 'utf8'), { bundler: b.bundler, file: b.configFile, nextMode: b.nextMode });
    if (r.manual) {
      plan.supported = false;
      plan.manual = r.manual;
    } else {
      plan.supported = true;
      plan.patched = r.text;
      plan.alreadyConfigured = !r.changed;
    }
  } else {
    plan.supported = false;
    plan.manual = b.bundler === 'vite' || b.bundler === 'webpack' || b.bundler === 'next'
      ? `No ${b.bundler} config file found: create one with ${b.bundler === 'next' ? 'webpack/turbopack' : 'plugins'}: [${call(b.bundler === 'next' ? 'webpack' : b.bundler)}].`
      : MANUAL[b.bundler] || 'Unknown bundler: see https://github.com/zh-lx/code-inspector#-usage and pass the options ' + `{ ${OPTIONS} }.`;
  }
  plan.devDependency = dep;
  return plan;
}

export function formatPlan(plan) {
  const lines = [
    `code-inspector-plugin ${INSPECTOR.version} (${INSPECTOR.license}, ${INSPECTOR.repo})`,
    `purpose: ${INSPECTOR.purpose}; development builds only (it does nothing in production builds).`,
    `bundler: ${plan.bundler || 'unknown'}${plan.nextMode ? ` (${plan.nextMode})` : ''}${plan.configFile ? `, config: ${plan.configFile}` : ''}`,
    `1. devDependency: "${INSPECTOR.package}": "${INSPECTOR.version}"  then ${plan.install.split(' (')[0]}`,
  ];
  if (plan.supported) lines.push(plan.alreadyConfigured ? `2. ${plan.configFile} already loads the plugin.` : `2. ${plan.configFile} becomes:\n\n${plan.patched}`);
  else lines.push(`2. by hand: ${plan.manual}`);
  lines.push(
    '',
    'Needs the user\'s approval at G3: list it in .dfw/stack.json dependencies, e.g.',
    `  {"name":"${INSPECTOR.package}","version":"${INSPECTOR.version}","dev":true,"license":"${INSPECTOR.license}","purpose":"live review source mapping"}`,
    'then "dfw approve G3 ..." and "dfw inspector setup --write".',
  );
  return lines.join('\n');
}

/** True when .dfw/stack.json lists code-inspector-plugin (dependencies or devDependencies). */
export function stackApproves(stackJson) {
  const lists = [stackJson?.dependencies, stackJson?.devDependencies];
  for (const l of lists) {
    if (!l) continue;
    if (Array.isArray(l) && l.some((d) => (typeof d === 'string' ? d.split('@')[0] || d : d?.name) === INSPECTOR.package)) return true;
    if (!Array.isArray(l) && typeof l === 'object' && INSPECTOR.package in l) return true;
  }
  return false;
}

/** Apply the plan: refused unless G3 is approved and the dependency is in the approved stack. */
export function inspectorSetup(root, { write = false } = {}) {
  const plan = inspectorPlan(root);
  if (!write) return { plan, written: [] };
  const state = loadState(root);
  const stackJson = readJson(projectPaths(root).stack, null);
  const missing = [];
  if (!state || !gatePassed(state, 'G3')) missing.push('G3 (technology stack) is not approved');
  if (!stackApproves(stackJson)) missing.push(`.dfw/stack.json does not list "${INSPECTOR.package}" in its dependencies`);
  if (missing.length) {
    throw new DfwError(`dfw inspector setup --write refused: ${missing.join('; ')}. New dependencies need the user's approval: propose ${INSPECTOR.package}@${INSPECTOR.version} (dev-only, ${INSPECTOR.license}) with "dfw stack set" (dependencies: [{"name":"${INSPECTOR.package}","version":"${INSPECTOR.version}","dev":true}]), get the user's OK, "dfw approve G3 --note ...", then run this again.`);
  }
  if (!plan.supported) throw new DfwError(`dfw cannot edit this project's bundler config automatically: ${plan.manual}`);
  const written = [];
  if (!plan.alreadyConfigured) {
    const file = path.join(root, plan.configFile);
    if (!fs.existsSync(`${file}.dfw-backup`)) fs.copyFileSync(file, `${file}.dfw-backup`); // the user's original, kept once
    fs.writeFileSync(file, plan.patched);
    written.push(plan.configFile);
  }
  const pkgFile = path.join(root, 'package.json');
  const raw = fs.readFileSync(pkgFile, 'utf8');
  const pkg = JSON.parse(raw);
  if (pkg.devDependencies?.[INSPECTOR.package] !== INSPECTOR.version) {
    pkg.devDependencies = { ...pkg.devDependencies, [INSPECTOR.package]: INSPECTOR.version };
    const indent = raw.match(/^[ \t]+(?=")/m)?.[0] || '  ';
    fs.writeFileSync(pkgFile, JSON.stringify(pkg, null, indent) + (raw.endsWith('\n') ? '\n' : ''));
    written.push('package.json');
  }
  return { plan, written };
}
