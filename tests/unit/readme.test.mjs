// README.md stays in step with the product: every public command of the binary's command table
// is named there as `dfw <command>`, and every repository file or folder it names exists.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const README = 'README.md';
/** The Rust file that names every command once (`pub mod command`). */
const COMMAND_NAMES = 'native/src/core/word.rs';
const COMMAND_MODULE = 'pub mod command {';
/** Internal commands start with this and are never documented. */
const INTERNAL_PREFIX = '__';
/** A path in README is a repository path when it starts with one of these folders. */
const REPO_ROOTS = ['core', 'native', 'adapters', 'overlays', 'registry', 'scripts', 'tests', 'media', 'plugins', '.github'];
/** README names workflow references by their path inside the skill. */
const SKILL_REFERENCES = { prefix: 'references/', dir: 'core/workflow/references/' };
/** Placeholders and globs in a path (`<version>`, `vN`, `**`): not a real file. */
const PLACEHOLDER = /[<>*]/;

const read = (/** @type {string} */ rel) => fs.readFileSync(path.join(REPO, rel), 'utf8');

/** The public command names of the `command` module of word.rs. */
export function commandNames(/** @type {string} */ rust) {
  const start = rust.indexOf(COMMAND_MODULE);
  assert.ok(start >= 0, `${COMMAND_NAMES} has no "${COMMAND_MODULE}"`);
  const body = rust.slice(start, rust.indexOf('\n}', start));
  return [...body.matchAll(/pub const \w+: &str = "([^"]+)";/g)].map((m) => m[1]).filter((n) => !n.startsWith(INTERNAL_PREFIX));
}

/** Commands of `names` that `readme` never writes as `dfw <name>`. */
export function missingCommands(/** @type {string} */ readme, /** @type {string[]} */ names) {
  return names.filter((n) => !new RegExp(`\\bdfw ${n.replace(/[-]/g, '\\-')}(?![\\w-])`).test(readme));
}

/** The repository paths `readme` names in backticks or as a relative link target. */
export function repoPaths(/** @type {string} */ readme) {
  const found = [...readme.matchAll(/`([^`\s]+)`/g), ...readme.matchAll(/\]\(([^)\s#:]+)\)/g)].map((m) => m[1]);
  const out = new Set();
  for (const raw of found) {
    if (PLACEHOLDER.test(raw)) continue;
    const rel = raw.startsWith(SKILL_REFERENCES.prefix) ? SKILL_REFERENCES.dir + raw.slice(SKILL_REFERENCES.prefix.length) : raw;
    if (REPO_ROOTS.some((root) => rel === root || rel === `${root}/` || rel.startsWith(`${root}/`)) || /^[A-Z_]+\.md$/.test(rel))
      out.add(rel);
  }
  return [...out];
}

test('README names every public command of the command table', () => {
  const names = commandNames(read(COMMAND_NAMES));
  assert.ok(names.length > 40, `only ${names.length} commands read from ${COMMAND_NAMES}`);
  assert.deepEqual(missingCommands(read(README), names), []);
});

test('every repository path README names exists', () => {
  const paths = repoPaths(read(README));
  assert.ok(paths.length > 20, `only ${paths.length} paths found`);
  assert.deepEqual(
    paths.filter((p) => !fs.existsSync(path.join(REPO, p))),
    [],
  );
});

test('the README checks catch a missing command and a missing file', () => {
  assert.deepEqual(missingCommands('run `dfw status` and dfw verify-snapshotx', ['status', 'verify-snapshot']), ['verify-snapshot']);
  assert.deepEqual(missingCommands('`dfw type scale`', ['type']), []);
  assert.deepEqual(repoPaths('see `core/nope.md`, `design/x`, `core/<v>/a`, [g](media/demo.gif), `references/a.md`'), [
    'core/nope.md',
    'core/workflow/references/a.md',
    'media/demo.gif',
  ]);
});
