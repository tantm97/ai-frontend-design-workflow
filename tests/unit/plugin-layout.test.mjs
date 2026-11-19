// The one dfw plugin folder (plugins/dfw) that Claude Code and GitHub Copilot CLI both install:
// shared files once at the root, each host's skill, agents and hooks under hosts/<host>/ named only
// by that host's manifest, no location a host reads by default at the root, both marketplaces on
// ./plugins/dfw (openspec plugin-packaging).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assembleDfw, listFiles, strayPlugins, ROOT_DEFAULTS } from '../../scripts/lib/build.mjs';
import { layoutCheck } from '../../scripts/lib/checks.mjs';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const DFW = path.join(REPO, 'plugins', 'dfw');
const readJson = (f) => JSON.parse(fs.readFileSync(f, 'utf8'));

/** A copy of the build inputs of the dfw plugin in a temporary root, to build and break freely. */
function buildRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-layout-'));
  for (const rel of [
    'core/bin',
    'core/schemas',
    'core/templates',
    'core/workflow',
    'core/manifest.json',
    'adapters',
    'registry/skills.json',
  ])
    fs.cpSync(path.join(REPO, rel), path.join(root, rel), { recursive: true });
  return root;
}

test('layout: shared files once at the root, host text under hosts/<host>/, nothing at a default location', () => {
  for (const rel of ['bin/dfw', 'bin/dfw.cmd', 'schemas/state.schema.json', 'templates/screen.html', 'dfw.json', 'GENERATED.md'])
    assert.ok(fs.existsSync(path.join(DFW, rel)), rel);
  for (const rel of ROOT_DEFAULTS) assert.ok(!fs.existsSync(path.join(DFW, rel)), `no root ${rel}`);
  assert.deepEqual(fs.readdirSync(path.join(DFW, 'hosts')).sort(), ['claude', 'copilot']);
  assert.ok(!fs.existsSync(path.join(REPO, 'plugins', 'dfw-copilot')), 'no second dfw folder');
  // No file under hosts/ repeats a root file: the launcher, schemas and templates exist once.
  const root = new Set(listFiles(DFW).filter((f) => !f.startsWith('hosts/')));
  for (const host of ['claude', 'copilot'])
    for (const f of listFiles(path.join(DFW, 'hosts', host)))
      if (root.has(f))
        assert.ok(
          !fs.readFileSync(path.join(DFW, f)).equals(fs.readFileSync(path.join(DFW, 'hosts', host, f))),
          `hosts/${host}/${f} is shared`,
        );
  assert.equal(listFiles(DFW).filter((f) => f.endsWith('/bin/dfw') || f === 'bin/dfw').length, 1);
  assert.deepEqual(Object.keys(readJson(path.join(DFW, 'dfw.json'))), ['version', 'dependencies'], 'dfw.json records no host');
});

test('layout: the Claude Code manifest names only hosts/claude/, agents file by file, hooks without --host', () => {
  const pj = readJson(path.join(DFW, '.claude-plugin', 'plugin.json'));
  assert.equal(pj.name, 'dfw');
  assert.equal(pj.skills, './hosts/claude/skills/');
  assert.equal(pj.hooks, './hosts/claude/hooks.json');
  assert.deepEqual(pj.agents, [
    './hosts/claude/agents/design-director.md',
    './hosts/claude/agents/screen-builder.md',
    './hosts/claude/agents/skill-steward.md',
    './hosts/claude/agents/visual-reviewer.md',
  ]);
  for (const a of pj.agents) assert.ok(fs.existsSync(path.join(DFW, a)), a);
  assert.ok(Array.isArray(pj.dependencies) && pj.dependencies.includes('impeccable'));
  const hooks = readJson(path.join(DFW, 'hosts', 'claude', 'hooks.json'));
  const cmds = Object.values(hooks.hooks).flatMap((list) => list.flatMap((m) => m.hooks.map((h) => h.command)));
  assert.equal(cmds.length, 5);
  for (const c of cmds) assert.match(c, /^"\$\{CLAUDE_PLUGIN_ROOT\}\/bin\/dfw" hook [a-z-]+$/);
  const skill = fs.readFileSync(path.join(DFW, 'hosts', 'claude', 'skills', 'dfw', 'SKILL.md'), 'utf8');
  assert.match(skill, /AskUserQuestion/);
  assert.match(skill, /\$\{CLAUDE_SKILL_DIR\}/);
  for (const f of listFiles(path.join(DFW, 'hosts', 'claude')).filter((x) => x.endsWith('.md'))) {
    const t = fs.readFileSync(path.join(DFW, 'hosts', 'claude', f), 'utf8');
    assert.doesNotMatch(t, /\bask_user\b|\{\{[A-Z_]+\}\}/, f);
  }
});

test('layout: the Copilot manifest is legacy, names only hosts/copilot/; version 1 hooks all with --host copilot', () => {
  const pj = readJson(path.join(DFW, 'plugin.json'));
  assert.equal(pj.name, 'dfw');
  assert.equal(pj.$schema, undefined, 'no Agent Plugins $schema');
  assert.deepEqual([pj.skills, pj.agents, pj.hooks], ['hosts/copilot/skills/', 'hosts/copilot/agents/', 'hosts/copilot/hooks.json']);
  assert.equal(pj.dependencies, undefined, 'kept in dfw.json');
  const agents = fs.readdirSync(path.join(DFW, 'hosts', 'copilot', 'agents')).sort();
  assert.deepEqual(agents, ['design-director.agent.md', 'screen-builder.agent.md', 'skill-steward.agent.md', 'visual-reviewer.agent.md']);
  const hooks = readJson(path.join(DFW, 'hosts', 'copilot', 'hooks.json'));
  assert.equal(hooks.version, 1);
  for (const ev of ['SessionStart', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'Stop'])
    for (const h of hooks.hooks[ev]) {
      assert.match(h.bash, /--host copilot/, ev);
      assert.match(h.powershell, /--host copilot/, ev);
    }
  const skill = fs.readFileSync(path.join(DFW, 'hosts', 'copilot', 'skills', 'dfw', 'SKILL.md'), 'utf8');
  assert.match(skill, /the ask_user tool/);
  assert.match(skill, /\.\/references\//);
  for (const f of listFiles(path.join(DFW, 'hosts', 'copilot')).filter((x) => x.endsWith('.md'))) {
    const t = fs.readFileSync(path.join(DFW, 'hosts', 'copilot', f), 'utf8');
    assert.doesNotMatch(t, /CLAUDE_SKILL_DIR|AskUserQuestion|\{\{[A-Z_]+\}\}/, f);
  }
});

test('marketplaces: both list dfw from ./plugins/dfw at the core version; the repo passes the layout check', () => {
  const version = readJson(path.join(REPO, 'core', 'manifest.json')).version;
  for (const file of ['.claude-plugin/marketplace.json', '.github/plugin/marketplace.json']) {
    const mp = readJson(path.join(REPO, file));
    assert.equal(mp.name, 'dfw-marketplace');
    const dfw = mp.plugins.filter((p) => p.name === 'dfw');
    assert.equal(dfw.length, 1, file);
    assert.equal(dfw[0].source, './plugins/dfw', file);
    assert.equal(dfw[0].version, version, file);
  }
  assert.match(readJson(path.join(REPO, '.github/plugin/marketplace.json')).plugins[0].description, /for GitHub Copilot/);
  for (const a of ['claude-code', 'copilot'])
    assert.equal(readJson(path.join(REPO, 'adapters', a, 'assemble.json')).plugin, 'dfw', `${a} builds plugins/dfw`);
  assert.deepEqual(layoutCheck(), []);
});

test('build: a default location at the root is refused and no plugin folder is written', () => {
  const root = buildRoot();
  const out = path.join(root, 'plugins');
  const cc = path.join(root, 'adapters', 'core-copy.json');
  const spec = readJson(cc);
  spec.shared.push({ from: 'core/workflow/agents', to: 'agents' });
  fs.writeFileSync(cc, JSON.stringify(spec));
  assert.throws(
    () => assembleDfw({ root, outRoot: out }),
    (e) => {
      assert.match(e.message, /refused, nothing written/);
      assert.match(e.message, /dfw\/agents: a host reads this location by default; .*hosts\/<host>\//);
      return true;
    },
  );
  assert.deepEqual(fs.readdirSync(out), [], 'no plugin folder and no staging folder left');
  // An adapter file at the root is refused the same way (Claude Code always loads a root skills/).
  spec.shared.pop();
  spec.shared.push({ from: 'core/workflow/SKILL.md', to: 'skills/dfw/SKILL.md' });
  fs.writeFileSync(cc, JSON.stringify(spec));
  assert.throws(() => assembleDfw({ root, outRoot: out }), /dfw\/skills: a host reads this location by default/);
  fs.rmSync(root, { recursive: true, force: true });
});

test('build: one folder for both hosts; a leftover plugins/dfw-copilot is deleted and reported by the check', () => {
  const root = buildRoot();
  const out = path.join(root, 'plugins');
  fs.mkdirSync(path.join(out, 'dfw-copilot', 'agents'), { recursive: true });
  fs.writeFileSync(path.join(out, 'dfw-copilot', 'hooks.json'), '{}');
  assert.deepEqual(strayPlugins(out, ['dfw']), ['plugins/dfw-copilot: not part of a clean build (delete it, or run npm run build)']);
  const r = assembleDfw({ root, outRoot: out });
  assert.deepEqual(r, { plugin: 'dfw', hosts: ['claude', 'copilot'] });
  assert.deepEqual(fs.readdirSync(out), ['dfw']);
  assert.deepEqual(strayPlugins(out, ['dfw']), []);
  // The same files as the committed folder.
  assert.deepEqual(listFiles(path.join(out, 'dfw')), listFiles(DFW));
  // The layout check names what breaks the layout.
  fs.mkdirSync(path.join(out, 'dfw-copilot'));
  fs.mkdirSync(path.join(out, 'dfw', 'skills'));
  fs.mkdirSync(path.join(out, 'dfw', 'hosts', 'claude', 'bin'));
  fs.copyFileSync(path.join(out, 'dfw', 'bin', 'dfw'), path.join(out, 'dfw', 'hosts', 'claude', 'bin', 'dfw'));
  const pj = path.join(out, 'dfw', 'plugin.json');
  fs.writeFileSync(pj, JSON.stringify({ ...readJson(pj), $schema: 'x', agents: 'hosts/claude/agents/' }));
  const errs = layoutCheck({ pluginsRoot: out, root: REPO });
  assert.ok(
    errs.includes('plugins/dfw-copilot: retired (one plugins/dfw serves every host); delete it and run npm run build'),
    errs.join('\n'),
  );
  assert.ok(errs.some((e) => e.startsWith('plugins/dfw/skills: a host reads this location by default')));
  assert.ok(errs.includes('plugins/dfw/plugin.json: has a $schema (must stay a legacy manifest, not Agent Plugins)'));
  assert.ok(errs.includes('plugins/dfw/plugin.json: names hosts/claude/agents/, outside hosts/copilot/'));
  assert.ok(errs.some((e) => e.startsWith('plugins/dfw/hosts/claude/bin/dfw: the same file is shared at plugins/dfw/bin/dfw')));
  fs.rmSync(root, { recursive: true, force: true });
});
