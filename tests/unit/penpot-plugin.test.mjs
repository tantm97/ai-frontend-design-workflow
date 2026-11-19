// The Penpot plugin (core/studio/penpot/plugin.js, browser code that runs in Penpot's plugin
// sandbox) against a fake of the Penpot plugin API: board updates in place and live threads.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));

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
  const file = {
    id: 'file-1',
    name: 'Fixture',
    pages: [],
    _data: {},
    setPluginData: (k, v) => (file._data[k] = v),
    getPluginData: (k) => file._data[k] || '',
  };
  let current = null;
  const sent = [];
  const penpot = {
    ui: { open() {}, onMessage: (fn) => (penpot.handler = fn), sendMessage: (m) => sent.push(m) },
    currentFile: file,
    library: {
      local: {
        components: [],
        tokens: { sets: [], themes: [] },
        createComponent: () => ({ setPluginData() {}, mainInstance: () => null }),
      },
    },
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
  pages: [
    {
      title: 'Stock',
      boards: [
        {
          key: 'items|a|desktop',
          name: 'items / desktop',
          x: 0,
          y: 0,
          width: 800,
          height: 600,
          background: { color: '#ffffff', opacity: 1 },
          layers: [{ path: '0', name: label, kind: 'rect', x: 10, y: 10, w: 100, h: 40 }],
        },
      ],
    },
  ],
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
  assert.deepEqual(
    board.children.map((c) => [c.name, c.x]),
    [['new', 510]],
  );
  assert.equal(JSON.parse(board.getPluginData('dfw-board')).exportId, 'px_2');
});

test('penpot plugin: live threads are the ones on dfw boards; replies are posted into them', async () => {
  const p = loadPlugin();
  await p.send({ type: 'build', export: exportDoc('px_1', 'old') });
  const page = p.file.pages[0];
  const board = page.root.children[0];
  const posted = [];
  const mk = (seq, onBoard) => ({
    seqNumber: seq,
    resolved: false,
    board: onBoard ? board : null,
    position: { x: 30, y: 40 },
    findComments: async () => [{ content: `c${seq}`, user: { name: 'Designer' } }],
    reply: async (text) => posted.push([seq, text]),
  });
  page.threads = [mk(1, true), mk(2, false)];
  const live = await p.send({ type: 'threads' });
  assert.deepEqual([live.data.fileId, live.data.exportId, live.data.threads.map((t) => t.seq)], ['file-1', 'px_1', [1]]);
  assert.equal(live.data.threads[0].boardKey, 'items|a|desktop');
  const r = await p.send({
    type: 'reply',
    replies: [
      { seq: 1, key: 'k1', text: '[dfw] fb_001 fixed: done' },
      { seq: 9, key: 'k9', text: 'gone' },
    ],
  });
  assert.deepEqual(r.done, [{ seq: 1, key: 'k1' }]);
  assert.deepEqual(posted, [[1, '[dfw] fb_001 fixed: done']]);
});
