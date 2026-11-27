#!/usr/bin/env node
// A C compiler and archiver that build nothing, for the Windows clippy of scripts/check.mjs on a
// macOS or Linux host. Clippy never links, so the C code of a dependency (ring) needs only the files
// its build script expects to find: the compiler's `-o <file>` and the archiver's `<ops> <archive>`
// are written empty. Anything else (the compiler family probe `-E <file>`) writes nothing.
import fs from 'node:fs';
import { pathToFileURL } from 'node:url';

const OUTPUT_FLAG = '-o';
const FLAG_PREFIX = '-';

/** The file a compiler (`... -o <file> ...`) or an archiver (`<ops> <archive> <objects...>`) call writes. */
export function outputOf(args) {
  const at = args.indexOf(OUTPUT_FLAG);
  if (at >= 0) return args[at + 1];
  return args.length > 1 && !args[0].startsWith(FLAG_PREFIX) ? args[1] : undefined;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const out = outputOf(process.argv.slice(2));
  if (out) fs.writeFileSync(out, '');
}
