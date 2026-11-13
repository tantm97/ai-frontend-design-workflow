// Studio pages: groups manifest screens into canvas pages (Figma-style).
// - "Design System" (id "system") always comes first.
// - Declared manifest.pages [{id,title,description,notes,sections,screens}] plus screen.page assignments.
// - Unassigned screens go to "Screens"; ids starting with "_" go to "Explorations".

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
  return [{ id: 'system', title: 'Design System', system: true, screens: [], ...systemExtra }, ...pages];
}
