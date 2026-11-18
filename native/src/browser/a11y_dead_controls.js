async function deadControls() {
  const sel = 'button[aria-pressed], button[aria-expanded], [role="tab"][aria-selected], [role="switch"], th[aria-sort] button, button[aria-sort]';
  const out = [];
  for (const el of [...document.querySelectorAll(sel)].slice(0, 12)) {
    if (el.closest('form') || el.disabled || !el.getClientRects().length) continue;
    let changed = 0;
    const mo = new MutationObserver((m) => (changed += m.length));
    mo.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    el.click();
    await new Promise((r) => setTimeout(r, 150));
    mo.disconnect();
    if (!changed) out.push((el.getAttribute('aria-label') || el.textContent || el.tagName).trim().slice(0, 60));
  }
  return out;
}
