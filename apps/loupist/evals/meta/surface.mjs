/** Strict data-only patch application: never invokes git, a shell, or candidate code. */
import { createHash } from 'node:crypto';
import { mkdir, readdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';

export const DEFAULT_CONTEXT = { reserve_tokens: 16384, keep_recent_tokens: 20000 };
export const sha256 = value => createHash('sha256').update(value).digest('hex');
export function editable(path) {
  return ['SYSTEM.md', 'tool-catalog.json', 'context-policy.json'].includes(path)
    || /^skills\/[a-z0-9-]+\/SKILL\.md$/u.test(path);
}
export function lockedRegions(text) {
  const regions = [], marker = /<!-- (\/?safety:locked) -->/gu;
  let start = null;
  for (const match of text.matchAll(marker)) {
    if (match[1] === 'safety:locked') {
      if (start !== null) throw new Error('Nested safety lock');
      start = match.index;
    } else {
      if (start === null) throw new Error('Unmatched safety lock');
      regions.push(text.slice(start, match.index + match[0].length)); start = null;
    }
  }
  if (start !== null) throw new Error('Unclosed safety lock');
  return regions;
}
function frontmatter(text) {
  const match = /^---\n([\s\S]*?)\n---\n/u.exec(text);
  if (!match) throw new Error('Skill frontmatter is required');
  return match[0];
}
function maskDescriptions(value) {
  if (Array.isArray(value)) return value.map(maskDescriptions);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, child]) =>
    [key, key === 'description' && typeof child === 'string' ? '' : maskDescriptions(child)]));
  return value;
}
export function validateContext(value) {
  if (!value || Object.keys(value).sort().join(',') !== 'keep_recent_tokens,reserve_tokens') throw new Error('Only the two compaction parameters are editable');
  for (const [key, defaultValue] of Object.entries(DEFAULT_CONTEXT)) {
    if (!Number.isSafeInteger(value[key]) || value[key] < defaultValue / 2 || value[key] > defaultValue * 1.5) throw new Error(`Context policy ${key} exceeds ±50% of default`);
  }
}
export function validateChange(path, before, after) {
  if (!editable(path)) throw new Error(`Out-of-surface path: ${path}`);
  if (typeof before !== 'string' || typeof after !== 'string' || after.includes('\0') || Buffer.byteLength(after) > 1024 * 1024) throw new Error('Invalid asset text');
  if (path === 'SYSTEM.md') {
    const locks = lockedRegions(before);
    if (!locks.length || JSON.stringify(locks) !== JSON.stringify(lockedRegions(after))) throw new Error('Locked safety region changed or missing');
  } else if (path === 'tool-catalog.json') {
    if (JSON.stringify(maskDescriptions(JSON.parse(before))) !== JSON.stringify(maskDescriptions(JSON.parse(after)))) throw new Error('Only catalog description strings are editable');
  } else if (path === 'context-policy.json') validateContext(JSON.parse(after));
  else if (frontmatter(before) !== frontmatter(after)) throw new Error('Skill frontmatter is immutable');

}

export async function readSurface(root) {
  const assets = {};
  async function visit(dir, prefix = '') {
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      const path = prefix + entry.name;
      if (entry.isSymbolicLink()) throw new Error('Symlink in editable surface');
      if (entry.isDirectory()) await visit(join(dir, entry.name), `${path}/`);
      else if (entry.isFile() && editable(path)) assets[path] = await readFile(join(dir, entry.name), 'utf8');
      else throw new Error(`Unexpected file in editable surface: ${path}`);
    }
  }
  await visit(root);
  return assets;
}
export async function writeSurface(root, assets) {
  for (const [path, text] of Object.entries(assets)) {
    if (!editable(path)) throw new Error(`Out-of-surface path: ${path}`);
    const target = join(root, path);
    await mkdir(dirname(target), { recursive: true });
    await writeFile(target, text, { mode: 0o600 });
  }
}
export function surfaceHash(assets) {
  return sha256(JSON.stringify(Object.entries(assets).sort(([a], [b]) => a.localeCompare(b))));
}

/** A strict subset of unified diff: existing regular text files, exact context, no fuzz. */
export function applyPatch(assets, patch) {
  if (Buffer.byteLength(patch) > 2 * 1024 * 1024) throw new Error('Patch is too large');
  const lines = patch.replace(/^```(?:diff|patch)?\n/u, '').replace(/\n```\s*$/u, '').split('\n');
  const candidate = { ...assets }, touched = new Set();
  let i = 0;
  while (i < lines.length) {
    if (lines[i] === '' && i === lines.length - 1) break;
    if (lines[i].startsWith('diff --git ')) i++;
    if (/^index [a-f0-9]+\.\.[a-f0-9]+(?: 100644)?$/u.test(lines[i] ?? '')) i++;
    const old = /^--- a\/(.+)$/u.exec(lines[i++] ?? '');
    const next = /^\+\+\+ b\/(.+)$/u.exec(lines[i++] ?? '');
    if (!old || !next || old[1] !== next[1] || !editable(old[1]) || !Object.hasOwn(assets, old[1]) || touched.has(old[1])) throw new Error('Invalid, duplicate, new, or out-of-surface patch path');
    const path = old[1]; touched.add(path);
    const source = assets[path].split('\n');
    if (source.at(-1) !== '') throw new Error('Assets must end with a newline');
    source.pop();
    const output = []; let cursor = 0, hunks = 0;
    while (i < lines.length && lines[i].startsWith('@@')) {
      const hunk = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(?:.*)$/u.exec(lines[i++]);
      if (!hunk) throw new Error('Invalid hunk header');
      const oldCount = Number(hunk[2] ?? 1), newCount = Number(hunk[4] ?? 1);
      const start = Number(hunk[1]) - (oldCount === 0 ? 0 : 1);
      if (start < cursor || start > source.length) throw new Error('Overlapping or invalid hunk');
      output.push(...source.slice(cursor, start)); cursor = start;
      if (Number(hunk[3]) - (newCount === 0 ? 0 : 1) !== output.length) throw new Error('Invalid new hunk offset');
      let removed = 0, added = 0;
      while (removed < oldCount || added < newCount) {
        const line = lines[i++], kind = line?.[0], body = line?.slice(1);
        if (![' ', '-', '+'].includes(kind)) throw new Error('Invalid unified diff body');
        if (kind !== '+') {
          if (source[cursor++] !== body) throw new Error('Patch context mismatch');
          removed++;
        }
        if (kind !== '-') { output.push(body); added++; }
        if (removed > oldCount || added > newCount) throw new Error('Invalid hunk counts');
      }
      hunks++;
    }
    if (!hunks) throw new Error('Patch has no hunks');
    output.push(...source.slice(cursor));
    candidate[path] = output.join('\n') + '\n';
    validateChange(path, assets[path], candidate[path]);
  }
  if (!touched.size || surfaceHash(candidate) === surfaceHash(assets)) throw new Error('Empty candidate diff');
  return candidate;
}

// Whole-file hunks keep reports portable without running git in the shared checkout.
export function diffSurface(before, after) {
  return Object.keys(before).sort().filter(path => before[path] !== after[path]).map(path => {
    const old = before[path].slice(0, -1).split('\n'), next = after[path].slice(0, -1).split('\n');
    return `--- a/${path}\n+++ b/${path}\n@@ -1,${old.length} +1,${next.length} @@\n${old.map(s => '-' + s).join('\n')}\n${next.map(s => '+' + s).join('\n')}\n`;
  }).join('');
}
