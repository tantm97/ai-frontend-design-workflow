#!/usr/bin/env node
// One-command installer:
//   npx github:tantm97/ai-frontend-design-workflow            (install, user scope)
//   npx github:tantm97/ai-frontend-design-workflow install --scope project --no-runtime
//   npx github:tantm97/ai-frontend-design-workflow uninstall
// Adds the dfw marketplace to Claude Code, installs dfw (its plugin dependencies pull in the
// design skills), installs the pinned screenshot runtime and the Impeccable engine, then runs doctor.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const MARKET = 'dfw-marketplace';
const REPO = 'tantm97/ai-frontend-design-workflow';
const PLUGINS = JSON.parse(fs.readFileSync(path.join(ROOT, '.claude-plugin', 'marketplace.json'), 'utf8')).plugins.map((p) => p.name);

const argv = process.argv.slice(2);
const cmd = argv[0] && !argv[0].startsWith('-') ? argv[0] : 'install';
const flag = (n) => argv.includes(`--${n}`);
const opt = (n, d) => (argv.includes(`--${n}`) ? argv[argv.indexOf(`--${n}`) + 1] : d);
const scope = opt('scope', 'user');
const quiet = flag('quiet');

const c = { dim: (s) => `\x1b[2m${s}\x1b[0m`, ok: (s) => `\x1b[32m${s}\x1b[0m`, err: (s) => `\x1b[31m${s}\x1b[0m`, b: (s) => `\x1b[1m${s}\x1b[0m` };
const step = (s) => console.log(`${c.b('›')} ${s}`);
function run(bin, args, { allowFail = false, env } = {}) {
  if (!quiet) console.log(c.dim(`  $ ${bin} ${args.join(' ')}`));
  const r = spawnSync(bin, args, { encoding: 'utf8', env: { ...process.env, ...env }, shell: process.platform === 'win32' });
  if (r.error || (r.status !== 0 && !allowFail)) {
    const msg = (r.stderr || r.stdout || r.error?.message || '').trim();
    throw new Error(`${bin} ${args.join(' ')} failed${msg ? `:\n${msg}` : ''}`);
  }
  return r;
}

function help() {
  console.log(`ai-frontend-design-workflow (dfw) installer

  npx github:${REPO} [install] [--scope user|project|local] [--source <path|owner/repo>] [--no-runtime]
  npx github:${REPO} uninstall [--scope ...]
  npx github:${REPO} doctor

  install    add the marketplace, install dfw + the design skills it depends on, install the
             pinned Playwright runtime and the Impeccable engine (telemetry off), run doctor
  --scope    user (default, all projects), project (shared in .claude/settings.json), local
  --source   marketplace source; default ${REPO} (use a local clone path while developing)
  --no-runtime   skip Playwright/Chromium and the Impeccable engine (screenshots/full checks off)`);
}

function claudeOk() {
  const r = spawnSync('claude', ['--version'], { encoding: 'utf8', shell: process.platform === 'win32' });
  return r.status === 0 ? r.stdout.trim() : null;
}

function install() {
  const v = claudeOk();
  if (!v) throw new Error('Claude Code CLI ("claude") not found on PATH. Install Claude Code first: https://code.claude.com');
  const major = Number(process.versions.node.split('.')[0]);
  if (major < 20) throw new Error(`Node ${process.versions.node} is too old; dfw needs Node 20+.`);
  step(`Claude Code ${v}, Node ${process.versions.node}`);

  const source = opt('source', REPO);
  step(`Marketplace ${MARKET} from ${source}`);
  const list = run('claude', ['plugin', 'marketplace', 'list'], { allowFail: true }).stdout || '';
  if (list.includes(MARKET)) run('claude', ['plugin', 'marketplace', 'update', MARKET]);
  else run('claude', ['plugin', 'marketplace', 'add', source, '--scope', scope]);

  step(`Plugins (${scope} scope): ${PLUGINS.join(', ')}`);
  // dfw declares the skills as dependencies; installing each explicitly keeps older Claude Code versions working.
  for (const p of ['dfw', ...PLUGINS.filter((x) => x !== 'dfw')]) run('claude', ['plugin', 'install', `${p}@${MARKET}`, '--scope', scope], { allowFail: p !== 'dfw' });

  const dfw = [path.join(ROOT, 'plugins', 'dfw', 'bin', 'dfw')];
  if (!flag('no-runtime')) {
    step('Screenshot runtime (pinned Playwright + Chromium, one-time download)');
    run(process.execPath, [...dfw, 'setup', '--playwright']);
    step('Impeccable engine (detect only, telemetry and update checks off)');
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-setup-'));
    fs.mkdirSync(path.join(tmp, '.dfw'), { recursive: true });
    run(process.execPath, [...dfw, 'check', '--setup'], { allowFail: true, env: { CLAUDE_PROJECT_DIR: tmp } });
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  step('Doctor');
  const d = run(process.execPath, [...dfw, 'doctor'], { allowFail: true });
  console.log((d.stdout || '').split('\n').map((l) => `  ${l}`).join('\n'));
  console.log(`\n${c.ok('Done.')} Restart Claude Code (or run /reload-plugins), open a project and type ${c.b('/dfw')}.`);
}

function uninstall() {
  for (const p of PLUGINS) run('claude', ['plugin', 'uninstall', `${p}@${MARKET}`, '--scope', scope], { allowFail: true });
  run('claude', ['plugin', 'marketplace', 'remove', MARKET], { allowFail: true });
  console.log(c.ok('Removed dfw and its skills. Project files (.dfw/, design/) are untouched.'));
}

try {
  if (cmd === 'help' || flag('help') || flag('h')) help();
  else if (cmd === 'install') install();
  else if (cmd === 'uninstall') uninstall();
  else if (cmd === 'doctor') run(process.execPath, [path.join(ROOT, 'plugins', 'dfw', 'bin', 'dfw'), 'doctor']) && console.log('');
  else {
    help();
    process.exitCode = 1;
  }
} catch (e) {
  console.error(c.err(e.message));
  process.exitCode = 1;
}
