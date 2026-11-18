import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { notePresence, preToolUse } from '../../core/lib/hooks.mjs';
import { describeParts, renderSystemPage } from '../../core/lib/system-page.mjs';
import { composeHtml } from '../../core/lib/compose.mjs';
import { buildTokens } from '../../core/lib/tokens.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DFW = path.resolve(HERE, '../../core/bin/dfw.mjs');
const FIX = path.resolve(HERE, '../fixtures/penpot/project');

function project(state = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-presence-'));
  fs.cpSync(FIX, root, { recursive: true });
  fs.mkdirSync(path.join(root, '.dfw', 'reviews'), { recursive: true });
  fs.writeFileSync(path.join(root, '.dfw', 'state.json'), JSON.stringify({ mode: 'new-product', phase: 'concepts', active: true, design: { version: 1, status: 'draft', locked: false, approved: [] }, gates: {}, ...state }));
  const file = path.join(root, 'design', 'screens', 'demo', 'a.html');
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, '<main data-dfw-id="demo.main">\n  <h1 data-dfw-id="demo.title">Hello</h1>\n  <p>Plain line</p>\n  <dfw-use c="button" id="demo.cta">Go</dfw-use>\n  <span>After</span>\n</main>\n');
  return { root, file };
}

test('presence: the hook names the element the agent is about to edit', () => {
  const { root, file } = project();
  const at = (old) => notePresence(root, { tool_input: { old_string: old } }, file);
  assert.deepEqual([at('<h1 data-dfw-id="demo.title">Hello</h1>').file, at('<h1 data-dfw-id="demo.title">Hello</h1>').dfwId], ['screens/demo/a.html', 'demo.title']);
  assert.equal(at('Plain line').dfwId, 'demo.title', 'nearest id before the text');
  assert.equal(at('After').dfwId, 'demo.cta', 'an instance id counts');
  assert.equal(at('not in the file').dfwId, null);
  assert.equal(notePresence(root, { tool_input: { edits: [{ old_string: 'Hello' }] } }, file).dfwId, 'demo.title', 'MultiEdit: the first edit');
  assert.equal(notePresence(root, { tool_input: {} }, file).dfwId, null, 'Write: the file only');
  assert.equal(notePresence(root, { tool_input: {} }, path.join(root, 'src', 'app.ts')), null, 'not a design file');
  assert.equal(notePresence(root, { tool_input: {} }, path.join(root, 'design', 'approved', 'v1', 'a.html')), null);
  fs.rmSync(root, { recursive: true, force: true });
});

test('presence: written only for an edit the gate allows', () => {
  const { root, file } = project();
  const note = path.join(root, '.dfw', 'presence.json');
  const input = (f) => ({ cwd: root, tool_name: 'Edit', tool_input: { file_path: f, old_string: 'Hello', new_string: 'Hi' } });
  process.env.DFW_PROJECT_DIR = root;
  try {
    assert.ok(preToolUse(input(path.join(root, 'src', 'app.ts')))?.deny, 'implementation is gated');
    assert.ok(!fs.existsSync(note));
    assert.equal(preToolUse(input(file)), null);
    assert.equal(JSON.parse(fs.readFileSync(note, 'utf8')).dfwId, 'demo.title');
  } finally {
    delete process.env.DFW_PROJECT_DIR;
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('presence: the Studio passes the note on to its pages', async () => {
  const { root } = project();
  buildTokens(root);
  const port = 46000 + Math.floor(Math.random() * 900);
  const child = spawn(process.execPath, [DFW, '__studio-server', '--root', root, '--port', String(port)], { stdio: 'ignore', env: { ...process.env, DFW_HOME: path.join(root, '.home'), DFW_STUDIO_WATCH: 'poll' } });
  const base = `http://127.0.0.1:${port}`;
  const ctl = new AbortController();
  try {
    let res = null;
    for (let i = 0; i < 50 && !res; i++) res = await fetch(`${base}/__dfw/events`, { signal: ctl.signal }).catch(() => new Promise((r) => setTimeout(r, 100)));
    const reader = res.body.getReader();
    const dec = new TextDecoder();
    setTimeout(() => fs.writeFileSync(path.join(root, '.dfw', 'presence.json'), JSON.stringify({ file: 'screens/demo/a.html', dfwId: 'demo.title', at: new Date().toISOString() })), 300);
    let buf = '';
    const end = Date.now() + 5000;
    while (!/event: presence\ndata: (.*)\n/.test(buf) && Date.now() < end) buf += dec.decode((await reader.read()).value, { stream: true });
    assert.deepEqual(JSON.parse(/event: presence\ndata: (.*)\n/.exec(buf)[1]), { file: 'screens/demo/a.html', dfwId: 'demo.title' });
  } finally {
    ctl.abort();
    child.kill();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('design system page: a description becomes a summary, one meaning per name, and rules; nothing is dropped', () => {
  const def = {
    description: 'Text button (renders a link when href is set). primary = the one main action per view (Save, Send); ghost = low emphasis; sm = dense tables. href turns it into a link. Never two primaries in one view.',
    variants: ['primary', 'secondary', 'ghost'],
    sizes: ['md', 'sm'],
    props: ['icon', 'href'],
  };
  const p = describeParts(def);
  assert.equal(p.summary, 'Text button (renders a link when href is set).');
  assert.deepEqual([...p.meaning], [['primary', 'the one main action per view (Save, Send)'], ['ghost', 'low emphasis'], ['sm', 'dense tables']]);
  assert.deepEqual(p.notes, ['href turns it into a link.', 'Never two primaries in one view.']);
  const all = [p.summary, ...p.notes, ...[...p.meaning].map(([k, v]) => `${k} = ${v}`)].join(' ');
  for (const word of def.description.split(/[^\w]+/).filter((w) => w.length > 3)) assert.ok(all.includes(word), `kept: ${word}`);
  assert.deepEqual(describeParts({ description: '', variants: [], sizes: [], props: [] }), { summary: '', notes: [], meaning: new Map() });
});

test('design system page: components are documented in rows, in three text sizes', () => {
  const { root } = project();
  buildTokens(root);
  const html = composeHtml(root, renderSystemPage(root, { designBase: '../' })).html;
  assert.match(html, /<table class="ds-kv">/);
  assert.match(html, /<th scope="row">Master<\/th><td><code>design\/system\/components\/[\w-]+\.html<\/code>/);
  const css = /<style>([\s\S]*?)<\/style>/.exec(html)[1];
  const sizes = new Set([...css.matchAll(/font(?:-size)?:[^;{}]*?(\d+)px/g)].map((m) => Number(m[1])));
  assert.ok(![...sizes].some((n) => n < 12), `no documentation text under 12px (found ${[...sizes].join(', ')})`);
  fs.rmSync(root, { recursive: true, force: true });
});
