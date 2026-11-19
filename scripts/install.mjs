#!/usr/bin/env node
// One-command installer for machines with Node.js (without Node use install.sh):
//   npx github:tantm97/ai-frontend-design-workflow            (install, user scope)
//   npx github:tantm97/ai-frontend-design-workflow install --scope project --no-runtime
//   npx github:tantm97/ai-frontend-design-workflow uninstall
// It downloads the native dfw binary of this version from the GitHub release, checks it against
// the release's SHA256SUMS, puts it in ~/.config/dfw/bin/<version>/ and hands the command over to
// it ("dfw install|uninstall|doctor"): the binary adds the dfw marketplace, installs dfw and the
// design skills, the headless browser if none is found and the Impeccable engine, then runs doctor.
// DFW_BIN=<executable> uses that binary instead of downloading one (development).
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const REPO = 'tantm97/ai-frontend-design-workflow';
const VERSION = JSON.parse(fs.readFileSync(path.join(ROOT, 'core', 'manifest.json'), 'utf8')).version;

function help() {
  console.log(`ai-frontend-design-workflow (dfw ${VERSION}) installer

  npx github:${REPO} [install] [--host auto|claude|copilot|all] [--scope user|project|local] [--source <path|owner/repo>] [--no-runtime]
  npx github:${REPO} uninstall [--host ...] [--scope ...]
  npx github:${REPO} doctor

  install       download the dfw binary (checked against the release's SHA256SUMS), add the marketplace,
                install dfw + the design skills it depends on, make sure a headless browser exists,
                set up the Impeccable engine, run doctor
  --host        auto (default: every supported tool found on PATH), claude, copilot, all
  --scope       user (default, all projects), project (shared in .claude/settings.json), local
  --source      marketplace source; default ${REPO} (use a local clone path while developing)
  --no-runtime  skip the browser check/download and the Impeccable engine

  No Node.js? curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | sh`);
}

/** Release asset name for this machine, as install.sh and the release workflow name it. */
export function assetName(version, platform = process.platform, arch = process.arch) {
  return `dfw-${version}-${platform}-${arch}${platform === 'win32' ? '.exe' : ''}`;
}

/** The hash SHA256SUMS lists for a file name, or null. */
export function wantedSum(sums, name) {
  for (const line of sums.split('\n')) {
    const [hash, file] = line.trim().split(/\s+\*?/);
    if (file === name) return hash;
  }
  return null;
}

async function fetchBytes(url) {
  const res = await fetch(url, { redirect: 'follow', signal: AbortSignal.timeout(120_000) });
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return Buffer.from(await res.arrayBuffer());
}

/** Download, verify and install the binary of VERSION; its path. */
async function installBinary() {
  const home = process.env.DFW_HOME || path.join(process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config'), 'dfw');
  const dest = path.join(home, 'bin', VERSION, process.platform === 'win32' ? 'dfw.exe' : 'dfw');
  const name = assetName(VERSION);
  const base = `https://github.com/${REPO}/releases/download/v${VERSION}`;
  console.log(`› dfw ${VERSION} for ${process.platform}-${process.arch}`);
  let sums;
  try {
    sums = (await fetchBytes(`${base}/SHA256SUMS`)).toString('utf8');
  } catch (e) {
    throw new Error(`No dfw release v${VERSION} to download (${e.message}).`, { cause: e });
  }
  const want = wantedSum(sums, name);
  if (!want) throw new Error(`Release v${VERSION} has no binary for ${process.platform}-${process.arch}.`);
  const buf = await fetchBytes(`${base}/${name}`);
  const got = crypto.createHash('sha256').update(buf).digest('hex');
  if (got !== want) throw new Error(`Checksum mismatch for ${name} (expected ${want}, got ${got}); nothing installed.`);
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.writeFileSync(`${dest}.download`, buf, { mode: 0o755 });
  fs.renameSync(`${dest}.download`, dest);
  console.log(`  installed ${dest}`);
  return dest;
}

async function main(argv) {
  const cmd = argv[0] && !argv[0].startsWith('-') ? argv[0] : 'install';
  if (cmd === 'help' || argv.includes('--help') || argv.includes('-h')) return help();
  if (!['install', 'uninstall', 'doctor'].includes(cmd)) {
    help();
    process.exitCode = 1;
    return;
  }
  const bin = process.env.DFW_BIN || (await installBinary());
  const r = spawnSync(bin, cmd === argv[0] ? argv : [cmd, ...argv], { stdio: 'inherit' });
  if (r.error) throw r.error;
  process.exitCode = r.status ?? 1;
}

// Run as a program (npx starts it through a link in node_modules/.bin), not when a test imports it.
if (process.argv[1] && fs.realpathSync(process.argv[1]) === fs.realpathSync(fileURLToPath(import.meta.url))) {
  main(process.argv.slice(2)).catch((e) => {
    console.error(`\x1b[31m${e.message}\x1b[0m`);
    process.exitCode = 1;
  });
}
