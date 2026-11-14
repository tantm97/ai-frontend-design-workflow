// "dfw install" / "dfw uninstall": register dfw with Claude Code. Shared by the npx installer
// (scripts/install.mjs) and the standalone binary (install.sh), so a machine without Node can
// install everything with one command.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { PLUGIN_ROOT, IS_BINARY, readJson, selfCommand } from './util.mjs';
import { browserStatus, installBrowser } from './runtime.mjs';
import { binaryStatus, installBinary, RELEASE_REPO } from './binary.mjs';

export const MARKET = 'dfw-marketplace';

const c = { dim: (s) => `\x1b[2m${s}\x1b[0m`, ok: (s) => `\x1b[32m${s}\x1b[0m`, warn: (s) => `\x1b[33m${s}\x1b[0m`, b: (s) => `\x1b[1m${s}\x1b[0m` };

/** Plugins to install: dfw first, then the design skills it depends on. */
export function pluginList() {
  const deps = readJson(path.join(PLUGIN_ROOT, '.claude-plugin', 'plugin.json'), {})?.dependencies || [];
  return ['dfw', ...deps];
}

function runner(quiet) {
  return (bin, args, { allowFail = false, env } = {}) => {
    if (!quiet) console.log(c.dim(`  $ ${bin} ${args.join(' ')}`));
    const r = spawnSync(bin, args, { encoding: 'utf8', env: { ...process.env, ...env }, shell: process.platform === 'win32' });
    if (r.error || (r.status !== 0 && !allowFail)) {
      const msg = (r.stderr || r.stdout || r.error?.message || '').trim();
      throw new Error(`${bin} ${args.join(' ')} failed${msg ? `:\n${msg}` : ''}`);
    }
    return r;
  };
}

export async function install({ scope = 'user', source = RELEASE_REPO, runtime = true, binary = true, quiet = false } = {}) {
  const run = runner(quiet);
  const step = (s) => console.log(`${c.b('›')} ${s}`);
  const v = spawnSync('claude', ['--version'], { encoding: 'utf8', shell: process.platform === 'win32' });
  if (v.status !== 0) throw new Error('Claude Code CLI ("claude") not found on PATH. Install Claude Code first: https://code.claude.com');
  step(`Claude Code ${v.stdout.trim()}, dfw ${IS_BINARY ? 'binary' : `on Node ${process.versions.node}`}`);

  step(`Marketplace ${MARKET} from ${source}`);
  const list = run('claude', ['plugin', 'marketplace', 'list'], { allowFail: true }).stdout || '';
  if (list.includes(MARKET)) run('claude', ['plugin', 'marketplace', 'update', MARKET]);
  else run('claude', ['plugin', 'marketplace', 'add', source, '--scope', scope]);

  const plugins = pluginList();
  step(`Plugins (${scope} scope): ${plugins.join(', ')}`);
  // dfw declares the skills as dependencies; installing each explicitly keeps older Claude Code versions working.
  for (const p of plugins) run('claude', ['plugin', 'install', `${p}@${MARKET}`, '--scope', scope], { allowFail: p !== 'dfw' });

  const self = selfCommand();
  if (binary && !IS_BINARY) {
    const st = binaryStatus();
    if (st.installed) step(`dfw binary ${st.version} already installed`);
    else {
      step('dfw binary (faster start; Node is then optional)');
      try {
        const r = await installBinary({ log: () => {} });
        console.log(c.dim(`  ${r.path}`));
      } catch (e) {
        console.log(c.warn(`  skipped: ${e.message}`));
      }
    }
  }
  if (runtime) {
    const b = browserStatus();
    if (b.installed) step(`Browser for screenshots and layout checks: ${b.path}`);
    else {
      step('Headless Chrome for screenshots and layout checks (no Chrome/Edge/Chromium found)');
      try {
        await installBrowser({ log: (m) => console.log(c.dim(`  ${m}`)) });
      } catch (e) {
        console.log(c.warn(`  skipped: ${e.message}`));
      }
    }
    step('Impeccable engine (detect only, telemetry and update checks off)');
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-setup-'));
    fs.mkdirSync(path.join(tmp, '.dfw'), { recursive: true });
    run(self.cmd, [...self.args, 'check', '--setup'], { allowFail: true, env: { CLAUDE_PROJECT_DIR: tmp } });
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  step('Doctor');
  const d = run(self.cmd, [...self.args, 'doctor'], { allowFail: true });
  console.log((d.stdout || '').split('\n').map((l) => `  ${l}`).join('\n'));
  console.log(`\n${c.ok('Done.')} Restart Claude Code (or run /reload-plugins), open a project and type ${c.b('/dfw')}.`);
}

export function uninstall({ scope = 'user', quiet = false } = {}) {
  const run = runner(quiet);
  for (const p of pluginList()) run('claude', ['plugin', 'uninstall', `${p}@${MARKET}`, '--scope', scope], { allowFail: true });
  run('claude', ['plugin', 'marketplace', 'remove', MARKET], { allowFail: true });
  console.log(c.ok('Removed dfw and its skills. Project files (.dfw/, design/) are untouched.'));
}
