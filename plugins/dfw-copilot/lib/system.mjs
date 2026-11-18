// Design-system scaffolding: style/preset tokens and an empty component inventory, explore 2-3
// options side by side (as token themes), then promote the chosen one. Components are designed
// per product from the inventory; the starter kit is only a reference (inventory.mjs).
import fs from 'node:fs';
import path from 'node:path';
import { PLUGIN_ROOT, projectPaths, readJson, writeJson, DfwError } from './util.mjs';
import { buildTokens } from './tokens.mjs';
import { buildSystem, loadComponents } from './compose.mjs';
import { ensureInventory } from './inventory.mjs';
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

/**
 * Start the design system: tokens + fonts (+ the style's motif master) and an empty component inventory.
 * Components are NOT copied from the kit: every master is designed for the product from the inventory;
 * the kit is a read-only reference ("dfw system ref"). Existing tokens are kept unless force is set.
 */
export function systemInit(root, { preset = 'crisp', style = null, fonts = 'vi', force = false } = {}) {
  const p = projectPaths(root);
  const sys = path.join(p.design, 'system');
  fs.mkdirSync(path.join(sys, 'components'), { recursive: true });
  const inventoryCreated = ensureInventory(root);
  if (style) {
    if (!force && !isPlaceholderTokens(path.join(p.tokens, 'tokens.json'))) throw new DfwError('design/tokens/tokens.json already holds a design system; a style guide replaces its tokens, fonts and motif master. Pass --force to replace them.');
    applyStyle(root, style, { fonts });
    buildTokens(root);
    buildSystem(root);
    return { preset: `style:${style}`, tokensWritten: true, inventoryCreated };
  }
  const tokFile = path.join(p.tokens, 'tokens.json');
  let tokensWritten = false;
  const { tokens, head } = presetFiles(preset);
  if (force || isPlaceholderTokens(tokFile)) {
    fs.mkdirSync(p.tokens, { recursive: true });
    fs.copyFileSync(tokens, tokFile);
    tokensWritten = true;
  }
  const headFile = path.join(sys, 'head.html');
  if (force || !fs.existsSync(headFile)) fs.writeFileSync(headFile, head);
  buildTokens(root);
  buildSystem(root);
  return { preset, tokensWritten, inventoryCreated };
}

/** Write presets as token themes "opt-<name>" so the same screens can be compared in the Studio. */
export function systemOptions(root, names, { fonts = 'vi' } = {}) {
  if (names.length < 2 || names.length > 3) throw new DfwError('Give 2-3 style guides or presets to compare, e.g. dfw system options industrial-utility swiss-grid ops-dense');
  // Validate every name before touching the current options: a typo must not leave half the themes replaced.
  for (const n of names) isStyle(n) ? getStyle(n) : presetFiles(n);
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

/** Promote one option (or preset) to the base tokens and remove the option themes. The option's tokens win, edits included. */
export function systemChoose(root, name, { fonts = 'vi' } = {}) {
  const p = projectPaths(root);
  const themesDir = path.join(p.tokens, 'themes');
  if (isStyle(name)) {
    // Fonts, motif, style.json and rationale come from the guide; the tokens come from the option theme the user
    // compared and approved, so edits made to opt-<id>.json during the comparison are kept.
    const opt = path.join(themesDir, `opt-${name}.json`);
    const edited = fs.existsSync(opt) ? fs.readFileSync(opt) : null;
    applyStyle(root, name, { fonts });
    if (edited) fs.writeFileSync(path.join(p.tokens, 'tokens.json'), edited);
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
