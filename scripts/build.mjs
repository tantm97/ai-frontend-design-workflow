#!/usr/bin/env node
// Build vendored plugins from the lock (the lock is the INPUT; builds are reproducible).
//   node scripts/build.mjs            the Studio's injected bundles (build-overlay.mjs), then every
//                                     plugin + marketplace.json + notices
//   node scripts/build.mjs --only id  rebuild one plugin
//   node scripts/build.mjs --check    rebuild into a temp dir and fail if committed plugins or the
//                                     committed bundles differ
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { buildAll, REPO_ROOT, loadRegistry, listFiles, strayPlugins } from './lib/build.mjs';

const args = process.argv.slice(2);
const only = args.includes('--only') ? args[args.indexOf('--only') + 1].split(',') : undefined;

/** core/studio/overlay.js and live.js from their modules (the binary embeds them, so first). */
function overlay(check) {
  const r = spawnSync(process.execPath, [path.join(REPO_ROOT, 'scripts', 'build-overlay.mjs'), ...(check ? ['--check'] : [])], {
    stdio: 'inherit',
  });
  if (r.status !== 0) throw new Error(check ? 'the Studio bundles are stale (npm run build:overlay)' : 'build-overlay failed');
}

try {
  if (!only) overlay(args.includes('--check'));
  if (args.includes('--check')) {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-build-check-'));
    buildAll({ only, outRoot: tmp });
    const diffs = [];
    const targets = [
      ...(only ? [] : [...new Set((loadRegistry().local || []).map((l) => l.plugin))].map((plugin) => ({ plugin }))),
      ...loadRegistry().skills.filter((s) => !only || only.includes(s.id)),
    ];
    // A folder in plugins/ that a clean build does not make (a retired plugins/dfw-copilot, a
    // plugin dropped from the registry) would still be committed and listed by nobody.
    if (!only)
      diffs.push(
        ...strayPlugins(
          path.join(REPO_ROOT, 'plugins'),
          targets.map((t) => t.plugin),
        ),
      );
    for (const s of targets) {
      const a = path.join(REPO_ROOT, 'plugins', s.plugin);
      const b = path.join(tmp, s.plugin);
      const fa = new Set(listFiles(a));
      const fb = new Set(listFiles(b));
      for (const f of fb) if (!fa.has(f)) diffs.push(`${s.plugin}/${f}: missing in committed copy`);
      for (const f of fa) if (!fb.has(f)) diffs.push(`${s.plugin}/${f}: not produced by the build (hand edit?)`);
      for (const f of fa)
        if (fb.has(f) && !fs.readFileSync(path.join(a, f)).equals(fs.readFileSync(path.join(b, f))))
          diffs.push(`${s.plugin}/${f}: content differs from a clean build`);
    }
    fs.rmSync(tmp, { recursive: true, force: true });
    if (diffs.length) {
      console.error(`Build check failed (${diffs.length}):\n${diffs.slice(0, 50).join('\n')}`);
      process.exit(1);
    }
    console.log('Build check: committed plugins match a clean build from the lock.');
  } else {
    const res = buildAll({ only });
    if (!only) console.log('assembled dfw (core + adapters/claude-code + adapters/copilot)');
    for (const r of res) console.log(`built ${r.id} @ ${r.sha.slice(0, 7)} (${r.overlay.length} overlay op(s))`);
  }
} catch (e) {
  console.error(`build failed: ${e.message}`);
  process.exit(1);
}
