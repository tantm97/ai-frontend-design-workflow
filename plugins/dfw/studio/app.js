// dfw Studio client. Vanilla JS, no build step.
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

const store = {
  get(k, d) {
    try {
      const v = localStorage.getItem(`dfw:${k}`);
      return v == null ? d : JSON.parse(v);
    } catch {
      return d;
    }
  },
  set(k, v) {
    try {
      localStorage.setItem(`dfw:${k}`, JSON.stringify(v));
    } catch {}
  },
};

const S = {
  manifest: null,
  state: null,
  comments: [],
  mode: 'view',
  zoom: store.get('zoom', 0.5),
  activeBps: store.get('bps', null),
  theme: store.get('theme', ''),
  uiState: '',
  filter: 'active',
  selected: null,
  pending: null, // selection awaiting a comment
};

const ACTIVE = ['open', 'acknowledged', 'reopened'];
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

async function api(path, opts = {}) {
  const res = await fetch(path, { headers: { 'Content-Type': 'application/json' }, ...opts });
  const data = await res.json();
  if (!res.ok) throw new Error(data.error || res.statusText);
  return data;
}

function toast(msg) {
  const t = $('#toast');
  t.textContent = msg;
  t.hidden = false;
  clearTimeout(toast.t);
  toast.t = setTimeout(() => (t.hidden = true), 2600);
}

/* ---------- Rendering ---------- */

function frameKey(screen, variant, bp) {
  return `${screen}::${variant}::${bp}`;
}

function variantSrc(v) {
  const q = new URLSearchParams();
  if (S.theme) q.set('theme', S.theme);
  if (S.uiState) q.set('state', S.uiState);
  const qs = q.toString();
  return `/design/${v.file}${qs ? `?${qs}` : ''}`;
}

function renderHeader() {
  const m = S.manifest;
  $('#product').textContent = m.product || 'Design Studio';
  const st = S.state;
  const parts = [];
  if (st) {
    parts.push(`v${st.design.version} ${st.design.status}`);
    parts.push(`phase: ${st.phase}`);
  } else parts.push('no workflow state');
  if (m.inferred) parts.push('manifest inferred');
  $('#meta').textContent = parts.join(' · ');

  const bpWrap = $('#breakpoints');
  bpWrap.innerHTML = '';
  if (!S.activeBps) S.activeBps = m.breakpoints.map((b) => b.id);
  for (const bp of m.breakpoints) {
    const b = document.createElement('button');
    b.type = 'button';
    b.textContent = `${bp.label || bp.id} ${bp.width}`;
    b.setAttribute('aria-pressed', String(S.activeBps.includes(bp.id)));
    b.onclick = () => {
      const on = S.activeBps.includes(bp.id);
      if (on && S.activeBps.length === 1) return;
      S.activeBps = on ? S.activeBps.filter((x) => x !== bp.id) : [...S.activeBps, bp.id];
      store.set('bps', S.activeBps);
      renderHeader();
      renderCanvas();
    };
    bpWrap.append(b);
  }

  const themes = m.themes || [];
  $('#theme-wrap').hidden = !themes.length;
  $('#theme').innerHTML = `<option value="">default</option>` + themes.map((t) => `<option ${t === S.theme ? 'selected' : ''}>${esc(t)}</option>`).join('');
  const states = m.states || [];
  $('#state-wrap').hidden = !states.length;
  $('#state').innerHTML = `<option value="">default</option>` + states.map((t) => `<option ${t === S.uiState ? 'selected' : ''}>${esc(t)}</option>`).join('');
  $('#zoom-label').textContent = `${Math.round(S.zoom * 100)}%`;
}

function renderSidebar() {
  const list = $('#screen-list');
  list.innerHTML = '';
  let flow = null;
  for (const s of S.manifest.screens || []) {
    if (s.flow && s.flow !== flow) {
      flow = s.flow;
      const li = document.createElement('li');
      li.className = 'flow';
      li.textContent = flow;
      list.append(li);
    }
    const open = S.comments.filter((c) => c.screen === s.id && ACTIVE.includes(c.status)).length;
    const li = document.createElement('li');
    li.innerHTML = `<a href="#screen-${esc(s.id)}" data-screen="${esc(s.id)}"><span>${esc(s.title || s.id)}</span>${open ? `<span class="n">${open}</span>` : ''}</a>`;
    list.append(li);
  }
}

function renderCanvas() {
  const inner = $('#canvas-inner');
  inner.innerHTML = '';
  const screens = S.manifest.screens || [];
  $('#empty').hidden = screens.length > 0;
  const bps = S.manifest.breakpoints.filter((b) => S.activeBps.includes(b.id));
  for (const s of screens) {
    const sec = document.createElement('section');
    sec.className = 'screen';
    sec.id = `screen-${s.id}`;
    sec.innerHTML = `<div class="screen-head"><h3>${esc(s.title || s.id)}</h3><span class="id">${esc(s.id)}</span>${s.description ? `<p>${esc(s.description)}</p>` : ''}</div>`;
    const variants = document.createElement('div');
    variants.className = 'variants';
    for (const v of s.variants || []) {
      const col = document.createElement('div');
      col.className = 'variant';
      col.innerHTML = `<div class="variant-label">${esc(v.label || v.id)}${v.mode ? `<span class="tag">${esc(v.mode)}</span>` : ''}${v.recommended ? '<span class="tag">recommended</span>' : ''}</div>`;
      const frames = document.createElement('div');
      frames.className = 'frames';
      for (const bp of bps) {
        const f = document.createElement('div');
        f.className = 'frame';
        const w = Math.round(bp.width * S.zoom);
        const h = Math.round(bp.height * S.zoom);
        const key = frameKey(s.id, v.id, bp.id);
        f.innerHTML = `<div class="frame-caption"><span>${esc(bp.label || bp.id)} · ${bp.width}×${bp.height}</span><a href="${variantSrc(v)}" target="_blank" rel="noopener">open</a></div>
          <div class="frame-box" style="width:${w}px;height:${h}px" data-key="${esc(key)}">
            <iframe title="${esc(`${s.title || s.id} – ${v.label || v.id} – ${bp.label || bp.id}`)}" name="${esc(key)}" src="${variantSrc(v)}"
              style="width:${bp.width}px;height:${bp.height}px;transform:scale(${S.zoom})" loading="lazy"></iframe>
          </div>`;
        const iframe = $('iframe', f);
        iframe.addEventListener('load', () => syncFrame(iframe));
        frames.append(f);
      }
      col.append(frames);
      variants.append(col);
    }
    sec.append(variants);
    inner.append(sec);
  }
}

function commentsFor(screen, variant) {
  return S.comments.filter((c) => c.screen === screen && c.variant === variant);
}

function syncFrame(iframe) {
  const [screen, variant] = iframe.name.split('::');
  const pins = commentsFor(screen, variant)
    .filter((c) => c.status !== 'dismissed')
    .map((c) => ({ id: c.id, n: Number(c.id.replace('fb_', '')), selector: c.target?.selector, dfwId: c.target?.dfwId, status: c.status }));
  iframe.contentWindow?.postMessage({ type: 'dfw:mode', comment: S.mode === 'comment' }, '*');
  iframe.contentWindow?.postMessage({ type: 'dfw:pins', pins }, '*');
}

function syncAllFrames() {
  for (const f of $$('.frame-box iframe')) syncFrame(f);
}

function statusLabel(s) {
  return { open: 'Open', acknowledged: 'In progress', reopened: 'Reopened', fixed: 'To verify', resolved: 'Resolved', dismissed: 'Dismissed' }[s] || s;
}

function renderComments() {
  const list = $('#comment-list');
  const f = S.filter;
  const items = S.comments
    .filter((c) => (f === 'active' ? ACTIVE.includes(c.status) : f === 'fixed' ? c.status === 'fixed' : f === 'closed' ? ['resolved', 'dismissed'].includes(c.status) : true))
    .sort((a, b) => b.id.localeCompare(a.id));
  $('#open-count').textContent = S.comments.filter((c) => ACTIVE.includes(c.status) || c.status === 'fixed').length;
  list.innerHTML = items.length ? '' : `<li class="muted" style="padding:12px">Nothing here.</li>`;
  for (const c of items) {
    const li = document.createElement('li');
    li.className = `comment${S.selected === c.id ? ' selected' : ''}`;
    li.dataset.id = c.id;
    const where = [c.screen, c.variant, c.breakpoint].filter(Boolean).join(' / ');
    const target = c.target?.dfwId || c.target?.selector || '(area)';
    const last = c.thread?.slice(-2) || [];
    li.innerHTML = `
      <div class="comment-top"><span class="num">#${esc(c.id.replace('fb_', ''))}</span><span>${esc(where)}</span><span class="status" data-s="${esc(c.status)}">${statusLabel(c.status)}</span></div>
      <div class="comment-target">${esc(target)}${c.target?.source ? ` · ${esc(c.target.source)}` : ''}</div>
      <p class="comment-text">${esc(c.comment)}</p>
      ${last.length ? `<ul class="thread">${last.map((t) => `<li><span class="by">${esc(t.by)} · ${esc(new Date(t.at).toLocaleString())}</span>${esc(t.message)}</li>`).join('')}</ul>` : ''}
      <div class="comment-actions"></div>`;
    const actions = $('.comment-actions', li);
    const btn = (label, cls, fn) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = label;
      if (cls) b.className = cls;
      b.onclick = (e) => {
        e.stopPropagation();
        fn();
      };
      actions.append(b);
    };
    if (c.status === 'fixed') {
      btn('Looks right · resolve', 'ok', () => patch(c.id, { status: 'resolved', message: 'Verified in studio.' }));
      btn('Not yet · reopen', '', () => askReason(actions, (msg) => patch(c.id, { status: 'reopened', message: msg })));
    } else if (ACTIVE.includes(c.status)) {
      btn('Dismiss', '', () => askReason(actions, (msg) => patch(c.id, { status: 'dismissed', message: msg })));
    } else {
      btn('Reopen', '', () => askReason(actions, (msg) => patch(c.id, { status: 'reopened', message: msg })));
    }
    li.onclick = () => focusComment(c);
    list.append(li);
  }
}

function askReason(container, done) {
  if ($('textarea', container)) return;
  const ta = document.createElement('textarea');
  ta.rows = 2;
  ta.placeholder = 'Why? (sent to Claude)';
  const ok = document.createElement('button');
  ok.type = 'button';
  ok.textContent = 'Send';
  ok.className = 'ok';
  ok.onclick = (e) => {
    e.stopPropagation();
    if (ta.value.trim()) done(ta.value.trim());
  };
  ta.onclick = (e) => e.stopPropagation();
  container.append(ta, ok);
  ta.focus();
}

async function patch(id, body) {
  try {
    await api(`/__dfw/api/comments/${id}`, { method: 'PATCH', body: JSON.stringify(body) });
    await loadComments();
  } catch (e) {
    toast(e.message);
  }
}

function focusComment(c) {
  S.selected = c.id;
  renderComments();
  const bp = c.breakpoint && S.activeBps.includes(c.breakpoint) ? c.breakpoint : S.activeBps[0];
  const box = document.querySelector(`.frame-box[data-key="${CSS.escape(frameKey(c.screen, c.variant, bp))}"]`);
  if (!box) return toast('That frame is not visible (check breakpoints).');
  box.scrollIntoView({ behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth', block: 'center', inline: 'center' });
  box.classList.remove('flash');
  void box.offsetWidth;
  box.classList.add('flash');
  $('iframe', box).contentWindow?.postMessage({ type: 'dfw:highlight', selector: c.target?.selector, dfwId: c.target?.dfwId }, '*');
}

/* ---------- Comment composer ---------- */

function openComposer(sel, iframe) {
  const [screen, variant, bp] = iframe.name.split('::');
  const s = S.manifest.screens.find((x) => x.id === screen);
  const v = s?.variants.find((x) => x.id === variant);
  S.pending = { screen, variant, breakpoint: bp, file: v?.file, state: S.uiState || null, theme: S.theme || null, target: sel.target };
  $('#composer-target').textContent = `${screen} / ${variant} / ${bp} → ${sel.target.dfwId || sel.target.selector}`;
  const rect = iframe.getBoundingClientRect();
  const x = rect.left + (sel.target.bbox[0] + sel.target.bbox[2]) * S.zoom + 12;
  const y = rect.top + sel.target.bbox[1] * S.zoom;
  const comp = $('#composer');
  comp.hidden = false;
  comp.style.left = `${Math.min(Math.max(8, x), innerWidth - 356)}px`;
  comp.style.top = `${Math.min(Math.max(64, y), innerHeight - 220)}px`;
  $('#composer-text').value = '';
  $('#composer-text').focus();
}

function closeComposer() {
  $('#composer').hidden = true;
  S.pending = null;
  for (const f of $$('.frame-box iframe')) f.contentWindow?.postMessage({ type: 'dfw:clear-selection' }, '*');
}

async function saveComment() {
  const text = $('#composer-text').value.trim();
  if (!text || !S.pending) return;
  try {
    const c = await api('/__dfw/api/comments', { method: 'POST', body: JSON.stringify({ ...S.pending, comment: text }) });
    closeComposer();
    S.selected = c.id;
    S.filter = 'active';
    setFilterTabs();
    await loadComments();
    toast(`Comment #${c.id.replace('fb_', '')} saved. Tell Claude to apply the open comments.`);
  } catch (e) {
    toast(e.message);
  }
}

/* ---------- Data ---------- */

async function loadComments() {
  S.comments = await api('/__dfw/api/comments');
  renderComments();
  renderSidebar();
  syncAllFrames();
}

async function loadAll() {
  const [manifest, st] = await Promise.all([api('/__dfw/api/manifest'), api('/__dfw/api/state')]);
  S.manifest = manifest;
  S.state = st.state;
  if (S.activeBps) S.activeBps = S.activeBps.filter((id) => manifest.breakpoints.some((b) => b.id === id));
  if (!S.activeBps?.length) S.activeBps = manifest.breakpoints.map((b) => b.id);
  renderHeader();
  renderCanvas();
  await loadComments();
}

/* ---------- Events ---------- */

function setMode(mode) {
  S.mode = mode;
  document.body.dataset.mode = mode;
  for (const b of $$('.segmented button')) b.setAttribute('aria-checked', String(b.dataset.mode === mode));
  if (mode === 'view') closeComposer();
  syncAllFrames();
}

function setZoom(z) {
  S.zoom = Math.min(1.5, Math.max(0.15, Math.round(z * 100) / 100));
  store.set('zoom', S.zoom);
  renderHeader();
  renderCanvas();
}

function setFilterTabs() {
  for (const b of $$('.tabs button')) b.setAttribute('aria-selected', String(b.dataset.filter === S.filter));
}

window.addEventListener('message', (e) => {
  const d = e.data || {};
  const iframe = $$('.frame-box iframe').find((f) => f.contentWindow === e.source);
  if (!iframe) return;
  if (d.type === 'dfw:select' && S.mode === 'comment') openComposer(d, iframe);
  if (d.type === 'dfw:pin') {
    const c = S.comments.find((x) => x.id === d.id);
    if (c) {
      S.selected = c.id;
      if (!$('#panel').offsetParent) document.body.classList.remove('panel-closed');
      renderComments();
      document.querySelector(`.comment[data-id="${d.id}"]`)?.scrollIntoView({ block: 'nearest' });
    }
  }
  if (d.type === 'dfw:key') handleKey(d.key, d.meta);
});

function handleKey(key, meta) {
  if (key === 'Escape') return closeComposer();
  if (key === 'c' || key === 'C') return setMode(S.mode === 'comment' ? 'view' : 'comment');
  if (key === 'v' || key === 'V') return setMode('view');
  if (key === '+' || key === '=') return setZoom(S.zoom + 0.1);
  if (key === '-') return setZoom(S.zoom - 0.1);
  if (key === '0') return setZoom(0.5);
}

document.addEventListener('keydown', (e) => {
  if (e.target.closest('textarea, input, select')) {
    if (e.target.id === 'composer-text' && e.key === 'Enter' && (e.metaKey || e.ctrlKey)) saveComment();
    if (e.key === 'Escape') closeComposer();
    return;
  }
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  handleKey(e.key);
});

for (const b of $$('.segmented button')) b.onclick = () => setMode(b.dataset.mode);
$('#zoom-in').onclick = () => setZoom(S.zoom + 0.1);
$('#zoom-out').onclick = () => setZoom(S.zoom - 0.1);
$('#zoom-label').onclick = () => setZoom(0.5);
$('#theme').onchange = (e) => {
  S.theme = e.target.value;
  store.set('theme', S.theme);
  renderCanvas();
};
$('#state').onchange = (e) => {
  S.uiState = e.target.value;
  renderCanvas();
};
$('#composer-cancel').onclick = closeComposer;
$('#composer-save').onclick = saveComment;
$('#panel-toggle').onclick = () => {
  const closed = document.body.classList.toggle('panel-closed');
  $('#panel-toggle').setAttribute('aria-expanded', String(!closed));
};
for (const b of $$('.tabs button')) {
  b.onclick = () => {
    S.filter = b.dataset.filter;
    setFilterTabs();
    renderComments();
  };
}
$('#new-session').onclick = async () => {
  await api('/__dfw/api/sessions', { method: 'POST' });
  toast('New review round started.');
  await loadComments();
};

const es = new EventSource('/__dfw/events');
es.addEventListener('design', async (e) => {
  const { files = [] } = JSON.parse(e.data);
  if (files.some((f) => /design\.json$/.test(f))) return loadAll();
  // Reload frames in place to keep scroll position of the canvas.
  for (const f of $$('.frame-box iframe')) f.contentWindow?.location.reload();
  toast('Design updated.');
});
es.addEventListener('comments', () => loadComments());

setMode('view');
loadAll().catch((e) => toast(e.message));
