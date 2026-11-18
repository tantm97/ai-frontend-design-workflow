import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildExport, syncThreads, importChanges, exportStale } from '../../core/lib/penpot.mjs';
import { allComments, updateComment } from '../../core/lib/comments.mjs';
import { undelivered, formatForClaude } from '../../core/lib/inbox.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIX = path.join(HERE, '..', 'fixtures', 'penpot');
const read = (f) => JSON.parse(fs.readFileSync(path.join(FIX, f), 'utf8'));

function project() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-penpot-live-'));
  fs.cpSync(path.join(FIX, 'project'), root, { recursive: true });
  fs.mkdirSync(path.join(root, '.dfw', 'penpot', 'exports'), { recursive: true });
  fs.writeFileSync(path.join(root, '.dfw', 'state.json'), JSON.stringify({ design: { version: 1, status: 'draft', locked: false } }));
  const doc = buildExport({ renders: read('renders.json'), tokens: { base: read('project/design/tokens/tokens.json'), themes: {} }, product: 'Fixture', generatedAt: new Date(Date.now() + 2000).toISOString() }); // rendered after the files were copied
  fs.writeFileSync(path.join(root, '.dfw', 'penpot', 'exports', `${doc.id}.json`), JSON.stringify(doc));
  fs.writeFileSync(path.join(root, '.dfw', 'penpot', 'export.json'), JSON.stringify(doc));
  return { root, doc };
}

const thread = (seq, messages, extra = {}) => ({ seq, resolved: false, page: 'Stock', boardKey: 'items|a|desktop', x: 120, y: 175, comments: messages.map((content) => ({ content, user: 'Designer' })), ...extra });
const one = (root) => allComments(root)[0];

test('penpot live: a new thread becomes one dfw comment, and stays one', () => {
  const { root, doc } = project();
  const payload = { fileId: 'file-1', exportId: doc.id, threads: [thread(1, ['Make low stock more visible']), thread(2, ['Old note'], { resolved: true })] };
  const first = syncThreads(root, payload);
  assert.equal(first.comments.length, 1); // the resolved thread was never open for dfw
  assert.deepEqual(first.replies, []);
  const c = one(root);
  assert.equal(c.provider, 'penpot');
  assert.match(c.comment, /Make low stock more visible/);
  assert.equal(c.screen, 'items');
  const again = syncThreads(root, payload);
  assert.deepEqual([again.comments.length, again.updated.length, allComments(root).length], [0, 0, 1]);
  // "Send changes" from the same file does not import the thread a second time.
  importChanges(root, { schema: 'dfw-penpot-changes/1', exportId: doc.id, fileId: 'file-1', boards: [], comments: payload.threads });
  assert.equal(allComments(root).length, 1);
  fs.rmSync(root, { recursive: true, force: true });
});

test('penpot live: what dfw does comes back as thread replies, once', () => {
  const { root, doc } = project();
  const t = thread(1, ['Make low stock more visible']);
  const base = { fileId: 'file-1', exportId: doc.id };
  syncThreads(root, { ...base, threads: [t] });
  const id = one(root).id;
  updateComment(root, id, { status: 'acknowledged' });
  const ack = syncThreads(root, { ...base, threads: [t] }).replies;
  assert.equal(ack.length, 1);
  assert.match(ack[0].text, new RegExp(`^\\[dfw\\] ${id} seen`));
  assert.equal(ack[0].seq, 1);
  // Not confirmed as posted yet: offered again. Confirmed: not offered again.
  assert.equal(syncThreads(root, { ...base, threads: [t] }).replies.length, 1);
  assert.equal(syncThreads(root, { ...base, threads: [t], posted: [{ seq: 1, key: ack[0].key }] }).replies.length, 0);
  updateComment(root, id, { status: 'fixed', message: 'Low stock rows now have a warning badge.' });
  const fixed = syncThreads(root, { ...base, threads: [t] }).replies;
  assert.equal(fixed.length, 1);
  assert.match(fixed[0].text, /fixed: Low stock rows now have a warning badge\.[\s\S]*Resolve this thread to accept/);
  // The reply dfw posted is in the thread now: it is not a designer message.
  const withReply = thread(1, ['Make low stock more visible', fixed[0].text]);
  const r = syncThreads(root, { ...base, threads: [withReply], posted: [{ seq: 1, key: fixed[0].key }] });
  assert.deepEqual([r.comments.length, r.updated.length, r.replies.length], [0, 0, 0]);
  assert.equal(one(root).status, 'fixed');
  // Another file's thread #1 is another thread.
  assert.equal(syncThreads(root, { fileId: 'file-2', exportId: doc.id, threads: [] }).replies.length, 0);
  fs.rmSync(root, { recursive: true, force: true });
});

test('penpot live: replying reopens, resolving accepts a fix or dismisses an open comment', () => {
  const { root, doc } = project();
  const base = { fileId: 'file-1', exportId: doc.id };
  syncThreads(root, { ...base, threads: [thread(1, ['A']), thread(3, ['B'])] });
  const [a, b] = allComments(root).map((c) => c.id);
  updateComment(root, a, { status: 'fixed', message: 'Done.' });
  // Designer replies after the fix: reopened with the message, and the agent is told.
  let r = syncThreads(root, { ...base, threads: [thread(1, ['A', '[dfw] fixed: Done.', 'Still too small']), thread(3, ['B'])] });
  assert.deepEqual(r.updated, [{ id: a, status: 'reopened' }]);
  assert.match(formatForClaude(undelivered(root).filter((c) => c.id === a)), /reopened by user: "Still too small"/);
  updateComment(root, a, { status: 'fixed', message: 'Bigger now.' });
  // Resolving the thread accepts the fix; resolving an open thread dismisses the comment.
  r = syncThreads(root, { ...base, threads: [thread(1, ['A', 'Still too small'], { resolved: true }), thread(3, ['B'], { resolved: true })] });
  assert.deepEqual(r.updated, [{ id: a, status: 'resolved' }, { id: b, status: 'dismissed' }]);
  const byId = new Map(allComments(root).map((c) => [c.id, c]));
  assert.equal(byId.get(a).thread.at(-1).by, 'user');
  // Nothing changes on the next check; un-resolving the thread reopens.
  r = syncThreads(root, { ...base, threads: [thread(1, ['A', 'Still too small'], { resolved: true }), thread(3, ['B'], { resolved: true })] });
  assert.deepEqual(r.updated, []);
  r = syncThreads(root, { ...base, threads: [thread(1, ['A', 'Still too small']), thread(3, ['B'], { resolved: true })] });
  assert.deepEqual(r.updated, [{ id: a, status: 'reopened' }]);
  fs.rmSync(root, { recursive: true, force: true });
});

test('penpot live: a follow-up on a comment being worked on reaches the agent', () => {
  const { root, doc } = project();
  const base = { fileId: 'file-1', exportId: doc.id };
  syncThreads(root, { ...base, threads: [thread(1, ['A'])] });
  const id = one(root).id;
  updateComment(root, id, { status: 'acknowledged' });
  assert.equal(undelivered(root).length, 0);
  syncThreads(root, { ...base, threads: [thread(1, ['A', 'Also the header'])] });
  assert.equal(one(root).status, 'acknowledged');
  assert.match(formatForClaude(undelivered(root)), /follow-up by user: "Also the header"/);
  fs.rmSync(root, { recursive: true, force: true });
});

test('penpot live: boards are behind once a design file changes after the export', () => {
  const { root, doc } = project();
  assert.equal(exportStale(root), false);
  const screen = path.join(root, 'design', 'screens');
  const file = fs.readdirSync(screen, { recursive: true }).map(String).find((f) => f.endsWith('.html'));
  const future = new Date(Date.now() + 60_000);
  fs.utimesSync(path.join(screen, file), future, future);
  assert.equal(exportStale(root), true);
  assert.equal(syncThreads(root, { fileId: 'f', exportId: doc.id, threads: [] }).stale, true);
  fs.rmSync(root, { recursive: true, force: true });
});

/* ------------------------------------------------------------ the plugin code, against a small fake of Penpot's API */

function fakePenpot() {
  let ids = 0;
  const shape = (type) => {
    const s = { id: `s${++ids}`, type, name: '', x: 0, y: 0, width: 0, height: 0, fills: [], children: [], parent: null, _data: {} };
    s.setPluginData = (k, v) => (s._data[k] = v);
    s.getPluginData = (k) => s._data[k] || '';
    s.resize = (w, h) => Object.assign(s, { width: w, height: h });
    s.appendChild = (c) => {
      c.parent?.children.splice(c.parent.children.indexOf(c), 1);
      c.parent = s;
      s.children.push(c);
    };
    s.remove = () => {
      s.parent?.children.splice(s.parent.children.indexOf(s), 1);
      s.parent = null;
    };
    return s;
  };
  const page = (name) => {
    const p = { id: `p${++ids}`, name, root: shape('root'), threads: [] };
    p.findCommentThreads = async () => p.threads;
    return p;
  };
  const file = { id: 'file-1', name: 'Fixture', pages: [], _data: {}, setPluginData: (k, v) => (file._data[k] = v), getPluginData: (k) => file._data[k] || '' };
  let current = null;
  const sent = [];
  const penpot = {
    ui: { open() {}, onMessage: (fn) => (penpot.handler = fn), sendMessage: (m) => sent.push(m) },
    currentFile: file,
    library: { local: { components: [], tokens: { sets: [], themes: [] }, createComponent: () => ({ setPluginData() {}, mainInstance: () => null }) } },
    fonts: { findByName: () => null },
    createPage: () => {
      const p = page('');
      file.pages.push(p);
      return p;
    },
    openPage: async (p) => (current = p),
    createBoard: () => {
      const b = shape('board');
      current.root.appendChild(b);
      return b;
    },
    createRectangle: () => {
      const r = shape('rectangle');
      current.root.appendChild(r);
      return r;
    },
    createText: (t) => Object.assign(shape('text'), { characters: t }),
  };
  return { penpot, file, sent };
}

function loadPlugin() {
  const env = fakePenpot();
  const code = fs.readFileSync(path.join(HERE, '../../core/studio/penpot/plugin.js'), 'utf8');
  new Function('penpot', code)(env.penpot);
  return { ...env, send: async (msg) => (await env.penpot.handler(msg), env.sent.at(-1)) };
}

const exportDoc = (id, label) => ({
  schema: 'dfw-penpot-export/1',
  id,
  components: [],
  pages: [{ title: 'Stock', boards: [{ key: 'items|a|desktop', name: 'items / desktop', x: 0, y: 0, width: 800, height: 600, background: { color: '#ffffff', opacity: 1 }, layers: [{ path: '0', name: label, kind: 'rect', x: 10, y: 10, w: 100, h: 40 }] }] }],
});

test('penpot plugin: Update boards redraws a board in place, so its comment threads stay', async () => {
  const p = loadPlugin();
  assert.equal((await p.send({ type: 'build', export: exportDoc('px_1', 'old') })).stats.boards, 1);
  const board = p.file.pages[0].root.children[0];
  board.x = 500; // the designer moved the board
  const built = await p.send({ type: 'build', export: exportDoc('px_2', 'new'), options: { update: true } });
  assert.deepEqual([built.stats.boards, built.stats.updated], [1, 1]);
  assert.equal(p.file.pages.length, 1);
  assert.equal(p.file.pages[0].root.children.length, 1);
  assert.equal(p.file.pages[0].root.children[0], board);
  assert.deepEqual(board.children.map((c) => [c.name, c.x]), [['new', 510]]);
  assert.equal(JSON.parse(board.getPluginData('dfw-board')).exportId, 'px_2');
});

test('penpot plugin: live threads are the ones on dfw boards; replies are posted into them', async () => {
  const p = loadPlugin();
  await p.send({ type: 'build', export: exportDoc('px_1', 'old') });
  const page = p.file.pages[0];
  const board = page.root.children[0];
  const posted = [];
  const mk = (seq, onBoard) => ({ seqNumber: seq, resolved: false, board: onBoard ? board : null, position: { x: 30, y: 40 }, findComments: async () => [{ content: `c${seq}`, user: { name: 'Designer' } }], reply: async (text) => posted.push([seq, text]) });
  page.threads = [mk(1, true), mk(2, false)];
  const live = await p.send({ type: 'threads' });
  assert.deepEqual([live.data.fileId, live.data.exportId, live.data.threads.map((t) => t.seq)], ['file-1', 'px_1', [1]]);
  assert.equal(live.data.threads[0].boardKey, 'items|a|desktop');
  const r = await p.send({ type: 'reply', replies: [{ seq: 1, key: 'k1', text: '[dfw] fb_001 fixed: done' }, { seq: 9, key: 'k9', text: 'gone' }] });
  assert.deepEqual(r.done, [{ seq: 1, key: 'k1' }]);
  assert.deepEqual(posted, [[1, '[dfw] fb_001 fixed: done']]);
});

test('penpot live: a reply in a thread that stays resolved reopens the comment and reaches the agent', () => {
  const { root, doc } = project();
  const base = { fileId: 'file-1', exportId: doc.id };
  syncThreads(root, { ...base, threads: [thread(1, ['Make it red'])] });
  syncThreads(root, { ...base, threads: [thread(1, ['Make it red'], { resolved: true })] });
  assert.equal(one(root).status, 'dismissed');
  assert.equal(undelivered(root).length, 0);
  syncThreads(root, { ...base, threads: [thread(1, ['Make it red', 'Actually please do it'], { resolved: true })] });
  assert.equal(one(root).status, 'reopened');
  assert.match(formatForClaude(undelivered(root)), /Actually please do it/);
  const again = syncThreads(root, { ...base, threads: [thread(1, ['Make it red', 'Actually please do it'], { resolved: true })] });
  assert.deepEqual(again.updated, [], 'the same messages change nothing');
  fs.rmSync(root, { recursive: true, force: true });
});

test('penpot live: "seen, working on it" is posted once, also after a later reply', async () => {
  const { root, doc } = project();
  const t = thread(1, ['Make it red']);
  const base = { fileId: 'file-1', exportId: doc.id };
  syncThreads(root, { ...base, threads: [t] });
  const id = one(root).id;
  updateComment(root, id, { status: 'acknowledged' });
  const first = syncThreads(root, { ...base, threads: [t] }).replies;
  assert.deepEqual(first.map((r) => r.text), [`[dfw] ${id} seen, working on it.`]);
  await new Promise((res) => setTimeout(res, 5));
  updateComment(root, id, { message: 'Which red: brand or status?' });
  const next = syncThreads(root, { ...base, threads: [t], posted: first.map(({ seq, key }) => ({ seq, key })) }).replies;
  assert.deepEqual(next.map((r) => r.text), [`[dfw] ${id}: Which red: brand or status?`]);
  fs.rmSync(root, { recursive: true, force: true });
});
