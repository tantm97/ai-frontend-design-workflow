import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

import { newState, transitionBlockers, setPhase, approveGate, saveState, loadState } from '../../core/lib/state.mjs';
import { decideWrite } from '../../core/lib/gate.mjs';
import { loadConfig, DEFAULTS } from '../../core/lib/config.mjs';
import { lintHtml, lintCss, lintTokens } from '../../core/lib/lint.mjs';
import { tokensToDeclarations } from '../../core/lib/tokens.mjs';
import { addComment, updateComment, allComments } from '../../core/lib/comments.mjs';
import { freezeDesign, verifySnapshot, reopenDesign } from '../../core/lib/freeze.mjs';
import { globToRegExp } from '../../core/lib/util.mjs';

const DFW = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../core/bin/dfw');

function tmpProject() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-unit-'));
  process.env.DFW_HOME = path.join(dir, '.home');
  return dir;
}
function cli(root, ...args) {
  return execFileSync(process.execPath, [DFW, ...args], { cwd: root, env: { ...process.env, DFW_HOME: path.join(root, '.home') }, encoding: 'utf8' });
}

test('glob matching', () => {
  assert.ok(globToRegExp('design/**').test('design/screens/a.html'));
  assert.ok(globToRegExp('design/approved/**').test('design/approved/v1/x.html'));
  assert.ok(!globToRegExp('design/**').test('src/design/x'));
  assert.ok(globToRegExp('PRODUCT.md').test('PRODUCT.md'));
});

test('state machine: concepts needs G1, implement needs G2+G3', () => {
  const s = newState({ mode: 'new-product' });
  assert.deepEqual(transitionBlockers(s, 'concepts'), ['G1 (design direction) is not approved yet.']);
  const b = transitionBlockers(s, 'implement');
  assert.equal(b.length, 2);
  s.gates.G1 = { approvedAt: 'x' };
  assert.deepEqual(transitionBlockers(s, 'concepts'), []);
  assert.equal(newState({ mode: 'quick' }).mode, 'quick-change', 'mode aliases');
  const q = newState({ mode: 'quick-change' });
  assert.deepEqual(transitionBlockers(q, 'concepts'), [], 'quick-change waives G1');
  const d = newState({ mode: 'design-only' });
  assert.ok(transitionBlockers(d, 'implement').some((r) => r.includes('does not include implementation')));
});

test('write gate decisions', () => {
  const root = '/proj';
  const config = loadConfig(null);
  const s = newState({ mode: 'new-product' });
  const w = (f) => decideWrite({ root, state: s, config, absFile: path.join(root, f) });
  assert.equal(w('design/screens/a/a.html').allow, true);
  assert.equal(w('PRODUCT.md').allow, true);
  assert.equal(w('.dfw/notes.md').allow, true);
  assert.equal(w('src/App.tsx').allow, false);
  assert.equal(decideWrite({ root, state: s, config, absFile: '/elsewhere/x.js' }).allow, true, 'outside project');
  assert.equal(w('design/approved/v1/screens/a.html').allow, false, 'snapshots immutable');
  // After freeze: design locked, handoff allowed, code still blocked until G3 + implement phase.
  s.design = { version: 1, status: 'approved', locked: true, approved: [{ version: 'v1' }] };
  assert.equal(w('design/screens/a/a.html').allow, false);
  assert.equal(w('DESIGN.md').allow, false);
  assert.equal(w('design/handoff/v1.md').allow, true);
  assert.equal(w('src/App.tsx').allow, false);
  s.gates.G3 = { approvedAt: 'x' };
  s.phase = 'implement';
  assert.equal(w('src/App.tsx').allow, true);
  // Paused workflow lets everything through.
  s.active = false;
  s.phase = 'discovery';
  assert.equal(w('src/App.tsx').allow, true);
  // Review-only blocks everything except .dfw.
  const r = newState({ mode: 'review-only' });
  assert.equal(decideWrite({ root, state: r, config, absFile: '/proj/design/x.html' }).allow, false);
  assert.equal(decideWrite({ root, state: r, config, absFile: '/proj/.dfw/report.md' }).allow, true);
});

test('config: gate.allow is a union across layers', () => {
  const root = tmpProject();
  fs.mkdirSync(path.join(root, '.dfw'), { recursive: true });
  fs.writeFileSync(path.join(root, '.dfw', 'config.json'), JSON.stringify({ gate: { allow: ['docs/**'] }, checks: { onEdit: false } }));
  const c = loadConfig(root);
  assert.ok(c.gate.allow.includes('docs/**'));
  assert.ok(c.gate.allow.includes('.dfw/**'), 'defaults kept');
  assert.equal(c.checks.onEdit, false);
  assert.equal(c.checks.sloplint, DEFAULTS.checks.sloplint);
});

test('lint: ids, alt, colors, focus', () => {
  const html = `<html><head><link href="../../tokens/tokens.css"></head><body><button data-dfw-id="a.b">x</button><button data-dfw-id="a.b">y</button><img src="x.png"><a href="#">z</a></body></html>`;
  const f = lintHtml('x.html', html);
  const rules = f.map((x) => x.rule);
  assert.ok(rules.includes('dfw-id-duplicate'));
  assert.ok(rules.includes('img-alt'));
  assert.ok(rules.includes('dfw-id-missing'));
  assert.ok(rules.includes('html-lang'));
  const css = lintCss('design/styles/x.css', ':root{--x:#fff}\n.a{color:#123456}\n.b:focus{outline:none}');
  assert.deepEqual(css.map((x) => x.rule).sort(), ['focus-removed', 'hardcoded-color']);
});

test('tokens: DTCG 2025.10 values and aliases', () => {
  const tree = {
    color: {
      $type: 'color',
      blue: { $value: { colorSpace: 'srgb', components: [0, 0.4, 1], hex: '#0066ff' } },
      accent: { $value: '{color.blue}' },
      soft: { $value: { colorSpace: 'oklch', components: [0.9, 0.05, 250], alpha: 0.5 } },
    },
    space: { $type: 'dimension', 4: { $value: { value: 16, unit: 'px' } } },
    font: { body: { $type: 'fontFamily', $value: ['Source Sans 3', 'sans-serif'] } },
    shadow: { card: { $type: 'shadow', $value: { color: '{color.blue}', offsetX: { value: 0, unit: 'px' }, offsetY: { value: 1, unit: 'px' }, blur: { value: 2, unit: 'px' }, spread: { value: 0, unit: 'px' } } } },
  };
  const d = Object.fromEntries(tokensToDeclarations(tree));
  assert.equal(d['--color-blue'], '#0066ff');
  assert.equal(d['--color-accent'], 'var(--color-blue)');
  assert.equal(d['--color-soft'], 'oklch(0.9 0.05 250 / 0.5)');
  assert.equal(d['--space-4'], '16px');
  assert.equal(d['--font-body'], '"Source Sans 3", sans-serif');
  assert.equal(d['--shadow-card'], '0px 1px 2px 0px var(--color-blue)');
  assert.deepEqual(lintTokens('t.json', tree), []);
  assert.equal(lintTokens('t.json', { a: { $type: 'color', $value: '{nope}' } })[0].rule, 'token-alias');
});

test('feedback lifecycle: only the user resolves', () => {
  const root = tmpProject();
  saveState(root, newState({}));
  fs.mkdirSync(path.join(root, 'design/screens/home'), { recursive: true });
  fs.writeFileSync(path.join(root, 'design/screens/home/a.html'), '<html>\n<body>\n<button data-dfw-id="home.cta">Go</button>\n</body></html>');
  const c = addComment(root, { screen: 'home', variant: 'a', file: 'screens/home/a.html', target: { dfwId: 'home.cta' }, comment: 'Bigger' });
  assert.equal(c.id, 'fb_001');
  assert.equal(c.target.source, 'design/screens/home/a.html:3');
  assert.throws(() => updateComment(root, c.id, { status: 'resolved', by: 'user' }), /cannot go from "open"/);
  assert.throws(() => updateComment(root, c.id, { status: 'fixed' }), /needs a short note/);
  updateComment(root, c.id, { status: 'fixed', message: 'done' });
  assert.throws(() => updateComment(root, c.id, { status: 'resolved' }), /Only the user/);
  assert.throws(() => updateComment(root, c.id, { status: 'dismissed' }), /cannot go from/);
  updateComment(root, c.id, { status: 'resolved', by: 'user' });
  assert.equal(allComments(root)[0].status, 'resolved');
  assert.equal(addComment(root, { comment: 'second' }).id, 'fb_002');
});

test('freeze: snapshot, lock, tamper detection, reopen', () => {
  const root = tmpProject();
  let s = newState({});
  s.gates.G1 = { approvedAt: 'x' };
  s.phase = 'concepts';
  saveState(root, s);
  fs.mkdirSync(path.join(root, 'design/screens/home'), { recursive: true });
  fs.writeFileSync(path.join(root, 'design/design.json'), JSON.stringify({ schema: 1, screens: [] }));
  fs.writeFileSync(path.join(root, 'design/screens/home/a.html'), '<html></html>');
  fs.writeFileSync(path.join(root, 'DESIGN.md'), '# Design');
  s = freezeDesign(root, loadState(root), { note: 'ok' });
  assert.equal(s.design.status, 'approved');
  assert.ok(fs.existsSync(path.join(root, 'design/approved/v1/docs/DESIGN.md')));
  assert.deepEqual(verifySnapshot(root, 'v1'), []);
  fs.writeFileSync(path.join(root, 'design/approved/v1/screens/home/a.html'), '<html>x</html>');
  assert.deepEqual(verifySnapshot(root, 'v1'), ['modified: screens/home/a.html']);
  assert.throws(() => freezeDesign(root, loadState(root), {}), /already approved/);
  s = reopenDesign(root, loadState(root), { reason: 'client feedback' });
  assert.equal(s.design.version, 2);
  assert.equal(s.design.locked, false);
  assert.equal(s.gates.G2, null);
});

test('cli: init, gates, approve G2 blocked by MUST findings', () => {
  const root = tmpProject();
  cli(root, 'init', '--product', 'Test');
  assert.ok(fs.existsSync(path.join(root, 'PRODUCT.md')));
  assert.ok(fs.existsSync(path.join(root, 'design/tokens/tokens.css')));
  assert.throws(() => cli(root, 'phase', 'concepts'), /G1/);
  cli(root, 'phase', 'direction');
  cli(root, 'approve', 'G1', '--note', 'Direction A');
  cli(root, 'phase', 'concepts');
  fs.mkdirSync(path.join(root, 'design/screens/home'), { recursive: true });
  fs.writeFileSync(path.join(root, 'design/screens/home/a.html'), '<!doctype html><html lang="en"><head><link rel="stylesheet" href="../../tokens/tokens.css"></head><body><img src="x.png" data-dfw-id="hero.img"></body></html>');
  const m = JSON.parse(fs.readFileSync(path.join(root, 'design/design.json'), 'utf8'));
  m.screens = [{ id: 'home', variants: [{ id: 'a', file: 'screens/home/a.html' }] }];
  fs.writeFileSync(path.join(root, 'design/design.json'), JSON.stringify(m));
  assert.throws(() => cli(root, 'approve', 'G2', '--note', 'v1'), /MUST finding/);
  fs.writeFileSync(path.join(root, 'design/screens/home/a.html'), '<!doctype html><html lang="en"><head><link rel="stylesheet" href="../../tokens/tokens.css"></head><body><img src="x.png" alt="" data-dfw-id="hero.img"></body></html>');
  const out = cli(root, 'approve', 'G2', '--note', 'v1');
  assert.match(out, /Design frozen as v1/);
  assert.throws(() => cli(root, 'phase', 'implement'), /G3/);
  cli(root, 'stack', 'set', '--json', '{"framework":{"name":"Vue"}}');
  cli(root, 'approve', 'G3', '--note', 'Vue 3 + plain CSS');
  cli(root, 'phase', 'implement');
  assert.match(cli(root, 'status'), /phase=implement/);
  assert.match(cli(root, 'handoff'), /design\/handoff\/v1\.md/);
  const decisions = cli(root, 'decisions');
  assert.match(decisions, /gate:G1/);
  assert.match(decisions, /gate:G3/);
});

test('hooks: pre-tool-use denies code before gates, fails open on bad input', () => {
  const root = tmpProject();
  cli(root, 'init');
  const run = (input) => execFileSync(process.execPath, [DFW, 'hook', 'pre-tool-use'], { input: JSON.stringify(input), env: { ...process.env, CLAUDE_PROJECT_DIR: root, DFW_HOME: path.join(root, '.home') }, encoding: 'utf8' });
  const deny = JSON.parse(run({ tool_name: 'Write', tool_input: { file_path: path.join(root, 'src/main.ts') }, cwd: root }));
  assert.equal(deny.hookSpecificOutput.permissionDecision, 'deny');
  assert.equal(run({ tool_name: 'Write', tool_input: { file_path: path.join(root, 'design/x.html') }, cwd: root }), '');
  assert.equal(run({ tool_name: 'Read', tool_input: { file_path: path.join(root, 'src/main.ts') }, cwd: root }), '');
  const bad = execFileSync(process.execPath, [DFW, 'hook', 'pre-tool-use'], { input: 'not json', env: { ...process.env, CLAUDE_PROJECT_DIR: root }, encoding: 'utf8' });
  assert.equal(bad, '');
});

test('language: asked once per session, last used remembered', () => {
  const root = tmpProject();
  const st = JSON.parse(cli(root, 'lang', 'status', '--session', 'abc'));
  assert.equal(st.chosen, false);
  assert.equal(st.lastUsed, 'en');
  cli(root, 'lang', 'set', 'vi', '--session', 'abc');
  assert.equal(JSON.parse(cli(root, 'lang', 'status', '--session', 'abc')).chosen, true);
  const other = JSON.parse(cli(root, 'lang', 'status', '--session', 'def'));
  assert.equal(other.chosen, false, 'new session asks again');
  assert.equal(other.lastUsed, 'vi', 'offers last used as default');
});
