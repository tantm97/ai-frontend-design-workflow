// AI coding tools ("hosts") that dfw plugs into. Everything that differs between Claude Code,
// GitHub Copilot and future hosts lives here, so the rest of core/ stays tool-agnostic:
// - hook input normalization (field names, tool names, JSON-string arguments),
// - hook output formats (context, deny a tool call, block stopping),
// - text placeholders rendered into SKILL.md, references and agents at build time,
// - install commands and plugin locations.
// Adding a host = one entry here + adapters/<host>/.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const copilotHome = () => process.env.COPILOT_HOME || path.join(os.homedir(), '.copilot');

/**
 * Where Copilot keeps dfw-marketplace plugins: a marketplace added from a local directory is used
 * in place ("live", <dir>/plugins/<dir name>); a GitHub one is copied to installed-plugins/.
 * Returns { root: directory holding the plugin folders, dfw: folder of the dfw plugin }.
 */
export function copilotPluginDirs() {
  let src = null;
  try {
    src = JSON.parse(fs.readFileSync(path.join(copilotHome(), 'settings.json'), 'utf8'))?.extraKnownMarketplaces?.['dfw-marketplace']?.source;
  } catch {}
  if (src?.source === 'directory' && src.path) return { root: path.join(src.path, 'plugins'), dfw: path.join(src.path, 'plugins', 'dfw-copilot') };
  const root = path.join(copilotHome(), 'installed-plugins', 'dfw-marketplace');
  return { root, dfw: path.join(root, 'dfw') };
}

export const HOSTS = {
  claude: {
    id: 'claude',
    name: 'Claude Code',
    status: 'supported',
    cli: 'claude',
    adapter: 'claude-code',
    // Hook events the host fires and whether a hook can add model context there.
    context: { SessionStart: true, UserPromptSubmit: true, PostToolUse: true },
    text: {
      HOST_NAME: 'Claude Code',
      SKILL_DIR: '${CLAUDE_SKILL_DIR}',
      ASK_TOOL: 'AskUserQuestion',
      BACKGROUND: 'Bash `run_in_background`',
      HOST_SETTINGS: '`~/.claude/settings.json`',
      DFW_CLI: '`dfw` is a CLI on your PATH (from this plugin\'s bin/).',
      INVOKE_SKILL: 'Invoke skills with the Skill tool by their full `plugin:skill` name.',
    },
    qualifiedSkillNames: true,
    installedPluginDirs: () => [], // Claude Code uses a versioned cache; see locate.mjs
  },
  copilot: {
    id: 'copilot',
    name: 'GitHub Copilot',
    status: 'supported',
    cli: 'copilot',
    adapter: 'copilot',
    // Copilot ignores additionalContext on userPromptSubmitted: comments ride on PostToolUse and Stop.
    context: { SessionStart: true, UserPromptSubmit: false, PostToolUse: true },
    text: {
      HOST_NAME: 'GitHub Copilot',
      SKILL_DIR: '.',
      ASK_TOOL: 'the ask_user tool',
      BACKGROUND: 'an async/background shell command',
      HOST_SETTINGS: 'your Copilot configuration (`~/.copilot/`)',
      DFW_CLI:
        '`dfw` is a CLI. If `dfw` is not on your PATH, run it as `~/.copilot/installed-plugins/dfw-marketplace/dfw/bin/dfw` (`dfw install --host copilot` also links it into `~/.local/bin`).',
      INVOKE_SKILL: 'Load a skill by its name (for example `impeccable`, `frontend-design`); custom agents by their name (for example `screen-builder`).',
    },
    qualifiedSkillNames: false,
    installedPluginDirs: () => [copilotPluginDirs().root],
    dfwPluginDir: () => copilotPluginDirs().dfw,
  },
  // Text rendering only: the guide served by "dfw mcp" (core/lib/mcp.mjs) to tools without a dfw plugin.
  mcp: {
    id: 'mcp',
    name: 'MCP',
    status: 'internal',
    text: {
      HOST_NAME: 'your AI coding tool (through the dfw MCP server)',
      SKILL_DIR: '__DFW_SKILL_DIR__',
      ASK_TOOL: 'your tool for asking the user (or a short numbered question in chat)',
      BACKGROUND: 'a background shell command if your tool has one; otherwise check `dfw comments inbox` whenever you pause',
      HOST_SETTINGS: "your tool's own settings",
      DFW_CLI:
        '`dfw` is a CLI, also available as the `dfw` MCP tool (arguments as a list, cwd = project root). This tool has no write hook: before creating or editing any file in a dfw project, call the `dfw_can_write` MCP tool and never write a DENIED file.',
      INVOKE_SKILL: 'Design skills named here (impeccable, frontend-design, ...) may not be installed in your tool; when one is missing, follow this guide and its references directly. Agents named here (screen-builder, ...) are roles: do their work yourself.',
    },
    qualifiedSkillNames: false,
  },
  // Planned plugins (hooks): same core, new adapter. Until then they use "dfw mcp install <tool>".
  opencode: { id: 'opencode', name: 'OpenCode', status: 'planned', cli: 'opencode', mcp: true },
  codex: { id: 'codex', name: 'OpenAI Codex', status: 'planned', cli: 'codex', mcp: true },
  cursor: { id: 'cursor', name: 'Cursor', status: 'planned', cli: 'cursor-agent', mcp: true },
};

export const SUPPORTED_HOSTS = Object.values(HOSTS).filter((h) => h.status === 'supported').map((h) => h.id);

export function getHost(id) {
  const h = HOSTS[id || 'claude'];
  if (!h) throw new Error(`Unknown host "${id}". Known: ${Object.keys(HOSTS).join(', ')}`);
  return h;
}

/** Host for this process: --host flag, then DFW_HOST, then Claude Code. */
export function currentHost(flag) {
  return getHost(flag || process.env.DFW_HOST || 'claude');
}

// Runtime tool names that write files, mapped to dfw's canonical names.
const TOOL_ALIASES = {
  Edit: 'Edit', Write: 'Write', MultiEdit: 'MultiEdit', NotebookEdit: 'NotebookEdit',
  edit: 'Edit', str_replace_editor: 'Edit', create: 'Write', write: 'Write', apply_patch: 'ApplyPatch', ApplyPatch: 'ApplyPatch',
};

function parseMaybeJson(v) {
  if (typeof v !== 'string') return v || {};
  try {
    return JSON.parse(v);
  } catch {
    return { input: v }; // e.g. a raw apply_patch body
  }
}

/**
 * Normalize a hook payload from any host into dfw's shape:
 * { session_id, cwd, tool_name, tool_input, stop_hook_active, prompt, event }.
 * Claude Code and Copilot's PascalCase (VS Code compatible) format already use snake_case;
 * Copilot's camelCase format uses sessionId/toolName/toolArgs.
 */
export function normalizeHookInput(raw = {}) {
  const tool = raw.tool_name ?? raw.toolName;
  return {
    ...raw,
    event: raw.hook_event_name ?? raw.hookEventName,
    session_id: raw.session_id ?? raw.sessionId,
    cwd: raw.cwd,
    tool_name: tool ? TOOL_ALIASES[tool] || tool : undefined,
    tool_input: parseMaybeJson(raw.tool_input ?? raw.toolArgs),
    stop_hook_active: Boolean(raw.stop_hook_active ?? raw.stopHookActive),
    prompt: raw.prompt,
  };
}

/**
 * Turn a neutral hook result into the host's output JSON.
 * result: { context } | { deny } | { block } (or null).
 */
export function formatHookOutput(hostId, event, result) {
  if (!result) return null;
  const host = getHost(hostId);
  if (result.block) return { decision: 'block', reason: result.block };
  if (result.deny) {
    if (host.id === 'claude') return { hookSpecificOutput: { hookEventName: event, permissionDecision: 'deny', permissionDecisionReason: result.deny } };
    return { permissionDecision: 'deny', permissionDecisionReason: result.deny };
  }
  if (result.context) {
    if (host.context[event] === false) return null;
    if (host.id === 'claude') return { hookSpecificOutput: { hookEventName: event, additionalContext: result.context } };
    return { additionalContext: result.context };
  }
  return null;
}

/** Render host placeholders ({{SKILL_DIR}}, host blocks) in skill/reference/agent text. */
export function renderText(text, hostId) {
  const host = getHost(hostId);
  let out = text.replace(/<!--\s*host:([\w,]+)\s*-->\n?([\s\S]*?)<!--\s*\/host\s*-->\n?/g, (_, ids, body) => (ids.split(',').includes(host.id) ? body : ''));
  out = out.replace(/\{\{([A-Z_]+)\}\}/g, (m, k) => (k in host.text ? host.text[k] : m));
  if (!host.qualifiedSkillNames) {
    out = out.replace(/`(?:frontend-design|ux-research|ui-ux-pro-max|impeccable|emil-design-eng|hallmark|taste-reference|dfw):([a-z0-9-]+)`/g, '`$1`');
  }
  return out;
}
