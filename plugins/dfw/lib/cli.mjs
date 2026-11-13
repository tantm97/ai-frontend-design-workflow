// dfw command-line interface. Used by the dfw skill, agents, hooks and the user.
import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import { spawn, spawnSync } from 'node:child_process';
import {
  PLUGIN_ROOT, parseArgs, parseValue, findProjectRoot, projectPaths, readJson, writeJson, appendJsonl,
  readJsonl, nowIso, readStdin, DfwError, copyDir, listFiles, claudeDir, globalDir,
} from './util.mjs';
import { loadConfig, setConfigValue, getConfigValue } from './config.mjs';
import { loadState, saveState, newState, setPhase, approveGate, setMode, summarize, MODES, PHASES, GATES, currentApproval } from './state.mjs';
import { freezeDesign, reopenDesign, verifySnapshot } from './freeze.mjs';
import { runChecks, formatReport } from './check.mjs';
import { buildTokens } from './tokens.mjs';
import { detectStack } from './stack.mjs';
import { addComment, allComments, updateComment, newSession } from './comments.mjs';
import { languageStatus, setLanguage } from './lang.mjs';
import { HANDLERS } from './hooks.mjs';
import { impeccableLauncher, pluginRoot, sloplintScript, tasteCopyLintScript } from './locate.mjs';
import { playwrightStatus, installPlaywright, screenshot } from './runtime.mjs';
import { buildSystem, loadComponents } from './compose.mjs';
import { systemInit, systemOptions, systemChoose, systemSummary, listPresets } from './system.mjs';
import { searchIcons, addIcons } from './icons.mjs';
import { fontSubsets } from './fonts.mjs';
import { listStyles, searchStyles, getStyle, formatStyle, applyStyle, matchStyles, AXES } from './style.mjs';
import { auditDesignSystem } from './dsaudit.mjs';
import { layoutCheck, changedScreens, markChecked } from './layout.mjs';
import { buildPalette, typeScale } from './color.mjs';
import { waitForComments, formatForClaude, undelivered, markDelivered } from './inbox.mjs';

const HELP = `dfw - design-first workflow CLI

Workflow
  dfw init [--mode <mode>] [--product <name>]   Start the workflow in this project
  dfw status [--json]                           Phase, gates, design version, comments
  dfw mode <mode>                               Change workflow mode (${Object.keys(MODES).join(', ')})
  dfw phase <phase> [--force --reason <r>]      Move to a phase (${PHASES.join(', ')})
  dfw approve <G1|DS|G2|G3|G4> --note <text>    Record the user's approval (DS = design system, G2 freezes the design)
        [--accept-findings]                     G2 only: approve despite MUST findings (recorded)
  dfw reopen --reason <text>                    Re-open an approved design as a new version
  dfw decide --level <1-5> --topic <t> <text>   Record a decision (1=user ... 5=external skill)
  dfw decisions [--json]                        List recorded decisions
  dfw pause | resume                            Disable/enable gates and hooks for this project

Design system (built before any screen)
  dfw system init [--style <id> | --preset crisp|soft|industrial] [--only a,b] [--force]   Starter kit components + style tokens
  dfw system presets                            List presets
  dfw style list | search <words> | show <id>   Curated style guides (concept, named palette + rules, type, motifs)
  dfw style apply <id> [--fonts latin]          Turn a guide into tokens + fonts + motif master (latin = English-only fonts)
  dfw style match [--brief design/system/brief.json]   Rank guides against the brief's personality axes, with reasons
  dfw blueprint [list|<screen>] [--json]        Screen blueprints (goal, layout, above-the-fold, data range, feedback) to agree before building
  dfw layout check [--screen a,b] [--bp ids] [--states] [--changed]   Rendered check: overlap, clipping, overflow, anchors (with long lists), empty space, cross-screen consistency
  dfw system audit [--json]                     Senior review: contrast, accent vs status hues, neutrals, type rhythm, rationale
  dfw palette build --brand <hex|hue> ...       OKLCH ramps + roles picked by contrast + harmonised status colours
  dfw type scale --base 16 --ratio 1.2 --display-ratio 1.333 [--write]   Modular type scale
  dfw system options <a> <b> [<c>]               Compare 2-3 style guides/presets as themes (opt-<name>) on the same screens
  dfw system choose <preset>                    Promote one option to design/tokens/tokens.json
  dfw system list [--json]                      Components, variants, sizes, props
  dfw build                                     tokens.css + system/components.css (the hook does this on edit)
  dfw icons search <words> | add <names...> | list   Curated Lucide icons -> design/system/icons/
  dfw fonts check <family...>                   Subsets a Google Font ships (e.g. vietnamese)

Design workspace
  dfw tokens build                              DTCG tokens.json -> tokens.css (+ components.css)
  dfw lint [files...] [--json]                  dfw design lint only (fast)
  dfw check [files...|--url <u>] [--json] [--setup]   dfw lint + Impeccable + sloplint (+ Taste copy-lint)
  dfw studio start|stop|status [--port <n>]     Local Design Studio (canvas + click-to-comment)
  dfw shoot [--screen <id>|--page <id>] [--variant <id>] [--bp <ids>] [--state <s>|--states] [--theme <t>|--themes] [--system]
                                                Screenshots in one browser run (Playwright); --url <u> --out <file> for any URL
  dfw verify-snapshot [<version>]               Check an approved snapshot was not modified
  dfw handoff                                   Generate design/handoff/v<N>.md skeleton

Feedback (contract: schemas/feedback.schema.json)
  dfw comments [list] [--status open|fixed|...|active] [--json]
  dfw comments show <id>
  dfw comments ack <id> [--message <m>]
  dfw comments fixed <id> --message <what changed>
  dfw comments reply <id> --message <m>
  dfw comments dismiss <id> --message <why>     (only when the user agreed)
  dfw comments resolve <id> --by user           (only when the user confirmed)
  dfw comments add --json '<comment>'           (providers/adapters)
  dfw comments new-round
  dfw comments wait [--timeout <s>]             Block until the user posts a Studio comment (run in the background)
  dfw comments inbox [--peek]                   Undelivered comments (what the hooks inject)

Stack and settings
  dfw stack detect [--write]                    Inspect the existing stack (.dfw/stack.json draft)
  dfw stack set --json '<stack>'                Write the proposed stack (approve with G3)
  dfw lang status|set <code> [--session <id>]   Communication language for this session
  dfw config get <key> | set <key> <value> [--global|--project|--local]
  dfw doctor                                    Environment and installation checks
  dfw setup --playwright                        Install pinned Playwright + Chromium for screenshots
`;

function out(data, json) {
  if (json) console.log(JSON.stringify(data, null, 2));
  else console.log(typeof data === 'string' ? data : JSON.stringify(data, null, 2));
}

function requireState(root) {
  const s = loadState(root);
  if (!s) throw new DfwError(`No dfw workflow in ${root}. Run "dfw init" first.`);
  return s;
}

function templatesDir() {
  return path.join(PLUGIN_ROOT, 'templates');
}

function cmdInit(root, a) {
  const p = projectPaths(root);
  if (fs.existsSync(p.state)) throw new DfwError(`dfw is already initialized here (${p.state}). Use "dfw status".`);
  const state = newState({ mode: a.mode || 'new-product', product: a.product || '' });
  if (MODES[state.mode].requiresApprovedDesign) {
    throw new DfwError(`Mode "${state.mode}" needs an existing approved design; initialize with another mode first.`);
  }
  saveState(root, state);
  fs.mkdirSync(p.reviews, { recursive: true });
  if (!fs.existsSync(p.config)) writeJson(p.config, { schema: 1 });
  if (!fs.existsSync(p.decisions)) fs.writeFileSync(p.decisions, '');
  fs.writeFileSync(path.join(p.dfw, '.gitignore'), 'config.local.json\nstudio.json\nstudio.log\nshots/\nchecks/\n');
  const created = [];
  for (const doc of ['PRODUCT.md', 'DESIGN.md']) {
    if (!fs.existsSync(path.join(root, doc))) {
      fs.copyFileSync(path.join(templatesDir(), doc), path.join(root, doc));
      created.push(doc);
    }
  }
  if (!fs.existsSync(p.design)) {
    copyDir(path.join(templatesDir(), 'design'), p.design);
    const m = readJson(p.manifest);
    m.product = state.product || path.basename(root);
    writeJson(p.manifest, m);
    created.push('design/');
    try {
      buildTokens(root);
    } catch {}
  }
  return `Initialized dfw (${state.mode}) in ${root}\n${created.length ? `Created: ${created.join(', ')}\n` : ''}${summarize(state)}`;
}

function cmdStatus(root, a) {
  const state = loadState(root);
  const comments = state ? allComments(root) : [];
  const count = (st) => comments.filter((c) => st.includes(c.status)).length;
  const studio = readJson(projectPaths(root).studio, null);
  const data = {
    root,
    state,
    approved: state ? currentApproval(state) : null,
    comments: { active: count(['open', 'acknowledged', 'reopened']), fixed: count(['fixed']), resolved: count(['resolved']) },
    studio: studio && isAlive(studio.pid) ? studio : null,
    stack: readJson(projectPaths(root).stack, null),
  };
  if (a.json) return out(data, true);
  const lines = [summarize(state)];
  if (state) {
    lines.push(`comments: ${data.comments.active} active, ${data.comments.fixed} awaiting user verification, ${data.comments.resolved} resolved`);
    lines.push(`studio: ${data.studio ? data.studio.url : 'not running'}`);
    lines.push(`stack: ${data.stack ? `${data.stack.framework?.name || '?'}${state.gates.G3 ? ' (approved)' : ' (proposed)'}` : 'not set'}`);
    data.components = loadComponents(root).size;
    const next = nextStep(state, data);
    if (next) lines.push(`next: ${next}`);
  }
  return out(lines.join('\n'));
}

function nextStep(state, data) {
  const appr = currentApproval(state);
  switch (state.phase) {
    case 'discovery':
      return 'finish discovery (PRODUCT.md), then "dfw phase direction"';
    case 'direction':
      return state.gates.G1 ? '"dfw phase system"' : 'present directions; record the choice with "dfw approve G1 --note ..."';
    case 'system':
      if (state.gates.DS) return '"dfw phase concepts"';
      return data.components ? 'review the Design System page in the studio; when the user approves: "dfw approve DS --note ..."' : 'build the design system: "dfw system init --preset <p>", then adapt tokens and components';
    case 'concepts':
      if (data.comments.active) return `apply ${data.comments.active} open review comment(s)`;
      if (data.comments.fixed) return 'ask the user to verify fixed comments in the studio';
      return appr ? 'design approved; "dfw phase stack"' : 'review in the studio; when the user approves: "dfw approve G2 --note ..."';
    case 'stack':
      return state.gates.G3 ? '"dfw phase implement"' : 'propose/confirm the stack; "dfw approve G3 --note ..."';
    case 'implement':
      return 'implement from design/handoff; then "dfw phase verify"';
    case 'verify':
      return 'compare against the approved design; when accepted: "dfw approve G4 --note ..."';
    case 'audit':
      return 'review-only: report findings, change nothing';
    default:
      return null;
  }
}

function cmdApprove(root, a) {
  const gate = (a._[1] || '').toUpperCase();
  const state = requireState(root);
  if (!a.note) throw new DfwError('Approvals record what the user approved: pass --note "<summary of what was approved>".');
  if (gate === 'DS') {
    const lib = loadComponents(root);
    if (!lib.size) throw new DfwError('DS needs a design system: components in design/system/components/ (start with "dfw system init").');
    buildSystem(root);
    const config = loadConfig(root);
    const sysFiles = fs.readdirSync(path.join(projectPaths(root).design, 'system', 'components')).map((f) => `design/system/components/${f}`);
    const results = runChecks(root, ['design/tokens/tokens.json', ...sysFiles], config, { timeoutMs: 30000, tools: ['dfw'] });
    results.push({ tool: 'dfw-ds-audit', status: 'ran', findings: auditDesignSystem(root).map((f) => (f.rule === 'rationale' ? { ...f, level: 'error' } : f)) });
    const rep = formatReport(results, { max: 30 });
    if (rep.tally.error && !a['accept-findings']) throw new DfwError(`DS blocked: ${rep.tally.error} MUST finding(s) in the design system.\n${rep.text}`);
    const opts = fs.existsSync(path.join(projectPaths(root).tokens, 'themes')) && fs.readdirSync(path.join(projectPaths(root).tokens, 'themes')).some((f) => f.startsWith('opt-'));
    if (opts && !a.force) throw new DfwError('Preset options are still active (opt-* themes). Promote the chosen one first: dfw system choose <preset>.');
  }
  if (gate === 'G2') {
    const config = loadConfig(root);
    const results = runChecks(root, null, config, { timeoutMs: 60000 });
    const rep = formatReport(results, { max: 30 });
    if (rep.tally.error && !a['accept-findings']) {
      throw new DfwError(`G2 blocked: ${rep.tally.error} MUST finding(s) in the design.\n${rep.text}\nFix them, or if the user explicitly accepts them, re-run with --accept-findings (it is recorded).`);
    }
    if (rep.tally.error) {
      appendJsonl(projectPaths(root).decisions, { at: nowIso(), level: 1, topic: 'gate:G2:accepted-findings', decision: `User accepted ${rep.tally.error} MUST finding(s) at freeze.`, findings: rep.findings.filter((f) => f.level === 'error').slice(0, 50), by: 'user' });
    }
    const open = allComments(root).filter((c) => ['open', 'acknowledged', 'reopened', 'fixed'].includes(c.status));
    if (open.length && !a['accept-open-comments']) {
      throw new DfwError(`G2 blocked: ${open.length} review comment(s) are not resolved or dismissed (${open.map((c) => c.id).join(', ')}). Resolve them with the user, or pass --accept-open-comments if the user says so.`);
    }
    const s = freezeDesign(root, state, { note: a.note });
    return out(`Design frozen as ${s.design.approved.at(-1).version} (sha256 ${s.design.approved.at(-1).hash.slice(0, 12)}).\n${summarize(s)}`);
  }
  if (gate === 'G3') {
    const stack = readJson(projectPaths(root).stack, null);
    if (stack) {
      stack.status = 'approved';
      stack.approvedAt = nowIso();
      writeJson(projectPaths(root).stack, stack);
    }
  }
  const s = approveGate(root, state, gate, { note: a.note });
  return out(`${gate} approved: ${GATES[gate]}.\n${summarize(s)}`);
}

function isAlive(pid) {
  if (!pid) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function portFree(port) {
  return new Promise((resolve) => {
    const srv = net.createServer();
    srv.once('error', () => resolve(false));
    srv.once('listening', () => srv.close(() => resolve(true)));
    srv.listen(port, '127.0.0.1');
  });
}

async function waitFor(url, ms = 5000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try {
      const r = await fetch(url);
      if (r.ok) return true;
    } catch {}
    await new Promise((r) => setTimeout(r, 120));
  }
  return false;
}

async function cmdStudio(root, a) {
  const p = projectPaths(root);
  const sub = a._[1] || 'status';
  const info = readJson(p.studio, null);
  if (sub === 'status') return out(info && isAlive(info.pid) ? `studio running: ${info.url} (pid ${info.pid})` : 'studio not running');
  if (sub === 'stop') {
    if (info && isAlive(info.pid)) process.kill(info.pid);
    if (fs.existsSync(p.studio)) fs.unlinkSync(p.studio);
    return out('studio stopped');
  }
  if (sub !== 'start') throw new DfwError('Usage: dfw studio start|stop|status');
  if (info && isAlive(info.pid)) return out(`studio already running: ${info.url}`);
  const config = loadConfig(root);
  let port = Number(a.port || config.studio.port || 4600);
  for (let i = 0; i < 20 && !(await portFree(port)); i++) port++;
  fs.mkdirSync(p.dfw, { recursive: true });
  const log = fs.openSync(path.join(p.dfw, 'studio.log'), 'a');
  const child = spawn(process.execPath, [path.join(PLUGIN_ROOT, 'studio', 'server.mjs'), '--root', root, '--port', String(port)], {
    detached: true,
    stdio: ['ignore', log, log],
  });
  child.unref();
  const url = `http://127.0.0.1:${port}/`;
  if (!(await waitFor(`${url}__dfw/api/manifest`))) throw new DfwError(`Studio did not start; see ${path.join(p.dfw, 'studio.log')}`);
  writeJson(p.studio, { pid: child.pid, port, url, startedAt: nowIso() });
  return out(`studio running: ${url}`);
}

async function cmdShoot(root, a) {
  const p = projectPaths(root);
  const config = loadConfig(root);
  const bpsAll = readJson(p.manifest, {}).breakpoints || config.studio.breakpoints;
  const bpIds = a.bp ? String(a.bp).split(',') : bpsAll.map((b) => b.id);
  const bps = bpsAll.filter((b) => bpIds.includes(b.id));
  let jobs = [];
  if (a.url) {
    for (const bp of bps) {
      const outFile = a.out ? path.resolve(a.out.replace('{bp}', bp.id)) : path.join(p.dfw, 'shots', 'url', `${new URL(a.url).pathname.replace(/\W+/g, '_') || 'root'}__${bp.id}.png`);
      jobs.push({ url: a.url, width: bp.width, height: bp.height, out: outFile, selector: a.selector, fullPage: !a.viewport });
    }
  } else {
    const info = readJson(p.studio, null);
    if (!info || !isAlive(info.pid)) throw new DfwError('Start the studio first (dfw studio start) or pass --url.');
    const m = readJson(p.manifest, null);
    if (!m) throw new DfwError('design/design.json not found.');
    const state = loadState(root);
    const version = state ? `v${state.design.version}` : 'v1';
    const pageScreens = a.page ? new Set(((m.pages || []).find((pg) => pg.id === a.page) || { screens: [] }).screens.concat((m.screens || []).filter((x) => x.page === a.page).map((x) => x.id))) : null;
    const shotsDir = path.join(p.dfw, 'shots', version);
    const name = (...parts) => parts.filter(Boolean).join('__');
    if (a.system) {
      for (const bp of bps.slice(0, 1)) jobs.push({ url: `${info.url}__dfw/system.html`, width: bp.width, height: bp.height, out: path.join(shotsDir, name('_system', bp.id) + '.png'), fullPage: true });
    }
    const screens = a.system && !a.screen && !a.page ? [] : m.screens || [];
    for (const s of screens) {
      if (a.screen && s.id !== a.screen) continue;
      if (pageScreens && !pageScreens.has(s.id)) continue;
      const states = a.states ? [null, ...(s.states || m.states || [])] : [a.state || null];
      const themes = a.themes ? (m.themes || []).length ? m.themes : [null] : [a.theme || null];
      for (const v of s.variants || []) {
        if (a.variant && v.id !== a.variant) continue;
        for (const bp of bps) {
          for (const st of states) {
            for (const th of themes) {
              const q = new URLSearchParams();
              if (th) q.set('theme', th);
              if (st) q.set('state', st);
              jobs.push({
                url: `${info.url}design/${v.file}${q.toString() ? `?${q}` : ''}`,
                width: bp.width,
                height: bp.height,
                out: path.join(shotsDir, name(s.id.replace(/\//g, '__'), v.id, bp.id, th, st, a.selector ? `el-${String(a.selector).replace(/[^\w.-]+/g, '_').replace(/^_+|_+$/g, '').slice(0, 60)}` : '') + '.png'),
                selector: a.selector,
                fullPage: !a.viewport,
              });
            }
          }
        }
      }
    }
  }
  if (!jobs.length) throw new DfwError('Nothing to screenshot (check --screen/--variant/--bp).');
  const res = await screenshot(jobs);
  return out(res.map((r) => `${r.ok ? 'ok  ' : 'FAIL'} ${path.relative(root, r.out)}${r.error ? ` (${r.error})` : ''}`).join('\n'));
}

async function cmdCommentsWait(root, a) {
  const list = await waitForComments(root, { timeoutMs: (Number(a.timeout) || 3600) * 1000 });
  if (!list.length) return out('No new comments before the timeout. Re-run "dfw comments wait" in the background to keep listening.');
  return out(formatForClaude(list) + '\nAfter fixing, start "dfw comments wait" in the background again to keep listening.');
}

function cmdComments(root, a) {
  const sub = a._[1] || 'list';
  const id = a._[2];
  const msg = a.message || a.m;
  switch (sub) {
    case 'list': {
      let list = allComments(root);
      if (a.status === 'active') list = list.filter((c) => ['open', 'acknowledged', 'reopened'].includes(c.status));
      else if (a.status) list = list.filter((c) => String(a.status).split(',').includes(c.status));
      if (a.json) return out(list, true);
      if (!list.length) return out('No comments.');
      return out(
        list
          .map((c) => `${c.id} [${c.status}] ${[c.screen, c.variant, c.breakpoint].filter(Boolean).join('/')} -> ${c.target?.dfwId || c.target?.selector || '?'}${c.target?.source ? ` (${c.target.source})` : ''}\n    "${c.comment}"${c.thread?.length ? `\n    last: ${c.thread.at(-1).by}: ${c.thread.at(-1).message}` : ''}`)
          .join('\n'),
      );
    }
    case 'show': {
      const c = allComments(root).find((x) => x.id === id);
      if (!c) throw new DfwError(`Comment ${id} not found.`);
      return out(c, true);
    }
    case 'ack':
      return out(updateComment(root, id, { status: 'acknowledged', message: msg }), true);
    case 'fixed':
      return out(updateComment(root, id, { status: 'fixed', message: msg, evidence: a.evidence }), true);
    case 'reply':
      if (!msg) throw new DfwError('--message is required.');
      return out(updateComment(root, id, { message: msg }), true);
    case 'dismiss':
      if (!msg) throw new DfwError('Dismissing needs --message explaining why (and the user must agree).');
      return out(updateComment(root, id, { status: 'dismissed', message: msg, by: a.by || 'claude' }), true);
    case 'resolve':
      return out(updateComment(root, id, { status: 'resolved', message: msg || 'Confirmed by user.', by: a.by || 'claude' }), true);
    case 'reopen':
      return out(updateComment(root, id, { status: 'reopened', message: msg || 'Reopened.', by: a.by || 'user' }), true);
    case 'add':
      return out(addComment(root, JSON.parse(a.json)), true);
    case 'new-round':
      return out(newSession(root).session, true);
    case 'inbox': {
      const list = undelivered(root);
      if (!a.peek) markDelivered(root, list);
      return out(list.length ? formatForClaude(list) : 'No new comments.');
    }
    default:
      throw new DfwError(`Unknown comments subcommand "${sub}".`);
  }
}

function cmdHandoff(root) {
  const p = projectPaths(root);
  const state = requireState(root);
  const appr = currentApproval(state);
  if (!appr) throw new DfwError('Handoff is generated from an approved design (G2).');
  const dir = path.join(p.approved, appr.version);
  const m = readJson(path.join(dir, 'design.json'), {});
  const stack = readJson(p.stack, null);
  const tokens = fs.existsSync(path.join(dir, 'tokens', 'tokens.css')) ? fs.readFileSync(path.join(dir, 'tokens', 'tokens.css'), 'utf8').match(/--[\w-]+(?=:)/g) || [] : [];
  const comps = listFiles(path.join(dir, 'components')).filter((f) => f.endsWith('.html'));
  const resolved = allComments(root).filter((c) => c.status === 'resolved' && c.designVersion === appr.version);
  const file = path.join(p.handoff, `${appr.version}.md`);
  if (fs.existsSync(file)) throw new DfwError(`${path.relative(root, file)} already exists.`);
  const tpl = fs.readFileSync(path.join(templatesDir(), 'handoff.md'), 'utf8');
  const screens = (m.screens || [])
    .map((s) => {
      const chosen = s.variants.find((v) => v.id === s.approvedVariant) || s.variants.find((v) => v.recommended) || s.variants[0];
      return `| \`${s.id}\` | ${s.title || ''} | \`${chosen?.id}\` (\`design/approved/${appr.version}/${chosen?.file}\`) | ${s.route || 'TBD'} |`;
    })
    .join('\n');
  const body = tpl
    .replaceAll('{{version}}', appr.version)
    .replaceAll('{{hash}}', appr.hash)
    .replaceAll('{{product}}', m.product || state.product || '')
    .replaceAll('{{approvedAt}}', appr.at)
    .replaceAll('{{stack}}', stack ? `${stack.framework?.name || 'TBD'}; styling: ${(stack.styling || []).map((s) => s.name || s).join(', ') || 'TBD'}; UI libraries: ${(stack.uiLibraries || []).map((s) => s.name || s).join(', ') || 'none'} (${state.gates.G3 ? 'approved' : 'NOT YET APPROVED'})` : 'NOT SET - gate G3 required')
    .replaceAll('{{screens}}', screens || '| | | | |')
    .replaceAll('{{components}}', comps.map((c) => `- \`design/approved/${appr.version}/components/${c}\``).join('\n') || '- (none extracted)')
    .replaceAll('{{tokens}}', tokens.length ? `${tokens.length} CSS custom properties in \`design/approved/${appr.version}/tokens/tokens.css\` (source: \`tokens/tokens.json\`, W3C DTCG).` : 'No tokens found.')
    .replaceAll('{{breakpoints}}', (m.breakpoints || loadConfig(root).studio.breakpoints).map((b) => `${b.id} ${b.width}px`).join(', '))
    .replaceAll('{{resolved}}', resolved.map((c) => `- ${c.id}: ${c.comment} -> ${c.resolution?.note || ''}`).join('\n') || '- none');
  fs.mkdirSync(p.handoff, { recursive: true });
  fs.writeFileSync(file, body);
  return out(`Wrote ${path.relative(root, file)}. Fill in the sections marked TODO with the user's requirements before implementing.`);
}

function cmdDoctor(root) {
  const rows = [];
  const ok = (name, good, detail) => rows.push(`${good ? 'ok  ' : 'WARN'} ${name}${detail ? ` - ${detail}` : ''}`);
  const major = Number(process.versions.node.split('.')[0]);
  ok('node >= 20', major >= 20, process.versions.node);
  const py = spawnSync('python3', ['--version'], { encoding: 'utf8' });
  ok('python3 (UI UX Pro Max search)', py.status === 0, (py.stdout || py.stderr || 'not found').trim());
  ok('git', spawnSync('git', ['--version']).status === 0);
  for (const name of ['frontend-design', 'ux-research', 'ui-ux-pro-max', 'impeccable', 'emil-design-eng', 'hallmark', 'taste-reference']) {
    const r = pluginRoot(name);
    ok(`plugin ${name}`, Boolean(r), r || 'not found (install from dfw-marketplace)');
  }
  const imp = impeccableLauncher();
  ok('impeccable engine downloaded', Boolean(imp?.ready), imp ? (imp.ready ? imp.version : 'run "dfw check --setup" once (downloads a checksum-verified binary from GitHub)') : 'impeccable plugin not found');
  ok('sloplint', Boolean(sloplintScript()));
  ok('taste copy-lint', Boolean(tasteCopyLintScript()), 'optional');
  const pw = playwrightStatus();
  ok('playwright (screenshots)', pw.installed, pw.installed ? `${pw.version} in ${pw.dir}` : 'optional: dfw setup --playwright');
  // Duplicate installs of the same upstream skills from other marketplaces load twice.
  const installed = readJson(path.join(claudeDir(), 'plugins', 'installed_plugins.json'), null);
  if (installed?.plugins) {
    const names = Object.keys(installed.plugins);
    const ours = ['frontend-design', 'impeccable', 'ui-ux-pro-max', 'hallmark', 'taste-skill', 'taste', 'emil-design-eng', 'example-skills'];
    const dups = names.filter((n) => ours.includes(n.split('@')[0]) && !n.endsWith('@dfw-marketplace'));
    ok('no duplicate upstream installs', !dups.length, dups.length ? `also installed outside dfw-marketplace: ${dups.join(', ')} (skills would load twice; consider disabling them)` : '');
  } else ok('duplicate install check', true, 'skipped (no installed_plugins.json; plugins may be loaded with --plugin-dir)');
  const state = loadState(root);
  ok('project workflow', true, state ? `${state.mode}/${state.phase}` : 'not initialized here');
  if (state) {
    for (const ap of state.design.approved) {
      const problems = verifySnapshot(root, ap.version);
      ok(`approved snapshot ${ap.version} intact`, !problems.length, problems.slice(0, 5).join('; '));
    }
  }
  ok('global config dir', true, globalDir());
  return out(rows.join('\n'));
}

export async function main(argv) {
  const a = parseArgs(argv);
  const cmd = a._[0];
  if (!cmd || cmd === 'help' || a.help) return out(HELP);
  if (cmd === 'hook') {
    const handler = HANDLERS[a._[1]];
    if (!handler) throw new DfwError(`Unknown hook "${a._[1]}"`);
    let input = {};
    try {
      input = JSON.parse((await readStdin()) || '{}');
    } catch {}
    try {
      const res = await handler(input);
      if (res) process.stdout.write(JSON.stringify(res));
    } catch (e) {
      // Fail open: never break the session because of a dfw bug.
      process.stderr.write(`dfw hook ${a._[1]} error: ${e.message}\n`);
    }
    return;
  }
  const root = findProjectRoot(a.root || undefined);
  switch (cmd) {
    case 'init':
      return out(cmdInit(root, a));
    case 'status':
      return cmdStatus(root, a);
    case 'mode':
      return out(summarize(setMode(root, requireState(root), a._[1])));
    case 'phase':
      return out(summarize(setPhase(root, requireState(root), a._[1], { force: Boolean(a.force), reason: a.reason })));
    case 'approve':
      return cmdApprove(root, a);
    case 'reopen':
      return out(summarize(reopenDesign(root, requireState(root), { reason: a.reason })));
    case 'pause':
    case 'resume': {
      const s = requireState(root);
      s.active = cmd === 'resume';
      s.history.push({ at: nowIso(), event: cmd, reason: a.reason });
      saveState(root, s);
      return out(`dfw ${cmd === 'pause' ? 'paused (gates and hooks off)' : 'resumed'} for ${root}`);
    }
    case 'decide': {
      const level = Number(a.level);
      if (!(level >= 1 && level <= 5)) throw new DfwError('--level must be 1 (user) .. 5 (external skill recommendation).');
      const text = a._.slice(1).join(' ') || a.text;
      if (!text) throw new DfwError('Decision text is required.');
      const entry = { at: nowIso(), level, topic: a.topic || 'general', decision: text, by: level === 1 ? 'user' : 'orchestrator', overrides: a.overrides || undefined };
      appendJsonl(projectPaths(root).decisions, entry);
      return out(entry, true);
    }
    case 'decisions': {
      const list = readJsonl(projectPaths(root).decisions);
      if (a.json) return out(list, true);
      return out(list.map((d) => `${d.at.slice(0, 16)} L${d.level} [${d.topic}] ${d.decision}`).join('\n') || 'No decisions recorded.');
    }
    case 'build':
    case 'tokens': {
      if (cmd === 'tokens' && a._[1] !== 'build') throw new DfwError('Usage: dfw tokens build');
      const r = buildTokens(root);
      const sys = buildSystem(root);
      return out(`Wrote ${path.relative(root, r.out)}${r.themes.length ? ` (themes: ${r.themes.join(', ')})` : ''}${sys.components ? `; ${sys.components} component(s) -> design/system/components.css` : ''}`);
    }
    case 'system': {
      const sub = a._[1] || 'list';
      if (sub === 'presets') return out(listPresets().map((p) => `${p.name.padEnd(12)} ${p.description.replace(/^dfw starter preset "[^"]+"\. /, '').replace(/ Generated by[\s\S]*$/, '')}`).join('\n'));
      if (sub === 'init') {
        const r = systemInit(root, { preset: a.preset || 'crisp', style: a.style || null, fonts: a.fonts === 'latin' ? 'latin' : 'vi', force: Boolean(a.force), components: a.only ? String(a.only).split(',') : null });
        return out(`Design system from preset "${r.preset}": ${r.copied.length} component(s) copied${r.kept.length ? `, ${r.kept.length} kept` : ''}; tokens ${r.tokensWritten ? 'written' : 'kept (pass --force to replace)'}; ${r.icons.length} icon(s). Open the Design System page in the studio.`);
      }
      if (sub === 'options') return out(`Option themes: ${systemOptions(root, a._.slice(2), { fonts: a.fonts === 'latin' ? 'latin' : 'vi' }).themes.join(', ')}. Compare them in the studio (theme menu / "Themes" toggle), then: dfw system choose <preset>.`);
      if (sub === 'choose') return out(`Tokens now come from "${systemChoose(root, a._[2], { fonts: a.fonts === 'latin' ? 'latin' : 'vi' }).chosen}". Adapt design/tokens/tokens.json to the direction, then review the Design System page.`);
      if (sub === 'audit') {
        const f = auditDesignSystem(root);
        if (a.json) return out(f, true);
        const label = { error: 'MUST', warn: 'SHOULD', info: 'NOTE' };
        if (f.some((x) => x.level === 'error')) process.exitCode = 2;
        return out(f.length ? f.map((x) => `- [${label[x.level]}] (${x.rule}) ${x.message}`).join('\n') : 'Design system audit: no findings.');
      }
      if (sub === 'list') {
        const sm = systemSummary(root);
        if (a.json) return out(sm, true);
        return out(sm.components.map((c) => `${c.name.padEnd(14)} ${[c.variants.length ? `variants: ${c.variants.join('|')}` : '', c.sizes.length ? `sizes: ${c.sizes.join('|')}` : '', c.props.length ? `props: ${c.props.join(', ')}` : ''].filter(Boolean).join('  ')}`).join('\n') || 'No components yet (dfw system init).');
      }
      throw new DfwError('Usage: dfw system init|presets|options|choose|list');
    }
    case 'style': {
      const sub = a._[1] || 'list';
      if (sub === 'list') return out(listStyles().map((g) => `${g.id.padEnd(20)} ${g.concept}`).join('\n'));
      if (sub === 'search') return out(searchStyles(a._.slice(2).join(' ')).slice(0, Number(a.limit) || 5).map((g) => `${g.id.padEnd(20)} score ${g.score}  ${g.concept}`).join('\n') || 'No match.');
      if (sub === 'show') return a.json ? out(getStyle(a._[2]), true) : out(formatStyle(getStyle(a._[2])));
      if (sub === 'match') {
        const bf = path.resolve(root, a.brief || 'design/system/brief.json');
        const brief = readJson(bf, null);
        if (!brief) throw new DfwError(`Brief not found: ${path.relative(root, bf)} (template: dfw init creates design/system/brief.json).`);
        const res = matchStyles(brief).slice(0, Number(a.limit) || 5);
        if (a.json) return out(res, true);
        return out(['Personality wanted: ' + Object.entries(brief.personality || {}).map(([k, v]) => `${k} ${v} (${AXES[k]?.[0]}→${AXES[k]?.[1]})`).join(', '), ...res.map((r) => `${String(r.fit).padStart(3)}%  ${r.id.padEnd(20)} ${r.mismatches.length ? `differs: ${r.mismatches.join('; ')}` : 'close on every axis'}${r.matches.length ? ` · keywords: ${r.matches.join(', ')}` : ''}${r.notes.length ? ` · ${r.notes.join(', ')}` : ''}`)].join('\n'));
      }
      if (sub === 'apply') {
        const r = applyStyle(root, a._[2], { fonts: a.fonts === 'latin' ? 'latin' : 'vi' });
        buildTokens(root);
        buildSystem(root);
        return out(`Applied style "${r.id}" (fonts: ${r.fonts.display} / ${r.fonts.body} / ${r.fonts.mono}). Wrote design/tokens/tokens.json, design/system/head.html, design/system/style.json${getStyle(r.id).motifs.length ? ', design/system/components/motif.html' : ''}.`);
      }
      throw new DfwError('Usage: dfw style list | search <words> | match [--brief f] | show <id> [--json] | apply <id|file.json> [--fonts latin]');
    }
    case 'type': {
      if (a._[1] !== 'scale') throw new DfwError('Usage: dfw type scale --base 16 --ratio 1.2 --display-ratio 1.333 [--from 28] [--write]');
      const ts = typeScale({ base: Number(a.base) || 16, ratio: Number(a.ratio) || 1.2, displayRatio: Number(a['display-ratio']) || 1.333, displayFrom: a.from ? Number(a.from) : undefined });
      if (a.write) {
        const f = path.join(projectPaths(root).tokens, 'tokens.json');
        const t = readJson(f);
        t.font = t.font || {};
        t.font.size = { $type: 'dimension', ...(t.font.size || {}) };
        for (const [k, v] of Object.entries({ ...ts.ui, ...ts.display })) t.font.size[k] = { $value: { value: v, unit: 'px' } };
        t.font.size.md.$description = `Body. Scale: base ${ts.base}, UI ratio ${ts.ratio}, display ratio ${ts.displayRatio}.`;
        writeJson(f, t);
        buildTokens(root);
      }
      return out(`UI: ${Object.entries(ts.ui).map(([k, v]) => `${k} ${v}`).join(' · ')}\nDisplay: ${Object.entries(ts.display).map(([k, v]) => `${k} ${v}`).join(' · ')}${a.write ? '\nWritten to design/tokens/tokens.json.' : ''}`);
    }
    case 'palette': {
      if (a._[1] !== 'build') throw new DfwError('Usage: dfw palette build --brand <hex|hue> [--chroma 0.14] --neutral-hue <h> --tint 0.01 [--signature <hue>|--harmony analogous|complementary|split-complementary|triadic] [--accent brand|signature|neutral] [--names brand,neutral,signature] [--dark] [--json]');
      const num = (v) => (v == null ? undefined : Number(v));
      const brand = String(a.brand || '').startsWith('#') ? { hex: a.brand, chroma: num(a.chroma) } : { hue: num(a.brand), chroma: num(a.chroma) ?? 0.14 };
      if (brand.hex == null && Number.isNaN(brand.hue ?? NaN)) throw new DfwError('--brand needs a hex colour or an OKLCH hue (0-360).');
      const sig = a.signature != null ? { hue: num(a.signature), chroma: num(a['signature-chroma']) } : a.harmony ? { harmony: a.harmony, chroma: num(a['signature-chroma']) } : undefined;
      const [nb, nn, ns] = String(a.names || 'brand,neutral,signature').split(',');
      const pal = buildPalette({ brand, neutral: { hue: num(a['neutral-hue']), tint: num(a.tint) ?? 0.01 }, signature: sig, accentFrom: a.accent, dark: Boolean(a.dark), warmSurface: Boolean(a['warm-surface']) }, { brand: nb, neutral: nn, signature: ns });
      if (a.json) return out(pal, true);
      const lines = Object.entries(pal.ramps).map(([n, r]) => `${n.padEnd(14)} ${Object.entries(r).map(([k, v]) => `${k} ${v}`).join('  ')}`);
      lines.push('Roles:', ...Object.entries(pal.roles).map(([k, v]) => `  ${k.padEnd(14)} ${v}`), 'Status:', `  ${Object.entries(pal.status).map(([k, v]) => `${k} ${v}`).join('  ')}`, ...pal.notes.map((n) => `Note: ${n}`));
      lines.push('Next: put the ramps in color.palette.* with usage rules, roles as aliases, then "dfw system audit".');
      return out(lines.join('\n'));
    }
    case 'blueprint': {
      const m = readJson(projectPaths(root).manifest, {});
      const list = (m.screens || []).filter((x) => (a._[1] && a._[1] !== 'list' ? x.id === a._[1] : !String(x.id).startsWith('_')));
      if (a.json) return out(list.map((x) => ({ id: x.id, title: x.title, blueprint: x.blueprint || null })), true);
      return out(list.map((x) => {
        const b = x.blueprint;
        if (!b) return `${x.id}: NO BLUEPRINT`;
        return [`${x.id} · ${x.title || ''} · layout ${b.layout}`, `  goal: ${b.goal}`, b.primaryAction ? `  primary action: ${b.primaryAction}` : '', `  above the fold: ${(b.aboveFold || []).join('; ')}`, ...(b.regions || []).map((r) => `  region ${r.priority ?? ''} ${r.name}: ${r.content}`), `  data: empty → ${b.data?.empty || '?'} | typical → ${b.data?.typical || '?'} | max → ${b.data?.max || '?'}`, `  feedback: ${b.feedback}`].filter(Boolean).join('\n');
      }).join('\n\n') || 'No screens.');
    }
    case 'layout': {
      if ((a._[1] || 'check') !== 'check') throw new DfwError('Usage: dfw layout check [--screen a,b] [--bp ids] [--states] [--no-stress] [--changed] [--no-system] [--json]');
      let screens = a.screen ? String(a.screen).split(',') : null;
      if (a.changed) {
        const ch = changedScreens(root);
        screens = ch.screens;
        if (!screens.length && !ch.changedComps.length && !ch.tokensChanged) return out('Layout: nothing changed since the last check.');
      }
      const res = await layoutCheck(root, { screens, bp: a.bp ? String(a.bp).split(',') : null, states: Boolean(a.states), stress: !a['no-stress'], system: !a['no-system'] && (!a.changed || changedScreens(root).changedComps.length > 0 || !screens) });
      if (res.status !== 'ran') return out(`Layout check ${res.status}: ${res.detail}`);
      const errors = res.findings.filter((f) => f.level === 'error').length;
      if (!errors) markChecked(root);
      if (errors) process.exitCode = 2;
      if (a.json) return out(res, true);
      const label = { error: 'MUST', warn: 'SHOULD', info: 'NOTE' };
      return out([`Layout: ${res.renders} render(s), ${errors} must, ${res.findings.length - errors} should.`, ...res.findings.map((f) => `- [${label[f.level]}] (${f.rule}) ${f.message}`)].join('\n'));
    }
    case 'icons': {
      const sub = a._[1] || 'list';
      if (sub === 'search') return out(searchIcons(a._.slice(2).join(' '), Number(a.limit) || 20).map((i) => `${i.name.padEnd(24)} ${i.tags.join(', ')}`).join('\n') || 'No match.');
      if (sub === 'add') {
        const r = addIcons(root, a._.slice(2));
        const miss = r.missing.map((m) => `  ${m}: not found${r.suggestions[m].length ? ` (try: ${r.suggestions[m].join(', ')})` : ''}`);
        if (r.missing.length) process.exitCode = 2;
        return out([`Added ${r.added.length} icon(s) to design/system/icons/.`, ...miss].join('\n'));
      }
      if (sub === 'list') {
        const dir = path.join(projectPaths(root).design, 'system', 'icons');
        return out(fs.existsSync(dir) ? fs.readdirSync(dir).filter((f) => f.endsWith('.svg')).map((f) => f.slice(0, -4)).join(' ') || '(none)' : '(none)');
      }
      throw new DfwError('Usage: dfw icons search <words> | add <names...> | list');
    }
    case 'fonts': {
      if (a._[1] !== 'check' || a._.length < 3) throw new DfwError('Usage: dfw fonts check <family...>  (quote names with spaces)');
      const res = await Promise.all(a._.slice(2).map((f) => fontSubsets(f).catch((e) => ({ family: f, ok: false, error: e.message, subsets: [] }))));
      if (a.json) return out(res, true);
      return out(res.map((r) => (r.ok ? `${r.family}: ${r.subsets.join(', ')}${r.subsets.includes('vietnamese') ? '' : '  (NO vietnamese subset)'}` : `${r.family}: not found on Google Fonts${r.error ? ` (${r.error})` : ''}`)).join('\n'));
    }
    case 'lint':
    case 'check': {
      const config = loadConfig(root);
      if (a.setup) {
        const l = impeccableLauncher();
        if (!l) throw new DfwError('Impeccable plugin not found.');
        const tmp = path.join(projectPaths(root).dfw, 'checks', 'setup.html');
        fs.mkdirSync(path.dirname(tmp), { recursive: true });
        fs.writeFileSync(tmp, '<!doctype html><html lang="en"><title>x</title><p>x</p></html>');
        const r = spawnSync(l.cmd, ['detect', '--json', tmp], { env: { ...process.env, ...config.env }, encoding: 'utf8', timeout: 180000 });
        return out(r.status === 0 || r.status === 2 ? 'Impeccable engine ready.' : `Impeccable setup failed: ${(r.stderr || '').slice(0, 500)}`);
      }
      const targets = a.url ? [a.url] : a._.length > 1 ? a._.slice(1) : null;
      const results = runChecks(root, targets, config, { timeoutMs: Number(a.timeout) || 60000, allowDownload: false, tools: cmd === 'lint' ? ['dfw'] : undefined });
      const rep = formatReport(results, { max: Number(a.max) || 60 });
      if (a.json) out({ results, tally: rep.tally }, true);
      else out(rep.text);
      if (rep.tally.error) process.exitCode = 2;
      return;
    }
    case 'studio':
      return cmdStudio(root, a);
    case 'shoot':
      return cmdShoot(root, a);
    case 'verify-snapshot': {
      const state = requireState(root);
      const v = a._[1] || currentApproval(state)?.version || state.design.approved.at(-1)?.version;
      if (!v) throw new DfwError('No approved snapshot.');
      const problems = verifySnapshot(root, v);
      if (problems.length) process.exitCode = 2;
      return out(problems.length ? `Snapshot ${v} was modified:\n${problems.join('\n')}` : `Snapshot ${v} intact.`);
    }
    case 'handoff':
      return cmdHandoff(root);
    case 'comments':
      return a._[1] === 'wait' ? cmdCommentsWait(root, a) : cmdComments(root, a);
    case 'stack': {
      const sub = a._[1];
      if (sub === 'detect') {
        const d = detectStack(root);
        if (a.write) writeJson(projectPaths(root).stack, { ...d, status: 'proposed', source: 'detected' });
        return out(d, true);
      }
      if (sub === 'set') {
        const data = a.file ? readJson(path.resolve(a.file)) : JSON.parse(a.json || '{}');
        writeJson(projectPaths(root).stack, { ...data, status: 'proposed', proposedAt: nowIso() });
        const s = loadState(root);
        if (s?.gates.G3) {
          s.gates.G3 = null;
          s.history.push({ at: nowIso(), event: 'stack-changed', note: 'G3 invalidated by a new stack proposal' });
          saveState(root, s);
        }
        return out('Stack proposal written to .dfw/stack.json. Approve with "dfw approve G3 --note ..." after the user confirms.');
      }
      throw new DfwError('Usage: dfw stack detect [--write] | dfw stack set --json <json>|--file <f>');
    }
    case 'lang': {
      const sub = a._[1] || 'status';
      if (sub === 'status') return out(languageStatus(a.session), true);
      if (sub === 'set') return out(setLanguage(a._[2], a.session), true);
      throw new DfwError('Usage: dfw lang status|set <code> [--session <id>]');
    }
    case 'config': {
      const sub = a._[1];
      if (sub === 'get') return out(a._[2] ? getConfigValue(root, a._[2]) : loadConfig(root), true);
      if (sub === 'set') {
        const scope = a.global ? 'global' : a.local ? 'local' : 'project';
        const file = setConfigValue(scope, root, a._[2], parseValue(a._[3]));
        return out(`Set ${a._[2]} in ${file}`);
      }
      throw new DfwError('Usage: dfw config get [key] | set <key> <value> [--global|--project|--local]');
    }
    case 'doctor':
      return cmdDoctor(root);
    case 'setup': {
      if (a.playwright) return out(installPlaywright(), true);
      if (a['marketplace-repo']) {
        const file = setConfigValue('global', root, 'marketplaceRepo', path.resolve(a['marketplace-repo']));
        return out(`Recorded marketplace repo in ${file}`);
      }
      throw new DfwError('Usage: dfw setup --playwright | --marketplace-repo <path>');
    }
    default:
      throw new DfwError(`Unknown command "${cmd}". Run "dfw help".`);
  }
}
