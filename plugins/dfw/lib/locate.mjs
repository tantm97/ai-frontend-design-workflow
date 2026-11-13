// Locate sibling plugins (Impeccable, Hallmark, Taste reference) and external tools.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { claudeDir, MARKETPLACE_NAME, PLUGIN_ROOT, readJson, globalDir } from './util.mjs';

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
  if (fs.existsSync(path.join(sibling, '.claude-plugin', 'plugin.json'))) return sibling;
  // 1b. Dev checkout: core/ next to plugins/.
  const dev = path.resolve(PLUGIN_ROOT, '..', 'plugins', name);
  if (fs.existsSync(path.join(dev, '.claude-plugin', 'plugin.json'))) return dev;
  // 2. Plugin cache: <claudeDir>/plugins/cache/<marketplace>/<plugin>/<version>/
  const cached = newestDir(path.join(claudeDir(), 'plugins', 'cache', MARKETPLACE_NAME, name));
  if (cached && fs.existsSync(path.join(cached, '.claude-plugin', 'plugin.json'))) return cached;
  // 3. Marketplace repo recorded in global config.
  const repo = readJson(path.join(globalDir(), 'config.json'), {}).marketplaceRepo;
  if (repo) {
    const p = path.join(repo, 'plugins', name);
    if (fs.existsSync(path.join(p, '.claude-plugin', 'plugin.json'))) return p;
  }
  return null;
}

export function impeccableLauncher() {
  if (process.env.IMPECCABLE_BIN && fs.existsSync(process.env.IMPECCABLE_BIN)) {
    return { cmd: process.env.IMPECCABLE_BIN, ready: true };
  }
  const root = pluginRoot('impeccable');
  if (!root) return null;
  const launcher = path.join(root, 'skills', 'impeccable', 'scripts', 'impeccable');
  if (!fs.existsSync(launcher)) return null;
  // The launcher downloads a checksum-verified engine on first use. Hooks must not
  // trigger that download (it can exceed the hook timeout), so report readiness.
  let version = '';
  try {
    version = fs.readFileSync(path.join(root, 'skills', 'impeccable', 'scripts', 'VERSION'), 'utf8').trim();
  } catch {}
  const home = process.env.IMPECCABLE_HOME || path.join(os.homedir(), '.impeccable');
  const ready = version ? fs.existsSync(path.join(home, 'bin', version, 'impeccable')) : false;
  return { cmd: launcher, ready, version };
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
