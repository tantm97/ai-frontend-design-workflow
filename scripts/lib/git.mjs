// Fetch upstream repositories at an exact commit (lockfile is the build input).
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

const git = (args, opts = {}) => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], ...opts }).trim();

export function repoUrl(repo) {
  return /^https?:|^git@|^\//.test(repo) ? repo : `https://github.com/${repo}.git`;
}

export function resolveRef(repo, ref = 'HEAD') {
  const out = git(['ls-remote', repoUrl(repo), ref === 'HEAD' ? 'HEAD' : `refs/heads/${ref}`]);
  const sha = out.split(/\s+/)[0];
  if (!/^[0-9a-f]{40}$/.test(sha || '')) {
    const tag = git(['ls-remote', repoUrl(repo), `refs/tags/${ref}`]).split(/\s+/)[0];
    if (/^[0-9a-f]{40}$/.test(tag || '')) return tag;
    throw new Error(`Cannot resolve ${repo}@${ref}`);
  }
  return sha;
}

/**
 * Checkout of repo at sha, cached under cacheRoot. Returns the directory.
 * With `paths`, uses a blobless partial clone + sparse checkout so only the
 * vendored paths are downloaded (some upstream repos are hundreds of MB).
 */
export function fetchAt(repo, sha, cacheRoot, paths = null) {
  if (!/^[0-9a-f]{40}$/.test(sha)) throw new Error(`Lock entries must be full 40-char SHAs (got "${sha}" for ${repo}).`);
  const dir = path.join(cacheRoot, `${repo.replace(/[^\w.-]+/g, '__')}@${sha}`);
  const marker = path.join(dir, '.dfw-complete');
  const want = paths ? [...new Set(paths)].sort() : null;
  if (fs.existsSync(marker)) {
    let have = {};
    try {
      have = JSON.parse(fs.readFileSync(marker, 'utf8'));
    } catch {
      have = { paths: null }; // full checkout from an older cache format
    }
    if (!have.paths || (want && want.every((p) => have.paths.includes(p)))) return dir;
    // Widen the sparse checkout to the new union of paths.
    const union = [...new Set([...have.paths, ...want])].sort();
    git(['sparse-checkout', 'set', '--no-cone', ...union.map((p) => `/${p}`)], { cwd: dir, timeout: 600000 });
    fs.writeFileSync(marker, JSON.stringify({ sha, paths: union }));
    return dir;
  }
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  git(['init', '-q'], { cwd: dir });
  git(['remote', 'add', 'origin', repoUrl(repo)], { cwd: dir });
  if (want) {
    git(['config', 'extensions.partialClone', 'origin'], { cwd: dir });
    git(['sparse-checkout', 'set', '--no-cone', ...want.map((p) => `/${p}`)], { cwd: dir });
    git(['-c', 'protocol.version=2', 'fetch', '-q', '--depth', '1', '--filter=blob:none', 'origin', sha], { cwd: dir, timeout: 600000 });
  } else {
    git(['-c', 'protocol.version=2', 'fetch', '-q', '--depth', '1', 'origin', sha], { cwd: dir, timeout: 600000 });
  }
  git(['-c', 'advice.detachedHead=false', 'checkout', '-q', 'FETCH_HEAD'], { cwd: dir, timeout: 600000 });
  const head = git(['rev-parse', 'HEAD'], { cwd: dir });
  if (head !== sha) throw new Error(`Fetched ${head} but expected ${sha} for ${repo}`);
  fs.writeFileSync(marker, JSON.stringify({ sha, paths: want }));
  return dir;
}

export function commitCount(repo, from, to) {
  try {
    const out = execFileSync('gh', ['api', `repos/${repo}/compare/${from}...${to}`, '--jq', '.total_commits'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
    return Number(out.trim());
  } catch {
    return null;
  }
}
