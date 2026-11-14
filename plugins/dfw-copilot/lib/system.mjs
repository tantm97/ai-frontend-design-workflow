// Design-system scaffolding: start from the token-driven starter kit, explore 2-3
// preset options side by side (as token themes), then promote the chosen one.
import fs from 'node:fs';
import path from 'node:path';
import { PLUGIN_ROOT, projectPaths, readJson, writeJson, DfwError } from './util.mjs';
import { buildTokens } from './tokens.mjs';
import { buildSystem, loadComponents } from './compose.mjs';
import { addIcons } from './icons.mjs';
import { isStyle, getStyle, styleTokens, headFor, applyStyle } from './style.mjs';

export const KIT_DIR = path.join(PLUGIN_ROOT, 'kits', 'base');

export function listPresets() {
  const dir = path.join(KIT_DIR, 'presets');
  return fs.existsSync(dir)
    ? fs
        .readdirSync(dir)
        .filter((f) => f.endsWith('.tokens.json'))
        .map((f) => {
          const name = f.replace('.tokens.json', '');
          return { name, description: readJson(path.join(dir, f), {}).$description || '' };
        })
    : [];
}

function presetFiles(name) {
  const tokens = path.join(KIT_DIR, 'presets', `${name}.tokens.json`);
  if (!fs.existsSync(tokens)) throw new DfwError(`Unknown preset "${name}". Presets: ${listPresets().map((p) => p.name).join(', ')}`);
  const head = path.join(KIT_DIR, 'presets', `${name}.head.html`);
  return { tokens, head: fs.existsSync(head) ? fs.readFileSync(head, 'utf8') : '' };
}

function isPlaceholderTokens(file) {
  if (!fs.existsSync(file)) return true;
  return /Placeholder tokens created by dfw init/.test(fs.readFileSync(file, 'utf8'));
}

/** Icons referenced by component masters (templates and examples). */
function iconsUsed(root) {
  const names = new Set();
  const dir = path.join(projectPaths(root).design, 'system', 'components');
  if (!fs.existsSync(dir)) return [];
  for (const f of fs.readdirSync(dir)) {
    const t = fs.readFileSync(path.join(dir, f), 'utf8');
    for (const m of t.matchAll(/<dfw-icon\b[^>]*\bname="([a-z0-9-]+)"/g)) names.add(m[1]);
    for (const m of t.matchAll(/<dfw-use\b[^>]*?\bicon(?:-end)?="([a-z0-9-]+)"/g)) names.add(m[1]);
  }
  return [...names];
}

/**
 * Copy the starter kit into design/system/ and (when tokens are still the placeholder) the preset tokens.
 * Existing component files are kept unless force is set.
 */
export function systemInit(root, { preset = 'crisp', style = null, fonts = 'vi', force = false, components = null } = {}) {
  const p = projectPaths(root);
  const sys = path.join(p.design, 'system');
  const compDir = path.join(sys, 'components');
  fs.mkdirSync(compDir, { recursive: true });
  const src = path.join(KIT_DIR, 'components');
  const copied = [];
  const kept = [];
  for (const f of fs.readdirSync(src).filter((x) => x.endsWith('.html'))) {
    if (components && !components.includes(f.replace('.html', ''))) continue;
    const dest = path.join(compDir, f);
    if (fs.existsSync(dest) && !force) kept.push(f);
    else {
      fs.copyFileSync(path.join(src, f), dest);
      copied.push(f);
    }
  }
  const tokFile = path.join(p.tokens, 'tokens.json');
  let tokensWritten = false;
  if (style) {
    applyStyle(root, style, { fonts });
    const icons = addIcons(root, iconsUsed(root));
    buildTokens(root);
    buildSystem(root);
    return { preset: `style:${style}`, copied, kept, tokensWritten: true, icons: icons.added };
  }
  const { tokens, head } = presetFiles(preset);
  if (force || isPlaceholderTokens(tokFile)) {
    fs.mkdirSync(p.tokens, { recursive: true });
    fs.copyFileSync(tokens, tokFile);
    tokensWritten = true;
  }
  const headFile = path.join(sys, 'head.html');
  if (force || !fs.existsSync(headFile)) fs.writeFileSync(headFile, head);
  const icons = addIcons(root, iconsUsed(root));
  buildTokens(root);
  buildSystem(root);
  return { preset, copied, kept, tokensWritten, icons: icons.added };
}

/** Write presets as token themes "opt-<name>" so the same screens can be compared in the Studio. */
export function systemOptions(root, names, { fonts = 'vi' } = {}) {
  if (names.length < 2 || names.length > 3) throw new DfwError('Give 2-3 style guides or presets to compare, e.g. dfw system options industrial-utility swiss-grid ops-dense');
  const p = projectPaths(root);
  const themesDir = path.join(p.tokens, 'themes');
  fs.mkdirSync(themesDir, { recursive: true });
  for (const f of fs.readdirSync(themesDir)) if (f.startsWith('opt-')) fs.rmSync(path.join(themesDir, f));
  const heads = [];
  for (const n of names) {
    if (isStyle(n)) {
      const g = getStyle(n);
      writeJson(path.join(themesDir, `opt-${n}.json`), styleTokens(g, { fonts }));
      heads.push(headFor(g, fonts).replace(/<!--[\s\S]*?-->\s*/g, '').trim());
      continue;
    }
    const { tokens, head } = presetFiles(n);
    fs.copyFileSync(tokens, path.join(themesDir, `opt-${n}.json`));
    heads.push(head.replace(/<!--[\s\S]*?-->\s*/g, '').trim());
  }
  const headFile = path.join(p.design, 'system', 'head.html');
  const current = fs.existsSync(headFile) ? fs.readFileSync(headFile, 'utf8') : '';
  const block = `<!-- dfw-options-begin -->\n${[...new Set(heads.join('\n').split('\n'))].join('\n')}\n<!-- dfw-options-end -->`;
  fs.mkdirSync(path.dirname(headFile), { recursive: true });
  fs.writeFileSync(headFile, /<!-- dfw-options-begin -->/.test(current) ? current.replace(/<!-- dfw-options-begin -->[\s\S]*?<!-- dfw-options-end -->/, block) : `${current.trim()}\n${block}\n`);
  const m = readJson(p.manifest, null);
  if (m) {
    m.themes = [...new Set([...(m.themes || []).filter((t) => !String(t).startsWith('opt-')), ...names.map((n) => `opt-${n}`)])];
    writeJson(p.manifest, m);
  }
  const r = buildTokens(root);
  return { themes: r.themes.filter((t) => t.startsWith('opt-')) };
}

/** Promote one option (or preset) to the base tokens and remove the option themes. */
export function systemChoose(root, name, { fonts = 'vi' } = {}) {
  const p = projectPaths(root);
  const themesDir = path.join(p.tokens, 'themes');
  if (isStyle(name)) {
    applyStyle(root, name, { fonts });
  } else {
    const opt = path.join(themesDir, `opt-${name}.json`);
    const { tokens, head } = presetFiles(name);
    fs.copyFileSync(fs.existsSync(opt) ? opt : tokens, path.join(p.tokens, 'tokens.json'));
    fs.writeFileSync(path.join(p.design, 'system', 'head.html'), head);
  }
  if (fs.existsSync(themesDir)) for (const f of fs.readdirSync(themesDir)) if (f.startsWith('opt-')) fs.rmSync(path.join(themesDir, f));
  const m = readJson(p.manifest, null);
  if (m) {
    m.themes = (m.themes || []).filter((t) => !String(t).startsWith('opt-'));
    writeJson(p.manifest, m);
  }
  buildTokens(root);
  buildSystem(root);
  return { chosen: name };
}

export function systemSummary(root) {
  const lib = loadComponents(root);
  return { components: [...lib.values()].map((d) => ({ name: d.name, group: d.group, variants: d.variants, sizes: d.sizes, props: d.props, file: d.file })) };
}
