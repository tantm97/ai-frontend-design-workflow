// The command table: help, version, unknown commands, status and a first write.
// Run: node --test tests/golden/
import { describe } from 'node:test';
import assert from 'node:assert/strict';
import { run, assertSame, skipReason, NOW, suite } from './harness.mjs';

const test = suite('skeleton');

describe('skeleton', { skip: skipReason() }, () => {
  // The recorded help was the Node CLI's plus the telemetry line, then the 0.6.0 lines of
  // `setup --binary` and `install` (no --no-binary). `--help` wins on every command: the Node
  // CLI ran install, uninstall and mcp before it looked at --help, so `dfw install --help`
  // installed; these cases were recorded from the binary.
  test('help: dfw help, dfw, and --help on any command, install, uninstall and mcp included', () => {
    for (const args of [['help'], [], ['status', '--help'], ['install', '--help'], ['uninstall', '--help'], ['mcp', '--help']]) {
      const r = run(args);
      assertSame(r, { stderr: true });
      assert.equal(r.native.code, 0);
      assert.match(r.native.stdout, /^dfw - design-first workflow CLI\n/);
      assert.match(r.native.stdout, /^ {2}dfw telemetry status\|on\|off\|endpoint <url>\|show\|flush /m);
    }
  });

  test('an unknown command fails the same way', () => {
    const r = run(['no-such-command']);
    assertSame(r, { stderr: true });
    assert.equal(r.native.code, 1);
    assert.equal(r.native.stderr, 'Unknown command "no-such-command". Run "dfw help".\n');
    // The internal server commands exist only as the first argument.
    assertSame(run(['--root', '.', '__studio-server']), { stderr: true });
  });

  test('version differs only in the runtime name', () => {
    const r = run(['version']);
    assertSame(r);
    assert.match(r.native.stdout, /^dfw <version> \(<runtime>\)\n$/);
  });

  test('status is identical, with and without a workflow', () => {
    for (const args of [['status'], ['status', '--json']]) {
      const r = run(args);
      assertSame(r, { stderr: true });
      assert.equal(r.native.code, 0);
      assert.deepEqual(r.native.files, {});
    }
    assert.match(run(['status', '--json']).native.stdout, /"root": "<root>"/);
    assertSame(run(['status'], { state: null }), { stderr: true });
  });

  test('a command that writes: same file, same timestamp, same exit code on error', () => {
    const r = run(['decide', '--level', '2', '--topic', 'skeleton', 'keep', 'the', 'table']);
    assertSame(r, { stderr: true });
    assert.equal(
      r.native.files['.dfw/decisions.jsonl'],
      `{"at":"${NOW}","level":2,"topic":"skeleton","decision":"keep the table","by":"orchestrator"}\n`,
    );
    const bad = run(['decide', '--level', '9', 'x']);
    assertSame(bad, { stderr: true });
    assert.equal(bad.native.code, 1);
  });
});
