#!/usr/bin/env node
// One-command installer (needs Node; without Node use install.sh, which fetches the dfw binary):
//   npx github:tantm97/ai-frontend-design-workflow            (install, user scope)
//   npx github:tantm97/ai-frontend-design-workflow install --scope project --no-runtime
//   npx github:tantm97/ai-frontend-design-workflow uninstall
// Adds the dfw marketplace to Claude Code, installs dfw (its plugin dependencies pull in the
// design skills), fetches the dfw binary for this version when released, makes sure a headless
// browser is available, sets up the Impeccable engine, then runs doctor.
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const REPO = 'tantm97/ai-frontend-design-workflow';
const { install, uninstall } = await import('../plugins/dfw/lib/install.mjs');

const argv = process.argv.slice(2);
const cmd = argv[0] && !argv[0].startsWith('-') ? argv[0] : 'install';
const flag = (n) => argv.includes(`--${n}`);
const opt = (n, d) => (argv.includes(`--${n}`) ? argv[argv.indexOf(`--${n}`) + 1] : d);

function help() {
  console.log(`ai-frontend-design-workflow (dfw) installer

  npx github:${REPO} [install] [--scope user|project|local] [--source <path|owner/repo>] [--no-runtime] [--no-binary]
  npx github:${REPO} uninstall [--scope ...]
  npx github:${REPO} doctor

  install       add the marketplace, install dfw + the design skills it depends on, fetch the dfw
                binary, make sure a headless browser exists, set up the Impeccable engine, run doctor
  --scope       user (default, all projects), project (shared in .claude/settings.json), local
  --source      marketplace source; default ${REPO} (use a local clone path while developing)
  --no-runtime  skip the browser check/download and the Impeccable engine
  --no-binary   keep running dfw on Node (do not fetch the binary)

  No Node.js? curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | sh`);
}

try {
  if (cmd === 'help' || flag('help') || flag('h')) help();
  else if (cmd === 'install') await install({ scope: opt('scope', 'user'), source: opt('source', REPO), runtime: !flag('no-runtime'), binary: !flag('no-binary'), quiet: flag('quiet') });
  else if (cmd === 'uninstall') uninstall({ scope: opt('scope', 'user'), quiet: flag('quiet') });
  else if (cmd === 'doctor') spawnSync(process.execPath, [path.join(ROOT, 'plugins', 'dfw', 'bin', 'dfw.mjs'), 'doctor'], { stdio: 'inherit' });
  else {
    help();
    process.exitCode = 1;
  }
} catch (e) {
  console.error(`\x1b[31m${e.message}\x1b[0m`);
  process.exitCode = 1;
}
