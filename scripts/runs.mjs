#!/usr/bin/env node
import { readdir, open } from 'node:fs/promises';
import { constants } from 'node:fs';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';

const MAX_DAY_BYTES = 20 * 1024 * 1024;
const usage = 'Usage: runs.mjs tail|summary (--loupist <folder> | --app <app-data-root>) [--kind chat|world|learning|briefing|scheduled|connection_sign_in] [--failed] [--since 24h]';
function options(args) {
  const [command, ...rest] = args;
  if (!['tail', 'summary'].includes(command)) throw new Error(usage);
  const parsed = { command, since: '24h' };
  for (let i = 0; i < rest.length; i++) {
    const key = rest[i];
    if (key === '--failed') { parsed.failed = true; continue; }
    if (!['--loupist', '--app', '--kind', '--since'].includes(key) || !rest[i + 1] || rest[i + 1].startsWith('--')) throw new Error(usage);
    parsed[key.slice(2)] = rest[++i];
  }
  if (Boolean(parsed.loupist) === Boolean(parsed.app)) throw new Error(usage);
  if (parsed.kind && !['chat', 'world', 'learning', 'briefing', 'scheduled', 'connection_sign_in'].includes(parsed.kind)) throw new Error('Unknown run kind');
  if (!/^\d+(?:m|h|d)$/.test(parsed.since)) throw new Error('--since must be a duration such as 30m, 24h, or 7d');
  return parsed;
}
export async function readRuns(directory) {
  let names;
  try { names = await readdir(directory, { withFileTypes: true }); }
  catch (error) { if (error.code === 'ENOENT') return { runs: [], skipped: 0 }; throw error; }
  const runs = []; let skipped = 0;
  for (const entry of names.filter(e => e.isFile() && /^runs-\d{8}\.jsonl$/.test(e.name)).sort((a, b) => a.name.localeCompare(b.name))) {
    const handle = await open(join(directory, entry.name), constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    let text;
    try {
      const stat = await handle.stat();
      if (!stat.isFile() || stat.size > MAX_DAY_BYTES) throw new Error('Run log exceeds the daily size limit');
      text = await handle.readFile('utf8');
    } finally { await handle.close(); }
    for (const line of text.split('\n')) {
      if (!line.trim()) continue;
      try {
        const run = JSON.parse(line);
        if (typeof run.kind !== 'string' || typeof run.runId !== 'string' || !Number.isFinite(run.startedAtMs) || !Number.isFinite(run.durationMs) || !['ok', 'failed', 'cancelled', 'deferred'].includes(run.status)) throw new Error('invalid record');
        runs.push(run);
      } catch { skipped++; }
    }
  }
  return { runs: runs.sort((a, b) => a.startedAtMs - b.startedAtMs), skipped };
}
function percentile(values, fraction) { return values.length ? values[Math.max(0, Math.ceil(values.length * fraction) - 1)] : null; }
export function summarize(runs) {
  const counts = {}; const failures = {}; const duration = [];
  const tokens = { input: 0, cachedInput: 0, output: 0, knownRuns: 0, unknownRuns: 0 };
  for (const run of runs) {
    counts[run.kind] = (counts[run.kind] ?? 0) + 1;
    if (run.status === 'failed') failures[run.kind] = (failures[run.kind] ?? 0) + 1;
    duration.push(run.durationMs);
    if (run.usage && ['input', 'cachedInput', 'output'].every(k => Number.isFinite(run.usage[k]))) {
      for (const key of ['input', 'cachedInput', 'output']) tokens[key] += run.usage[key];
      tokens.knownRuns++;
    } else tokens.unknownRuns++;
  }
  duration.sort((a, b) => a - b);
  return { total: runs.length, counts, failures, p50Ms: percentile(duration, 0.5), p95Ms: percentile(duration, 0.95), usage: tokens };
}
const safe = value => String(value ?? 'unknown').replace(/[\u0000-\u001f\u007f-\u009f]/g, ' ').slice(0, 256);
export async function main(args) {
  const opt = options(args);
  const directory = opt.loupist ? join(resolve(opt.loupist), '.loupist', 'logs') : join(resolve(opt.app), 'logs');
  const loaded = await readRuns(directory);
  let runs = loaded.runs.filter(run => (!opt.kind || run.kind === opt.kind) && (!opt.failed || run.status === 'failed'));
  if (loaded.skipped) process.stderr.write(`Skipped ${loaded.skipped} malformed or incomplete log lines.\n`);
  if (opt.command === 'tail') {
    if (!runs.length) return 'No matching runs.\n';
    return runs.slice(-20).map(run => `${new Date(run.startedAtMs).toISOString()} ${safe(run.kind)} ${safe(run.status)} ${run.durationMs}ms run=${safe(run.runId)} model=${safe(run.model)} effort=${safe(run.effort)} usage=${run.usage ? `in:${run.usage.input} cached:${run.usage.cachedInput} out:${run.usage.output}` : 'unknown'}${run.failureKind ? ` failure=${safe(run.failureKind)}` : ''}`).join('\n') + '\n';
  }
  const units = { m: 60_000, h: 3_600_000, d: 86_400_000 };
  const since = Date.now() - Number(opt.since.slice(0, -1)) * units[opt.since.at(-1)];
  runs = runs.filter(run => run.startedAtMs + run.durationMs >= since);
  const summary = summarize(runs);
  // Scheduled/briefing records describe the same execution as their Chat record.
  // Usage totals count the Chat record once, including a scheduled record only
  // when its corresponding Chat entry is not in this selection.
  const chats = new Set(runs.filter(r => r.kind === 'chat').map(r => `${r.loupistId}:${r.runId}`));
  summary.usage = summarize(runs.filter(r => !['scheduled', 'briefing'].includes(r.kind) || !chats.has(`${r.loupistId}:${r.runId}`))).usage;
  return JSON.stringify(summary, null, 2) + '\n';
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).then(text => process.stdout.write(text), error => { process.stderr.write(`${error.message}\n`); process.exitCode = 1; });
}
