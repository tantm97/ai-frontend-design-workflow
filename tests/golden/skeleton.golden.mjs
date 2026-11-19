// The command table: help, version, unknown commands, status and a first write.
// Run: node --test tests/golden/
import { describe } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { run, assertSame, skipReason, NOW, suite, expected } from './harness.mjs';

const test = suite('skeleton');

describe('skeleton', { skip: skipReason() }, () => {
  // The recorded help was the Node CLI's plus the telemetry line, then the 0.6.0 lines of
  // `setup --binary` and `install` (no --no-binary). The Node CLI ran install, uninstall and mcp
  // before it looked at --help, so `dfw install --help` installed; these cases were recorded
  // from the binary.
  test('help: dfw help, dfw and -h print the whole help', () => {
    for (const args of [['help'], [], ['-h'], ['nope', '--help']]) {
      const r = run(args);
      assertSame(r, { stderr: true });
      assert.equal(r.native.code, 0);
      assert.match(r.native.stdout, /^dfw - design-first workflow CLI\n/);
      assert.match(r.native.stdout, /^  dfw telemetry status\|on\|off\|endpoint <url>\|show\|flush /m);
    }
  });

  // TR-3 (0.6.1): --help, -h and a lone help word after a command print that command's help and
  // never run it. A fake `claude` on PATH records any call and HOME and DFW_HOME are empty
  // folders: an install that ran would leave a trace in one of them.
  test('help of one command: --help, -h and a lone help word never run it', () => {
    const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-help-')));
    try {
      const bin = path.join(tmp, 'bin');
      const calls = path.join(tmp, 'claude-calls.txt');
      fs.mkdirSync(bin);
      fs.mkdirSync(path.join(tmp, 'home'));
      fs.mkdirSync(path.join(tmp, 'dfw-home'));
      fs.writeFileSync(path.join(bin, 'claude'), `#!/bin/sh\necho "$@" >> '${calls}'\n`, { mode: 0o755 });
      fs.writeFileSync(path.join(bin, 'copilot'), `#!/bin/sh\necho "copilot $@" >> '${calls}'\n`, { mode: 0o755 });
      const env = { PATH: `${bin}${path.delimiter}/usr/bin${path.delimiter}/bin`, HOME: path.join(tmp, 'home'), DFW_HOME: path.join(tmp, 'dfw-home'), XDG_CONFIG_HOME: path.join(tmp, 'home', '.config') };
      const forms = [
        [['install', '-h'], 'install'],
        [['install', '--help'], 'install'],
        [['install', 'help'], 'install'],
        [['install', '--host', 'claude', '-h'], 'install'],
        [['uninstall', '-h'], 'uninstall'],
        [['uninstall', 'help'], 'uninstall'],
        [['mcp', 'install', 'codex', '-h'], 'mcp'],
        [['mcp', 'help'], 'mcp'],
        [['setup', '--browser', '-h'], 'setup'],
        [['telemetry', 'on', '--help'], 'telemetry'],
        [['config', 'set', 'a', '1', '--global', '-h'], 'config'],
        [['status', '-h'], 'status'],
        [['approve', 'help'], 'approve'],
        [['hook', 'stop', '--help'], 'hook'],
      ];
      for (const [args, cmd] of forms) {
        const r = run(args, { env });
        assertSame(r, { stderr: true });
        assert.equal(r.native.code, 0, args.join(' '));
        assert.equal(r.native.stderr, '', args.join(' '));
        assert.ok(r.native.stdout.startsWith(`  dfw ${cmd}`), `${args.join(' ')}: ${r.native.stdout.slice(0, 80)}`);
        assert.ok(r.native.stdout.endsWith('\n\nRun "dfw help" for every command.\n'), args.join(' '));
        assert.deepEqual(r.native.files, {}, args.join(' '));
      }
      assert.ok(!fs.existsSync(calls), `claude or copilot was called: ${fs.existsSync(calls) && fs.readFileSync(calls, 'utf8')}`);
      assert.deepEqual(fs.readdirSync(path.join(tmp, 'home')), []);
      assert.deepEqual(fs.readdirSync(path.join(tmp, 'dfw-home')), []);
      // A flag those commands do not know, or a stray word, is a usage error: nothing runs.
      for (const args of [['install', '--hots', 'claude'], ['install', 'now'], ['uninstall', '--force'], ['mcp', 'install', 'codex', 'cursor'], ['setup', 'browser'], ['telemetry', 'off', 'now'], ['config', 'set', 'a', '1', 'b', '--global']]) {
        const r = run(args, { env });
        assertSame(r, { stderr: true });
        assert.equal(r.native.code, 1, args.join(' '));
        assert.match(r.native.stderr, /^(Unknown option --\S+|Unexpected word "\S+") for "dfw \S+"\.\n  dfw /, args.join(' '));
      }
      assert.ok(!fs.existsSync(calls));
      assert.deepEqual(fs.readdirSync(path.join(tmp, 'dfw-home')), []);
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
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
    assert.equal(r.native.files['.dfw/decisions.jsonl'], `{"at":"${NOW}","level":2,"topic":"skeleton","decision":"keep the table","by":"orchestrator"}\n`);
    const bad = run(['decide', '--level', '9', 'x']);
    assertSame(bad, { stderr: true });
    assert.equal(bad.native.code, 1);
  });
});
