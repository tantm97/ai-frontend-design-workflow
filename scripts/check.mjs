// Every check of AGENTS.md ("The checks") in one command, in CI's order, so none is skipped
// before a push: `npm run check`. Flags:
//   --stress   run the Rust unit tests 5 more times with 4x the threads, to shake out tests that
//              lean on timing (they passed on a laptop and failed on windows-latest)
//   --no-chrome  skip the checks that need Chrome (they are reported as "not run", never passed)
// Stops at the first failure and prints which check failed and how to run it alone.
import { spawnSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const NATIVE = path.join(REPO, 'native');
const args = new Set(process.argv.slice(2));
const chrome = !args.has('--no-chrome');
const cargo = process.platform === 'win32' ? 'cargo.exe' : 'cargo';
const env = { ...process.env, PATH: `${path.join(os.homedir(), '.cargo', 'bin')}${path.delimiter}${process.env.PATH}` };
const bin = path.join('native', 'target', 'debug', process.platform === 'win32' ? 'dfw.exe' : 'dfw');

/** @type {{ name: string, cmd: string, args: string[], cwd?: string, env?: Record<string, string>, chrome?: boolean }[]} */
const steps = [
  { name: 'npm run lint', cmd: 'npm', args: ['run', 'lint'] },
  { name: 'npm test', cmd: 'npm', args: ['test'] },
  { name: 'npm run build', cmd: 'npm', args: ['run', 'build'] },
  { name: 'build --check', cmd: 'node', args: ['scripts/build.mjs', '--check'] },
  { name: 'validate', cmd: 'node', args: ['scripts/validate.mjs'] },
  { name: 'cargo fmt --check', cmd: cargo, args: ['fmt', '--check'], cwd: NATIVE },
  { name: 'cargo build', cmd: cargo, args: ['build'], cwd: NATIVE },
  { name: 'cargo clippy', cmd: cargo, args: ['clippy', '--all-targets', '--', '-D', 'warnings'], cwd: NATIVE },
  { name: 'cargo test', cmd: cargo, args: ['test'], cwd: NATIVE },
  { name: 'golden', cmd: 'node', args: ['--test', 'tests/golden/'], env: { DFW_REQUIRE_BIN: '1' } },
  {
    name: 'cargo test -- --ignored (Chrome)',
    cmd: cargo,
    args: ['test', '--', '--ignored', '--test-threads=1'],
    cwd: NATIVE,
    chrome: true,
  },
  {
    name: 'golden with Chrome',
    cmd: 'node',
    args: ['--test', '--test-concurrency=1', 'tests/golden/'],
    env: { DFW_REQUIRE_BIN: '1', DFW_TEST_CHROME: '1' },
    chrome: true,
  },
  {
    name: 'Studio e2e',
    cmd: 'node',
    args: ['tests/e2e/studio.e2e.mjs'],
    env: { DFW_BIN: bin, DFW_RUNTIME_DIR: path.join(os.homedir(), '.config', 'dfw', 'runtime') },
    chrome: true,
  },
];
if (args.has('--stress')) {
  const threads = String(Math.max(8, os.availableParallelism() * 4));
  for (let i = 1; i <= 5; i++)
    steps.push({
      name: `cargo test stress ${i}/5`,
      cmd: cargo,
      args: ['test', '--bin', 'dfw', '--', `--test-threads=${threads}`],
      cwd: NATIVE,
    });
}

const notRun = [];
for (const s of steps) {
  if (s.chrome && !chrome) {
    notRun.push(s.name);
    continue;
  }
  const started = Date.now();
  process.stdout.write(`… ${s.name}\n`);
  const r = spawnSync(s.cmd, s.args, {
    cwd: s.cwd || REPO,
    env: { ...env, ...s.env },
    stdio: ['ignore', 'pipe', 'pipe'],
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
    shell: process.platform === 'win32',
  });
  if (r.status !== 0) {
    process.stdout.write(`${r.stdout || ''}${r.stderr || ''}`.split('\n').slice(-60).join('\n'));
    console.error(
      `\nFAILED: ${s.name}. Run it alone: ${[s.cmd, ...s.args].join(' ')}${s.cwd ? ` (in ${path.relative(REPO, s.cwd)})` : ''}`,
    );
    process.exit(1);
  }
  process.stdout.write(`✓ ${s.name} (${Math.round((Date.now() - started) / 1000)} s)\n`);
}
if (notRun.length) console.log(`Not run (--no-chrome): ${notRun.join(', ')}`);
console.log('All checks passed. Windows runs in CI only: wait for its native job before calling the PR ready.');
