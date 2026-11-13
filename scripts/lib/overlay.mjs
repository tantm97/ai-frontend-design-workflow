// Declarative overlays applied to vendored upstream files at build time.
// Every op must apply; a patch that no longer matches upstream fails the build.
import fs from 'node:fs';
import path from 'node:path';
import { setKey, dropKeys } from './frontmatter.mjs';

export class OverlayError extends Error {}

export function loadOverlay(repoRoot, id) {
  const file = path.join(repoRoot, 'overlays', `${id}.json`);
  if (!fs.existsSync(file)) return { ops: [] };
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

export function applyOverlay(pluginDir, overlay, repoRoot) {
  const log = [];
  for (const [i, op] of (overlay.ops || []).entries()) {
    const where = `overlay op #${i + 1} (${op.op} ${op.file || ''})`;
    if (!op.why) throw new OverlayError(`${where}: every op needs a "why".`);
    const file = op.file ? path.join(pluginDir, op.file) : null;
    const read = () => {
      if (!fs.existsSync(file)) throw new OverlayError(`${where}: file not found in upstream copy. Review the patch.`);
      return fs.readFileSync(file, 'utf8');
    };
    switch (op.op) {
      case 'description': {
        fs.writeFileSync(file, setKey(read(), 'description', op.value));
        break;
      }
      case 'dropKeys': {
        const r = dropKeys(read(), op.keys);
        if (r.dropped.length !== op.keys.length) {
          throw new OverlayError(`${where}: keys not present upstream (${op.keys.filter((k) => !r.dropped.includes(k)).join(', ')}). Upstream may have changed; review the patch.`);
        }
        fs.writeFileSync(file, r.text);
        break;
      }
      case 'replace': {
        const re = new RegExp(op.pattern, op.flags ?? 'g');
        const text = read();
        const count = (text.match(new RegExp(op.pattern, (op.flags || '').includes('g') ? op.flags : `${op.flags || ''}g`)) || []).length;
        if (count < (op.min ?? 1)) throw new OverlayError(`${where}: pattern /${op.pattern}/ matched ${count} time(s), expected >= ${op.min ?? 1}. Review the patch.`);
        fs.writeFileSync(file, text.replace(re, op.with));
        break;
      }
      case 'delete': {
        for (const p of op.paths) {
          const abs = path.join(pluginDir, p);
          if (!fs.existsSync(abs)) throw new OverlayError(`${where}: ${p} does not exist upstream.`);
          fs.rmSync(abs, { recursive: true });
        }
        break;
      }
      case 'write': {
        const content = op.from ? fs.readFileSync(path.join(repoRoot, op.from), 'utf8') : op.content;
        fs.mkdirSync(path.dirname(file), { recursive: true });
        fs.writeFileSync(file, content);
        break;
      }
      default:
        throw new OverlayError(`${where}: unknown op.`);
    }
    log.push({ op: op.op, file: op.file || op.paths?.join(','), why: op.why });
  }
  return log;
}
