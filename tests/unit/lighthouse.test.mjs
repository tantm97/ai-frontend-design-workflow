import { test } from 'node:test';
import assert from 'node:assert/strict';
import { lighthouseReport } from '../../core/lib/lighthouse.mjs';

test('lighthouse: no stdout (spawn failed) or "null" is an error result, not a crash', () => {
  for (const stdout of [null, undefined, '', 'null', '42', 'not json']) {
    const r = lighthouseReport(stdout, 'http://localhost:3000', 'spawnSync npx ENOENT');
    assert.deepEqual([r.status, r.detail, r.findings], ['error', 'spawnSync npx ENOENT', []], String(stdout));
  }
  assert.equal(lighthouseReport(null, 'http://localhost:3000').detail, 'no output');
});

test('lighthouse: a category without a score reads "n/a", not 0; low audits become findings', () => {
  const lhr = {
    categories: {
      performance: { title: 'Performance', score: null, auditRefs: [{ id: 'lcp', weight: 25 }] },
      seo: { title: 'SEO', score: 0.92, auditRefs: [{ id: 'meta', weight: 1 }, { id: 'gone', weight: 1 }] },
    },
    audits: { lcp: { id: 'lcp', title: 'Largest Contentful Paint', score: 0.3, scoreDisplayMode: 'numeric', displayValue: '6.1 s' }, meta: { id: 'meta', title: 'Has a meta description', score: 1, scoreDisplayMode: 'binary' } },
  };
  const r = lighthouseReport(JSON.stringify(lhr), 'http://localhost:3000');
  assert.equal(r.status, 'ran');
  assert.equal(r.detail, 'scores: Performance n/a, SEO 92');
  assert.deepEqual(r.findings.map((f) => [f.level, f.rule, f.message]), [['warn', 'lighthouse-lcp', 'Performance: Largest Contentful Paint (6.1 s)']]);
  assert.equal(lighthouseReport('{"categories":{"seo":{"title":"SEO","score":1,"auditRefs":[{"id":"x","weight":1}]}}}', 'u').status, 'ran', 'no audits map');
});
