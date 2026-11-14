// The single-file dfw binary (Node SEA, built per platform by CI and attached to the GitHub
// release v<version>). "dfw setup --binary" downloads the one matching this plugin version and
// verifies it against the release's SHA256SUMS; bin/dfw then prefers it over Node.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { PLUGIN_ROOT, globalDir, readJson, DfwError, IS_BINARY } from './util.mjs';

export const RELEASE_REPO = 'tantm97/ai-frontend-design-workflow';

export function dfwVersion() {
  return readJson(path.join(PLUGIN_ROOT, '.claude-plugin', 'plugin.json'), {})?.version || process.env.DFW_VERSION || '0.0.0';
}

/** Release asset name for this machine, e.g. dfw-0.4.0-darwin-arm64 / dfw-0.4.0-win32-x64.exe. */
export function assetName(version = dfwVersion(), platform = process.platform, arch = process.arch) {
  return `dfw-${version}-${platform}-${arch}${platform === 'win32' ? '.exe' : ''}`;
}

export function binaryPath(version = dfwVersion()) {
  return path.join(globalDir(), 'bin', version, process.platform === 'win32' ? 'dfw.exe' : 'dfw');
}

export function binaryStatus() {
  const version = dfwVersion();
  const p = binaryPath(version);
  return { running: IS_BINARY, installed: fs.existsSync(p), path: p, version };
}

async function get(url) {
  const res = await fetch(url, { redirect: 'follow', signal: AbortSignal.timeout(120000) });
  if (!res.ok) throw new DfwError(`HTTP ${res.status} for ${url}`);
  return Buffer.from(await res.arrayBuffer());
}

/** Download + verify the binary for this plugin version into ~/.config/dfw/bin/<version>/. */
export async function installBinary({ version = dfwVersion(), log = console.error } = {}) {
  const name = assetName(version);
  const base = `https://github.com/${RELEASE_REPO}/releases/download/v${version}`;
  log(`Downloading ${name} from github.com/${RELEASE_REPO} (release v${version}) ...`);
  let sums;
  try {
    sums = (await get(`${base}/SHA256SUMS`)).toString('utf8');
  } catch (e) {
    throw new DfwError(`No dfw binary release for v${version} yet (${e.message}). dfw keeps running on Node.`);
  }
  const want = sums
    .split('\n')
    .map((l) => l.trim().split(/\s+\*?/))
    .find(([, f]) => f === name)?.[0];
  if (!want) throw new DfwError(`Release v${version} has no binary for ${process.platform}-${process.arch}. dfw keeps running on Node.`);
  const buf = await get(`${base}/${name}`);
  const got = crypto.createHash('sha256').update(buf).digest('hex');
  if (got !== want) throw new DfwError(`Checksum mismatch for ${name} (expected ${want}, got ${got}); not installed.`);
  const dest = binaryPath(version);
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  const tmp = `${dest}.download`;
  fs.writeFileSync(tmp, buf, { mode: 0o755 });
  fs.renameSync(tmp, dest);
  return { installed: true, path: dest, version, sha256: got };
}
