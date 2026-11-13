// Delivers Studio review comments to Claude without the user having to relay them:
// - `dfw comments wait` (run in the background by the agent) exits as soon as a new comment arrives;
// - the UserPromptSubmit and Stop hooks inject undelivered comments into the conversation.
// Each comment state is delivered once (.dfw/reviews/.delivered.json).
import fs from 'node:fs';
import path from 'node:path';
import { projectPaths, readJson, writeJson } from './util.mjs';
import { allComments } from './comments.mjs';

const ACTIVE = new Set(['open', 'reopened']);
const key = (c) => `${c.id}:${c.status}:${c.updatedAt || c.createdAt}`;
const deliveredFile = (root) => path.join(projectPaths(root).dfw, 'reviews', '.delivered.json');

export function undelivered(root) {
  const seen = new Set(readJson(deliveredFile(root), []));
  return allComments(root).filter((c) => ACTIVE.has(c.status) && !seen.has(key(c)));
}

export function markDelivered(root, comments) {
  if (!comments.length) return;
  const f = deliveredFile(root);
  const seen = new Set(readJson(f, []));
  for (const c of comments) seen.add(key(c));
  fs.mkdirSync(path.dirname(f), { recursive: true });
  writeJson(f, [...seen].slice(-2000));
}

export function formatForClaude(comments) {
  const lines = comments.map((c) => {
    const where = [c.screen, c.variant, c.breakpoint, c.state, c.theme].filter(Boolean).join(' / ') || 'design system';
    const target = c.target?.dfwId || c.target?.selector || '?';
    const last = c.thread?.length ? c.thread.at(-1) : null;
    const reopened = c.status === 'reopened' && last ? `\n    reopened by ${last.by}: "${last.message}"` : '';
    return `- ${c.id} [${c.status}] ${where} -> ${target}${c.target?.source ? ` (${c.target.source})` : ''}${c.target?.component ? ` component ${c.target.component}` : ''}\n    "${c.comment}"${reopened}`;
  });
  return `${comments.length} new Studio comment(s) from the user:\n${lines.join('\n')}\nApply them now (references/phase-review.md > "Applying comments"): "dfw comments ack <id>", fix (prefer design-system fixes when the comment is about a component or token), check, then "dfw comments fixed <id> --message \\"<what changed>\\"". If a comment conflicts with an approved decision or is ambiguous, reply with "dfw comments reply <id> --message ..." and ask the user.`;
}

/** Block until undelivered comments exist (or timeout). Returns the comments (marked delivered). */
export async function waitForComments(root, { timeoutMs = 3600_000, pollMs = 1000, settleMs = 1500 } = {}) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    let list = undelivered(root);
    if (list.length) {
      // Let the user finish a burst of comments before waking Claude.
      await new Promise((r) => setTimeout(r, settleMs));
      list = undelivered(root);
      markDelivered(root, list);
      return list;
    }
    await new Promise((r) => setTimeout(r, pollMs));
  }
  return [];
}
