#!/usr/bin/env node
// Vendors the Lucide icon set (ISC) into core/assets/icons/lucide.json at a pinned tag.
// Usage: node scripts/vendor-icons.mjs [--tag 1.64.0] [--from <existing lucide checkout>]
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const opt = (n, d) => (args.includes(`--${n}`) ? args[args.indexOf(`--${n}`) + 1] : d);
const tag = opt('tag', '1.64.0');
const git = (cwd, ...a) => execFileSync('git', a, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();

let src = opt('from');
if (!src) {
  src = fs.mkdtempSync(path.join(os.tmpdir(), 'lucide-'));
  git(src, 'clone', '-q', '--depth', '1', '--branch', tag, '--filter=blob:none', '--sparse', 'https://github.com/lucide-icons/lucide.git', '.');
  git(src, 'sparse-checkout', 'set', 'icons');
} else {
  git(src, 'fetch', '-q', '--depth', '1', '--filter=blob:none', 'origin', 'tag', tag);
  git(src, 'checkout', '-q', tag);
}
const sha = git(src, 'rev-parse', 'HEAD');
const out = path.join(ROOT, 'core', 'assets', 'icons');
fs.rmSync(path.join(out, 'lucide'), { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });
// One bundle file: { name: [innerSvg, tags...] }. Every Lucide icon shares the same root attributes.
const icons = {};
for (const f of fs.readdirSync(path.join(src, 'icons')).sort()) {
  if (!f.endsWith('.svg')) continue;
  const name = f.slice(0, -4);
  const svg = fs.readFileSync(path.join(src, 'icons', f), 'utf8').replace(/<!--[\s\S]*?-->/g, '');
  const inner = svg.replace(/^[\s\S]*?<svg[^>]*>/, '').replace(/<\/svg>[\s\S]*$/, '').replace(/\s*\n\s*/g, '').replace(/\s+\/>/g, '/>');
  const metaFile = path.join(src, 'icons', `${name}.json`);
  const meta = fs.existsSync(metaFile) ? JSON.parse(fs.readFileSync(metaFile, 'utf8')) : {};
  icons[name] = [inner, ...new Set([...(meta.tags || []).filter((t) => !t.startsWith('$')), ...(meta.categories || [])])];
}
fs.writeFileSync(path.join(out, 'LICENSE-lucide'), fs.readFileSync(path.join(src, 'LICENSE'), 'utf8'));
const svgRoot = '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">';
fs.writeFileSync(path.join(out, 'lucide.json'), JSON.stringify({ source: 'https://github.com/lucide-icons/lucide', tag, commit: sha, license: 'ISC', svgRoot, icons }) + '\n');
const index = icons;
console.log(`Vendored ${Object.keys(index).length} Lucide icons (${tag}, ${sha.slice(0, 10)}) into ${path.relative(ROOT, out)}`);
