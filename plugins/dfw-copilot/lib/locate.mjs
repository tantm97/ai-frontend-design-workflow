// Locate sibling plugins (Impeccable, Hallmark, Taste reference) and external tools.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { claudeDir, MARKETPLACE_NAME, PLUGIN_ROOT, readJson, writeJson, globalDir, manifestPath } from './util.mjs';
import { allowUnverifiedEngine } from './config.mjs';
import { HOSTS } from './host.mjs';

function newestDir(dir) {
  if (!fs.existsSync(dir)) return null;
  const subs = fs
    .readdirSync(dir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => path.join(dir, d.name))
    .sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs);
  return subs[0] || null;
}

/** Root directory of a plugin from the dfw marketplace, installed or in the dev repo. */
export function pluginRoot(name) {
  // 1. Sibling in the same marketplace checkout (dev mode, or plugin cache layout).
  const sibling = path.resolve(PLUGIN_ROOT, '..', name);
  if (manifestPath(sibling)) return sibling;
  // 1b. Dev checkout: core/ next to plugins/.
  const dev = path.resolve(PLUGIN_ROOT, '..', 'plugins', name);
  if (manifestPath(dev)) return dev;
  // 2. Plugin cache: <claudeDir>/plugins/cache/<marketplace>/<plugin>/<version>/
  const cached = newestDir(path.join(claudeDir(), 'plugins', 'cache', MARKETPLACE_NAME, name));
  if (cached && manifestPath(cached)) return cached;
  // 2b. Other hosts' install directories (e.g. ~/.copilot/installed-plugins/dfw-marketplace/<plugin>).
  for (const h of Object.values(HOSTS)) {
    for (const dir of h.installedPluginDirs?.() || []) {
      const p = path.join(dir, name);
      if (manifestPath(p)) return p;
    }
  }
  // 3. Marketplace repo recorded in global config.
  const repo = readJson(path.join(globalDir(), 'config.json'), {}).marketplaceRepo;
  if (repo) {
    const p = path.join(repo, 'plugins', name);
    if (manifestPath(p)) return p;
  }
  return null;
}

/** Release asset names for a platform. Windows on ARM also runs the x64 build, as the upstream launcher does. */
export function engineAssets(platform, arch) {
  const o = { darwin: 'darwin', linux: 'linux', win32: 'windows' }[platform];
  const a = { arm64: 'arm64', x64: 'x64' }[arch];
  if (!o || !a) return [];
  const exe = o === 'windows' ? '.exe' : '';
  return o === 'windows' && a === 'arm64' ? [`impeccable-windows-arm64${exe}`, `impeccable-windows-x64${exe}`] : [`impeccable-${o}-${a}${exe}`];
}

const isFile = (f) => {
  try {
    return Boolean(f) && fs.statSync(f).isFile();
  } catch {
    return false;
  }
};

/**
 * True when `file` has one of the pinned SHA-256 digests. A passing result is remembered in
 * `stampFile` under path + size + mtime, so a hook does not hash the 19 MB engine on every edit.
 */
export function verifyEngine(file, digests, stampFile) {
  let st;
  try {
    st = fs.statSync(file);
  } catch {
    return false;
  }
  if (!st.isFile() || !digests.length) return false;
  let stamps = {};
  try {
    stamps = readJson(stampFile, {}) || {};
  } catch {
    /* unreadable stamp file: hash again */
  }
  const s = Object.hasOwn(stamps, file) ? stamps[file] : null;
  if (s && s.size === st.size && s.mtimeMs === st.mtimeMs && digests.includes(s.sha256)) return true;
  const sha256 = crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
  if (!digests.includes(sha256)) return false;
  try {
    writeJson(stampFile, { ...stamps, [file]: { size: st.size, mtimeMs: st.mtimeMs, sha256 } });
  } catch {
    /* read-only dfw home: verified now, hashed again next time */
  }
  return true;
}

/**
 * Find the Impeccable engine dfw may run for the skill in `skillDir`.
 * Only the version cache (~/.impeccable/bin/<version>/impeccable[.exe]) is trusted, and only after
 * its SHA-256 matches the vendored engine-checksums.txt. The other places the upstream launcher
 * looks (IMPECCABLE_BIN, a sibling build, ~/.impeccable/bin/impeccable, PATH) are never hashed, so
 * they are used only with `allowUnverified` (global config impeccable.allowUnverifiedEngine).
 * dfw runs `cmd` directly with `env`; it does not go through the sh launcher.
 */
export function resolveEngine({
  skillDir,
  platform = process.platform,
  arch = process.arch,
  env = process.env,
  home = os.homedir(),
  stampFile = path.join(globalDir(), 'engine-verified.json'),
  allowUnverified = false,
}) {
  const scripts = path.join(skillDir, 'scripts');
  const win = platform === 'win32';
  const exe = win ? '.exe' : '';
  const launcher = path.join(scripts, win ? 'impeccable.cmd' : 'impeccable');
  const read = (name) => {
    try {
      return fs.readFileSync(path.join(scripts, name), 'utf8');
    } catch {
      return '';
    }
  };
  const version = read('VERSION').trim().split(/\s+/)[0] || '';
  const assets = engineAssets(platform, arch);
  const digests = read('engine-checksums.txt')
    .split(/\r?\n/)
    .map((l) => l.trim().split(/\s+/))
    .filter(([v, asset, sum]) => version && v === version && assets.includes(asset) && /^[0-9a-f]{64}$/.test(sum || ''))
    .map(([, , sum]) => sum);
  const cached = path.join(env.IMPECCABLE_HOME || path.join(home, '.impeccable'), 'bin', version || 'unknown', `impeccable${exe}`);
  // The launcher's unverified candidates, in its order: two before the version cache, PATH after it.
  const before = [
    env.IMPECCABLE_BIN,
    assets[0] && path.join(scripts, 'bin', assets[0].replace(/^impeccable-|\.exe$/g, ''), `impeccable${exe}`),
    path.join(home, '.impeccable', 'bin', `impeccable${exe}`),
  ].filter(isFile);
  const onPath = String(env.PATH ?? env.Path ?? '')
    .split(win ? ';' : ':')
    .filter(Boolean)
    .flatMap((d) => (win ? ['.exe', '.com', '.cmd', '.bat'] : ['']).map((x) => path.join(d, `impeccable${x}`)))
    .filter(isFile)
    .slice(0, 1);
  const base = { launcher, version, cached, env: { IMPECCABLE_SKILL_DIR: skillDir, IMPECCABLE_SELF: launcher } };
  const use = (cmd, verified) => ({ ...base, cmd, ready: true, verified, unverified: [] });
  if (allowUnverified && before.length) return use(before[0], false);
  if (verifyEngine(cached, digests, stampFile)) return use(cached, true);
  if (allowUnverified && onPath.length) return use(onPath[0], false);
  const setup = 'run "dfw check --setup" once';
  const detail = !digests.length
    ? `No pinned Impeccable engine for ${platform}-${arch}.`
    : isFile(cached)
      ? `Engine at ${cached} does not match the pinned checksum; delete it, then ${setup}.`
      : `Engine not downloaded yet; ${setup}.`;
  return { ...base, cmd: null, ready: false, verified: false, unverified: [...before, ...onPath], detail };
}

/** How to start the upstream launcher (used only to download the engine): no sh on Windows. */
export function launcherCommand(launcher, args, { platform = process.platform, env = process.env } = {}) {
  if (platform !== 'win32') return { cmd: launcher, args, options: {} };
  // A .cmd file cannot be spawned directly; cmd.exe needs the whole command line in one quoted string.
  const line = [launcher, ...args].map((x) => `"${x}"`).join(' ');
  return { cmd: env.ComSpec || 'cmd.exe', args: ['/d', '/s', '/c', `"${line}"`], options: { windowsVerbatimArguments: true } };
}

/** The engine for the installed Impeccable plugin, or null when the plugin is not installed. */
export function impeccableEngine(opts = {}) {
  const root = pluginRoot('impeccable');
  if (!root) return null;
  const skillDir = path.join(root, 'skills', 'impeccable');
  const e = resolveEngine({ skillDir, allowUnverified: allowUnverifiedEngine(), ...opts });
  return fs.existsSync(e.launcher) ? e : null;
}

export function sloplintScript() {
  const root = pluginRoot('hallmark');
  if (!root) return null;
  const p = path.join(root, 'skills', 'hallmark', 'scripts', 'sloplint.mjs');
  return fs.existsSync(p) ? p : null;
}

export function tasteCopyLintScript() {
  const root = pluginRoot('taste-reference');
  if (!root) return null;
  const p = path.join(root, 'skills', 'taste-reference', 'scripts', 'copy-lint.mjs');
  return fs.existsSync(p) ? p : null;
}
