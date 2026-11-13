// Orchestrated checks: dfw lint + Impeccable detect + Hallmark sloplint (+ optional Taste copy-lint).
// Severity is normalized to: error (Must fix), warn (Should fix), info (Nice to have).
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { lintDesign } from './lint.mjs';
import { buildSystem, buildComposed } from './compose.mjs';
import { auditDesignSystem } from './dsaudit.mjs';
import { impeccableLauncher, sloplintScript, tasteCopyLintScript } from './locate.mjs';
import { readJson, projectPaths, toPosix } from './util.mjs';

export const UI_EXT = new Set(['.html', '.htm', '.css', '.scss', '.sass', '.less', '.jsx', '.tsx', '.vue', '.svelte', '.astro']);

function run(cmd, args, { cwd, env, timeoutMs }) {
  const r = spawnSync(cmd, args, {
    cwd,
    env: { ...process.env, ...env },
    encoding: 'utf8',
    timeout: timeoutMs,
    maxBuffer: 32 * 1024 * 1024,
  });
  return { status: r.status, stdout: r.stdout || '', stderr: r.stderr || '', error: r.error, timedOut: r.error?.code === 'ETIMEDOUT' };
}

function parseJsonLoose(s) {
  const start = s.search(/[[{]/);
  if (start < 0) return null;
  try {
    return JSON.parse(s.slice(start));
  } catch {
    return null;
  }
}

function sloplintScope(root) {
  const m = readJson(projectPaths(root).manifest, {});
  return m.surface === 'marketing' ? 'page' : 'app';
}

export function runSloplint(root, targets, { env, timeoutMs }) {
  const script = sloplintScript();
  if (!script) return { tool: 'sloplint', status: 'unavailable', findings: [] };
  const files = targets.filter((t) => /\.(html|css|jsx|tsx|vue|svelte|astro)$/.test(t));
  if (!files.length) return { tool: 'sloplint', status: 'skipped', findings: [] };
  const scope = sloplintScope(root);
  const r = run(process.execPath, [script, ...files, '--json', '--scope', scope], { cwd: root, env, timeoutMs });
  if (r.timedOut) return { tool: 'sloplint', status: 'timeout', findings: [] };
  const data = parseJsonLoose(r.stdout);
  if (!data) return { tool: 'sloplint', status: 'error', detail: (r.stderr || r.stdout).slice(0, 400), findings: [] };
  // Hallmark leads marketing pages only. On product UI its Floor is a strong
  // recommendation (precedence level 5), so FAILs do not block approval there.
  const grade = { FAIL: scope === 'page' ? 'error' : 'warn', REFLEX: 'warn', WARN: 'warn' };
  const findings = (data.findings || [])
    // Gate 20 is Hallmark's own stamp requirement; it only applies to pages Hallmark built.
    .filter((f) => String(f.gate) !== '20' && grade[f.grade])
    .map((f) => ({
      level: grade[f.grade],
      rule: `hallmark-${f.gate}`,
      file: toPosix(path.relative(root, path.resolve(root, f.file || ''))),
      line: f.line || 1,
      message: `${f.evidence}${f.fix ? ` | fix: ${f.fix}` : ''}`,
      source: 'sloplint',
    }));
  return { tool: 'sloplint', status: 'ran', findings };
}

export function runImpeccable(root, targets, { env, timeoutMs, allowDownload = false }) {
  const l = impeccableLauncher();
  if (!l) return { tool: 'impeccable', status: 'unavailable', findings: [] };
  if (!l.ready && !allowDownload) {
    return { tool: 'impeccable', status: 'not-installed', detail: 'Engine not downloaded yet; run "dfw check --setup" once.', findings: [] };
  }
  const files = targets.filter((t) => UI_EXT.has(path.extname(t)) || /^https?:/.test(t));
  if (!files.length) return { tool: 'impeccable', status: 'skipped', findings: [] };
  const r = run(l.cmd, ['detect', '--json', ...files], { cwd: root, env, timeoutMs });
  if (r.timedOut) return { tool: 'impeccable', status: 'timeout', findings: [] };
  const data = parseJsonLoose(r.stdout);
  if (!Array.isArray(data)) return { tool: 'impeccable', status: 'error', detail: (r.stderr || r.stdout).slice(0, 400), findings: [] };
  const sev = { error: 'error', critical: 'error', warning: 'warn', warn: 'warn', info: 'info', advisory: 'info' };
  const findings = data.map((f) => ({
    level: f.advisory ? 'info' : sev[f.severity] || 'warn',
    rule: `impeccable-${f.antipattern}`,
    file: toPosix(path.relative(root, path.resolve(root, f.file || ''))),
    line: f.line || 1,
    message: `${f.name}: ${f.description}${f.snippet ? ` [${String(f.snippet).slice(0, 80)}]` : ''}`,
    source: 'impeccable',
  }));
  return { tool: 'impeccable', status: 'ran', findings };
}

export function runTasteCopyLint(root, targets, { env, timeoutMs }) {
  const script = tasteCopyLintScript();
  if (!script) return { tool: 'taste-copy-lint', status: 'unavailable', findings: [] };
  const files = targets.filter((t) => /\.(html|md|txt)$/.test(t));
  if (!files.length) return { tool: 'taste-copy-lint', status: 'skipped', findings: [] };
  const r = run(process.execPath, [script, ...files, '--json'], { cwd: root, env, timeoutMs });
  const data = parseJsonLoose(r.stdout);
  if (!data) return { tool: 'taste-copy-lint', status: 'error', detail: (r.stderr || r.stdout).slice(0, 400), findings: [] };
  const findings = [];
  (data.files || []).forEach((file, i) => {
    for (const f of file.findings || []) {
      if (f.sev === 'INFO') continue;
      findings.push({
        level: f.sev === 'FAIL' ? 'warn' : 'info', // reference-mode skill: never a hard blocker
        rule: `taste-${f.id}`,
        file: files[i] || '',
        line: 1,
        message: `${f.msg || f.id}${f.fix ? ` | fix: ${f.fix}` : ''}`,
        source: 'taste-copy-lint',
      });
    }
  });
  return { tool: 'taste-copy-lint', status: 'ran', findings };
}

/**
 * Run all enabled checks on `targets` (project-relative paths or URLs).
 * When targets is null, checks the whole design workspace.
 */
export function runChecks(root, targets, config, { timeoutMs, allowDownload = false, tools } = {}) {
  const p = projectPaths(root);
  const c = config.checks || {};
  const env = config.env || {};
  const budget = timeoutMs || c.timeoutMs || 8000;
  const designTargets = targets
    ? targets.filter((t) => !/^https?:/.test(t))
    : fs.existsSync(p.design)
      ? ['design']
      : [];
  const results = [];
  const want = (name) => !tools || tools.includes(name);

  if (want('dfw')) {
    const lintTargets = targets ? designTargets.filter((t) => t.startsWith('design/')) : null;
    const findings = targets && !lintTargets.length ? [] : lintDesign(root, lintTargets);
    results.push({ tool: 'dfw-lint', status: 'ran', findings });
    if (!targets || lintTargets.some((t) => t.startsWith('design/tokens/') || t.startsWith('design/system/'))) {
      results.push({ tool: 'dfw-ds-audit', status: 'ran', findings: auditDesignSystem(root) });
    }
  }
  const external = targets || designTargets;
  if (!['impeccable', 'sloplint', 'taste'].some(want)) return results;
  // External tools read plain HTML: give them composed copies (components and icons expanded)
  // and map findings back to the source files.
  buildSystem(root);
  const expanded = external
    .flatMap((t) => (t === 'design' ? listDesignUi(root) : [t]))
    .filter((t) => !t.startsWith('design/system/components/'));
  const composed = new Map();
  const toolTargets = expanded.map((t) => {
    if (/^https?:/.test(t) || !t.endsWith('.html')) return t;
    const [r] = buildComposed(root, [t]);
    if (!r?.composed) return t;
    const rel = toPosix(path.relative(root, r.out));
    composed.set(rel, t);
    return rel;
  });
  const remap = (res) => {
    for (const f of res.findings || []) {
      const k = toPosix(String(f.file || ''));
      const hit = composed.get(k) || composed.get(k.replace(/^.*?(\.dfw\/build\/)/, '$1'));
      if (hit) f.file = hit;
    }
    return res;
  };
  if (want('impeccable') && c.impeccable !== false) results.push(remap(runImpeccable(root, toolTargets, { env, timeoutMs: budget, allowDownload })));
  if (want('sloplint') && c.sloplint !== false) results.push(remap(runSloplint(root, toolTargets, { env, timeoutMs: budget })));
  if (want('taste') && c.tasteCopyLint) results.push(remap(runTasteCopyLint(root, toolTargets, { env, timeoutMs: budget })));
  return results;
}

function listDesignUi(root) {
  const dir = projectPaths(root).design;
  const out = [];
  const walk = (abs, rel) => {
    for (const e of fs.readdirSync(abs, { withFileTypes: true })) {
      const r = `${rel}/${e.name}`;
      if (e.isDirectory()) {
        if (e.name === 'approved' || e.name === 'handoff' || e.name === 'components' && rel.endsWith('/system')) continue;
        walk(path.join(abs, e.name), r);
      } else if (UI_EXT.has(path.extname(e.name))) out.push(r);
    }
  };
  if (fs.existsSync(dir)) walk(dir, 'design');
  return out;
}

export function flatten(results) {
  return results.flatMap((r) => r.findings);
}

export function formatReport(results, { max = 50 } = {}) {
  const all = flatten(results);
  const order = { error: 0, warn: 1, info: 2 };
  all.sort((a, b) => order[a.level] - order[b.level] || a.file.localeCompare(b.file) || a.line - b.line);
  const label = { error: 'MUST', warn: 'SHOULD', info: 'NICE' };
  const lines = [];
  const tally = { error: 0, warn: 0, info: 0 };
  for (const f of all) tally[f.level]++;
  lines.push(`Checks: ${tally.error} must, ${tally.warn} should, ${tally.info} nice. Tools: ${results.map((r) => `${r.tool}=${r.status}`).join(', ')}`);
  for (const f of all.slice(0, max)) lines.push(`- [${label[f.level]}] ${f.file}:${f.line} (${f.source}/${f.rule}) ${f.message}`);
  if (all.length > max) lines.push(`- ... ${all.length - max} more (run "dfw check --json" for all).`);
  for (const r of results) if (r.detail) lines.push(`  note: ${r.tool}: ${r.detail}`);
  return { text: lines.join('\n'), tally, findings: all };
}
