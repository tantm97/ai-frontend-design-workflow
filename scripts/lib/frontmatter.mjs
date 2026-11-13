// Line-based SKILL.md frontmatter editing. Deliberately conservative: it only
// touches top-level keys and fails loudly when the structure is unexpected.

export function split(text) {
  if (!text.startsWith('---\n') && !text.startsWith('---\r\n')) return { has: false, lines: [], body: text };
  const lines = text.split('\n');
  const end = lines.indexOf('---', 1) === -1 ? lines.findIndex((l, i) => i > 0 && l.trim() === '---') : lines.indexOf('---', 1);
  if (end < 0) throw new Error('Unterminated frontmatter');
  return { has: true, lines: lines.slice(1, end), body: lines.slice(end + 1).join('\n') };
}

function join({ lines, body }) {
  return ['---', ...lines, '---', body].join('\n');
}

/** Index range [start, end) of a top-level key including its continuation lines. */
function keyRange(lines, key) {
  const start = lines.findIndex((l) => new RegExp(`^${key.replace(/[-]/g, '\\-')}\\s*:`).test(l));
  if (start < 0) return null;
  let end = start + 1;
  while (end < lines.length && (/^\s+\S/.test(lines[end]) || lines[end].trim() === '')) {
    if (lines[end].trim() === '' && !/^\s/.test(lines[end + 1] || '')) break;
    end++;
  }
  return [start, end];
}

export function getKey(text, key) {
  const fm = split(text);
  if (!fm.has) return undefined;
  const r = keyRange(fm.lines, key);
  if (!r) return undefined;
  const raw = fm.lines.slice(r[0], r[1]).join('\n').replace(new RegExp(`^${key}\\s*:\\s*`), '');
  const v = raw.trim();
  if (/^[>|][-+]?$/.test(v.split('\n')[0])) return v.split('\n').slice(1).map((l) => l.trim()).join(' ');
  if (v.startsWith('"')) {
    try {
      return JSON.parse(v);
    } catch {
      return v.slice(1, -1);
    }
  }
  if (v.startsWith("'")) return v.slice(1, -1).replace(/''/g, "'");
  return v.replace(/\n\s+/g, ' ');
}

export function setKey(text, key, value) {
  const fm = split(text);
  if (!fm.has) throw new Error(`No frontmatter; cannot set ${key}`);
  const line = `${key}: ${JSON.stringify(value)}`;
  const r = keyRange(fm.lines, key);
  if (r) fm.lines.splice(r[0], r[1] - r[0], line);
  else {
    const nameIdx = fm.lines.findIndex((l) => /^name\s*:/.test(l));
    fm.lines.splice(nameIdx + 1, 0, line);
  }
  return join(fm);
}

export function dropKeys(text, keys) {
  const fm = split(text);
  if (!fm.has) throw new Error('No frontmatter');
  const dropped = [];
  for (const k of keys) {
    const r = keyRange(fm.lines, k);
    if (r) {
      fm.lines.splice(r[0], r[1] - r[0]);
      dropped.push(k);
    }
  }
  return { text: join(fm), dropped };
}

export function topLevelKeys(text) {
  const fm = split(text);
  if (!fm.has) return [];
  return fm.lines.filter((l) => /^[A-Za-z_][\w-]*\s*:/.test(l)).map((l) => l.split(':')[0].trim());
}
