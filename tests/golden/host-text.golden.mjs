// The host text table has two homes: native/src/core/host.rs (the binary renders the MCP guide
// with it) and scripts/lib/host.mjs (the build renders every host plugin with it). This suite
// renders every file of core/workflow for every host with both and fails when they differ, so the
// two cannot drift. Nothing is recorded: the build's renderer is the reference.
import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { HOSTS, renderText } from '../../scripts/lib/host.mjs';
import { REPO, nativeBinary, skipReason } from './harness.mjs';

const WORKFLOW = path.join(REPO, 'core', 'workflow');

function markdownFiles(dir) {
  return fs
    .readdirSync(dir, { withFileTypes: true })
    .flatMap((e) => (e.isDirectory() ? markdownFiles(path.join(dir, e.name)) : e.name.endsWith('.md') ? [path.join(dir, e.name)] : []))
    .sort();
}

describe('host text: the build and the binary render the same', { skip: skipReason() }, () => {
  const files = markdownFiles(WORKFLOW);

  test('every workflow file, for every host with text', () => {
    assert.ok(files.length > 20, 'the workflow files are found');
    for (const host of Object.keys(HOSTS)) {
      for (const file of files) {
        const text = fs.readFileSync(file, 'utf8');
        const r = spawnSync(nativeBinary(), ['__debug', 'host-render', host], { input: text, encoding: 'utf8', maxBuffer: 16 << 20 });
        assert.equal(r.status, 0, r.stderr);
        // __debug host-render prints the text and a newline.
        assert.equal(r.stdout, `${renderText(text, host)}\n`, `${host}: ${path.relative(REPO, file)}`);
      }
    }
  });

  test('the placeholders and blocks the files do not use yet', () => {
    const text = `${Object.keys(HOSTS.claude.text)
      .map((k) => `${k}: {{${k}}}`)
      .join('\n')}\n<!-- host:claude -->a<!-- /host -->\n<!--host:copilot,mcp-->\nb\n<!--/host-->\n\`impeccable:impeccable\` {{UNKNOWN}}\n`;
    for (const host of Object.keys(HOSTS)) {
      const r = spawnSync(nativeBinary(), ['__debug', 'host-render', host], { input: text, encoding: 'utf8' });
      assert.equal(r.stdout, `${renderText(text, host)}\n`, host);
    }
  });
});
