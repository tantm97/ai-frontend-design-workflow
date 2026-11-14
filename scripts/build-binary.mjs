#!/usr/bin/env node
// Build the single-file dfw binary for THIS platform (Node SEA):
//   node scripts/build-binary.mjs [--node <node executable to embed>] [--out dist]
// 1. esbuild bundles scripts/sea/entry.mjs + plugins/dfw/lib into one CommonJS file
// 2. the plugin's data files (everything but bin/ and lib/) become the asset plugin.json.gz
// 3. Node writes the SEA blob (with V8 code cache) and postject injects it into a copy of node
// The node you embed must be a portable build: the official nodejs.org binary (CI uses
// actions/setup-node). Distro/Homebrew builds link shared libraries and only run locally.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const PLUGIN = path.join(REPO, 'plugins', 'dfw');
const require = createRequire(path.join(REPO, 'package.json'));
const argv = process.argv.slice(2);
const opt = (n, d) => (argv.includes(`--${n}`) ? argv[argv.indexOf(`--${n}`) + 1] : d);

const nodeExe = path.resolve(opt('node', process.execPath));
const outDir = path.resolve(REPO, opt('out', 'dist'));
const work = path.join(REPO, '.cache', 'sea');
const version = JSON.parse(fs.readFileSync(path.join(PLUGIN, '.claude-plugin', 'plugin.json'), 'utf8')).version;
const plat = process.platform;
const name = `dfw-${version}-${plat}-${process.arch}${plat === 'win32' ? '.exe' : ''}`;
const log = (s) => console.log(`› ${s}`);
const sh = (cmd, args) => {
  const r = spawnSync(cmd, args, { stdio: 'inherit' });
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(' ')} failed`);
};

fs.rmSync(work, { recursive: true, force: true });
fs.mkdirSync(work, { recursive: true });
fs.mkdirSync(outDir, { recursive: true });

// 1. Bundle.
log(`bundle dfw ${version}`);
const esbuild = require('esbuild');
await esbuild.build({
  entryPoints: [path.join(REPO, 'scripts', 'sea', 'entry.mjs')],
  outfile: path.join(work, 'dfw.cjs'),
  bundle: true,
  platform: 'node',
  format: 'cjs',
  target: 'node22',
  // No minify: functions sent to the browser (page.evaluate) are serialized from source.
  minify: false,
  legalComments: 'none',
  logLevel: 'warning',
  // import.meta.url is only read when DFW_PLUGIN_ROOT is unset, which the entry never allows.
  logOverride: { 'empty-import-meta': 'silent' },
});

// 2. Embedded plugin files.
const files = {};
const walk = (dir, rel = '') => {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const r = rel ? `${rel}/${e.name}` : e.name;
    if (!rel && (e.name === 'bin' || e.name === 'lib')) continue;
    if (e.name === '.DS_Store') continue;
    const f = path.join(dir, e.name);
    if (e.isDirectory()) walk(f, r);
    else if (e.isFile()) files[r] = { mode: fs.statSync(f).mode & 0o777, b64: fs.readFileSync(f).toString('base64') };
  }
};
walk(PLUGIN);
const hash = crypto.createHash('sha256').update(JSON.stringify(files)).digest('hex').slice(0, 12);
const asset = path.join(work, 'plugin.json.gz');
fs.writeFileSync(asset, zlib.gzipSync(JSON.stringify({ version, hash, files }), { level: 9 }));
log(`embedded ${Object.keys(files).length} plugin files (${(fs.statSync(asset).size / 1024).toFixed(0)} KB gz)`);

// 3. SEA blob + inject.
const config = path.join(work, 'sea-config.json');
const blob = path.join(work, 'sea-prep.blob');
fs.writeFileSync(config, JSON.stringify({ main: path.join(work, 'dfw.cjs'), output: blob, disableExperimentalSEAWarning: true, useCodeCache: true, assets: { 'plugin.json.gz': asset } }, null, 2));
log(`sea blob with ${nodeExe} (node ${spawnSync(nodeExe, ['--version'], { encoding: 'utf8' }).stdout.trim()})`);
sh(nodeExe, ['--experimental-sea-config', config]);

const out = path.join(outDir, name);
fs.copyFileSync(nodeExe, out);
fs.chmodSync(out, 0o755);
if (plat === 'darwin') sh('codesign', ['--remove-signature', out]);
log(`inject into ${path.relative(REPO, out)}`);
const { inject } = require('postject');
await inject(out, 'NODE_SEA_BLOB', fs.readFileSync(blob), {
  sentinelFuse: 'NODE_SEA_FUSE_fce680ab2cc467b6e072b8b5df1996b2',
  ...(plat === 'darwin' ? { machoSegmentName: 'NODE_SEA' } : {}),
});
// macOS refuses to run an unsigned arm64 binary: ad-hoc sign it.
if (plat === 'darwin') sh('codesign', ['--sign', '-', out]);

const sha = crypto.createHash('sha256').update(fs.readFileSync(out)).digest('hex');
fs.writeFileSync(`${out}.sha256`, `${sha}  ${name}\n`);
log(`${name}: ${(fs.statSync(out).size / 1e6).toFixed(1)} MB, sha256 ${sha}`);
