// Hook entry points (wired in hooks/hooks.json). Every handler is fail-open:
// an internal error never blocks the user's session; it is reported on stderr.
import fs from 'node:fs';
import path from 'node:path';
import { findProjectRoot, toPosix } from './util.mjs';
import { loadState, summarize } from './state.mjs';
import { loadConfig } from './config.mjs';
import { decideWrite, filesFromToolInput, preToolUseResponse } from './gate.mjs';
import { runChecks, formatReport, UI_EXT } from './check.mjs';
import { pruneSessions } from './lang.mjs';
import { allComments } from './comments.mjs';

function rootFrom(input) {
  return findProjectRoot(process.env.CLAUDE_PROJECT_DIR || input.cwd || process.cwd());
}

export function sessionStart(input) {
  if (input.session_id && process.env.CLAUDE_ENV_FILE) {
    try {
      fs.appendFileSync(process.env.CLAUDE_ENV_FILE, `export DFW_SESSION_ID='${String(input.session_id).replace(/'/g, '')}'\n`);
    } catch {}
  }
  pruneSessions();
  const root = rootFrom(input);
  const state = loadState(root);
  if (!state || state.active === false) return null;
  const open = allComments(root).filter((c) => ['open', 'reopened', 'acknowledged'].includes(c.status)).length;
  const fixed = allComments(root).filter((c) => c.status === 'fixed').length;
  const lines = [
    summarize(state),
    `review comments: ${open} open, ${fixed} awaiting user verification`,
    `Session id for "dfw lang": ${input.session_id || 'unknown'}.`,
    'This project uses the dfw design-first workflow. Before working on its UI, invoke the dfw skill (it asks the communication language once per session and enforces approval gates).',
  ];
  return { hookSpecificOutput: { hookEventName: 'SessionStart', additionalContext: lines.join('\n') } };
}

export function preToolUse(input) {
  const files = filesFromToolInput(input.tool_name, input.tool_input);
  if (!files.length) return null;
  const root = rootFrom(input);
  const state = loadState(root);
  if (!state) return null;
  const config = loadConfig(root);
  for (const f of files) {
    const res = preToolUseResponse(decideWrite({ root, state, config, absFile: f }));
    if (res) return res;
  }
  return null;
}

export function postToolUse(input) {
  const files = filesFromToolInput(input.tool_name, input.tool_input);
  if (!files.length) return null;
  const root = rootFrom(input);
  const state = loadState(root);
  if (!state || state.active === false) return null;
  const config = loadConfig(root);
  if (config.checks?.onEdit === false) return null;
  const targets = files
    .map((f) => toPosix(path.relative(root, path.resolve(root, f))))
    .filter((rel) => !rel.startsWith('..') && !rel.startsWith('design/approved/') && (UI_EXT.has(path.extname(rel)) || /^design\/.*\.json$/.test(rel)));
  if (!targets.length) return null;
  const results = runChecks(root, targets, config, { timeoutMs: config.checks?.timeoutMs || 8000 });
  const { text, tally } = formatReport(results, { max: config.checks?.maxFindings || 12 });
  if (!tally.error && !tally.warn) return null;
  return {
    hookSpecificOutput: {
      hookEventName: 'PostToolUse',
      additionalContext: `dfw checks on ${targets.join(', ')}:\n${text}\nFix MUST items before moving on; SHOULD items unless the brief or a recorded decision says otherwise.`,
    },
  };
}

export const HANDLERS = { 'session-start': sessionStart, 'pre-tool-use': preToolUse, 'post-tool-use': postToolUse };
