// "dfw mcp": dfw as a Model Context Protocol server (stdio, JSON-RPC 2.0, no dependencies), for AI coding
// tools without a dfw plugin (OpenCode, Codex, Cursor...). It gives them the workflow text and the CLI.
// MCP cannot intercept the tool's own file writes, so there is no hard gate here: the instructions ask
// the model to call dfw_can_write before editing. Claude Code and Copilot keep their plugins and hooks.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { PLUGIN_ROOT, findProjectRoot, selfCommand, pluginManifest } from './util.mjs';
import { loadState } from './state.mjs';
import { loadConfig } from './config.mjs';
import { decideWrite } from './gate.mjs';

const SKILL_DIR_MARK = '__DFW_SKILL_DIR__'; // rendered into mcp/ at build time (host.mjs "mcp")
const BLOCKED = new Set(['hook', 'mcp', '__studio-server', 'install', 'uninstall', 'setup']);
const TOOL_TIMEOUT_MS = 10 * 60 * 1000;

/** The mcp/ text tree built for this plugin (dev checkout: the built Claude Code plugin's copy). */
function guideDir() {
  const own = path.join(PLUGIN_ROOT, 'mcp');
  return fs.existsSync(own) ? own : path.resolve(PLUGIN_ROOT, '..', 'plugins', 'dfw', 'mcp');
}

function guide(topic = '') {
  const dir = guideDir();
  const rel = topic ? String(topic).replace(/^\/+/, '') : 'SKILL.md';
  const file = path.resolve(dir, rel);
  if (!file.startsWith(dir + path.sep) || !file.endsWith('.md') || !fs.existsSync(file)) {
    throw new Error(`Unknown topic "${topic}". Topics: ${topics().join(', ')}`);
  }
  return fs.readFileSync(file, 'utf8').replaceAll(SKILL_DIR_MARK, dir);
}

function topics() {
  const dir = guideDir();
  const out = [];
  for (const sub of ['references', 'policies', 'stacks']) {
    const d = path.join(dir, sub);
    if (fs.existsSync(d)) for (const f of fs.readdirSync(d).sort()) if (f.endsWith('.md')) out.push(`${sub}/${f}`);
  }
  return out;
}

function runDfw(args, cwd) {
  if (!Array.isArray(args) || !args.length || !args.every((a) => typeof a === 'string')) throw new Error('"args" must be a non-empty list of strings, e.g. ["status"].');
  if (BLOCKED.has(args[0])) throw new Error(`"dfw ${args[0]}" is not available through MCP; ask the user to run it in a terminal.`);
  const self = selfCommand();
  return new Promise((resolve) => {
    const child = spawn(self.cmd, [...self.args, ...args], { cwd: cwd || process.cwd(), env: { ...process.env, DFW_HOST: 'mcp' } });
    let out = '';
    child.stdout.on('data', (d) => (out += d));
    child.stderr.on('data', (d) => (out += d));
    const t = setTimeout(() => child.kill(), TOOL_TIMEOUT_MS);
    child.on('error', (e) => (out += e.message));
    child.on('close', (code) => {
      clearTimeout(t);
      resolve({ text: out.trim() || `(exit ${code})`, isError: code !== 0 });
    });
  });
}

function canWrite(files, cwd) {
  if (!Array.isArray(files) || !files.length) throw new Error('"files" must be a non-empty list of paths.');
  const base = cwd || process.cwd();
  const root = findProjectRoot(base);
  const state = loadState(root);
  if (!state) return { text: 'No dfw workflow in this project: writes are not gated.', isError: false };
  const config = loadConfig(root);
  const lines = files.map((f) => {
    const d = decideWrite({ root, state, config, absFile: path.resolve(base, f) });
    return d.allow ? `ALLOWED ${f}` : `DENIED  ${f}: ${d.reason}`;
  });
  return { text: lines.join('\n'), isError: lines.some((l) => l.startsWith('DENIED')) };
}

const INSTRUCTIONS = [
  'dfw is a design-first frontend workflow: design, design system and approval gates come before UI code.',
  'For UI or design work, first call dfw_guide (no topic) and follow it; it lists the reference topics to load per phase.',
  'Run dfw commands with the dfw tool (args as a list, cwd = the project root).',
  'This tool has no write hook: before creating or editing any file in a dfw project, call dfw_can_write and never write a DENIED file.',
  'Only run "dfw approve" after the user explicitly approved in the chat.',
].join(' ');

const TOOLS = () => [
  {
    name: 'dfw_guide',
    description: `The dfw workflow instructions. No topic: the main guide (read it first). Topics: ${topics().join(', ')}.`,
    inputSchema: { type: 'object', properties: { topic: { type: 'string', description: 'Optional, e.g. "references/phase-concepts.md".' } } },
  },
  {
    name: 'dfw',
    description: 'Run a dfw CLI command, e.g. ["status"], ["init","--mode","new-product"], ["shoot","--page","borrow","--states"]. ["help"] lists every command.',
    inputSchema: {
      type: 'object',
      properties: { args: { type: 'array', items: { type: 'string' } }, cwd: { type: 'string', description: 'Project root (absolute path).' } },
      required: ['args'],
    },
  },
  {
    name: 'dfw_can_write',
    description: 'Check the approval gates before writing files (design files are open in design phases; implementation code needs G2 + G3 + the implement phase).',
    inputSchema: {
      type: 'object',
      properties: { files: { type: 'array', items: { type: 'string' } }, cwd: { type: 'string', description: 'Project root (absolute path).' } },
      required: ['files'],
    },
  },
];

/** Handle one JSON-RPC request; returns the result (or throws an Error with .code). */
export async function handle(method, params = {}) {
  switch (method) {
    case 'initialize':
      return {
        protocolVersion: params.protocolVersion || '2025-06-18',
        capabilities: { tools: {}, prompts: {} },
        serverInfo: { name: 'dfw', version: pluginManifest().version || '0.0.0' },
        instructions: INSTRUCTIONS,
      };
    case 'ping':
      return {};
    case 'tools/list':
      return { tools: TOOLS() };
    case 'tools/call': {
      const a = params.arguments || {};
      try {
        let r;
        if (params.name === 'dfw_guide') r = { text: guide(a.topic) };
        else if (params.name === 'dfw') r = await runDfw(a.args, a.cwd);
        else if (params.name === 'dfw_can_write') r = canWrite(a.files, a.cwd);
        else throw Object.assign(new Error(`Unknown tool ${params.name}`), { code: -32602 });
        return { content: [{ type: 'text', text: r.text }], isError: Boolean(r.isError) };
      } catch (e) {
        if (e.code) throw e;
        return { content: [{ type: 'text', text: e.message }], isError: true };
      }
    }
    case 'prompts/list':
      return { prompts: [{ name: 'dfw', description: 'Start or continue the dfw design-first workflow in this project.' }] };
    case 'prompts/get':
      if (params.name !== 'dfw') throw Object.assign(new Error(`Unknown prompt ${params.name}`), { code: -32602 });
      return { messages: [{ role: 'user', content: { type: 'text', text: guide() } }] };
    default:
      throw Object.assign(new Error(`Method not found: ${method}`), { code: -32601 });
  }
}

/** Serve MCP over stdio until stdin closes. */
export function serveMcp() {
  let buf = '';
  const send = (msg) => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', ...msg })}\n`);
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', async (chunk) => {
    buf += chunk;
    let i;
    while ((i = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, i).trim();
      buf = buf.slice(i + 1);
      if (!line) continue;
      let msg;
      try {
        msg = JSON.parse(line);
      } catch {
        send({ id: null, error: { code: -32700, message: 'Parse error' } });
        continue;
      }
      if (msg.id === undefined) continue; // notification
      try {
        send({ id: msg.id, result: await handle(msg.method, msg.params) });
      } catch (e) {
        send({ id: msg.id, error: { code: e.code || -32603, message: e.message } });
      }
    }
  });
  return new Promise((resolve) => process.stdin.on('end', resolve));
}

/* ---------------------------------------------------------------- install into each tool's MCP config */

const xdg = () => process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config');
export const MCP_TARGETS = {
  opencode: { name: 'OpenCode', file: () => path.join(xdg(), 'opencode', 'opencode.json'), format: 'opencode' },
  codex: { name: 'OpenAI Codex', file: () => path.join(process.env.CODEX_HOME || path.join(os.homedir(), '.codex'), 'config.toml'), format: 'toml' },
  cursor: { name: 'Cursor', file: () => path.join(os.homedir(), '.cursor', 'mcp.json'), format: 'mcpServers' },
};

const TOML_BLOCK = /^\[mcp_servers\.dfw\]\n(?:(?!\[)[^\n]*\n?)*/m;

/** New text of a tool's config with the dfw server added (or removed). Throws on a config it cannot parse. */
/** Command a tool runs to start the server: the plugin's sh launcher (survives Node upgrades), else node + dfw.mjs. */
function serverCommand() {
  const launcher = path.join(PLUGIN_ROOT, 'bin', 'dfw');
  return process.platform !== 'win32' && fs.existsSync(launcher) ? { cmd: launcher, args: [] } : selfCommand();
}

export function mcpConfigText(format, current, { remove = false, command = serverCommand() } = {}) {
  const cmd = [command.cmd, ...command.args, 'mcp'];
  if (format === 'toml') {
    const rest = current.replace(TOML_BLOCK, '').replace(/\n{3,}/g, '\n\n').replace(/\s*$/, '\n');
    if (remove) return rest.trim() ? rest : '';
    const block = `[mcp_servers.dfw]\ncommand = ${JSON.stringify(cmd[0])}\nargs = [${cmd.slice(1).map((a) => JSON.stringify(a)).join(', ')}]\n`;
    return rest.trim() ? `${rest}\n${block}` : block;
  }
  let json = {};
  if (current.trim()) {
    try {
      json = JSON.parse(current);
    } catch (e) {
      throw new Error(`cannot parse the existing config (${e.message}); add dfw by hand or fix the file`);
    }
  }
  const key = format === 'opencode' ? 'mcp' : 'mcpServers';
  json[key] = { ...(json[key] || {}) };
  if (remove) delete json[key].dfw;
  else json[key].dfw = format === 'opencode' ? { type: 'local', command: cmd, enabled: true } : { command: cmd[0], args: cmd.slice(1) };
  if (format === 'opencode' && !json.$schema) json.$schema = 'https://opencode.ai/config.json';
  return `${JSON.stringify(json, null, 2)}\n`;
}

/** Whether a tool's config already has the dfw server. */
export function mcpInstalled(id) {
  const t = MCP_TARGETS[id];
  try {
    const text = fs.readFileSync(t.file(), 'utf8');
    return t.format === 'toml' ? TOML_BLOCK.test(text) : Boolean(JSON.parse(text)?.[t.format === 'opencode' ? 'mcp' : 'mcpServers']?.dfw);
  } catch {
    return false;
  }
}

/** dfw mcp install|uninstall <tool|all> [--print]. Returns a report. */
export function mcpInstall(target, { remove = false, print = false } = {}) {
  const ids = target === 'all' ? Object.keys(MCP_TARGETS) : [target];
  const lines = [];
  for (const id of ids) {
    const t = MCP_TARGETS[id];
    if (!t) throw new Error(`Unknown tool "${id}". MCP install targets: ${Object.keys(MCP_TARGETS).join(', ')}, all.`);
    const file = t.file();
    const current = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '';
    const next = mcpConfigText(t.format, current, { remove });
    if (print) {
      lines.push(`# ${t.name}: ${file}\n${next}`);
      continue;
    }
    if (next === current) {
      lines.push(`${t.name}: unchanged (${file})`);
      continue;
    }
    if (current) fs.copyFileSync(file, `${file}.dfw-backup`); // keep the user's previous config
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, next);
    lines.push(`${t.name}: ${remove ? 'removed dfw from' : 'added dfw to'} ${file}${current ? ` (backup: ${path.basename(file)}.dfw-backup)` : ''}`);
  }
  if (!print && !remove) lines.push('Restart the tool, then ask it for UI work: it loads the guide through the dfw_guide tool. After moving or updating dfw, run this again.');
  return lines.join('\n');
}
