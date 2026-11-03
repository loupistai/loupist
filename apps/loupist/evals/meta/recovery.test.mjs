import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { retryMalformed, judgeWithRecovery } from './judging.mjs';
import { pairedComparison, acceptance, score } from './core.mjs';
import { assertStageSignature, assertReusableTraces, acquireRunLock } from './resume.mjs';
import { sha256 } from './surface.mjs';
import { suite as originalSuite, traces as originalTraces } from '../agent/fixtures/sample.mjs';
import { parseArgs } from './optimize.mjs';

const assets = { 'SYSTEM.md': 'prompt\n', 'tool-catalog.json': '{}\n', 'context-policy.json': '{"reserve_tokens":16384,"keep_recent_tokens":20000}\n' };
function fixture() {
  const suite = structuredClone(originalSuite);
  // Group the fixture's two synthetic turns into one coherent test run.
  const traces = structuredClone(originalTraces).filter(row => row.trial === 0);
  for (const trace of traces) trace.run_id = 'synthetic-recovery-trial';
  return { suite, traces };
}
test('format recovery retries at most twice with a reminder, then stays unknown', async () => {
  const prompts = [], events = [];
  const result = await retryMalformed('Original criterion', {}, { backend: { async judge(prompt) { prompts.push(prompt); throw new Error('Judge result is not JSON'); } }, record: e => events.push(e) });
  assert.equal(prompts.length, 3); assert.equal(prompts[0], 'Original criterion');
  assert.ok(prompts.slice(1).every(p => p.includes('Respond with JSON only')));
  assert.equal(result.pass, null); assert.equal(result.judge_error, true);
  assert.equal(events.filter(e => e.status === 'started').length, 3);
});
test('known malformed attempt gets only its two remaining retries and provider errors are not retried', async () => {
  let calls = 0;
  const result = await retryMalformed('criterion', {}, { priorAttempts: 1, backend: { async judge() { calls++; throw new Error('Judge result is not JSON'); } } });
  assert.equal(calls, 2); assert.equal(result.pass, null);
  calls = 0;
  await assert.rejects(retryMalformed('criterion', {}, { backend: { async judge() { calls++; throw new Error('quota exhausted'); } } }), /quota/u);
  assert.equal(calls, 1);
});
test('a successful format retry preserves the actual binary judgment', async () => {
  let calls = 0;
  const result = await retryMalformed('criterion', {}, { backend: { async judge() { if (++calls === 1) throw new Error('Judge result is not JSON'); return { pass: false, reason: 'Observed answer contradicts the supplied fact' }; } } });
  assert.equal(calls, 2); assert.equal(result.pass, false); assert.equal(result.judge_error, undefined);
});
test('paired acceptance excludes a judge-error scenario symmetrically', () => {
  const baseline = { complete: true, total: 3, passed: 1, scenarios: { a: true, b: false, c: false } };
  const candidate = { complete: true, total: 3, passed: 1, scenarios: { a: null, b: true, c: false } };
  const paired = pairedComparison(baseline, candidate);
  assert.deepEqual(paired.excluded, ['a']); assert.equal(paired.delta, 1);
  const smoke = { 'principles-v1': { complete: true, passed: 1, total: 1, checks: 1, checks_passed: 1, deterministic_failures: [] },
    'security-v1': { complete: true, passed: 1, total: 1, checks: 1, checks_passed: 1, deterministic_failures: [] } };
  assert.equal(acceptance(baseline, candidate, smoke, smoke).accepted, true);
  const noGain = { ...candidate, scenarios: { a: null, b: false, c: false } };
  assert.equal(acceptance(baseline, noGain, smoke, smoke).accepted, false);
});
test('stage reuse rejects changed assets or settings', () => {
  const stage = { assets: 'original', model: 'model', thinking: 'medium', trials: 2 };
  assert.doesNotThrow(() => assertStageSignature(stage, { ...stage }));
  assert.throws(() => assertStageSignature(stage, { ...stage, assets: 'changed' }), /differs/u);
  assert.throws(() => assertStageSignature(stage, { ...stage, trials: 1 }), /differs/u);
  assert.equal(parseArgs(['--resume', '/tmp/existing']).out, '/tmp/existing');
});
test('reusable traces must prove complete execution, model, prompt, catalog and policy identity', () => {
  const { suite, traces } = fixture();
  for (const trace of traces) {
    trace.observed_model = 'openai-codex/fixture-model';
    trace.harness = { ...trace.harness, agent_assets_override: true, agent_snapshot_sha256: 'a'.repeat(64),
      system_prompt_sha256: sha256(assets['SYSTEM.md']), tool_catalog_sha256: sha256(assets['tool-catalog.json']), context_policy: JSON.parse(assets['context-policy.json']) };
  }
  const args = { suite, traces, trials: 1, assets, model: 'openai-codex/fixture-model', thinking: 'low' };
  assert.equal(assertReusableTraces(args), 'a'.repeat(64));
  assert.throws(() => assertReusableTraces({ ...args, traces: traces.slice(1) }), /incomplete/u);
  assert.throws(() => assertReusableTraces({ ...args, model: 'different' }), /settings/u);
  assert.throws(() => assertReusableTraces({ ...args, assets: { ...assets, 'SYSTEM.md': 'changed' } }), /settings/u);
});
test('recovery uses existing cache without calls and does not cache an error as a boolean', async () => {
  const root = await mkdtemp(join(tmpdir(), 'meta-recovery-'));
  try {
    const { suite, traces } = fixture(); let calls = 0;
    const args = { suite, traces, cacheDir: join(root, 'cache'), directory: root, concurrency: 3,
      backend: { async judge() { calls++; return { pass: true, reason: 'Observed evidence satisfies the criterion' }; } } };
    const first = await judgeWithRecovery(args);
    assert.ok(calls > 0); assert.ok(first.judgments.every(row => row.pass === true));
    const previous = calls;
    const second = await judgeWithRecovery(args);
    assert.equal(calls, previous); assert.equal(second.usage.new_calls, 0);
    await rm(join(root, 'cache'), { recursive: true });
    const bad = await judgeWithRecovery({ ...args, backend: { async judge() { throw new Error('Judge result is not JSON'); } } });
    assert.ok(bad.judgments.every(row => row.pass === null && row.judge_error));
    assert.equal((await readdir(join(root, 'cache'))).length, 0);
    const scored = score(suite, traces, bad.judgments, 1);
    assert.equal(scored.scenarios['L-001'], null); assert.equal(scored.eligible_total, 0);
    assert.throws(() => score(suite, traces, bad.judgments.map((row, index) => index ? row : { ...row, item_id: 'fabricated' }), 1), /Invalid judge-error/u);
    assert.throws(() => score(suite, traces, [...bad.judgments, bad.judgments[0]], 1), /Invalid judge-error/u);
    assert.match(await readFile(join(root, 'judge-errors.jsonl'), 'utf8'), /judge_error/u);
  } finally { await rm(root, { recursive: true, force: true }); }
});
test('one run cannot start twice while the first invocation owns its lock', async () => {
  const root = await mkdtemp(join(tmpdir(), 'meta-lock-'));
  try {
    const out = join(root, 'run'), release = await acquireRunLock(out);
    await assert.rejects(acquireRunLock(out), /Run is locked/u);
    await release();
    const nextRelease = await acquireRunLock(out);
    await nextRelease();
  } finally { await rm(root, { recursive: true, force: true }); }
});
