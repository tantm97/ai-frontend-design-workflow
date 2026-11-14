// Curated icon library (Lucide, ISC, vendored at a pinned tag in assets/icons/lucide.json).
// Projects copy only the icons they use into design/system/icons/, so designs stay self-contained.
import fs from 'node:fs';
import path from 'node:path';
import { PLUGIN_ROOT, projectPaths, DfwError } from './util.mjs';

let bundle = null;
export function iconBundle() {
  if (!bundle) {
    const f = path.join(PLUGIN_ROOT, 'assets', 'icons', 'lucide.json');
    if (!fs.existsSync(f)) throw new DfwError('Icon library missing (assets/icons/lucide.json). Reinstall dfw.');
    bundle = JSON.parse(fs.readFileSync(f, 'utf8'));
  }
  return bundle;
}

export function iconSvg(name) {
  const b = iconBundle();
  const e = b.icons[name];
  return e ? `${b.svgRoot}${e[0]}</svg>\n` : null;
}

/** Rank icons by name/tag match. */
export function searchIcons(query, limit = 24) {
  const b = iconBundle();
  const words = String(query).toLowerCase().split(/[\s,]+/).filter(Boolean);
  const scored = [];
  for (const [name, [, ...tags]] of Object.entries(b.icons)) {
    let score = name === words.join('-') ? 20 : 0;
    for (const w of words) {
      if (name === w) score += 10;
      else if (name.split('-').includes(w)) score += 6;
      else if (name.includes(w)) score += 3;
      if (tags.includes(w)) score += 4;
      else if (tags.some((t) => t.includes(w))) score += 1;
    }
    if (score) scored.push({ name, score, tags: tags.slice(0, 6) });
  }
  return scored.sort((a, b) => b.score - a.score || a.name.localeCompare(b.name)).slice(0, limit);
}

export function addIcons(root, names) {
  const dir = path.join(projectPaths(root).design, 'system', 'icons');
  fs.mkdirSync(dir, { recursive: true });
  const added = [];
  const missing = [];
  for (const n of names) {
    const svg = iconSvg(n);
    if (!svg) missing.push(n);
    else {
      fs.writeFileSync(path.join(dir, `${n}.svg`), svg);
      added.push(n);
    }
  }
  return { added, missing, suggestions: Object.fromEntries(missing.map((m) => [m, searchIcons(m, 5).map((x) => x.name)])) };
}
