#!/usr/bin/env node
// dfw CLI entry point. Plugin bin/ directories are put on the Bash PATH by Claude Code.
import module from 'node:module';

const major = Number(process.versions.node.split('.')[0]);
if (major < 20) {
  console.error(`dfw needs Node.js 20 or newer (found ${process.versions.node}).`);
  process.exit(1);
}

// Hooks start a fresh process on every tool call: reuse compiled code between runs (Node 22.1+).
try {
  module.enableCompileCache?.();
} catch {}

const { main } = await import('../lib/cli.mjs');
main(process.argv.slice(2)).catch((err) => {
  console.error(err.exitCode ? err.message : err.stack || String(err));
  process.exit(err.exitCode || 1);
});
