// Layered configuration: plugin defaults < global < project < project-local.
// Explicit user decisions live in .dfw/decisions.jsonl and are applied by the
// orchestrator skill on top of this (they are not config values).
import path from 'node:path';
import { readJson, writeJson, deepMerge, globalDir, projectPaths, getPath, setPath } from './util.mjs';

export const DEFAULTS = {
  schema: 1,
  language: {
    // "session": ask on the first /dfw run in every session (user decision 2026-11-13).
    askPolicy: 'session',
    default: 'en',
  },
  gate: {
    // Paths (globs, relative to project root) always writable regardless of gates.
    allow: ['.dfw/**', 'design/handoff/**'],
    // Paths that belong to the design workspace (writable before G2, locked after).
    designPaths: ['design/**', 'PRODUCT.md', 'DESIGN.md'],
    // Frozen snapshots are never writable.
    immutable: ['design/approved/**'],
  },
  checks: {
    onEdit: 'fast', // PostToolUse checks: 'fast' (dfw lint), 'full' (all tools) or false
    timeoutMs: 8000,
    impeccable: true,
    sloplint: true,
    tasteCopyLint: false,
    maxFindings: 12,
  },
  studio: {
    port: 4600,
    breakpoints: [
      { id: 'desktop', label: 'Desktop', width: 1440, height: 900 },
      { id: 'tablet', label: 'Tablet', width: 834, height: 1112 },
      { id: 'mobile', label: 'Mobile', width: 390, height: 844 },
    ],
  },
  accessibility: { standard: 'WCAG 2.2', level: 'AA' },
  skills: {
    // Optional skills that must be enabled per project.
    optional: { hallmark: true, 'taste-reference': true },
    disabled: [],
  },
  env: {
    // Applied to every external tool dfw launches.
    DO_NOT_TRACK: '1',
    IMPECCABLE_NO_UPDATE_CHECK: '1',
    IMPECCABLE_HOOK_DISABLED: '1',
  },
};

export function globalConfigPath() {
  return path.join(globalDir(), 'config.json');
}

export function loadConfig(root) {
  const p = root ? projectPaths(root) : null;
  const layers = [
    ['defaults', DEFAULTS],
    ['global', readJson(globalConfigPath(), {})],
  ];
  if (p) {
    layers.push(['project', readJson(p.config, {})]);
    layers.push(['local', readJson(p.configLocal, {})]);
  }
  let merged = {};
  for (const [, layer] of layers) merged = deepMerge(merged, layer);
  // Some lists are unions across layers rather than replacements, so a project
  // can add to the defaults without accidentally dropping them.
  const union = (get) => [...new Set(layers.flatMap(([, layer]) => get(layer) || []))];
  merged.skills = { ...merged.skills, disabled: union((l) => l?.skills?.disabled) };
  merged.gate = {
    ...merged.gate,
    allow: union((l) => l?.gate?.allow),
    immutable: union((l) => l?.gate?.immutable),
  };
  return merged;
}

export function configFileFor(scope, root) {
  if (scope === 'global') return globalConfigPath();
  const p = projectPaths(root);
  if (scope === 'local') return p.configLocal;
  return p.config;
}

export function setConfigValue(scope, root, key, value) {
  const file = configFileFor(scope, root);
  const data = readJson(file, {});
  setPath(data, key, value);
  writeJson(file, data);
  return file;
}

export function getConfigValue(root, key) {
  return getPath(loadConfig(root), key);
}
