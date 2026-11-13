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

const HELP = `dfw - design-first workflow CLI

Workflow
  dfw init [--mode <mode>] [--product <name>]   Start the workflow in this project
  dfw status [--json]                           Phase, gates, design version, comments
  dfw mode <mode>                               Change workflow mode (${Object.keys(MODES).join(', ')})
  dfw phase <phase> [--force --reason <r>]      Move to a phase (${PHASES.join(', ')})
  dfw approve <G1|G2|G3|G4> --note <text>       Record the user's approval (G2 freezes the design)
        [--accept-findings]                     G2 only: approve despite MUST findings (recorded)
  dfw reopen --reason <text>                    Re-open an approved design as a new version
  dfw decide --level <1-5> --topic <t> <text>   Record a decision (1=user ... 5=external skill)
  dfw decisions [--json]                        List recorded decisions
  dfw pause | resume                            Disable/enable gates and hooks for this project

Design workspace
  dfw tokens build                              DTCG tokens.json -> tokens.css
  dfw lint [files...] [--json]                  dfw design lint only (fast)
  dfw check [files...|--url <u>] [--json] [--setup]   dfw lint + Impeccable + sloplint (+ Taste copy-lint)
  dfw studio start|stop|status [--port <n>]     Local Design Studio (canvas + click-to-comment)
  dfw shoot [--screen <id>] [--variant <id>] [--bp <ids>] [--url <u> --out <file>]   Screenshots (Playwright)
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
      return state.gates.G1 ? '"dfw phase concepts"' : 'present directions; record the choice with "dfw approve G1 --note ..."';
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
    for (const s of m.screens || []) {
      if (a.screen && s.id !== a.screen) continue;
      for (const v of s.variants || []) {
        if (a.variant && v.id !== a.variant) continue;
        for (const bp of bps) {
          const q = new URLSearchParams();
          if (a.theme) q.set('theme', a.theme);
          if (a.state) q.set('state', a.state);
          jobs.push({
            url: `${info.url}design/${v.file}${q.toString() ? `?${q}` : ''}`,
            width: bp.width,
            height: bp.height,
            out: path.join(p.dfw, 'shots', version, `${s.id.replace(/\//g, '__')}__${v.id}__${bp.id}${a.theme ? `__${a.theme}` : ''}${a.state ? `__${a.state}` : ''}${a.selector ? `__el-${String(a.selector).replace(/[^\w.-]+/g, '_').replace(/^_+|_+$/g, '').slice(0, 60)}` : ''}.png`),
            selector: a.selector,
            fullPage: !a.viewport,
          });
        }
      }
    }
  }
  if (!jobs.length) throw new DfwError('Nothing to screenshot (check --screen/--variant/--bp).');
  const res = await screenshot(jobs);
  return out(res.map((r) => `${r.ok ? 'ok  ' : 'FAIL'} ${path.relative(root, r.out)}${r.error ? ` (${r.error})` : ''}`).join('\n'));
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
      const res = handler(input);
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
    case 'tokens': {
      if (a._[1] !== 'build') throw new DfwError('Usage: dfw tokens build');
      const r = buildTokens(root);
      return out(`Wrote ${path.relative(root, r.out)}${r.themes.length ? ` (themes: ${r.themes.join(', ')})` : ''}`);
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
      return cmdComments(root, a);
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
