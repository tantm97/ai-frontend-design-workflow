#!/usr/bin/env node
// Bundles the scripts injected into pages (javascript.md J2): core/studio/overlay/ -> core/studio/overlay.js
// (every design frame and the live-review proxy) and core/studio/live/ -> core/studio/live.js (the
// live-review UI). Each stays ONE classic script (an IIFE, no import, no export), so the Studio, the
// review proxy and the binary serve it as before.
//   node scripts/build-overlay.mjs           write the bundles, print what changed
//   node scripts/build-overlay.mjs --check   exit 1 when a committed bundle differs from its sources
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'esbuild';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const STUDIO = path.join(ROOT, 'core', 'studio');
const BUNDLES = [
  { entry: 'overlay/main.js', out: 'overlay.js' },
  { entry: 'live/main.js', out: 'live.js' },
];
const check = process.argv.includes('--check');

async function bundle({ entry }) {
  const res = await build({
    entryPoints: [path.join(STUDIO, entry)],
    absWorkingDir: ROOT,
    bundle: true,
    format: 'iife',
    platform: 'browser',
    target: 'es2020',
    charset: 'utf8',
    write: false,
    legalComments: 'none',
    logLevel: 'silent',
    banner: {
      js: `// GENERATED from core/studio/${path.dirname(entry)}/ by scripts/build-overlay.mjs: edit the modules there, then run npm run build:overlay.`,
    },
  });
  return res.outputFiles[0].text;
}

let stale = 0;
for (const b of BUNDLES) {
  const file = path.join(STUDIO, b.out);
  const want = await bundle(b);
  const have = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : null;
  if (have === want) {
    console.log(`build:overlay: core/studio/${b.out} up to date`);
    continue;
  }
  if (check) {
    stale++;
    console.error(`build:overlay: core/studio/${b.out} is stale: run npm run build:overlay`);
    continue;
  }
  fs.writeFileSync(file, want);
  console.log(`build:overlay: wrote core/studio/${b.out} (${want.split('\n').length} lines)`);
}
if (stale) process.exit(1);
