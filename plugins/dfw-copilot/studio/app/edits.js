// @ts-check
// Edits from the Studio to the design source: text, spacing, component props, and the constrained
// layout edits (reorder, resize) the overlay reports. Every edit is journaled by the server for undo.
import { layoutScope } from './model.js';
import { S, api, post, toast } from './state.js';
import { reloadFrame, selectFrame, setPage } from './frames.js';

/** Layout edit scope of a frame (see model.js layoutScope). */
export const layoutScopeOf = (/** @type {any} */ f) => layoutScope(S.manifest?.breakpoints, f);

/**
 * Send one edit for a frame's file to the server.
 * @param {any} f
 * @param {Record<string, any>} op
 * @param {string} [label] what the toast says was done
 */
export async function edit(f, op, label = '') {
  // A resize is checked in the frame once it reloads with the new source (a stronger screen rule can still win).
  const verify = op.op === 'layout' && op.prop && op.value != null ? { id: op.id, prop: op.prop } : null;
  f.pendingVerify = verify;
  S.selfEditUntil = Date.now() + 2500; // the change that follows is the user's own, not the agent's
  try {
    const r = await api('/__dfw/api/edit', { method: 'POST', body: JSON.stringify({ file: f.file, ...op }) });
    if (!r.changed && f.pendingVerify === verify) f.pendingVerify = null;
    toast(r.changed ? `${label ? `${label}. ` : ''}Saved to ${r.file}:${r.line}  (⌘Z to undo)` : 'No change.');
  } catch (e) {
    f.pendingVerify = null;
    toast(e.message);
    reloadFrame(f);
  }
}

/** Undo the last Studio edit (Cmd/Ctrl+Z). */
export async function undo() {
  try {
    S.selfEditUntil = Date.now() + 2500;
    const r = await api('/__dfw/api/undo', { method: 'POST', body: '{}' });
    toast(`Undid ${r.op.op} edit in ${r.file}.`);
  } catch (e) {
    toast(e.message);
  }
}

/**
 * A reorder or resize from the overlay: resizes are breakpoint-aware; reordering changes the
 * source order, so it applies at every width.
 * @param {any} f
 * @param {Record<string, any>} d the dfw:layout-op message
 */
export function layoutOp(f, d) {
  if (f.present || f.system || !f.file) return;
  const { type, label, ...op } = d; // `type` is the message's, not part of the edit
  const scope = layoutScopeOf(f);
  if (op.op === 'layout') op.maxWidth = scope.maxWidth;
  if (op.prop == null) delete op.prop; // reset: the whole rule at this scope
  edit(f, op, label + (op.op === 'layout' && scope.scope ? ` (${scope.scope})` : ''));
}

/** Open the Design System page on a component's master. */
export function goToMaster(/** @type {string} */ name) {
  S.zoomOnSelect = true;
  setPage('system');
  selectFrame('system');
  const f = S.frames.get('system');
  f.pendingSelect = { dfwId: `system.component.${name}`, required: true };
  if (f.loaded) {
    post(f, { type: 'dfw:select-id', ...f.pendingSelect });
    f.pendingSelect = null;
  }
}
