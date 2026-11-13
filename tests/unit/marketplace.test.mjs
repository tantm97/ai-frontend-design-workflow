import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setKey, dropKeys, getKey } from '../../scripts/lib/frontmatter.mjs';
import { applyOverlay, OverlayError } from '../../scripts/lib/overlay.mjs';

const SKILL = `---
name: demo
description: >
  Long folded
  description here.
disable-model-invocation: true
metadata:
  a: 1
---

# Body
Run node <skill-dir>/scripts/x.mjs
`;

test('frontmatter: set folded description, drop keys, keep nested', () => {
  const t = setKey(SKILL, 'description', 'New "quoted" text');
  assert.equal(getKey(t, 'description'), 'New "quoted" text');
  assert.ok(t.includes('metadata:\n  a: 1'));
  assert.ok(!t.includes('Long folded'));
  const d = dropKeys(t, ['disable-model-invocation']);
  assert.deepEqual(d.dropped, ['disable-model-invocation']);
  assert.ok(!d.text.includes('disable-model-invocation'));
  assert.ok(d.text.endsWith('Run node <skill-dir>/scripts/x.mjs\n'));
  assert.equal(getKey(SKILL, 'description'), 'Long folded description here.');
});

test('overlay ops apply and fail loudly', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-ov-'));
  fs.mkdirSync(path.join(dir, 'skills/demo'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'skills/demo/SKILL.md'), SKILL);
  const log = applyOverlay(dir, {
    ops: [
      { op: 'description', file: 'skills/demo/SKILL.md', value: 'Narrow', why: 't' },
      { op: 'dropKeys', file: 'skills/demo/SKILL.md', keys: ['disable-model-invocation'], why: 't' },
      { op: 'replace', file: 'skills/demo/SKILL.md', pattern: '<skill-dir>', flags: 'g', with: '${CLAUDE_SKILL_DIR}', min: 1, why: 't' },
    ],
  }, dir);
  assert.equal(log.length, 3);
  const text = fs.readFileSync(path.join(dir, 'skills/demo/SKILL.md'), 'utf8');
  assert.match(text, /description: "Narrow"/);
  assert.match(text, /\$\{CLAUDE_SKILL_DIR\}\/scripts/);
  assert.throws(() => applyOverlay(dir, { ops: [{ op: 'replace', file: 'skills/demo/SKILL.md', pattern: 'not-there', with: 'x', why: 't' }] }, dir), OverlayError);
  assert.throws(() => applyOverlay(dir, { ops: [{ op: 'dropKeys', file: 'skills/demo/SKILL.md', keys: ['gone'], why: 't' }] }, dir), OverlayError);
  assert.throws(() => applyOverlay(dir, { ops: [{ op: 'description', file: 'skills/demo/SKILL.md', value: 'x' }] }, dir), /needs a "why"/);
});

test('every overlay op in the repo documents why', () => {
  const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), '../..');
  for (const f of fs.readdirSync(path.join(root, 'overlays')).filter((x) => x.endsWith('.json'))) {
    const o = JSON.parse(fs.readFileSync(path.join(root, 'overlays', f), 'utf8'));
    for (const op of o.ops) assert.ok(op.why, `${f}: op ${op.op} lacks why`);
  }
});
