/** Verify saved execution evidence before reusing any paid work. */
import { readFile, writeFile, mkdir, copyFile, readdir, open, unlink } from 'node:fs/promises';
import { join, resolve, dirname, isAbsolute } from 'node:path';
import { measuredTurns } from '../agent/measurement.mjs';
import { sha256, surfaceHash } from './surface.mjs';

export const readJson = async path => JSON.parse(await readFile(path, 'utf8'));
export async function acquireRunLock(out) {
  const path = resolve(out) + '.lock';
  await mkdir(dirname(path), { recursive: true });
  let handle;
  try { handle = await open(path, 'wx', 0o600); }
  catch (error) {
    if (error.code === 'EEXIST') throw new Error(`Run is locked: ${path}; verify its owner has exited before removing a stale lock`);
    throw error;
  }
  await handle.writeFile(JSON.stringify({ pid: process.pid, started_at: new Date().toISOString() }) + '\n');
  return async () => { await handle.close(); await unlink(path); };
}
export async function optionalJson(path) { try { return await readJson(path); } catch (error) { if (error.code === 'ENOENT') return null; throw error; } }
export async function suiteHash(suite, path) {
  const contents = { 'SYSTEM.md': JSON.stringify(suite, null, 2) + '\n' };
  for (const scenario of suite.scenarios) if (scenario.replay) {
    if (isAbsolute(scenario.replay) || scenario.replay.split(/[\\/]/u).some(p => !p || p === '.' || p === '..') || /held[-_]?out/iu.test(scenario.replay)) throw new Error('Unsafe replay path');
    contents[scenario.replay] = sha256(await readFile(resolve(dirname(path), scenario.replay)));
  }
  return surfaceHash(contents);
}
export function assertReusableTraces({ suite, traces, trials, assets, model, thinking }) {
  const { coverage } = measuredTurns(suite, traces, trials);
  if (!traces.length || coverage.completed_turns !== coverage.expected_turns || coverage.failed_turns
    || Object.values(coverage).some(value => Array.isArray(value) && value.length)) throw new Error('Cannot resume incomplete or mismatched agent execution');
  const system = sha256(assets['SYSTEM.md']), catalog = sha256(assets['tool-catalog.json']);
  const policy = JSON.parse(assets['context-policy.json']);
  const snapshots = new Set();
  for (const trace of traces) {
    const h = trace.harness;
    if (trace.observed_model !== model || trace.model?.thinking !== thinking || !h?.agent_assets_override
      || h.system_prompt_sha256 !== system || h.tool_catalog_sha256 !== catalog
      || !/^[a-f0-9]{64}$/u.test(h.agent_snapshot_sha256 ?? '')
      || Object.keys(policy).some(key => h.context_policy?.[key] !== policy[key])) throw new Error('Saved trace asset/model/context settings do not match resume');
    snapshots.add(h.agent_snapshot_sha256);
  }
  if (snapshots.size !== 1) throw new Error('Saved traces mix different asset bundles');
  return [...snapshots][0];
}
export function assertStageSignature(before, current) {
  if (JSON.stringify(before) !== JSON.stringify(current)) throw new Error('Saved stage differs in assets, suite, scenarios, trials, model, thinking or runner');
}
export async function preserveAttempt(out) {
  const previous = await readJson(join(out, 'run.json'));
  if (previous.status !== 'failed') throw new Error('Resume requires a failed run receipt');
  const root = join(out, 'attempts'); await mkdir(root, { recursive: true });
  const numbers = (await readdir(root)).map(Number).filter(Number.isSafeInteger);
  const directory = join(root, String(Math.max(0, ...numbers) + 1)); await mkdir(directory);
  await copyFile(join(out, 'run.json'), join(directory, 'run.json'));
  await copyFile(join(out, 'report.md'), join(directory, 'report.md'));
  return previous;
}
export async function writeJson(path, value) { await writeFile(path, JSON.stringify(value, null, 2) + '\n', { mode: 0o600 }); }
