import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { addComment, updateComment } from '../../core/lib/comments.mjs';
import { undelivered, waitForComments } from '../../core/lib/inbox.mjs';
import { newState, saveState } from '../../core/lib/state.mjs';

const DFW = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../core/bin/dfw');
function project() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-inbox-'));
  fs.mkdirSync(path.join(root, '.dfw'), { recursive: true });
  saveState(root, newState({ mode: 'new-product' }));
  return root;
}
const hook = (root, name, input = {}) => execFileSync(process.execPath, [DFW, 'hook', name], { input: JSON.stringify({ cwd: root, ...input }), env: { ...process.env, CLAUDE_PROJECT_DIR: root, DFW_HOME: path.join(root, '.home') }, encoding: 'utf8' });

test('inbox: wait wakes on a new comment and delivers each state once', async () => {
  const root = project();
  const p = waitForComments(root, { timeoutMs: 5000, pollMs: 50, settleMs: 50 });
  setTimeout(() => addComment(root, { comment: 'Make the confirm button bigger', screen: 'borrow', target: { dfwId: 'slip.confirm' } }), 100);
  const got = await p;
  assert.equal(got.length, 1);
  assert.equal(got[0].target.dfwId, 'slip.confirm');
  assert.equal(undelivered(root).length, 0);
  updateComment(root, got[0].id, { status: 'acknowledged' });
  updateComment(root, got[0].id, { status: 'fixed', message: 'lg size' });
  updateComment(root, got[0].id, { status: 'reopened', message: 'Still too small', by: 'user' });
  assert.equal(undelivered(root).length, 1, 'a reopen is delivered again');
  assert.deepEqual(await waitForComments(root, { timeoutMs: 1, pollMs: 1, settleMs: 1 }).then((x) => x.length), 1);
});

test('hooks: user-prompt-submit and stop inject undelivered comments once', () => {
  const root = project();
  assert.equal(hook(root, 'user-prompt-submit', { prompt: 'hi' }).trim(), '');
  addComment(root, { comment: 'Too much padding', screen: 'history', target: { dfwId: 'history.table' } });
  const ups = JSON.parse(hook(root, 'user-prompt-submit', { prompt: 'hi' }));
  assert.match(ups.hookSpecificOutput.additionalContext, /Too much padding/);
  assert.equal(hook(root, 'stop', {}).trim(), '');
  addComment(root, { comment: 'Use the danger variant', screen: 'assets' });
  const st = JSON.parse(hook(root, 'stop', {}));
  assert.equal(st.decision, 'block');
  assert.match(st.reason, /danger variant/);
  addComment(root, { comment: 'x' });
  assert.equal(hook(root, 'stop', { stop_hook_active: true }).trim(), '', 'never loops');
});
