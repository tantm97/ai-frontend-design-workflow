// Communication language. Policy (user decision): ask on the first /dfw run of
// EVERY session; offer the last-used language as the default; English if skipped.
// Technical artifacts are always English regardless of this setting.
import fs from 'node:fs';
import path from 'node:path';
import { globalDir, readJson, writeJson, nowIso } from './util.mjs';

const SESSION_TTL_DAYS = 30;

export function sessionId(explicit) {
  return explicit || process.env.DFW_SESSION_ID || process.env.CLAUDE_SESSION_ID || null;
}

function sessionFile(id) {
  return path.join(globalDir(), 'sessions', `${id.replace(/[^\w.-]/g, '_')}.json`);
}

export function lastUsedLanguage() {
  return readJson(path.join(globalDir(), 'config.json'), {}).language?.lastUsed || 'en';
}

export function languageStatus(explicitSession) {
  const id = sessionId(explicitSession);
  const last = lastUsedLanguage();
  if (!id) return { session: null, chosen: false, language: null, lastUsed: last };
  const s = readJson(sessionFile(id), null);
  return { session: id, chosen: Boolean(s?.language), language: s?.language || null, lastUsed: last };
}

export function setLanguage(code, explicitSession) {
  const lang = String(code || 'en').trim().toLowerCase() || 'en';
  const id = sessionId(explicitSession);
  if (id) writeJson(sessionFile(id), { language: lang, setAt: nowIso() });
  const cfgFile = path.join(globalDir(), 'config.json');
  const cfg = readJson(cfgFile, {});
  cfg.language = { ...(cfg.language || {}), lastUsed: lang };
  writeJson(cfgFile, cfg);
  return { session: id, language: lang };
}

export function pruneSessions() {
  const dir = path.join(globalDir(), 'sessions');
  if (!fs.existsSync(dir)) return;
  const cutoff = Date.now() - SESSION_TTL_DAYS * 86400e3;
  for (const f of fs.readdirSync(dir)) {
    const p = path.join(dir, f);
    try {
      if (fs.statSync(p).mtimeMs < cutoff) fs.unlinkSync(p);
    } catch {}
  }
}
