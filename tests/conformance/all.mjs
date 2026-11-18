// Entry for `node --test tests/conformance/`: Node resolves a directory argument through
// package.json "main", so this file loads every *.conformance.mjs next to it.
import fs from 'node:fs';

for (const file of fs.readdirSync(import.meta.dirname).sort()) {
  if (file.endsWith('.conformance.mjs')) await import(`./${file}`);
}
