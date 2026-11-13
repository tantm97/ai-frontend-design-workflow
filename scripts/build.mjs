#!/usr/bin/env node
// Build vendored plugins from the lock (the lock is the INPUT; builds are reproducible).
//   node scripts/build.mjs            rebuild every vendored plugin + marketplace.json + notices
//   node scripts/build.mjs --only id  rebuild one plugin
//   node scripts/build.mjs --check    rebuild into a temp dir and fail if committed plugins differ
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { buildAll, REPO_ROOT, loadRegistry, listFiles } from './lib/build.mjs';

const args = process.argv.slice(2);
const only = args.includes('--only') ? args[args.indexOf('--only') + 1].split(',') : undefined;

try {
  if (args.includes('--check')) {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-build-check-'));
    buildAll({ only, outRoot: tmp });
    const diffs = [];
    const targets = [...(only ? [] : (loadRegistry().local || []).map((l) => ({ plugin: l.plugin }))), ...loadRegistry().skills.filter((s) => !only || only.includes(s.id))];
    for (const s of targets) {
      const a = path.join(REPO_ROOT, 'plugins', s.plugin);
      const b = path.join(tmp, s.plugin);
      const fa = new Set(listFiles(a));
      const fb = new Set(listFiles(b));
      for (const f of fb) if (!fa.has(f)) diffs.push(`${s.plugin}/${f}: missing in committed copy`);
      for (const f of fa) if (!fb.has(f)) diffs.push(`${s.plugin}/${f}: not produced by the build (hand edit?)`);
      for (const f of fa) if (fb.has(f) && !fs.readFileSync(path.join(a, f)).equals(fs.readFileSync(path.join(b, f)))) diffs.push(`${s.plugin}/${f}: content differs from a clean build`);
    }
    fs.rmSync(tmp, { recursive: true, force: true });
    if (diffs.length) {
      console.error(`Build check failed (${diffs.length}):\n${diffs.slice(0, 50).join('\n')}`);
      process.exit(1);
    }
    console.log('Build check: committed plugins match a clean build from the lock.');
  } else {
    const res = buildAll({ only });
    if (!only) console.log('assembled dfw (core + adapters/claude-code)');
    for (const r of res) console.log(`built ${r.id} @ ${r.sha.slice(0, 7)} (${r.overlay.length} overlay op(s))`);
  }
} catch (e) {
  console.error(`build failed: ${e.message}`);
  process.exit(1);
}
