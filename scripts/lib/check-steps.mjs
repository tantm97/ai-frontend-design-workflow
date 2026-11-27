// The steps of `npm run check` (scripts/check.mjs), in CI's order. A module of its own so the unit
// tests can see which steps a host gets.
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
export const NATIVE = path.join(REPO, 'native');
const WIN32 = 'win32';

/**
 * The Windows target the macOS and Linux hosts lint for. CI's windows-latest clippy refused code the
 * other hosts accept: helpers and imports used only by `#[cfg(unix)]` tests are dead on Windows
 * (#21, #39, #59, #63). The GNU target needs no Windows SDK; clippy never links.
 */
export const WINDOWS_TARGET = 'x86_64-pc-windows-gnu';
/** Builds the C code of the dependencies for the Windows clippy: nothing (clippy never links). */
export const NO_OP_CC = path.join(REPO, 'scripts', 'lib', 'no-op-cc.mjs');
/** The cc crate's per-target compiler and archiver variables (`-` in the target becomes `_`). */
const ccVar = (tool, target) => `${tool}_${target.replaceAll('-', '_')}`;
const STRESS_RUNS = 5;

/**
 * @typedef {{ name: string, cmd: string, args: string[], cwd?: string, env?: Record<string, string>,
 *   chrome?: boolean, target?: string }} Step
 * @param {{ platform?: string, stress?: boolean, threads?: number }} [opts]
 * @returns {Step[]}
 */
export function checkSteps({ platform = process.platform, stress = false, threads = os.availableParallelism() } = {}) {
  const cargo = platform === WIN32 ? 'cargo.exe' : 'cargo';
  const bin = path.join('native', 'target', 'debug', platform === WIN32 ? 'dfw.exe' : 'dfw');
  const clippy = (target) => ['clippy', '--all-targets', ...(target ? ['--target', target] : []), '--', '-D', 'warnings'];
  /** @type {Step[]} */
  const steps = [
    { name: 'npm run lint', cmd: 'npm', args: ['run', 'lint'] },
    { name: 'npm test', cmd: 'npm', args: ['test'] },
    { name: 'npm run build', cmd: 'npm', args: ['run', 'build'] },
    { name: 'build --check', cmd: 'node', args: ['scripts/build.mjs', '--check'] },
    { name: 'validate', cmd: 'node', args: ['scripts/validate.mjs'] },
    { name: 'cargo fmt --check', cmd: cargo, args: ['fmt', '--check'], cwd: NATIVE },
    { name: 'cargo build', cmd: cargo, args: ['build'], cwd: NATIVE },
    { name: 'cargo clippy', cmd: cargo, args: clippy(), cwd: NATIVE },
  ];
  if (platform !== WIN32)
    steps.push({
      name: `cargo clippy (${WINDOWS_TARGET})`,
      cmd: cargo,
      args: clippy(WINDOWS_TARGET),
      cwd: NATIVE,
      env: { [ccVar('CC', WINDOWS_TARGET)]: NO_OP_CC, [ccVar('AR', WINDOWS_TARGET)]: NO_OP_CC },
      target: WINDOWS_TARGET,
    });
  steps.push(
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
  );
  if (stress) {
    const many = String(Math.max(8, threads * 4));
    for (let i = 1; i <= STRESS_RUNS; i++)
      steps.push({
        name: `cargo test stress ${i}/${STRESS_RUNS}`,
        cmd: cargo,
        args: ['test', '--bin', 'dfw', '--', `--test-threads=${many}`],
        cwd: NATIVE,
      });
  }
  return steps;
}
