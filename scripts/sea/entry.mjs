// Entry point of the single-file dfw binary (Node SEA). scripts/build-binary.mjs bundles this
// file plus plugins/dfw/lib into one CommonJS script and embeds the plugin's data files
// (kits, templates, studio, references, icons...) as the asset "plugin.json.gz".
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';
import { getAsset } from 'node:sea';

globalThis.__DFW_SEA = true;

function globalDir() {
  if (process.env.DFW_HOME) return process.env.DFW_HOME;
  return path.join(process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config'), 'dfw');
}

/** Unpack the embedded plugin files once per build into ~/.config/dfw/plugin/<version>-<hash>/. */
function extractEmbedded() {
  const data = JSON.parse(zlib.gunzipSync(Buffer.from(getAsset('plugin.json.gz'))).toString('utf8'));
  const dir = path.join(globalDir(), 'plugin', `${data.version}-${data.hash}`);
  if (fs.existsSync(path.join(dir, '.complete'))) return dir;
  const tmp = `${dir}.tmp-${process.pid}`;
  for (const [rel, file] of Object.entries(data.files)) {
    const f = path.join(tmp, rel);
    fs.mkdirSync(path.dirname(f), { recursive: true });
    fs.writeFileSync(f, Buffer.from(file.b64, 'base64'), { mode: file.mode });
  }
  fs.writeFileSync(path.join(tmp, '.complete'), data.hash);
  try {
    fs.renameSync(tmp, dir);
  } catch {
    fs.rmSync(tmp, { recursive: true, force: true }); // another process extracted it first
  }
  return dir;
}

// Launched by the plugin's bin/dfw: use that plugin directory (same version). Standalone: embedded copy.
const given = process.env.DFW_PLUGIN_ROOT;
process.env.DFW_PLUGIN_ROOT = given && fs.existsSync(path.join(given, 'kits')) ? given : extractEmbedded();

import('../../plugins/dfw/lib/cli.mjs')
  .then(({ main }) => main(process.argv.slice(2)))
  .catch((err) => {
    console.error(err.exitCode ? err.message : err.stack || String(err));
    process.exit(err.exitCode || 1);
  });
