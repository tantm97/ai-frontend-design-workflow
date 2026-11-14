// Pixel diff of screenshots ("dfw diff"): pixelmatch (Mapbox, ISC, vendored in assets/vendor) run inside the
// headless browser, which also decodes and encodes the PNGs, so no image library is needed.
// Uses: what changed between two approved design versions (after "dfw reopen"), and where an
// implementation screenshot departs from its design at G4. A number for triage, not a pass/fail:
// real data and font rendering always differ a little.
import fs from 'node:fs';
import path from 'node:path';
import { PLUGIN_ROOT, DfwError } from './util.mjs';
import { openRenderer } from './runtime.mjs';

// Runs in the page: decode both PNGs, pad to the larger size on white, diff, encode the diff image.
async function diffInPage({ a, b, threshold }) {
  const load = (src) => new Promise((ok, no) => {
    const img = new Image();
    img.onload = () => ok(img);
    img.onerror = () => no(new Error('cannot decode image'));
    img.src = src;
  });
  const [ia, ib] = await Promise.all([load(a), load(b)]);
  const w = Math.max(ia.naturalWidth, ib.naturalWidth);
  const h = Math.max(ia.naturalHeight, ib.naturalHeight);
  const pixels = (img) => {
    const c = new OffscreenCanvas(w, h);
    const x = c.getContext('2d');
    x.fillStyle = '#fff';
    x.fillRect(0, 0, w, h);
    x.drawImage(img, 0, 0);
    return x.getImageData(0, 0, w, h);
  };
  const da = pixels(ia);
  const db = pixels(ib);
  const out = new ImageData(w, h);
  const diff = pixelmatch(da.data, db.data, out.data, w, h, { threshold });
  const c = document.createElement('canvas');
  c.width = w;
  c.height = h;
  c.getContext('2d').putImageData(out, 0, 0);
  return { diff, w, h, sizeA: [ia.naturalWidth, ia.naturalHeight], sizeB: [ib.naturalWidth, ib.naturalHeight], png: c.toDataURL('image/png') };
}

const dataUrl = (f) => `data:image/png;base64,${fs.readFileSync(f).toString('base64')}`;

/** pairs: [{ a, b, out? }] (PNG paths). Returns [{ a, b, out, ratio, diff, w, h, sizeChanged, error }]. */
export async function diffImages(pairs, { threshold = 0.1 } = {}) {
  const lib = fs.readFileSync(path.join(PLUGIN_ROOT, 'assets', 'vendor', 'pixelmatch.js'), 'utf8');
  const r = await openRenderer();
  const results = [];
  try {
    const page = await r.page({ width: 800, height: 600 });
    await page.send('Runtime.evaluate', { expression: `${lib};window.pixelmatch = pixelmatch;` });
    for (const p of pairs) {
      try {
        const d = await page.evaluate(diffInPage, { a: dataUrl(p.a), b: dataUrl(p.b), threshold });
        if (p.out) {
          fs.mkdirSync(path.dirname(p.out), { recursive: true });
          fs.writeFileSync(p.out, Buffer.from(d.png.split(',')[1], 'base64'));
        }
        results.push({ ...p, diff: d.diff, w: d.w, h: d.h, ratio: d.diff / (d.w * d.h), sizeChanged: d.sizeA.join('x') !== d.sizeB.join('x') ? `${d.sizeA.join('x')} -> ${d.sizeB.join('x')}` : null });
      } catch (e) {
        results.push({ ...p, error: e.message });
      }
    }
  } finally {
    await r.close();
  }
  return results;
}

/** Pairs for "dfw diff <a> <b> [--out]": two files, or two folders matched by file name. */
export function diffPairs(a, b, out) {
  const isDir = (x) => fs.existsSync(x) && fs.statSync(x).isDirectory();
  if (isDir(a) && isDir(b)) {
    const names = fs.readdirSync(a).filter((f) => f.endsWith('.png') && fs.existsSync(path.join(b, f)));
    if (!names.length) throw new DfwError(`No PNG file names in common between ${a} and ${b}.`);
    return names.map((f) => ({ a: path.join(a, f), b: path.join(b, f), out: path.join(out || path.join(b, '_diff'), f) }));
  }
  for (const f of [a, b]) if (!fs.existsSync(f)) throw new DfwError(`Not found: ${f}`);
  return [{ a, b, out: out || b.replace(/(\.png)?$/i, '.diff.png') }];
}
