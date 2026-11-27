// The decisions of the dfw bridge panel (core/studio/penpot/ui.js, a classic script the panel page
// loads): when it updates the boards by itself, when it reads the session again, and whether
// "Keep boards up to date automatically" starts checked.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const code = fs.readFileSync(path.join(HERE, '../../core/studio/penpot/ui.js'), 'utf8');
const panel = new Function(`${code}\nreturn dfwPanel;`)();

/** Boards built from px_1, the design changed after the export and has been quiet for 12 s. */
const due = {
  auto: true,
  built: true,
  stale: true,
  boardsExportId: 'px_1',
  latestExportId: 'px_1',
  quietMs: 12000,
  busyMs: 0,
  sinceFailMs: null,
};

test('penpot panel: boards behind a quiet design are updated', () => {
  assert.equal(panel.shouldAutoUpdate(due), true);
  // A newer export (dfw penpot export from the CLI) is enough, without a design change.
  assert.equal(panel.shouldAutoUpdate({ ...due, stale: false, latestExportId: 'px_2' }), true);
  assert.equal(panel.shouldAutoUpdate({ ...due, quietMs: panel.QUIET_MS }), true);
});

test('penpot panel: nothing is drawn when the box is off, the file has no boards, or nothing changed', () => {
  assert.equal(panel.shouldAutoUpdate({ ...due, auto: false }), false);
  assert.equal(panel.shouldAutoUpdate({ ...due, built: false, boardsExportId: null }), false, 'never draws into a fresh file');
  assert.equal(panel.shouldAutoUpdate({ ...due, stale: false }), false, 'up to date');
  assert.equal(panel.shouldAutoUpdate({ ...due, stale: false, latestExportId: null }), false, 'no export at all');
});

test('penpot panel: it waits while the design changes, while an update runs, and after a failure', () => {
  assert.equal(panel.shouldAutoUpdate({ ...due, quietMs: 3000 }), false, 'still changing');
  assert.equal(panel.shouldAutoUpdate({ ...due, quietMs: null }), false, 'no design files');
  assert.equal(panel.shouldAutoUpdate({ ...due, busyMs: 2000 }), false, 'one update at a time');
  assert.equal(panel.shouldAutoUpdate({ ...due, busyMs: panel.BUSY_MS + 1 }), true, 'a stuck update is given up');
  assert.equal(panel.shouldAutoUpdate({ ...due, sinceFailMs: 5000 }), false, 'back-off after a failure');
  assert.equal(panel.shouldAutoUpdate({ ...due, sinceFailMs: panel.RETRY_MS }), true);
});

test('penpot panel: a refused token or a lost Studio reads the session again', () => {
  assert.equal(panel.needsSession(403), true, 'a restarted Studio has a new token');
  assert.equal(panel.needsSession(0), true);
  assert.equal(panel.needsSession(undefined), true, 'the request did not reach the Studio');
  for (const ok of [200, 404, 500]) assert.equal(panel.needsSession(ok), false, String(ok));
});

test('penpot panel: the box starts on for a Penpot project, unless the user chose otherwise here', () => {
  assert.equal(panel.autoDefault(null, 'penpot'), true);
  assert.equal(panel.autoDefault(null, 'studio'), false);
  assert.equal(panel.autoDefault(undefined, undefined), false, 'an older Studio sends no canvas');
  assert.equal(panel.autoDefault('0', 'penpot'), false);
  assert.equal(panel.autoDefault('1', 'studio'), true);
});
