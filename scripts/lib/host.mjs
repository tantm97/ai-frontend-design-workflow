// Host text for the build (scripts/lib/build.mjs): the placeholder values and `<!-- host:id -->`
// blocks rendered into SKILL.md, references and agents of each host plugin.
// The binary holds the same table in native/src/core/host.rs (it renders the MCP guide at run
// time). tests/golden/core.golden.mjs renders every file of core/workflow with this module and
// with the binary (`dfw __debug host-render <host>`) and fails when they differ: change both.

export const HOSTS = {
  claude: {
    id: 'claude',
    name: 'Claude Code',
    text: {
      HOST_NAME: 'Claude Code',
      SKILL_DIR: '${CLAUDE_SKILL_DIR}',
      ASK_TOOL: 'AskUserQuestion',
      BACKGROUND: 'Bash `run_in_background`',
      HOST_SETTINGS: '`~/.claude/settings.json`',
      DFW_CLI: "`dfw` is a CLI on your PATH (from this plugin's bin/).",
      INVOKE_SKILL: 'Invoke skills with the Skill tool by their full `plugin:skill` name.',
    },
    qualifiedSkillNames: true,
  },
  copilot: {
    id: 'copilot',
    name: 'GitHub Copilot',
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
  },
  // The guide served by "dfw mcp" to tools without a dfw plugin.
  mcp: {
    id: 'mcp',
    name: 'MCP',
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
};

export function getHost(id) {
  const key = id || 'claude';
  const h = Object.hasOwn(HOSTS, key) ? HOSTS[key] : null;
  if (!h) throw new Error(`Unknown host "${id}". Known: ${Object.keys(HOSTS).join(', ')}`);
  return h;
}

/** Render host placeholders ({{SKILL_DIR}}, host blocks) in skill/reference/agent text. */
export function renderText(text, hostId) {
  const host = getHost(hostId);
  let out = text.replace(/<!--\s*host:([\w,]+)\s*-->\n?([\s\S]*?)<!--\s*\/host\s*-->\n?/g, (_, ids, body) => (ids.split(',').includes(host.id) ? body : ''));
  out = out.replace(/\{\{([A-Z_]+)\}\}/g, (m, k) => (Object.hasOwn(host.text, k) ? host.text[k] : m));
  if (!host.qualifiedSkillNames) {
    out = out.replace(/`(?:frontend-design|ux-research|ui-ux-pro-max|impeccable|emil-design-eng|hallmark|taste-reference|dfw):([a-z0-9-]+)`/g, '`$1`');
  }
  return out;
}
