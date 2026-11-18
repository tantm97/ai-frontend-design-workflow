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
  review: {
    // dfw review: key that toggles comment mode on the running app ("off": the Comment button only).
    // Alt/Option+C rather than C, so the app's own single-key shortcuts keep working.
    shortcut: 'alt+c',
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

// Project layers come from the repository being opened, so they are untrusted: their `env` may only
// set names that cannot choose what runs or where it connects, and may only keep a privacy switch on.
// An allowlist, not a denylist: loader, proxy and tool-specific variables cannot be enumerated.
// The user's global config is trusted and may set any variable.
export const PROJECT_ENV_ALLOW = ['NO_COLOR', 'FORCE_COLOR', 'TZ', 'LANG', 'LC_ALL'];
export const PRIVACY_ENV = ['DO_NOT_TRACK', 'IMPECCABLE_NO_UPDATE_CHECK', 'IMPECCABLE_HOOK_DISABLED', 'IMPECCABLE_NO_TELEMETRY'];

/** A project layer without the `env` entries it may not set; one warning per dropped key. */
export function filterProjectEnv(layer, file, warn) {
  if (!layer || typeof layer !== 'object' || !('env' in layer)) return layer;
  const drop = (key) => warn(`dfw: ignored env.${key} in ${file} (a project config cannot set it; use the global config: dfw config set env.${key} <value> --global)`);
  const env = {};
  if (layer.env && typeof layer.env === 'object' && !Array.isArray(layer.env)) {
    for (const [k, v] of Object.entries(layer.env)) {
      if (typeof v === 'string' && (PROJECT_ENV_ALLOW.includes(k) || (PRIVACY_ENV.includes(k) && v === '1'))) env[k] = v;
      else drop(k);
    }
  } else warn(`dfw: ignored env in ${file} (it must be an object of strings)`);
  return { ...layer, env };
}

export function loadConfig(root, { warn = (m) => process.stderr.write(`${m}\n`), globalFile = globalConfigPath() } = {}) {
  const p = root ? projectPaths(root) : null;
  const layers = [
    ['defaults', DEFAULTS],
    ['global', readJson(globalFile, {})],
  ];
  if (p) {
    layers.push(['project', filterProjectEnv(readJson(p.config, {}), p.config, warn)]);
    layers.push(['local', filterProjectEnv(readJson(p.configLocal, {}), p.configLocal, warn)]);
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

/**
 * Opt-in to run an Impeccable engine dfw cannot checksum (IMPECCABLE_BIN, a sibling build,
 * ~/.impeccable/bin/impeccable, PATH). Read from the global config only: a project must not grant it.
 */
export function allowUnverifiedEngine(globalFile = globalConfigPath()) {
  return readJson(globalFile, {})?.impeccable?.allowUnverifiedEngine === true;
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
