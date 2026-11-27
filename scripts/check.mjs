// Every check of AGENTS.md ("The checks") in one command, in CI's order, so none is skipped
// before a push: `npm run check`. Flags:
//   --stress   run the Rust unit tests 5 more times with 4x the threads, to shake out tests that
//              lean on timing (they passed on a laptop and failed on windows-latest)
//   --no-chrome  skip the checks that need Chrome (they are reported as "not run", never passed)
// On macOS and Linux it also runs clippy for Windows (scripts/lib/check-steps.mjs WINDOWS_TARGET),
// which needs that target once: `rustup target add <target>` in native/ (it says so when missing).
// Stops at the first failure and prints which check failed and how to run it alone.
import { spawnSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { REPO, checkSteps } from './lib/check-steps.mjs';

const args = new Set(process.argv.slice(2));
const chrome = !args.has('--no-chrome');
const env = { ...process.env, PATH: `${path.join(os.homedir(), '.cargo', 'bin')}${path.delimiter}${process.env.PATH}` };
const steps = checkSteps({ stress: args.has('--stress') });

const run = (cmd, argv, cwd, extra) =>
  spawnSync(cmd, argv, {
    cwd: cwd || REPO,
    env: { ...env, ...extra },
    stdio: ['ignore', 'pipe', 'pipe'],
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
    shell: process.platform === 'win32',
  });
const fail = (s, text) => {
  process.stdout.write(text.split('\n').slice(-60).join('\n'));
  console.error(`\nFAILED: ${s.name}. Run it alone: ${[s.cmd, ...s.args].join(' ')}${s.cwd ? ` (in ${path.relative(REPO, s.cwd)})` : ''}`);
  process.exit(1);
};

const notRun = [];
for (const s of steps) {
  if (s.chrome && !chrome) {
    notRun.push(s.name);
    continue;
  }
  if (s.target) {
    const installed = run('rustup', ['target', 'list', '--installed'], s.cwd);
    if (!(installed.stdout || '').split('\n').includes(s.target))
      fail(s, `The Rust target ${s.target} is not installed. Add it once: rustup target add ${s.target} (in native/)\n`);
  }
  const started = Date.now();
  process.stdout.write(`… ${s.name}\n`);
  const r = run(s.cmd, s.args, s.cwd, s.env);
  if (r.status !== 0) fail(s, `${r.stdout || ''}${r.stderr || ''}`);
  process.stdout.write(`✓ ${s.name} (${Math.round((Date.now() - started) / 1000)} s)\n`);
}
if (notRun.length) console.log(`Not run (--no-chrome): ${notRun.join(', ')}`);
console.log('All checks passed. Windows runs in CI only: wait for its native job before calling the PR ready.');
