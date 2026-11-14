import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { normalizeHookInput, formatHookOutput, renderText, HOSTS, SUPPORTED_HOSTS } from '../../core/lib/host.mjs';
import { filesFromToolInput, filesFromPatch } from '../../core/lib/gate.mjs';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const DFW = path.join(REPO, 'core', 'bin', 'dfw.mjs');

test('host: supported hosts are Claude Code and GitHub Copilot; others planned', () => {
  assert.deepEqual(SUPPORTED_HOSTS, ['claude', 'copilot']);
  for (const id of ['opencode', 'codex', 'cursor']) assert.equal(HOSTS[id].status, 'planned');
});

test('host: normalizes Claude, Copilot PascalCase and Copilot camelCase payloads', () => {
  const claude = normalizeHookInput({ session_id: 's', tool_name: 'Write', tool_input: { file_path: 'a.ts' } });
  assert.equal(claude.tool_name, 'Write');
  const camel = normalizeHookInput({ sessionId: 's', toolName: 'create', toolArgs: '{"path":"src/a.ts"}', stopHookActive: true });
  assert.equal(camel.session_id, 's');
  assert.equal(camel.tool_name, 'Write');
  assert.deepEqual(camel.tool_input, { path: 'src/a.ts' });
  assert.equal(camel.stop_hook_active, true);
  assert.equal(normalizeHookInput({ toolName: 'str_replace_editor' }).tool_name, 'Edit');
  const patch = normalizeHookInput({ toolName: 'apply_patch', toolArgs: '*** Begin Patch\n*** Add File: src/x.ts\n*** Update File: design/a.html\n*** End Patch' });
  assert.deepEqual(filesFromToolInput(patch.tool_name, patch.tool_input), ['src/x.ts', 'design/a.html']);
  assert.deepEqual(filesFromPatch('*** Update File: a\n*** Move to: b'), ['a', 'b']);
});

test('host: output formats per host', () => {
  assert.deepEqual(formatHookOutput('claude', 'PreToolUse', { deny: 'no' }), { hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'deny', permissionDecisionReason: 'no' } });
  assert.deepEqual(formatHookOutput('copilot', 'PreToolUse', { deny: 'no' }), { permissionDecision: 'deny', permissionDecisionReason: 'no' });
  assert.deepEqual(formatHookOutput('copilot', 'SessionStart', { context: 'c' }), { additionalContext: 'c' });
  assert.equal(formatHookOutput('copilot', 'UserPromptSubmit', { context: 'c' }), null);
  assert.deepEqual(formatHookOutput('copilot', 'Stop', { block: 'r' }), { decision: 'block', reason: 'r' });
  assert.equal(formatHookOutput('claude', 'Stop', null), null);
});

test('host: renders skill text per host', () => {
  const src = 'Dir {{SKILL_DIR}}/references. Ask with {{ASK_TOOL}}. Use `impeccable:impeccable` and `dfw:screen-builder`.\n<!-- host:claude -->\nclaude only\n<!-- /host -->\n<!-- host:copilot -->\ncopilot only\n<!-- /host -->\n';
  const c = renderText(src, 'claude');
  assert.match(c, /\$\{CLAUDE_SKILL_DIR\}\/references/);
  assert.match(c, /AskUserQuestion/);
  assert.match(c, /`impeccable:impeccable`/);
  assert.match(c, /claude only/);
  assert.doesNotMatch(c, /copilot only/);
  const g = renderText(src, 'copilot');
  assert.match(g, /Dir \.\/references/);
  assert.match(g, /ask_user/);
  assert.match(g, /`impeccable` and `screen-builder`/);
  assert.match(g, /copilot only/);
  assert.doesNotMatch(g, /claude only|CLAUDE_SKILL_DIR/);
});

test('hook CLI: --host copilot denies in Copilot format', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-host-'));
  const env = { ...process.env, DFW_HOME: path.join(root, '.home'), CLAUDE_PROJECT_DIR: '' };
  execFileSync(process.execPath, [DFW, 'init', '--root', root], { cwd: root, env, encoding: 'utf8' });
  const run = (input, host) => execFileSync(process.execPath, [DFW, 'hook', 'pre-tool-use', ...(host ? ['--host', host] : [])], { input: JSON.stringify(input), cwd: root, env, encoding: 'utf8' });
  const out = JSON.parse(run({ cwd: root, toolName: 'create', toolArgs: JSON.stringify({ path: 'src/a.ts' }) }, 'copilot'));
  assert.equal(out.permissionDecision, 'deny');
  assert.equal(run({ cwd: root, toolName: 'create', toolArgs: JSON.stringify({ path: 'design/a.html' }) }, 'copilot'), '');
  const claude = JSON.parse(run({ cwd: root, tool_name: 'Write', tool_input: { file_path: path.join(root, 'src/a.ts') } }));
  assert.equal(claude.hookSpecificOutput.permissionDecision, 'deny');
  // Relative patch paths resolve against the session cwd (here design/), not the project root.
  const patch = (cwd, file) => run({ cwd, toolName: 'apply_patch', toolArgs: `*** Begin Patch\n*** Add File: ${file}\n*** End Patch` }, 'copilot');
  assert.equal(patch(path.join(root, 'design'), 'screens/a.html'), '');
  assert.equal(JSON.parse(patch(root, 'screens/a.html')).permissionDecision, 'deny');
});

test('build: Copilot plugin and marketplace are generated', () => {
  const dir = path.join(REPO, 'plugins', 'dfw-copilot');
  const pj = JSON.parse(fs.readFileSync(path.join(dir, 'plugin.json'), 'utf8'));
  assert.equal(pj.name, 'dfw');
  assert.equal(JSON.parse(fs.readFileSync(path.join(dir, 'dfw.json'), 'utf8')).host, 'copilot');
  const hooks = JSON.parse(fs.readFileSync(path.join(dir, 'hooks.json'), 'utf8'));
  assert.equal(hooks.version, 1);
  for (const ev of ['SessionStart', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'Stop']) assert.match(hooks.hooks[ev][0].bash, /--host copilot/);
  assert.ok(fs.existsSync(path.join(dir, 'agents', 'screen-builder.agent.md')));
  const skill = fs.readFileSync(path.join(dir, 'skills', 'dfw', 'SKILL.md'), 'utf8');
  assert.doesNotMatch(skill, /CLAUDE_SKILL_DIR|AskUserQuestion|\{\{[A-Z_]+\}\}/);
  const mp = JSON.parse(fs.readFileSync(path.join(REPO, '.github', 'plugin', 'marketplace.json'), 'utf8'));
  assert.equal(mp.name, 'dfw-marketplace');
  assert.equal(mp.plugins.find((p) => p.name === 'dfw').source, './plugins/dfw-copilot');
  const claudeMp = JSON.parse(fs.readFileSync(path.join(REPO, '.claude-plugin', 'marketplace.json'), 'utf8'));
  assert.equal(claudeMp.plugins.find((p) => p.name === 'dfw').source, './plugins/dfw');
});
