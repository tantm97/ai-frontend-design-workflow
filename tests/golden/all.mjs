// Entry for `node --test tests/golden/`: Node resolves a directory argument through package.json
// "main", so this file loads every *.golden.mjs next to it.
import fs from 'node:fs';

for (const file of fs.readdirSync(import.meta.dirname).sort()) {
  if (file.endsWith('.golden.mjs')) await import(`./${file}`);
}
