// Workflow state machine and approval gates (.dfw/state.json).
import fs from 'node:fs';
import { readJson, writeJson, nowIso, projectPaths, appendJsonl, DfwError } from './util.mjs';

export const PHASES = ['discovery', 'direction', 'system', 'concepts', 'stack', 'implement', 'verify', 'audit', 'done'];

export const GATES = {
  G1: 'Design direction approved',
  DS: 'Design system (foundations + components) approved',
  G2: 'Design frozen (approved version)',
  G3: 'Technology stack and major dependencies approved',
  G4: 'Implementation verified; deviations accepted',
};

export const MODES = {
  'new-product': { entry: 'discovery', waive: [], writesCode: true },
  'existing-project': { entry: 'discovery', waive: [], writesCode: true },
  'design-only': { entry: 'discovery', waive: [], writesCode: false },
  'implement-approved': { entry: 'stack', waive: [], writesCode: true, requiresApprovedDesign: true },
  'ui-refinement': { entry: 'concepts', waive: ['G1', 'DS'], writesCode: true },
  'quick-change': { entry: 'concepts', waive: ['G1', 'DS'], writesCode: true },
  'review-only': { entry: 'audit', waive: [], writesCode: false },
};

export const MODE_ALIASES = {
  new: 'new-product',
  existing: 'existing-project',
  design: 'design-only',
  implement: 'implement-approved',
  refine: 'ui-refinement',
  refinement: 'ui-refinement',
  quick: 'quick-change',
  review: 'review-only',
};

export function normalizeMode(mode) {
  return MODE_ALIASES[mode] || mode;
}

export function loadState(root) {
  return readJson(projectPaths(root).state, null);
}

export function saveState(root, state) {
  state.updatedAt = nowIso();
  writeJson(projectPaths(root).state, state);
  return state;
}

export function newState({ mode = 'new-product', product = '' } = {}) {
  mode = normalizeMode(mode);
  if (!MODES[mode]) throw new DfwError(`Unknown mode "${mode}". Modes: ${Object.keys(MODES).join(', ')}`);
  const at = nowIso();
  return {
    schema: 1,
    product,
    mode,
    phase: MODES[mode].entry,
    active: true,
    createdAt: at,
    updatedAt: at,
    design: { version: 1, status: 'draft', locked: false, approved: [] },
    gates: { G1: null, DS: null, G2: null, G3: null, G4: null },
    waived: Object.fromEntries(MODES[mode].waive.map((g) => [g, `waived by mode ${mode}`])),
    history: [{ at, event: 'init', mode }],
  };
}

export function gatePassed(state, gate) {
  return Boolean(state.gates?.[gate]) || Boolean(state.waived?.[gate]);
}

/** Current approved design (null while the latest version is still a draft). */
export function currentApproval(state) {
  if (state.design.status !== 'approved') return null;
  return state.design.approved.at(-1) || null;
}

/** Returns a list of reasons the transition is not allowed (empty = allowed). */
export function transitionBlockers(state, to) {
  const reasons = [];
  if (!PHASES.includes(to)) return [`Unknown phase "${to}". Phases: ${PHASES.join(', ')}`];
  const mode = MODES[state.mode];
  if (['stack', 'implement', 'verify'].includes(to) && !mode.writesCode && state.mode !== 'review-only') {
    reasons.push(`Mode "${state.mode}" does not include implementation; switch mode first (dfw mode <mode>).`);
  }
  switch (to) {
    case 'system':
      if (!gatePassed(state, 'G1')) reasons.push('G1 (design direction) is not approved yet.');
      break;
    case 'concepts':
      if (!gatePassed(state, 'G1')) reasons.push('G1 (design direction) is not approved yet.');
      if (!gatePassed(state, 'DS')) reasons.push('DS (design system) is not approved yet: build foundations and components first (phase "system").');
      break;
    case 'implement':
      if (!currentApproval(state)) reasons.push('G2: there is no approved, frozen design for the current version.');
      if (!gatePassed(state, 'G3')) reasons.push('G3: the technology stack is not approved yet.');
      break;
    case 'verify':
      if (!['implement', 'verify'].includes(state.phase) && state.mode !== 'implement-approved') {
        reasons.push('Verification follows implementation (current phase is ' + state.phase + ').');
      }
      break;
    case 'done':
      if (mode.writesCode && !gatePassed(state, 'G4')) reasons.push('G4 (verification accepted) is not approved yet.');
      if (!mode.writesCode && state.mode === 'design-only' && !currentApproval(state)) {
        reasons.push('Design-only work finishes with an approved design (G2).');
      }
      break;
    default:
  }
  return reasons;
}

export function setPhase(root, state, to, { force = false, reason = '' } = {}) {
  const blockers = transitionBlockers(state, to);
  if (blockers.length && !force) {
    throw new DfwError(`Cannot move to "${to}":\n- ${blockers.join('\n- ')}`);
  }
  const from = state.phase;
  state.phase = to;
  state.history.push({ at: nowIso(), event: 'phase', from, to, forced: blockers.length ? blockers : undefined, reason: reason || undefined });
  return saveState(root, state);
}

export function approveGate(root, state, gate, { note = '', extra = {} } = {}) {
  if (!GATES[gate]) throw new DfwError(`Unknown gate "${gate}". Gates: ${Object.keys(GATES).join(', ')}`);
  const p = projectPaths(root);
  if (gate === 'G1' && ['discovery'].includes(state.phase) && !note) {
    throw new DfwError('G1 is normally approved during the direction phase. Pass --note to record what was approved.');
  }
  if (gate === 'G3' && !fs.existsSync(p.stack)) {
    throw new DfwError('G3 needs .dfw/stack.json (run: dfw stack set ... or dfw stack detect --write).');
  }
  if (gate === 'G4' && state.phase !== 'verify') {
    throw new DfwError('G4 is approved at the end of the verify phase.');
  }
  const record = { approvedAt: nowIso(), note: note || undefined, ...extra };
  state.gates[gate] = record;
  state.history.push({ at: record.approvedAt, event: 'approve', gate, note: note || undefined, ...extra });
  appendJsonl(p.decisions, {
    at: record.approvedAt,
    level: 1,
    topic: `gate:${gate}`,
    decision: `${GATES[gate]}${note ? ` - ${note}` : ''}`,
    by: 'user',
  });
  return saveState(root, state);
}

export function setMode(root, state, mode) {
  mode = normalizeMode(mode);
  if (!MODES[mode]) throw new DfwError(`Unknown mode "${mode}". Modes: ${Object.keys(MODES).join(', ')}`);
  if (MODES[mode].requiresApprovedDesign && !currentApproval(state)) {
    throw new DfwError(`Mode "${mode}" needs an approved design (G2). None exists for the current version.`);
  }
  const from = state.mode;
  state.mode = mode;
  state.waived = { ...Object.fromEntries(MODES[mode].waive.map((g) => [g, `waived by mode ${mode}`])) };
  state.history.push({ at: nowIso(), event: 'mode', from, to: mode });
  return saveState(root, state);
}

export function summarize(state) {
  if (!state) return 'dfw: no workflow in this project (run /dfw to start).';
  const g = (k) => (state.gates[k] ? 'approved' : state.waived?.[k] ? 'waived' : 'open');
  const appr = currentApproval(state);
  return [
    `dfw: product="${state.product || '(unnamed)'}" mode=${state.mode} phase=${state.phase}`,
    `design: v${state.design.version} ${state.design.status}${state.design.locked ? ' (locked)' : ''}${appr ? `, approved ${appr.version}` : ''}`,
    `gates: G1=${g('G1')} G2=${appr ? 'approved' : 'open'} G3=${g('G3')} G4=${g('G4')}`,
  ].join('\n');
}
