// Visual feedback store: the tool-independent feedback contract (schemas/feedback.schema.json).
// One JSON file per review session under .dfw/reviews/. Any provider (dfw studio
// overlay, a future Agentation or Penpot adapter) writes the same shape.
import fs from 'node:fs';
import path from 'node:path';
import { readJson, writeJson, nowIso, projectPaths, DfwError } from './util.mjs';

export const STATUSES = ['open', 'acknowledged', 'fixed', 'resolved', 'reopened', 'dismissed'];
// Allowed transitions. "fixed" means Claude applied a change; only the user resolves.
const NEXT = {
  open: ['acknowledged', 'fixed', 'dismissed'],
  acknowledged: ['fixed', 'dismissed'],
  fixed: ['resolved', 'reopened'],
  reopened: ['acknowledged', 'fixed', 'dismissed'],
  resolved: ['reopened'],
  dismissed: ['reopened'],
};

function sessionFiles(root) {
  const dir = projectPaths(root).reviews;
  if (!fs.existsSync(dir)) return [];
  return fs
    .readdirSync(dir)
    .filter((f) => /^rv_.*\.json$/.test(f))
    .sort()
    .map((f) => path.join(dir, f));
}

export function currentSession(root, { create = true } = {}) {
  const files = sessionFiles(root);
  const last = files.at(-1);
  if (last) {
    const s = readJson(last);
    if (!s.closedAt) return { file: last, session: s };
  }
  if (!create) return null;
  return newSession(root);
}

export function newSession(root, { designVersion } = {}) {
  const now = new Date();
  const stamp = now.toISOString().replace(/[-:]/g, '').replace(/\..+/, '');
  const id = `rv_${stamp}`;
  const state = readJson(projectPaths(root).state, {});
  for (const f of sessionFiles(root)) {
    const s = readJson(f);
    if (!s.closedAt) {
      s.closedAt = nowIso();
      writeJson(f, s);
    }
  }
  const session = { schema: 1, id, startedAt: nowIso(), designVersion: designVersion || `v${state.design?.version ?? 1}`, comments: [] };
  const file = path.join(projectPaths(root).reviews, `${id}.json`);
  writeJson(file, session);
  return { file, session };
}

export function allComments(root) {
  const out = [];
  for (const f of sessionFiles(root)) {
    const s = readJson(f);
    for (const c of s.comments) out.push({ ...c, session: s.id });
  }
  return out;
}

function nextId(root) {
  let max = 0;
  for (const c of allComments(root)) {
    const n = Number(String(c.id).replace(/^fb_/, ''));
    if (n > max) max = n;
  }
  return `fb_${String(max + 1).padStart(3, '0')}`;
}

/** Line number of data-dfw-id="<id>" in a design file, for source mapping. */
export function locateSource(root, designFile, dfwId) {
  if (!designFile) return null;
  const abs = path.join(projectPaths(root).design, designFile);
  if (!fs.existsSync(abs)) return null;
  const text = fs.readFileSync(abs, 'utf8');
  if (dfwId) {
    const i = text.indexOf(`data-dfw-id="${dfwId}"`);
    if (i >= 0) return `design/${designFile}:${text.slice(0, i).split('\n').length}`;
  }
  return `design/${designFile}`;
}

export function addComment(root, input) {
  if (!input?.comment?.trim()) throw new DfwError('A comment needs text.');
  const { file, session } = currentSession(root);
  const target = { ...(input.target || {}) };
  if (!target.source) target.source = locateSource(root, input.file, target.dfwId);
  const c = {
    id: nextId(root),
    createdAt: nowIso(),
    designVersion: session.designVersion,
    screen: input.screen || null,
    variant: input.variant || null,
    breakpoint: input.breakpoint || null,
    state: input.state || null,
    theme: input.theme || null,
    file: input.file ? `design/${input.file}` : null,
    target,
    comment: input.comment.trim(),
    screenshot: input.screenshot || null,
    status: 'open',
    thread: [],
    resolution: null,
    provider: input.provider || 'dfw-studio',
  };
  session.comments.push(c);
  writeJson(file, session);
  return c;
}

function findComment(root, id) {
  for (const f of sessionFiles(root)) {
    const s = readJson(f);
    const c = s.comments.find((x) => x.id === id);
    if (c) return { file: f, session: s, comment: c };
  }
  throw new DfwError(`Comment ${id} not found.`);
}

export function updateComment(root, id, { status, message, by = 'claude', evidence } = {}) {
  const { file, session, comment } = findComment(root, id);
  const at = nowIso();
  if (status && status !== comment.status) {
    if (!STATUSES.includes(status)) throw new DfwError(`Unknown status "${status}".`);
    if (!NEXT[comment.status].includes(status)) {
      throw new DfwError(`Comment ${id}: cannot go from "${comment.status}" to "${status}" (allowed: ${NEXT[comment.status].join(', ')}).`);
    }
    if (status === 'resolved' && by !== 'user') throw new DfwError('Only the user resolves a comment (from the studio, or "dfw comments resolve --by user" when they say so).');
    if (status === 'fixed' && !message) throw new DfwError('Marking a comment fixed needs a short note describing the change.');
    comment.status = status;
    if (status === 'fixed') comment.resolution = { at, by, note: message, evidence: evidence || null };
  }
  if (message) comment.thread.push({ at, by, message, status: status || undefined });
  comment.updatedAt = at;
  writeJson(file, session);
  return comment;
}
