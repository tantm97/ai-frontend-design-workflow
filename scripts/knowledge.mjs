#!/usr/bin/env node
// Vendored knowledge files (registry/skills.json "knowledge"): one upstream file pinned to a commit,
// verified by sha256, split at its "## " headings into parts (one reference file each, so a phase
// loads only what it needs), each wrapped with attribution + license.
//   sync                 re-download every entry at its pinned commit and rewrite its reference file
//   check                compare each pin with the upstream default branch (no changes)
//   bump <id> [--to sha] move one entry to a new commit (prints the new sha256; review the diff, then commit)
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const REG = path.join(ROOT, 'registry', 'skills.json');
const sha256 = (s) => crypto.createHash('sha256').update(s).digest('hex');
const raw = (repo, commit, file) => `https://raw.githubusercontent.com/${repo}/${commit}/${file}`;

async function get(url) {
  const res = await fetch(url, { signal: AbortSignal.timeout(30000) });
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return res.text();
}

/** The reference file dfw ships: header, untouched upstream text between markers, license. */
export function wrap(k, body, license) {
  return `<!-- Vendored from ${k.repo}@${k.commit} ${k.path} (${k.license}). Do not edit: "npm run knowledge -- sync" regenerates it from the pin in registry/skills.json. -->
# ${k.title}

${k.usage}

<!-- upstream:begin -->
${body}<!-- upstream:end -->

## License (${k.license}, applies to the text between the upstream markers)

${license.trim()}
`;
}

/** The upstream text inside a vendored file (for validation). */
export function unwrap(text) {
  const m = /<!-- upstream:begin -->\n([\s\S]*)<!-- upstream:end -->/.exec(text);
  return m ? m[1] : null;
}

/** Markdown split at its "## " headings: the text before the first one, then each section in order. */
export function sections(body) {
  const at = [...body.matchAll(/^## (.+)$/gm)];
  const end = (i) => (i + 1 < at.length ? at[i + 1].index : body.length);
  return {
    pre: body.slice(0, at.length ? at[0].index : body.length),
    list: at.map((m, i) => ({ heading: m[1], text: body.slice(m.index, end(i)) })),
  };
}

/** The upstream text of each part: its sections in upstream order, the first part also the preamble. */
export function splitParts(k, body) {
  const { pre, list } = sections(body);
  const headings = list.map((s) => s.heading);
  if (headings.join('\n') !== k.sections.join('\n'))
    throw new Error(`${k.id}: upstream sections are now ${headings.join(', ')}; update "sections" and "parts" in registry/skills.json`);
  const assigned = k.parts.flatMap((p) => p.sections);
  if (assigned.length !== headings.length || headings.some((h) => !assigned.includes(h)))
    throw new Error(`${k.id}: every section must be in exactly one part`);
  return k.parts.map((p, i) => ({
    part: p,
    body:
      (i === 0 ? pre : '') +
      list
        .filter((s) => p.sections.includes(s.heading))
        .map((s) => s.text)
        .join(''),
  }));
}

/** The upstream text put back together from the parts' bodies, or null when a part is out of shape. */
export function joinParts(k, bodies) {
  const found = new Map();
  for (const [i, b] of bodies.entries()) {
    const { pre, list } = sections(b);
    if (i > 0 && pre) return null;
    for (const s of list) found.set(s.heading, s.text);
  }
  if (found.size !== k.sections.length || k.sections.some((h) => !found.has(h))) return null;
  return sections(bodies[0]).pre + k.sections.map((h) => found.get(h)).join('');
}

async function sync(k) {
  const body = await get(raw(k.repo, k.commit, k.path));
  if (sha256(body) !== k.sha256) throw new Error(`${k.id}: upstream content at ${k.commit} does not match the pinned sha256`);
  const license = await get(raw(k.repo, k.commit, k.licenseFile));
  for (const { part, body: text } of splitParts(k, body))
    fs.writeFileSync(path.join(ROOT, part.dest), wrap({ ...k, ...part }, text, license));
  return `${k.id}: ${k.parts.map((p) => p.dest).join(', ')} (${body.length} bytes) at ${k.commit.slice(0, 12)}`;
}

async function main() {
  const [cmd, id] = process.argv.slice(2);
  const reg = JSON.parse(fs.readFileSync(REG, 'utf8'));
  const list = reg.knowledge || [];
  if (cmd === 'sync') for (const k of list) console.log(await sync(k));
  else if (cmd === 'check') {
    for (const k of list) {
      const head = execFileSync('git', ['ls-remote', `https://github.com/${k.repo}`, 'HEAD'], { encoding: 'utf8' }).split(/\s/)[0];
      const upstream = await get(raw(k.repo, head, k.path));
      console.log(
        `${k.id}: pinned ${k.commit.slice(0, 12)}, upstream ${head.slice(0, 12)}${sha256(upstream) === k.sha256 ? ' (same content)' : ' (CONTENT CHANGED: npm run knowledge -- bump ' + k.id + ')'}`,
      );
    }
  } else if (cmd === 'bump') {
    const k = list.find((x) => x.id === id);
    if (!k) throw new Error(`Unknown knowledge id "${id}"`);
    const to = process.argv.includes('--to')
      ? process.argv[process.argv.indexOf('--to') + 1]
      : execFileSync('git', ['ls-remote', `https://github.com/${k.repo}`, 'HEAD'], { encoding: 'utf8' }).split(/\s/)[0];
    const body = await get(raw(k.repo, to, k.path));
    // Edit the pin as text so registry/skills.json keeps its formatting.
    const text = fs.readFileSync(REG, 'utf8').replace(k.commit, to).replace(k.sha256, sha256(body));
    fs.writeFileSync(REG, text);
    Object.assign(k, { commit: to, sha256: sha256(body) });
    console.log(await sync(k));
    console.log('Review "git diff", then run npm run build && npm run validate.');
  } else {
    console.log('Usage: npm run knowledge -- sync | check | bump <id> [--to <sha>]');
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url))
  main().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
