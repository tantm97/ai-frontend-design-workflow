// Component inventory: the design system is designed for the product, not copied from a kit.
// design/system/inventory.json lists every component the product needs, why, and which pages use it.
// The starter kit (kits/base/components) is a read-only reference (`dfw system ref`), never copied.
import fs from 'node:fs';
import path from 'node:path';
import { PLUGIN_ROOT, projectPaths, readJson, writeJson, DfwError } from './util.mjs';

export const KIT_COMPONENTS = path.join(PLUGIN_ROOT, 'kits', 'base', 'components');

export function inventoryPath(root) {
  return path.join(projectPaths(root).design, 'system', 'inventory.json');
}

const SKELETON = {
  $comment:
    'Component inventory (dfw system phase). One entry per component the product needs, derived from PRODUCT.md workflows and the screen blueprints, never from a kit list. name = master file name; purpose = the job it does in THIS product; usedBy = pages/screens that need it; states = every state its CSS must cover; ref = optional kit reference consulted with "dfw system ref <name>" (a checklist, never copied).',
  components: [],
};

export function loadInventory(root) {
  const f = inventoryPath(root);
  if (!fs.existsSync(f)) return null;
  const inv = readJson(f, null);
  if (!inv || !Array.isArray(inv.components)) throw new DfwError(`${path.relative(root, f)}: expected {"components": [...]}`);
  return inv;
}

/** Create the inventory skeleton if missing. Returns true when written. */
export function ensureInventory(root) {
  const f = inventoryPath(root);
  if (fs.existsSync(f)) return false;
  fs.mkdirSync(path.dirname(f), { recursive: true });
  writeJson(f, SKELETON);
  return true;
}

const norm = (s) => String(s || '').replace(/<!--[\s\S]*?-->/g, '').replace(/\s+/g, ' ').trim();
const block = (text, tag) => norm((text.match(new RegExp(`<${tag}[^>]*>([\\s\\S]*?)</${tag}>`, 'i')) || [])[1]);

/** Kit references: name, group, description, variants/sizes and the states their CSS covers. */
export function kitRefs() {
  if (!fs.existsSync(KIT_COMPONENTS)) return [];
  return fs
    .readdirSync(KIT_COMPONENTS)
    .filter((f) => f.endsWith('.html'))
    .sort()
    .map((f) => {
      const text = fs.readFileSync(path.join(KIT_COMPONENTS, f), 'utf8');
      const open = (text.match(/<dfw-component\b[^>]*>/) || [''])[0];
      const attr = (n) => (open.match(new RegExp(`\\b${n}="([^"]*)"`)) || [])[1] || '';
      const css = block(text, 'style');
      const states = ['hover', 'focus-visible', 'active', 'disabled', 'aria-busy', 'aria-invalid', 'aria-selected', 'aria-current', 'checked', 'placeholder', 'empty'].filter((s) => css.includes(s));
      return { name: f.replace(/\.html$/, ''), group: attr('group'), description: attr('description'), variants: attr('variants'), sizes: attr('sizes'), states };
    });
}

/** Full source of one kit master, for reading as a reference. */
export function kitRef(name) {
  const f = path.join(KIT_COMPONENTS, `${name}.html`);
  if (!fs.existsSync(f)) throw new DfwError(`No kit reference "${name}". Available: ${kitRefs().map((r) => r.name).join(', ')}`);
  return fs.readFileSync(f, 'utf8');
}

let kitCache = null;
function kitFingerprints() {
  if (kitCache) return kitCache;
  kitCache = { styles: new Map(), templates: new Map(), examples: new Map() };
  if (!fs.existsSync(KIT_COMPONENTS)) return kitCache;
  for (const f of fs.readdirSync(KIT_COMPONENTS).filter((x) => x.endsWith('.html'))) {
    const t = fs.readFileSync(path.join(KIT_COMPONENTS, f), 'utf8');
    const name = f.replace(/\.html$/, '');
    for (const [k, tag] of [['styles', 'style'], ['templates', 'template'], ['examples', 'dfw-examples']]) {
      const b = block(t, tag);
      if (b) kitCache[k].set(b, name);
    }
  }
  return kitCache;
}

/** Names used in screens and in other masters (c="name"). */
function usages(root) {
  const used = new Map();
  const scan = (dir, label) => {
    if (!fs.existsSync(dir)) return;
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const abs = path.join(dir, e.name);
      if (e.isDirectory()) scan(abs, label || e.name);
      else if (e.name.endsWith('.html')) {
        const t = fs.readFileSync(abs, 'utf8').replace(/<dfw-examples>[\s\S]*?<\/dfw-examples>/g, '');
        for (const m of t.matchAll(/<dfw-use\b[^>]*?\bc="([a-z0-9-]+)"/g)) {
          if (!used.has(m[1])) used.set(m[1], new Set());
          used.get(m[1]).add(label || e.name);
        }
      }
    }
  };
  const d = projectPaths(root).design;
  scan(path.join(d, 'screens'), null);
  scan(path.join(d, 'system', 'components'), 'system');
  return used;
}

/**
 * Inventory and kit-copy findings for the design system.
 * - kit-copy (MUST): a master whose styles + template, or whose examples, are the starter kit's verbatim.
 * - inventory-missing / inventory-orphan / inventory-unbuilt (SHOULD; MUST at the DS gate).
 * - unused-master (SHOULD): once real screens exist, a master no screen or other master uses.
 */
export function inventoryFindings(root) {
  const p = projectPaths(root);
  const dir = path.join(p.design, 'system', 'components');
  const findings = [];
  const add = (level, rule, file, message) => findings.push({ level, rule, file, line: 1, message, source: 'dfw-ds' });
  const masters = fs.existsSync(dir) ? fs.readdirSync(dir).filter((f) => f.endsWith('.html')).sort() : [];
  const kit = kitFingerprints();
  const names = [];
  for (const f of masters) {
    const rel = `design/system/components/${f}`;
    const text = fs.readFileSync(path.join(dir, f), 'utf8');
    for (const m of text.matchAll(/<dfw-component\b[^>]*\bname="([a-z0-9-]+)"/g)) names.push({ name: m[1], rel });
    const st = block(text, 'style');
    const tp = block(text, 'template');
    const ex = block(text, 'dfw-examples');
    if (st && tp && kit.styles.has(st) && kit.templates.has(tp)) {
      add('error', 'kit-copy', rel, `Master is the starter kit's "${kit.templates.get(tp)}" copied verbatim. Design it for this product (its job, content, states and the direction's look); the kit is a reference only ("dfw system ref ${kit.templates.get(tp)}").`);
    } else if (ex && kit.examples.has(ex)) {
      add('error', 'kit-copy', rel, `Examples are the starter kit's "${kit.examples.get(ex)}" examples (another product's content). Show this product's real content in every variant and state.`);
    }
  }
  const inv = (() => {
    try {
      return loadInventory(root);
    } catch (e) {
      add('error', 'inventory-invalid', 'design/system/inventory.json', e.message);
      return undefined;
    }
  })();
  if (inv === undefined) return findings;
  if (!inv || !inv.components.length) {
    if (masters.length) add('warn', 'inventory-missing', 'design/system/inventory.json', 'No component inventory. List every component this product needs in design/system/inventory.json (name, purpose, usedBy, states) before designing masters, so the system holds only what the screens use.');
    return findings;
  }
  const listed = new Map(inv.components.map((c) => [c.name, c]));
  for (const c of inv.components) {
    if (!c.name) add('warn', 'inventory-invalid', 'design/system/inventory.json', 'An inventory entry has no name.');
    else if (!c.purpose || !Array.isArray(c.usedBy) || !c.usedBy.length) add('warn', 'inventory-invalid', 'design/system/inventory.json', `Inventory entry "${c.name}" needs a purpose and at least one usedBy page/screen: a component nobody needs does not belong in the system.`);
    if (c.name && !names.some((n) => n.name === c.name)) add('warn', 'inventory-unbuilt', 'design/system/inventory.json', `Inventory lists "${c.name}" but there is no master for it in design/system/components/.`);
  }
  for (const n of names) if (!listed.has(n.name)) add('warn', 'inventory-orphan', n.rel, `Master "${n.name}" is not in design/system/inventory.json. Add it with the screens that need it, or delete it.`);
  let m = {};
  try {
    m = readJson(p.manifest, {});
  } catch {
    /* a malformed design.json is reported by lint (manifest-json) */
  }
  const realScreens = (m.screens || []).filter((s) => !String(s.id).startsWith('_'));
  if (realScreens.length) {
    const used = usages(root);
    for (const n of names) if (!used.has(n.name)) add('warn', 'unused-master', n.rel, `No screen or master uses "${n.name}". Remove it (and its inventory entry) or use it.`);
  }
  return findings;
}

/** Rules that block the DS gate (escalated from SHOULD). */
export const DS_GATE_RULES = new Set(['inventory-missing', 'inventory-orphan', 'inventory-unbuilt', 'inventory-invalid']);
