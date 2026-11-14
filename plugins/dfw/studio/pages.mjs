// Studio pages: groups manifest screens into canvas pages (Figma-style).
// - "Design System" (id "system") always comes first.
// - Declared manifest.pages [{id,title,description,notes,sections,screens}] plus screen.page assignments.
// - Unassigned screens go to "Screens"; ids starting with "_" go to "Explorations".

/**
 * Screen-to-screen links found in the design files: data-dfw-goto="screen[/variant]" and href to another
 * design file. files: Map(design-relative file -> html). Returns [{ from, to, label }] (screen ids).
 */
export function screenFlows(manifest, files) {
  const screens = (manifest.screens || []).filter((s) => !s.archived);
  const byFile = new Map();
  for (const s of screens) for (const v of s.variants || []) byFile.set(v.file, s.id);
  const ids = new Set(screens.map((s) => s.id));
  const edges = new Map();
  for (const s of screens) {
    for (const v of s.variants || []) {
      const html = files.get(v.file);
      if (!html) continue;
      const dir = v.file.split('/').slice(0, -1);
      for (const m of html.matchAll(/<(a|button)\b([^>]*)>([\s\S]*?)<\/\1>/gi)) {
        const goto = /data-dfw-goto="([^"]+)"/.exec(m[2])?.[1];
        const href = /\bhref="([^"#?]+\.html)[^"]*"/.exec(m[2])?.[1];
        let to = goto ? goto.split('/')[0] : null;
        if (!to && href && !/^[a-z]+:|^\//i.test(href)) {
          const parts = [...dir];
          for (const seg of href.split('/')) seg === '..' ? parts.pop() : seg !== '.' && parts.push(seg);
          to = byFile.get(parts.join('/'));
        }
        if (!to || to === s.id || !ids.has(to)) continue;
        const label = m[3].replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 40);
        const key = `${s.id}>${to}`;
        if (!edges.has(key)) edges.set(key, { from: s.id, to, label });
      }
    }
  }
  return [...edges.values()];
}

export function studioPages(manifest) {
  const screens = (manifest.screens || []).filter((s) => !s.archived);
  const byId = new Map(screens.map((s) => [s.id, s]));
  const pages = [];
  const index = new Map();
  const ensure = (id, title) => {
    if (!index.has(id)) {
      const p = { id, title: title || id, screens: [] };
      index.set(id, p);
      pages.push(p);
    }
    return index.get(id);
  };
  const assigned = new Set();
  let systemExtra = {};
  const add = (page, sid) => {
    if (!byId.has(sid) || page.screens.includes(sid)) return;
    page.screens.push(sid);
    assigned.add(sid);
  };
  for (const p of manifest.pages || []) {
    if (!p || !p.id) continue;
    if (p.id === 'system') {
      systemExtra = { description: p.description, notes: p.notes };
      continue;
    }
    const page = ensure(p.id, p.title);
    if (p.description) page.description = p.description;
    if (p.notes) page.notes = p.notes;
    if (p.sections) page.sections = p.sections;
    for (const sid of p.screens || []) add(page, sid);
    // Screens referenced only from sections ("id" or "id/variant") belong to the page too.
    for (const sec of p.sections || []) for (const ref of sec.screens || []) add(page, String(ref).split('/')[0]);
  }
  for (const s of screens) if (s.page && s.page !== 'system') add(ensure(s.page, s.page), s.id);
  const rest = screens.filter((s) => !assigned.has(s.id));
  const main = rest.filter((s) => !s.id.startsWith('_'));
  const explore = rest.filter((s) => s.id.startsWith('_'));
  if (main.length || !pages.length) for (const s of main) add(ensure('screens', 'Screens'), s.id);
  if (!pages.length && !main.length && !explore.length) ensure('screens', 'Screens');
  for (const s of explore) add(ensure('explorations', 'Explorations'), s.id);
  const flowScreens = [...new Set((manifest.flows || []).flatMap((e) => [e.from, e.to]))].filter((id) => byId.has(id));
  const flow = flowScreens.length ? [{ id: 'flow', title: 'Flow', flow: true, description: 'How screens link to each other (buttons and links with data-dfw-goto or href). Arrows carry the control text.', screens: flowScreens }] : [];
  return [{ id: 'system', title: 'Design System', system: true, screens: [], ...systemExtra }, ...flow, ...pages];
}
