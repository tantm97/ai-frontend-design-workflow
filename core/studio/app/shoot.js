// @ts-check
// Screenshots: the camera menu (screens grouped by page, the choice kept in localStorage, null = all)
// and the presenter's Screenshot button. PNGs are rendered by the Studio server.
import { esc, shotName } from './model.js';
import { $, P, S, pageModel, store, toast } from './state.js';
import { allFrameSpecs } from './frames.js';
import { presentSrc } from './present.js';
import { server } from './api.js';
import { SYSTEM_SCREEN } from '#studio/shared/enums.js';
import { TEXT } from '#studio/shared/text.js';

const shootKey = (/** @type {any} */ f) => (f.system ? SYSTEM_SCREEN : f.screen?.id || f.file);
/** @type {string[] | null} */
let shootPicked = store.get('shoot1', null);

/** Screens available to screenshot, grouped by page, each with its frame count under the current toolbar choices. */
function shootGroups() {
  const count = new Map();
  for (const f of allFrameSpecs()) count.set(shootKey(f), (count.get(shootKey(f)) || 0) + 1);
  const seen = new Set();
  const groups = [];
  for (const p of S.pages) {
    if (p.flow) continue;
    const items = [];
    for (const r of pageModel(p).rows)
      for (const f of r.frames) {
        const k = shootKey(f);
        if (seen.has(k)) continue;
        seen.add(k);
        items.push({ id: k, label: f.system ? TEXT.label.designSystem : f.screen?.title || k, n: count.get(k) || 0 });
      }
    if (items.length) groups.push({ id: p.id, title: p.title, items });
  }
  return groups;
}

const shootSelection = () => allFrameSpecs().filter((f) => !shootPicked || shootPicked.includes(shootKey(f)));

function renderShootMenu() {
  const groups = shootGroups();
  const ids = groups.flatMap((g) => g.items.map((i) => i.id));
  if (shootPicked) shootPicked = shootPicked.filter((id) => ids.includes(id));
  const on = (/** @type {string} */ id) => !shootPicked || shootPicked.includes(id);
  const set = (/** @type {string[] | null} */ next) => {
    shootPicked = next && next.length === ids.length ? null : next;
    store.set('shoot1', shootPicked);
    renderShootMenu();
  };
  const wrap = $('#shoot-items');
  wrap.innerHTML = '';
  /**
   * @param {string} cls
   * @param {string} label
   * @param {boolean} checked
   * @param {boolean} mixed
   * @param {(checked: boolean) => void} onChange
   * @param {number} [n]
   */
  const box = (cls, label, checked, mixed, onChange, n) => {
    const l = document.createElement('label');
    l.className = cls;
    l.innerHTML = `<input type="checkbox" ${checked ? 'checked' : ''}> <span>${esc(label)}</span>${n != null ? `<span class="shoot-n">${n}</span>` : ''}`;
    const i = l.querySelector('input');
    i.indeterminate = mixed;
    i.onchange = (e) => onChange(/** @type {HTMLInputElement} */ (e.target).checked);
    wrap.append(l);
  };
  const nOn = ids.filter(on).length;
  box('ms-all', TEXT.allScreens, nOn === ids.length, nOn > 0 && nOn < ids.length, (c) => set(c ? null : []));
  for (const g of groups) {
    const gOn = g.items.filter((i) => on(i.id)).length;
    const gIds = g.items.map((i) => i.id);
    if (g.items.length > 1 || g.items[0].label !== g.title)
      box('shoot-page', g.title, gOn === g.items.length, gOn > 0 && gOn < g.items.length, (c) =>
        set(c ? [...new Set([...ids.filter(on), ...gIds])] : ids.filter((id) => on(id) && !gIds.includes(id))),
      );
    for (const it of g.items)
      box(
        g.items.length > 1 || it.label !== g.title ? 'shoot-screen' : 'shoot-page',
        it.label,
        on(it.id),
        false,
        (c) => set(c ? [...ids.filter(on), it.id] : ids.filter((id) => on(id) && id !== it.id)),
        it.n,
      );
  }
  const frames = shootSelection().length;
  $('#shoot-count').textContent = TEXT.shootCount(frames);
  $('#shoot-go').textContent = TEXT.shootButton(frames);
  $('#shoot-go').disabled = !frames;
}

/**
 * Screenshot frame specs ({src, w, h, system, screen, variant, bp, state}) to PNG; one shot is also downloaded.
 * @param {any[]} list
 * @param {HTMLButtonElement} btn disabled while the server works
 */
async function shootFrames(list, btn) {
  if (btn.disabled) return;
  if (!list.length) return toast(TEXT.noScreenToShoot);
  btn.disabled = true;
  toast(TEXT.shooting(list.length));
  try {
    const res = await server.shoot({ frames: list.map((f) => ({ src: f.src, w: f.w, h: f.h, name: shotName(f, S.theme) })) });
    const ok = res.filter((/** @type {any} */ r) => r.ok);
    if (ok.length === 1) {
      const a = document.createElement('a');
      a.href = ok[0].url;
      a.download = ok[0].name;
      a.click();
    }
    const dir = ok[0]?.file.split('/').slice(0, -1).join('/');
    toast(TEXT.shot(ok.length, res.length, dir, ok.length < res.length ? res.find((/** @type {any} */ r) => !r.ok).error : null));
  } catch (e) {
    // A Studio server started before an update serves the new page but not the new API.
    toast(e.message === 'not found' ? TEXT.shootNeedsNewServer : TEXT.shootFailed(e.message));
  } finally {
    btn.disabled = false;
  }
}

/** The camera menu and the presenter's Screenshot button. */
export function initShoot() {
  // Toolbar camera: a menu to pick which screens to screenshot (all by default), grouped by page.
  $('#shoot-menu').addEventListener('toggle', () => $('#shoot-menu').open && renderShootMenu());
  $('#shoot-go').onclick = async () => {
    const list = shootSelection();
    await shootFrames(list, $('#shoot-go'));
    if (list.length) $('#shoot-menu').open = false;
  };
  // Present: the screen as shown (breakpoint, state, theme), full height.
  $('#p-shoot').onclick = () =>
    shootFrames(
      [
        {
          src: presentSrc(),
          w: P.system ? 1440 : P.bp.width,
          h: P.system ? 900 : P.bp.height,
          system: P.system,
          screen: P.screen,
          variant: P.variant,
          file: P.file,
          bp: P.bp,
          state: P.state,
        },
      ],
      $('#p-shoot'),
    );
}
