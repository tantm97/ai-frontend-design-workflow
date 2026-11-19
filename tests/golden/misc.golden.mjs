// Commands with no group of their own: stack detect|set, handoff, and mcp (the stdio server and
// install|uninstall). The recorded results are those of the Node CLI of 0.5.2.
// Run: node --test tests/golden/
import { describe } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { run, assertSame, skipReason, nativeBinary, NOW, MINIMAL_STATE, REPO, suite, expected } from './harness.mjs';

const test = suite('misc');

const ISO = /\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z/g;

/** The binary gives the recorded exit code, stdout, stderr and files. Returns its result. */
function same(args, opts, { stderr = true } = {}) {
  const r = run(args, opts);
  assertSame(r, { stderr });
  return r.native;
}

const st = (over = {}) => structuredClone({ ...MINIMAL_STATE, ...over });
const write = (root, rel, data) => {
  const file = path.join(root, rel);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, typeof data === 'string' ? data : JSON.stringify(data, null, 2) + '\n');
};

describe('stack', { skip: skipReason() }, () => {
  // detectStack stamps the real time (it does not read DFW_NOW).
  const detect = (setup, args = []) => same(['stack', 'detect', ...args], { setup, normalize: (t) => t.replace(ISO, '<time>') });

  test('detect: a folder with nothing to go on is a new project', () => {
    const r = detect(() => {});
    const d = JSON.parse(r.stdout);
    assert.equal(d.kind, 'unknown');
    assert.equal(d.isNewProject, true);
    assert.deepEqual(r.files, {}, 'no --write: nothing written');
  });

  test('detect: a JavaScript project (framework, styling, UI libraries, language, package manager)', () => {
    const r = detect((root) => {
      write(root, 'package.json', { dependencies: { react: '^19.0.0', next: '15.1.0', '@mui/material': '6', vue: '' }, devDependencies: { tailwindcss: '4.0.0', sass: '1', typescript: '5.7' } });
      write(root, 'components.json', {});
      write(root, 'yarn.lock', '');
      write(root, 'package-lock.json', {});
      write(root, 'Cargo.toml', '');
    });
    const d = JSON.parse(r.stdout);
    assert.equal(d.framework.name, 'Next.js (React)');
    assert.deepEqual(d.evidence, ['package.json', 'components.json', 'yarn.lock', 'Cargo.toml']);
  });

  test('detect: plain JavaScript without a lock file, tsconfig only, broken package.json', () => {
    detect((root) => write(root, 'package.json', { name: 'x' }));
    detect((root) => {
      write(root, 'package.json', { devDependencies: { svelte: '5', '@sveltejs/kit': '2', unocss: '1', 'bits-ui': '1' } });
      write(root, 'tsconfig.json', {});
      write(root, 'bun.lock', '');
      write(root, 'pnpm-lock.yaml', '');
    });
    const r = run(['stack', 'detect'], { setup: (root) => write(root, 'package.json', '{broken') });
    assertSame(r);
    assert.equal(r.native.code, 1);
  });

  test('detect: stacks that are not JavaScript, and plain HTML', () => {
    for (const files of [['composer.json'], ['Gemfile'], ['go.mod'], ['pubspec.yaml'], ['manage.py', 'pyproject.toml', 'index.html'], ['pyproject.toml'], ['Cargo.toml'], ['index.html']]) {
      const r = detect((root) => files.forEach((f) => write(root, f, '')));
      assert.equal(JSON.parse(r.stdout).isNewProject, false, files.join());
    }
  });

  test('detect --write records the proposal in .dfw/stack.json', () => {
    const r = detect((root) => write(root, 'package.json', { dependencies: { vue: '3.5.0' } }), ['--write']);
    const saved = JSON.parse(r.files['.dfw/stack.json']);
    assert.equal(saved.status, 'proposed');
    assert.equal(saved.source, 'detected');
    assert.deepEqual(Object.keys(saved).slice(-2), ['status', 'source']);
  });

  test('set --json writes the proposal; a status given in the JSON keeps its place', () => {
    const r = same(['stack', 'set', '--json', '{"status":"mine","framework":{"name":"Vue"},"n":1.50}']);
    assert.equal(r.files['.dfw/stack.json'], `{\n  "status": "proposed",\n  "framework": {\n    "name": "Vue"\n  },\n  "n": 1.5,\n  "proposedAt": "${NOW}"\n}\n`);
    assert.equal(r.files['.dfw/state.json'], undefined, 'G3 was not approved: the state is untouched');
  });

  test('set invalidates an approved G3', () => {
    const state = st({ gates: { ...MINIMAL_STATE.gates, G3: { at: '2026-01-01T00:00:00.000Z', note: 'ok' } } });
    const r = same(['stack', 'set', '--json', '{"framework":{"name":"Vue"}}'], { state });
    const saved = JSON.parse(r.files['.dfw/state.json']);
    assert.equal(saved.gates.G3, null);
    assert.deepEqual(saved.history.at(-1), { at: NOW, event: 'stack-changed', note: 'G3 invalidated by a new stack proposal' });
  });

  test('set --file, set without input, set in a folder without a workflow, --json alone', () => {
    same(['stack', 'set', '--file', 'proposal.json'], { setup: (root) => write(root, 'proposal.json', { framework: { name: 'Astro' }, styling: ['css'] }) });
    same(['stack', 'set']);
    same(['stack', 'set', '--json', '[1,"a"]'], { state: null });
    same(['stack', 'set', '--json']);
  });

  test('errors: usage, broken JSON, a missing file', () => {
    assert.equal(same(['stack']).code, 1);
    assert.equal(same(['stack', 'nope']).code, 1);
    // A JavaScript engine message or a stack trace on stderr: only code, stdout and files compare.
    assert.equal(same(['stack', 'set', '--json', '{broken'], {}, { stderr: false }).code, 1);
    assert.equal(same(['stack', 'set', '--file', 'missing.json'], {}, { stderr: false }).code, 1);
    assert.equal(same(['stack', 'set', '--file'], {}, { stderr: false }).code, 1);
  });
});

describe('handoff', { skip: skipReason() }, () => {
  const approved = (over = {}) =>
    st({
      phase: 'stack',
      design: { version: 1, status: 'approved', locked: true, approved: [{ version: 'v1', hash: 'abc123', at: '2026-01-01T00:00:00.000Z', note: 'ok' }] },
      gates: { ...MINIMAL_STATE.gates, G2: { at: '2026-01-01T00:00:00.000Z' } },
      ...over,
    });
  const snapshot = (design) => (root) => {
    write(root, 'design/approved/v1/design.json', design);
    write(root, 'design/approved/v1/tokens/tokens.css', ':root {\n  --color-bg: #fff;\n  --space-1: 4px; --x:1;\n  color: var(--color-bg);\n}\n');
    write(root, 'design/approved/v1/components/button.html', '<button></button>');
    write(root, 'design/approved/v1/components/forms/input.html', '<input>');
    write(root, 'design/approved/v1/components/notes.md', 'x');
  };
  const design = {
    product: 'Shop $& Co',
    breakpoints: [{ id: 'mobile', width: 390 }, { id: 'desktop', width: 1440 }],
    screens: [
      { id: 'home', title: 'Home', route: '/', approvedVariant: 'b', variants: [{ id: 'a', file: 'screens/home/a.html' }, { id: 'b', file: 'screens/home/b.html' }] },
      { id: 'cart', variants: [{ id: 'a', file: 'screens/cart/a.html' }, { id: 'r', file: 'screens/cart/r.html', recommended: true }] },
      { id: 'pay', title: '', approvedVariant: 'zz', variants: [{ id: 'only', file: 'screens/pay/only.html' }] },
      { id: 'empty', approvedVariant: 'x', variants: [] },
    ],
  };

  test('writes design/handoff/<version>.md from the approved snapshot, the stack and resolved comments', () => {
    const r = same(['handoff'], {
      state: approved({ gates: { ...MINIMAL_STATE.gates, G2: { at: 'x' }, G3: { at: 'y' } } }),
      setup: (root) => {
        snapshot(design)(root);
        write(root, '.dfw/stack.json', { framework: { name: 'Next.js (React)' }, styling: [{ name: 'Tailwind CSS' }, 'plain css'], uiLibraries: [] });
        write(root, '.dfw/reviews/rv_20260101T000000.json', {
          id: 'rv_20260101T000000',
          comments: [
            { id: 'fb_001', status: 'resolved', designVersion: 'v1', comment: "wider $' button", resolution: { note: 'done' } },
            { id: 'fb_002', status: 'resolved', designVersion: 'v1', comment: 'no note' },
            { id: 'fb_003', status: 'open', designVersion: 'v1', comment: 'still open' },
            { id: 'fb_004', status: 'resolved', designVersion: 'v0', comment: 'older version' },
          ],
        });
      },
    });
    const body = r.files['design/handoff/v1.md'];
    assert.match(body, /^# Handoff: Shop \{\{product\}\} Co design v1\n/, 'a "$&" in the product name is a replacement pattern, as in Node');
    assert.match(body, /Stack: Next\.js \(React\); styling: Tailwind CSS, plain css; UI libraries: none \(approved\)/);
    assert.match(body, /\| `cart` \|  \| `r` \(`design\/approved\/v1\/screens\/cart\/r\.html`\) \| TBD \|/);
    assert.match(body, /\| `empty` \|  \| `undefined` /);
    assert.match(body, /3 CSS custom properties/);
    assert.match(body, /- fb_002: no note -> \n/);
    assert.equal(r.stdout, "Wrote design/handoff/v1.md. Fill in the sections marked TODO with the user's requirements before implementing.\n");
  });

  test('without a stack, tokens, components, screens or comments: the fallbacks; breakpoints from the config', () => {
    const r = same(['handoff'], { state: approved({ product: 'From state' }) });
    const body = r.files['design/handoff/v1.md'];
    assert.match(body, /Stack: NOT SET - gate G3 required/);
    assert.match(body, /No tokens found\./);
    assert.match(body, /- \(none extracted\)/);
    assert.match(body, /^# Handoff: From state design v1\n/);
    same(['handoff'], { state: approved(), setup: (root) => write(root, '.dfw/stack.json', { framework: 'Vue' }) });
  });

  test('errors: no workflow, no approved design, the file exists, a screen without variants', () => {
    assert.equal(same(['handoff'], { state: null }).code, 1);
    assert.equal(same(['handoff']).code, 1);
    const exists = same(['handoff'], { state: approved(), setup: (root) => write(root, 'design/handoff/v1.md', 'mine') });
    assert.equal(exists.code, 1);
    assert.equal(exists.stderr, 'design/handoff/v1.md already exists.\n');
    assert.equal(same(['handoff'], { state: approved(), setup: snapshot({ screens: [{ id: 'x' }] }) }, { stderr: false }).code, 1);
  });
});

/** Run the binary with a temporary HOME; its result and the files under that home, with the recorded ones. */
function withHome(args, { setup, stdin = '' } = {}) {
  const inHome = () => {
    const home = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-mcp-')));
    try {
      setup?.(home);
      const env = { ...process.env };
      for (const k of ['DFW_NO_BINARY', 'DFW_VERSION', 'CLAUDE_PROJECT_DIR']) delete env[k];
      Object.assign(env, { HOME: home, USERPROFILE: home, XDG_CONFIG_HOME: path.join(home, '.config'), CODEX_HOME: path.join(home, '.codex'), DFW_HOME: path.join(home, '.dfw-home'), DFW_PLUGIN_ROOT: path.join(REPO, 'core') });
      const r = spawnSync(nativeBinary(), args, { cwd: home, env, input: stdin, encoding: 'utf8', timeout: 60_000 });
      if (r.error) throw r.error;
      const norm = (t) => t.split(home).join('<home>').split(REPO).join('<repo>');
      const files = {};
      const walk = (dir) => {
        for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
          const abs = path.join(dir, ent.name);
          if (ent.isDirectory()) walk(abs);
          else files[path.relative(home, abs).split(path.sep).join('/')] = norm(fs.readFileSync(abs, 'utf8'));
        }
      };
      walk(home);
      return { code: r.status, stdout: norm(r.stdout), stderr: norm(r.stderr), files };
    } finally {
      fs.rmSync(home, { recursive: true, force: true });
    }
  };
  const native = inHome();
  return { expected: expected(native), native };
}

function sameHome(args, opts, { stderr = true } = {}) {
  const r = withHome(args, opts);
  assert.equal(r.native.code, r.expected.code, `${r.expected.stderr}\n${r.native.stderr}`);
  assert.equal(r.native.stdout, r.expected.stdout);
  if (stderr) assert.equal(r.native.stderr, r.expected.stderr);
  assert.deepEqual(r.native.files, r.expected.files);
  return r.native;
}

describe('mcp install | uninstall', { skip: skipReason() }, () => {
  const existing = (home) => {
    write(home, '.cursor/mcp.json', '{"mcpServers":{"other":{"command":"o"}}}');
    write(home, '.codex/config.toml', 'model = "o3"\r\n\r\n[mcp_servers.dfw]\r\ncommand = "/old/dfw"\r\nargs = ["mcp"]\r\n\r\n\r\n\r\n[mcp_servers.other]\r\ncommand = "o"\r\n');
    write(home, '.config/opencode/opencode.json', { theme: 'dark', mcp: { dfw: { type: 'local', command: ['/old'] }, other: { type: 'local', command: ['o'] } } });
  };

  test('install all --print shows each config and writes nothing', () => {
    const r = sameHome(['mcp', 'install', 'all', '--print']);
    assert.match(r.stdout, /# OpenAI Codex: <home>\/\.codex\/config\.toml\n\[mcp_servers\.dfw\]\ncommand = ".*\/core\/bin\/dfw"\nargs = \["mcp"\]/);
    assert.deepEqual(r.files, {});
    sameHome(['mcp', 'install', 'all', '--print'], { setup: existing });
    sameHome(['mcp', 'uninstall', 'all', '--print'], { setup: existing });
  });

  test('install all writes each config; an existing one is backed up once', () => {
    const fresh = sameHome(['mcp', 'install', 'all']);
    assert.deepEqual(Object.keys(fresh.files).sort(), ['.codex/config.toml', '.config/opencode/opencode.json', '.cursor/mcp.json']);
    const r = sameHome(['mcp', 'install', 'all'], { setup: existing });
    assert.equal(r.files['.cursor/mcp.json.dfw-backup'], '{"mcpServers":{"other":{"command":"o"}}}');
    assert.equal(r.files['.codex/config.toml'].match(/\[mcp_servers\.dfw\]/g).length, 1);
    assert.ok(!r.files['.codex/config.toml'].includes('/old/dfw'));
    assert.ok(JSON.parse(r.files['.cursor/mcp.json']).mcpServers.other, 'other servers kept');
  });

  test('install one tool twice is unchanged the second time; an earlier backup is kept', () => {
    const r = sameHome(['mcp', 'install', 'codex'], {
      setup: (home) => {
        existing(home);
        write(home, '.codex/config.toml.dfw-backup', 'the first backup');
      },
    });
    assert.equal(r.files['.codex/config.toml.dfw-backup'], 'the first backup');
    const installed = (home) => {
      const env = { ...process.env, HOME: home, CODEX_HOME: path.join(home, '.codex'), DFW_PLUGIN_ROOT: path.join(REPO, 'core') };
      const done = spawnSync(nativeBinary(), ['mcp', 'install', 'codex'], { env, encoding: 'utf8' });
      assert.equal(done.status, 0, done.stderr);
    };
    assert.match(sameHome(['mcp', 'install', 'codex'], { setup: installed }).stdout, /^OpenAI Codex: unchanged \(/);
  });

  test('uninstall removes dfw and keeps the rest', () => {
    const r = sameHome(['mcp', 'uninstall', 'all'], { setup: existing });
    assert.ok(!r.files['.codex/config.toml'].includes('mcp_servers.dfw'));
    assert.deepEqual(JSON.parse(r.files['.config/opencode/opencode.json']).mcp, { other: { type: 'local', command: ['o'] } });
    sameHome(['mcp', 'uninstall', 'cursor']);
    sameHome(['mcp', 'uninstall', 'codex'], { setup: (home) => write(home, '.codex/config.toml', '[mcp_servers.dfw]\ncommand = "x"\n') });
  });

  test('errors: usage, no tool, an unknown tool, a config that is not JSON', () => {
    assert.equal(sameHome(['mcp', 'nope']).code, 1);
    assert.equal(sameHome(['mcp', 'install']).stderr, 'Which tool? opencode, codex, cursor, all\n');
    // Plain errors: the recorded stderr is a Node stack trace, and the JSON parser words its message
    // its own way: only the exit code, stdout and files compare.
    assert.equal(sameHome(['mcp', 'install', 'vim'], {}, { stderr: false }).code, 1);
    assert.equal(sameHome(['mcp', 'install', 'cursor'], { setup: (home) => write(home, '.cursor/mcp.json', '{broken') }, { stderr: false }).code, 1);
  });
});

describe('mcp server on stdio', { skip: skipReason() }, () => {
  let id = 0;
  const req = (method, params) => JSON.stringify({ jsonrpc: '2.0', id: ++id, method, ...(params === undefined ? {} : { params }) });
  const call = (name, args) => req('tools/call', { name, arguments: args });

  // The binary serves the guide from its embedded workflow text, which has no folder on disk:
  // where the recorded Node result named the built mcp/ folder, the binary names the topic
  // (`references/x.md`).
  const guideDir = '<repo>/plugins/dfw/mcp';
  const normalize = (t) => t.split(`${guideDir}/`).join('').split(guideDir).join('.');

  /** One scripted session against the server: the replies, line by line. */
  function session(lines, opts = {}) {
    const r = run(['mcp'], { stdin: lines.join('\n') + '\n', normalize, ...opts });
    assertSame(r, { stderr: true });
    assert.equal(r.native.code, 0);
    return r.native.stdout.trim().split('\n').map((l) => JSON.parse(l));
  }

  test('initialize, tools/list, prompts, ping, unknown method, a notification', () => {
    const replies = session([
      req('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 't', version: '1' } }),
      JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }),
      req('initialize'),
      req('initialize', { protocolVersion: 7 }),
      req('tools/list'),
      req('prompts/list'),
      req('prompts/get', { name: 'dfw' }),
      req('prompts/get', { name: 'other' }),
      req('prompts/get'),
      req('ping'),
      req('nope/method'),
      JSON.stringify({ jsonrpc: '2.0', id: 'text-id', method: ['ping'] }),
      JSON.stringify({ jsonrpc: '2.0', id: null, method: 'ping' }),
    ]);
    assert.equal(replies.length, 12, 'the notification gets no reply');
    assert.equal(replies[0].result.serverInfo.name, 'dfw');
    assert.match(replies[0].result.instructions, /dfw_can_write/);
    assert.deepEqual(replies[3].result.tools.map((t) => t.name), ['dfw_guide', 'dfw', 'dfw_can_write']);
    assert.match(replies[5].result.messages[0].content.text, /dfw_can_write/);
    assert.equal(replies[9].error.code, -32601);
  });

  test('each tool: dfw_guide, dfw, dfw_can_write, and their refusals', () => {
    const replies = session(
      [
        call('dfw_guide', {}),
        call('dfw_guide', { topic: 'references/phase-concepts.md' }),
        call('dfw_guide', { topic: '/references/phase-concepts.md' }),
        call('dfw_guide', { topic: '../../package.json' }),
        call('dfw_guide', { topic: 'references' }),
        call('dfw_guide', { topic: null }),
        call('dfw', { args: ['status'], cwd: '.' }),
        call('dfw', { args: ['status', '--json'] }),
        call('dfw', { args: ['phase', 'nowhere'] }),
        call('dfw', { args: ['install'] }),
        call('dfw', { args: [] }),
        call('dfw', { args: 'status' }),
        call('dfw', { args: ['status', 1] }),
        call('dfw_can_write', { files: ['src/App.tsx', 'design/screens/a.html', '../outside.txt', '.dfw/notes.md'] }),
        call('dfw_can_write', { files: ['design/screens/a.html'] }),
        call('dfw_can_write', { files: [] }),
        call('dfw_can_write', {}),
        call('nope', {}),
        req('tools/call', {}),
        req('tools/call'),
        req('tools/call', { name: 'dfw_guide' }),
      ],
      { env: { CLAUDE_PROJECT_DIR: '' } },
    );
    const [guide, ref, slash, bad] = replies.map((r) => r.result);
    assert.equal(guide.isError, false);
    assert.match(guide.content[0].text, /dfw_can_write/, 'guide rendered for MCP');
    assert.doesNotMatch(guide.content[0].text, /__DFW_SKILL_DIR__|CLAUDE_SKILL_DIR|AskUserQuestion|\{\{[A-Z_]+\}\}/);
    assert.match(ref.content[0].text, /Phase: concepts/);
    assert.equal(slash.content[0].text, ref.content[0].text);
    assert.equal(bad.isError, true);
    const status = replies[6].result;
    assert.equal(status.isError, false);
    assert.match(status.content[0].text, /discovery|phase/i);
    assert.equal(replies[8].result.isError, true, 'a command that fails is an error for the model');
    assert.equal(replies[9].result.isError, true, 'install is not available through MCP');
    const gate = replies[13].result;
    assert.equal(gate.isError, true);
    assert.match(gate.content[0].text, /DENIED {2}src\/App\.tsx/);
    assert.match(gate.content[0].text, /ALLOWED design\/screens\/a\.html/);
    assert.equal(replies[14].result.isError, false);
    assert.equal(replies[17].error.code, -32602);
  });

  test('dfw_can_write in a folder without a workflow', () => {
    const replies = session([call('dfw_can_write', { files: ['a.txt'] })], { state: null, env: { CLAUDE_PROJECT_DIR: '' } });
    assert.equal(replies[0].result.content[0].text, 'No dfw workflow in this project: writes are not gated.');
  });

  test('malformed lines and a batch array get an error reply; the next request is still served', () => {
    const replies = session(['null', '42', '"text"', 'true', '{broken', '', '   ', '[{"jsonrpc":"2.0","id":1,"method":"ping"}]', '[]', '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":null}', '{"id":8}', '{"jsonrpc":"2.0","id":2,"method":"ping"}']);
    assert.deepEqual(
      replies.map((m) => [m.id, m.error?.code ?? 'ok']),
      [[null, -32600], [null, -32600], [null, -32600], [null, -32600], [null, -32700], [null, -32600], [null, -32600], [7, -32603], [8, -32601], [2, 'ok']],
    );
  });

  test('a last line without its newline is not answered', () => {
    const r = run(['mcp'], { stdin: '{"jsonrpc":"2.0","id":1,"method":"ping"}\n{"jsonrpc":"2.0","id":2,"method":"ping"}' });
    assertSame(r, { stderr: true });
    assert.equal(r.native.stdout, '{"jsonrpc":"2.0","id":1,"result":{}}\n');
  });
});
