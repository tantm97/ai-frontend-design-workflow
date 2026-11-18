// Skeleton of the native port: the command table answers help, version and unknown commands
// itself and forwards everything else to the Node CLI.
// Run: DFW_BIN=native/target/debug/dfw node --test tests/conformance/
import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { both, assertSame, skipReason, NOW } from './harness.mjs';

describe('skeleton', { skip: skipReason() }, () => {
  test('help is identical (dfw help, dfw, --help on a command)', () => {
    for (const args of [['help'], [], ['status', '--help']]) {
      const r = both(args);
      assertSame(r, { stderr: true });
      assert.equal(r.native.code, 0);
      assert.match(r.native.stdout, /^dfw - design-first workflow CLI\n/);
    }
  });

  test('an unknown command fails the same way', () => {
    const r = both(['no-such-command']);
    assertSame(r, { stderr: true });
    assert.equal(r.native.code, 1);
    assert.equal(r.native.stderr, 'Unknown command "no-such-command". Run "dfw help".\n');
    // The internal server commands exist only as the first argument.
    assertSame(both(['--root', '.', '__studio-server']), { stderr: true });
  });

  test('version differs only in the runtime name', () => {
    const r = both(['version']);
    assertSame(r);
    assert.match(r.native.stdout, /^dfw \d+\.\d+\.\d+ \(<runtime>\)\n$/);
  });

  test('a forwarded command is identical (status, with and without a workflow)', () => {
    for (const args of [['status'], ['status', '--json']]) {
      const r = both(args);
      assertSame(r, { stderr: true });
      assert.equal(r.native.code, 0);
      assert.deepEqual(r.native.files, {});
    }
    assert.match(both(['status', '--json']).native.stdout, /"root": "<root>"/);
    assertSame(both(['status'], { state: null }), { stderr: true });
  });

  test('a forwarded command that writes: same file, same timestamp, same exit code on error', () => {
    const r = both(['decide', '--level', '2', '--topic', 'skeleton', 'keep', 'the', 'table']);
    assertSame(r, { stderr: true });
    assert.equal(r.native.files['.dfw/decisions.jsonl'], `{"at":"${NOW}","level":2,"topic":"skeleton","decision":"keep the table","by":"orchestrator"}\n`);
    const bad = both(['decide', '--level', '9', 'x']);
    assertSame(bad, { stderr: true });
    assert.equal(bad.native.code, 1);
  });

  test('DFW_NATIVE=off forwards even the native commands', () => {
    const r = both(['version'], { env: { DFW_NATIVE: 'off' } });
    assert.match(r.native.stdout, /^dfw \S+ \(<runtime>\)\n$/);
    assertSame(r);
  });
});
