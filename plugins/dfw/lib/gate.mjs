// PreToolUse write gate. Pure decision function + hook adapter.
import path from 'node:path';
import { matchesAny, toPosix } from './util.mjs';
import { currentApproval, gatePassed } from './state.mjs';

const WRITE_TOOLS = new Set(['Edit', 'Write', 'MultiEdit', 'NotebookEdit']);

/**
 * Decide whether a write to `absFile` is allowed.
 * Returns { allow: true } or { allow: false, reason }.
 */
export function decideWrite({ root, state, config, absFile }) {
  if (!state || state.active === false) return { allow: true };
  const rel = toPosix(path.relative(root, path.resolve(root, absFile)));
  if (rel.startsWith('..') || path.isAbsolute(rel)) return { allow: true }; // outside the project
  const gate = config.gate || {};

  if (matchesAny(rel, gate.immutable || [])) {
    return {
      allow: false,
      reason: `dfw gate: "${rel}" is part of a frozen, approved design snapshot and is immutable. To change the approved design, ask the user, then run "dfw reopen --reason <why>" and edit the working copy in design/.`,
    };
  }
  if (matchesAny(rel, gate.allow || [])) return { allow: true };

  if (state.mode === 'review-only') {
    return {
      allow: false,
      reason: `dfw gate: this project is in review-only mode; no files may be modified except under .dfw/. Report findings instead.`,
    };
  }

  if (matchesAny(rel, gate.designPaths || [])) {
    if (state.design?.locked) {
      return {
        allow: false,
        reason: `dfw gate: design v${state.design.version} is approved and locked. Visual changes to an approved design need user approval: ask the user, then run "dfw reopen --reason <why>".`,
      };
    }
    return { allow: true };
  }

  // Everything else is implementation code.
  const missing = [];
  if (!currentApproval(state)) missing.push('G2 (approved, frozen design)');
  if (!gatePassed(state, 'G3')) missing.push('G3 (approved technology stack)');
  if (!['implement', 'verify', 'done'].includes(state.phase)) missing.push(`phase "implement" (current: "${state.phase}")`);
  if (missing.length) {
    return {
      allow: false,
      reason: `dfw gate: implementation files cannot be modified yet ("${rel}"). Missing: ${missing.join(', ')}. Continue the design workflow (/dfw status) or, if the user explicitly wants this edit outside the workflow, ask them to add the path to gate.allow (dfw config set gate.allow ...).`,
    };
  }
  return { allow: true };
}

export function filesFromToolInput(toolName, input = {}) {
  if (!WRITE_TOOLS.has(toolName)) return [];
  const f = input.file_path || input.notebook_path || input.path;
  return f ? [f] : [];
}

export function preToolUseResponse(decision) {
  if (decision.allow) return null;
  return {
    hookSpecificOutput: {
      hookEventName: 'PreToolUse',
      permissionDecision: 'deny',
      permissionDecisionReason: decision.reason,
    },
  };
}
