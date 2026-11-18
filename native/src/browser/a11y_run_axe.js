async function runAxe() {
  const r = await window.axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice'] }, resultTypes: ['violations'] });
  return r.violations.map((v) => ({ id: v.id, impact: v.impact, help: v.help, nodes: v.nodes.length, target: String(v.nodes[0]?.target?.[0] || '').slice(0, 80), summary: String(v.nodes[0]?.failureSummary || '').split('\n').slice(1, 2).join(' ').trim().slice(0, 160) }));
}
