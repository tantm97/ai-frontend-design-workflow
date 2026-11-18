// "dfw install" / "dfw uninstall": register dfw with the AI coding tools ("hosts") on this machine:
// Claude Code and GitHub Copilot today (see host.mjs). Shared by the npx installer
// (scripts/install.mjs) and the standalone binary (install.sh), so a machine without Node can
// install everything with one command.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { PLUGIN_ROOT, IS_BINARY, readJson, selfCommand, pluginManifest } from './util.mjs';
import { HOSTS, SUPPORTED_HOSTS, getHost } from './host.mjs';
import { browserStatus, installBrowser } from './runtime.mjs';
import { axeStatus, installAxe } from './a11y.mjs';
import { binaryStatus, installBinary, RELEASE_REPO } from './binary.mjs';

export const MARKET = 'dfw-marketplace';

const c = { dim: (s) => `\x1b[2m${s}\x1b[0m`, ok: (s) => `\x1b[32m${s}\x1b[0m`, warn: (s) => `\x1b[33m${s}\x1b[0m`, b: (s) => `\x1b[1m${s}\x1b[0m` };

/** Plugins to install: dfw first, then the design skills it depends on. */
export function pluginList() {
  const deps = readJson(path.join(PLUGIN_ROOT, 'dfw.json'), null)?.dependencies || pluginManifest().dependencies || [];
  return ['dfw', ...deps];
}

/**
 * How to spawn on Windows. Host CLIs ("claude", "copilot") are .cmd shims and need a shell, where
 * Node joins the arguments unquoted: quote the ones cmd.exe would split or interpret. An absolute
 * executable (node.exe / dfw.exe, often under "Program Files") runs directly, no shell.
 */
export function spawnPlan(bin, args, platform = process.platform) {
  if (platform !== 'win32' || path.win32.isAbsolute(bin)) return { bin, args, shell: false };
  const q = (a) => (/[\s"&|<>^()%!,;=]/.test(a) || a === '' ? `"${String(a).replace(/"/g, '""')}"` : a);
  return { bin, args: args.map(q), shell: true };
}

const hasCli = (bin) => {
  const p = spawnPlan(bin, ['--version']);
  return spawnSync(p.bin, p.args, { encoding: 'utf8', shell: p.shell }).status === 0;
};

/**
 * What to do with the marketplace in Claude Code, from "claude plugin marketplace list --json":
 * "add" (not registered), "update" (registered from this source), "readd" (registered from
 * another source, e.g. GitHub while --source is a local clone: update would keep the old one).
 */
export function marketplaceAction(listOut, source, name = MARKET) {
  let list;
  try {
    list = JSON.parse(listOut);
  } catch {
    return String(listOut || '').includes(name) ? 'update' : 'add'; // older Claude Code: no --json
  }
  const cur = (Array.isArray(list) ? list : []).find((m) => m?.name === name);
  if (!cur) return 'add';
  const norm = (v) => String(v).replace(/^https:\/\/github\.com\//, '').replace(/\.git$/, '').replace(/[\\/]+$/, '');
  const known = [cur.repo, cur.path, cur.url, cur.directory].filter((v) => typeof v === 'string').map(norm);
  return known.includes(norm(source)) || known.includes(norm(path.resolve(source))) ? 'update' : 'readd';
}

/** Hosts to act on: --host claude|copilot|all, default "auto" = every supported host whose CLI is installed. */
export function resolveHosts(host = 'auto') {
  if (host === 'all') return SUPPORTED_HOSTS.flatMap((id) => resolveHosts(id)); // each must have its CLI
  if (host !== 'auto') {
    const h = getHost(host);
    if (h.status !== 'supported') throw new Error(`${h.name} support is planned but not available yet. Supported: ${SUPPORTED_HOSTS.join(', ')}.`);
    if (!hasCli(h.cli)) throw new Error(`${h.name} CLI ("${h.cli}") not found on PATH.`);
    return [h];
  }
  const found = SUPPORTED_HOSTS.map(getHost).filter((h) => hasCli(h.cli));
  if (!found.length) throw new Error(`No supported AI coding tool found on PATH (${SUPPORTED_HOSTS.map((id) => HOSTS[id].cli).join(', ')}). Install Claude Code or GitHub Copilot CLI first, or pass --host.`);
  return found;
}

/** Register the marketplace and install dfw + its skills in one host. */
function installInHost(h, { run, step, scope, source, plugins }) {
  if (h.id === 'claude') {
    step(`Claude Code: marketplace ${MARKET} from ${source}`);
    let list = run('claude', ['plugin', 'marketplace', 'list', '--json'], { allowFail: true });
    if (list.status !== 0) list = run('claude', ['plugin', 'marketplace', 'list'], { allowFail: true });
    const action = marketplaceAction(list.stdout || '', source);
    if (action === 'update') run('claude', ['plugin', 'marketplace', 'update', MARKET]);
    else {
      if (action === 'readd') run('claude', ['plugin', 'marketplace', 'remove', MARKET], { allowFail: true });
      run('claude', ['plugin', 'marketplace', 'add', source, '--scope', scope]);
    }
    step(`Claude Code plugins (${scope} scope): ${plugins.join(', ')}`);
    // dfw declares the skills as dependencies; installing each explicitly keeps older Claude Code versions working.
    for (const p of plugins) run('claude', ['plugin', 'install', `${p}@${MARKET}`, '--scope', scope], { allowFail: p !== 'dfw' });
    return 'Restart Claude Code (or run /reload-plugins), open a project and type /dfw.';
  }
  if (h.id === 'copilot') {
    step(`GitHub Copilot: marketplace ${MARKET} from ${source}`);
    // An explicit --source replaces a marketplace registered from somewhere else ("add" alone would fail and keep it).
    if (source !== RELEASE_REPO) run('copilot', ['plugin', 'marketplace', 'remove', MARKET], { allowFail: true });
    run('copilot', ['plugin', 'marketplace', 'add', source], { allowFail: true }); // already added -> update below
    run('copilot', ['plugin', 'marketplace', 'update', MARKET], { allowFail: true });
    step(`GitHub Copilot plugins: ${plugins.join(', ')}`);
    for (const p of plugins) {
      const r = run('copilot', ['plugin', 'install', `${p}@${MARKET}`], { allowFail: true });
      if (r.status !== 0) run('copilot', ['plugin', 'update', p], { allowFail: p !== 'dfw' });
    }
    linkCopilotCli();
    return 'Restart GitHub Copilot (CLI session or VS Code window), open a project and ask for UI work (the dfw skill loads itself).';
  }
  throw new Error(`No installer for ${h.name}`);
}

/** Copilot does not put plugin bin/ on PATH: link ~/.local/bin/dfw to the installed launcher (POSIX only). */
function linkCopilotCli() {
  if (process.platform === 'win32') return;
  const target = path.join(HOSTS.copilot.dfwPluginDir(), 'bin', 'dfw');
  const link = path.join(os.homedir(), '.local', 'bin', 'dfw');
  try {
    if (fs.existsSync(link) && !fs.lstatSync(link).isSymbolicLink()) return; // a real file (e.g. install.sh binary link target) - leave it
    if (fs.existsSync(link) && fs.readlinkSync(link) !== target && fs.existsSync(fs.readlinkSync(link))) return; // points at another working dfw
    fs.mkdirSync(path.dirname(link), { recursive: true });
    fs.rmSync(link, { force: true });
    fs.symlinkSync(target, link);
  } catch {}
}

function runner(quiet) {
  return (bin, args, { allowFail = false, env } = {}) => {
    if (!quiet) console.log(c.dim(`  $ ${bin} ${args.join(' ')}`));
    const p = spawnPlan(bin, args);
    const r = spawnSync(p.bin, p.args, { encoding: 'utf8', env: { ...process.env, ...env }, shell: p.shell });
    if (r.error || (r.status !== 0 && !allowFail)) {
      const msg = (r.stderr || r.stdout || r.error?.message || '').trim();
      throw new Error(`${bin} ${args.join(' ')} failed${msg ? `:\n${msg}` : ''}`);
    }
    return r;
  };
}

export async function install({ host = 'auto', scope = 'user', source = RELEASE_REPO, runtime = true, binary = true, quiet = false } = {}) {
  const run = runner(quiet);
  const step = (s) => console.log(`${c.b('›')} ${s}`);
  const hosts = resolveHosts(host);
  step(`Hosts: ${hosts.map((h) => h.name).join(', ')} · dfw ${IS_BINARY ? 'binary' : `on Node ${process.versions.node}`}`);
  const plugins = pluginList();
  const next = hosts.map((h) => installInHost(h, { run, step, scope, source, plugins }));

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
    if (!axeStatus().installed) {
      step('axe-core for the accessibility check (pinned, integrity-checked)');
      try {
        await installAxe({ log: (m) => console.log(c.dim(`  ${m}`)) });
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
  console.log(`\n${c.ok('Done.')} ${next.join(' ')}`);
}

export function uninstall({ host = 'auto', scope = 'user', quiet = false } = {}) {
  const run = runner(quiet);
  for (const h of resolveHosts(host)) {
    if (h.id === 'claude') {
      for (const p of pluginList()) run('claude', ['plugin', 'uninstall', `${p}@${MARKET}`, '--scope', scope], { allowFail: true });
      run('claude', ['plugin', 'marketplace', 'remove', MARKET], { allowFail: true });
    } else if (h.id === 'copilot') {
      for (const p of pluginList()) run('copilot', ['plugin', 'uninstall', p], { allowFail: true });
      run('copilot', ['plugin', 'marketplace', 'remove', MARKET], { allowFail: true });
    }
    console.log(c.ok(`${h.name}: removed dfw and its skills.`));
  }
  console.log('Project files (.dfw/, design/) are untouched.');
}
