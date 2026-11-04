import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { main, readRuns } from './runs.mjs';

test('fixture tail filters failures and summary counts percentiles and known usage once', async () => {
  const root = await mkdtemp(join(tmpdir(), 'loupist-runs-'));
  try {
    const dir = join(root, '.loupist/logs'); await mkdir(dir, { recursive: true });
    const base = { loupistId: 'loupist-fixture', runId: 'run-1', startedAtMs: Date.now() - 10_000, model: 'fixture/model', effort: 'medium', tools: {}, refusedTools: [], memoryChanges: {}, notes: [] };
    const records = [
      { ...base, kind: 'chat', status: 'ok', durationMs: 100, usage: { input: 10, cachedInput: 5, output: 2 } },
      { ...base, kind: 'briefing', status: 'ok', durationMs: 120, usage: { input: 10, cachedInput: 5, output: 2 } },
      { ...base, runId: 'run-2', kind: 'world', status: 'failed', failureKind: 'world_worker_failed', durationMs: 900, usage: null },
      { ...base, runId: 'run-old', startedAtMs: Date.now() - 2 * 86400000, kind: 'chat', status: 'ok', durationMs: 10, usage: null },
    ];
    await writeFile(join(dir, 'runs-20261029.jsonl'), records.map(r => JSON.stringify(r)).join('\n') + '\n{"incomplete":');
    assert.equal((await readRuns(dir)).skipped, 1);
    const tail = execFileSync(process.execPath, [fileURLToPath(new URL('./runs.mjs', import.meta.url)), 'tail', '--loupist', root, '--kind', 'world', '--failed'], { encoding: 'utf8' });
    assert.match(tail, /world failed 900ms/); assert.match(tail, /failure=world_worker_failed/); assert.doesNotMatch(tail, /run-1/);
    const summary = JSON.parse(await main(['summary', '--loupist', root, '--since', '24h']));
    assert.deepEqual(summary.counts, { chat: 1, briefing: 1, world: 1 });
    assert.deepEqual(summary.failures, { world: 1 });
    assert.equal(summary.p50Ms, 120); assert.equal(summary.p95Ms, 900);
    assert.deepEqual(summary.usage, { input: 10, cachedInput: 5, output: 2, knownRuns: 1, unknownRuns: 1 });
    await mkdir(join(root, 'logs'));
    await writeFile(join(root, 'logs/runs-20261029.jsonl'), JSON.stringify({ ...base, loupistId: null, kind: 'connection_sign_in', status: 'ok', durationMs: 20, usage: null }) + '\n');
    assert.match(await main(['tail', '--app', root]), /connection_sign_in ok/);
    await assert.rejects(main(['tail', '--loupist', root, '--app', root]), /Usage:/);
  } finally { await rm(root, { recursive: true, force: true }); }
});
