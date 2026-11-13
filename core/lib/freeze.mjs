// Design freeze (G2) and re-open for a new version.
import fs from 'node:fs';
import path from 'node:path';
import { listFiles, sha256, writeJson, readJson, nowIso, projectPaths, DfwError, copyDir } from './util.mjs';
import { approveGate, saveState } from './state.mjs';

const EXCLUDE = ['approved', 'handoff'];
// Canonical product/design documents live at the project root because
// Impeccable and Hallmark read them there; they are frozen with the design.
export const ROOT_DOCS = ['PRODUCT.md', 'DESIGN.md'];

export function hashDesign(root) {
  const designDir = projectPaths(root).design;
  const entries = listFiles(designDir, { exclude: EXCLUDE }).map((rel) => ({ path: rel, sha256: sha256(fs.readFileSync(path.join(designDir, rel))) }));
  for (const doc of ROOT_DOCS) {
    const abs = path.join(root, doc);
    if (fs.existsSync(abs)) entries.push({ path: `docs/${doc}`, sha256: sha256(fs.readFileSync(abs)) });
  }
  const overall = sha256(entries.map((e) => `${e.sha256}  ${e.path}`).join('\n'));
  return { files: entries, hash: overall };
}

export function freezeDesign(root, state, { note = '' } = {}) {
  const p = projectPaths(root);
  if (!fs.existsSync(p.manifest)) throw new DfwError('Cannot freeze: design/design.json does not exist.');
  if (state.design.status === 'approved') {
    throw new DfwError(`Version v${state.design.version} is already approved. Use "dfw reopen" to start a new version.`);
  }
  const version = `v${state.design.version}`;
  const dest = path.join(p.approved, version);
  if (fs.existsSync(dest)) throw new DfwError(`Snapshot ${path.relative(root, dest)} already exists; refusing to overwrite.`);
  const { files, hash } = hashDesign(root);
  copyDir(p.design, dest, { exclude: EXCLUDE });
  for (const doc of ROOT_DOCS) {
    const abs = path.join(root, doc);
    if (fs.existsSync(abs)) {
      fs.mkdirSync(path.join(dest, 'docs'), { recursive: true });
      fs.copyFileSync(abs, path.join(dest, 'docs', doc));
    }
  }
  const lock = { version, approvedAt: nowIso(), hash, note: note || undefined, files };
  writeJson(path.join(dest, 'approval.lock.json'), lock);
  state.design.status = 'approved';
  state.design.locked = true;
  state.design.approved.push({ version, hash, at: lock.approvedAt, note: note || undefined });
  return approveGate(root, state, 'G2', { note, extra: { version, hash } });
}

/** Verify an approved snapshot is intact (nobody edited it). */
export function verifySnapshot(root, version) {
  const dir = path.join(projectPaths(root).approved, version);
  const lock = readJson(path.join(dir, 'approval.lock.json'), null);
  if (!lock) throw new DfwError(`No approval.lock.json in ${dir}`);
  const problems = [];
  for (const f of lock.files) {
    const abs = path.join(dir, f.path);
    if (!fs.existsSync(abs)) problems.push(`missing: ${f.path}`);
    else if (sha256(fs.readFileSync(abs)) !== f.sha256) problems.push(`modified: ${f.path}`);
  }
  const extra = listFiles(dir).filter((r) => r !== 'approval.lock.json' && !lock.files.some((f) => f.path === r));
  for (const r of extra) problems.push(`added: ${r}`);
  return problems;
}

export function reopenDesign(root, state, { reason } = {}) {
  if (!reason) throw new DfwError('Re-opening an approved design needs --reason (it is recorded as a change proposal).');
  if (state.design.status !== 'approved') throw new DfwError('The current design version is not approved; nothing to re-open.');
  const from = state.design.version;
  state.design.version = from + 1;
  state.design.status = 'draft';
  state.design.locked = false;
  state.gates.G2 = null;
  state.gates.G4 = null;
  if (['implement', 'verify', 'done', 'stack'].includes(state.phase)) state.phase = 'concepts';
  state.history.push({ at: nowIso(), event: 'reopen', from: `v${from}`, to: `v${from + 1}`, reason });
  return saveState(root, state);
}
