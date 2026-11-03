import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile, mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { DEFAULT_DATASET, validateDataset, planRuns, taskRequest, scoreNumerical, summarize, quantile,
  comparisonConfigHash, readTraceAttempts, validateTrace, main } from './evaluate.mjs';
const datasetBytes = await readFile(DEFAULT_DATASET, 'utf8');
const dataset = validateDataset(JSON.parse(datasetBytes));
const datasetHash = createHash('sha256').update(datasetBytes).digest('hex');
const configuration = { effort: 'high', budget: { max_output_tokens: null, timeout_ms: 900000 }, data_access: { fixture: 'supplied', external: 'disabled' } };
const trace = (overrides = {}) => {
  const value = { run_id: 'run-a', task_id: 'D01', family: 'data', arm: 'app', product: null, condition: 'memory_off', repeat: 1, phase: 'first_use',
    started_at: '2026-10-03T00:00:00Z', dataset_hash: datasetHash, model: 'model-a', ...configuration, status: 'completed', duration_ms: 1000,
    answer: '```json\n{"metrics":{"total_revenue":300,"company_count":2}}\n```', usage: null, cost_usd: null, ...overrides };
  return { ...value, comparison_config_hash: overrides.comparison_config_hash ?? comparisonConfigHash(value) };
};
const review = (runId = 'run-a', scores = {}) => ({ run_id: runId, reviewer: 'independent-human', reviewed_at: '2026-10-03T01:00:00Z',
  scores: { method: 2, provenance: 2, uncertainty: 2, task_completion: 2, ...scores } });
const productTrace = (product = 'codex', overrides = {}) => trace({ run_id: product, arm: 'product', product,
  product_evidence: { trace_ref: `/actual/local/capture/${product}.json`, captured_at: '2026-10-03T00:01:00Z' }, ...overrides });

async function temporary(t) {
  const directory = await mkdtemp(join(tmpdir(), 'finance-eval-test-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  return directory;
}

test('48 facts remain a 576-cell default matrix; revisit repeats the same cells without changing the oracle', () => {
  assert.equal(planRuns(dataset).length, 48 * 4 * 3);
  assert.equal(planRuns(dataset, { baseline: true }).length, 12 * 4 * 3);
  assert.equal(new Set(planRuns(dataset).map((cell) => JSON.stringify(cell))).size, 576);
  const both = planRuns(dataset, { phases: ['revisit', 'first_use'] });
  assert.equal(both.length, 1152);
  assert.deepEqual(both.slice(0, 2).map((cell) => [cell.task_id, cell.repeat, cell.phase]), [['D01', 1, 'first_use'], ['D01', 1, 'revisit']]);
  const request = taskRequest(dataset.cases[0], both[0], datasetHash);
  assert.equal('expected' in request, false);
  assert.equal('review' in request, false);
  assert.ok(request.prompt.includes('total_revenue, company_count'));
  assert.equal(request.prompt.includes('"total_revenue":300'), false);
  const revisit = taskRequest(dataset.cases[0], both[1], datasetHash, trace({ answer: 'Prior measured incomplete result' }));
  assert.equal(revisit.prior_run_id, 'run-a');
  assert.ok(revisit.prompt.includes('prior run run-a'));
  assert.ok(revisit.prompt.includes('Prior measured incomplete result'));
  assert.equal(revisit.prompt.includes('"total_revenue":300'), false);
  assert.deepEqual(revisit.facts, request.facts);
  assert.throws(() => taskRequest(dataset.cases[0], both[1], datasetHash), /completed first-use/u);
  assert.throws(() => planRuns(dataset, { phases: ['invented'] }), /phases/u);
});

test('numerical accuracy measures actual structured values and rejects non-finite/coerced values', () => {
  const task = dataset.cases[0];
  assert.equal(scoreNumerical(task, trace().answer).numerical_accuracy, 1);
  assert.equal(scoreNumerical(task, '{"metrics":{"total_revenue":600,"company_count":2}}').numerical_accuracy, .5);
  assert.equal(scoreNumerical(task, '{"metrics":{"total_revenue":"300","company_count":null}}').numerical_accuracy, 0);
  assert.equal(scoreNumerical(task, 'No answer').numerical_accuracy, 0);
});

test('reports never synthesize quality, success, or cost from missing usage/reviews', () => {
  const result = summarize(dataset, [trace()]);
  assert.equal(result.summaries[0].numerical_accuracy, 1);
  assert.equal(result.summaries[0].quality, null);
  assert.equal(result.summaries[0].successful_tasks, null);
  assert.equal(result.summaries[0].cost_usd, null);
  assert.equal(result.summaries[0].cost_per_successful_task, null);
  assert.equal(result.summaries[0].p50_ms, 1000);
  assert.equal(result.summaries[0].p95_ms, 1000);
  assert.equal(summarize(dataset, []).measured_runs, 0);
});

test('all attempts contribute cost/time while latest completed result alone determines quality', () => {
  const failed = trace({ run_id: 'failed', status: 'failed', cost_usd: .25, duration_ms: 100 });
  const complete = trace({ run_id: 'complete', started_at: '2026-10-03T00:01:00Z', cost_usd: .5, duration_ms: 200 });
  const laterFailure = trace({ run_id: 'later-failure', started_at: '2026-10-03T00:02:00Z', status: 'cancelled', cost_usd: .25, duration_ms: 300 });
  const baseline = trace({ run_id: 'baseline', arm: 'same_model', cost_usd: .25, duration_ms: 150 });
  const report = summarize(dataset, [laterFailure, baseline, failed, complete], [review('complete'), review('baseline')]);
  const group = report.summaries.find((entry) => entry.arm === 'app');
  assert.equal(report.measured_runs, 4);
  assert.equal(report.measured_cells, 2);
  assert.equal(group.attempted, 3);
  assert.equal(group.tasks, 1);
  assert.equal(group.cost_usd, 1);
  assert.equal(group.all_attempts_duration_ms, 600);
  assert.equal(group.successful_tasks, 1);
  assert.equal(group.cost_per_successful_task, 1);
  assert.equal(group.quality, 1);
  assert.equal(report.results.find((entry) => entry.run_id === 'complete').selected_for_quality, true);
  assert.equal(report.paired[0].run_id, 'complete');
  assert.equal(report.paired[0].cost_delta_usd, .75);
  assert.equal(report.paired[0].latency_delta_ms, 450);
  assert.equal(report.paired[0].selected_completion_latency_delta_ms, 50);
});

test('selection uses newest completed timestamp, not highest score, and success requires every dimension', () => {
  const later = trace({ run_id: 'later', started_at: '2026-10-03T00:01:00Z', cost_usd: 1,
    answer: '{"metrics":{"total_revenue":600,"company_count":2}}' });
  const report = summarize(dataset, [later, trace({ cost_usd: 1 })], [review(), review('later')]);
  assert.equal(report.summaries[0].numerical_accuracy, .5);
  assert.equal(report.summaries[0].successful_tasks, 0);
  assert.equal(report.summaries[0].cost_per_successful_task, null);
  assert.equal(report.results.find((entry) => entry.run_id === 'later').selected_for_quality, true);
  const partial = summarize(dataset, [trace({ cost_usd: 1 })], [review('run-a', { provenance: 1 })]);
  assert.equal(partial.summaries[0].quality, .9);
  assert.equal(partial.summaries[0].successful_tasks, 0);
  assert.equal(partial.summaries[0].cost_per_successful_task, null);
  assert.deepEqual(report.success_criteria, { numerical_accuracy: 1, minimum_review_score: 2, review_dimensions: ['method', 'provenance', 'uncertainty', 'task_completion'] });
});

test('an interrupted or unpriced attempt prevents an apparent free retry', () => {
  const pending = trace({ run_id: 'interrupted', status: 'interrupted', duration_ms: null, cost_usd: null });
  const complete = trace({ run_id: 'done', started_at: '2026-10-03T00:01:00Z', cost_usd: 1 });
  const group = summarize(dataset, [pending, complete], [review('done')]).summaries[0];
  assert.equal(group.successful_tasks, 1);
  assert.equal(group.known_cost_usd, 1);
  assert.equal(group.cost_known_runs, 1);
  assert.equal(group.duration_known_runs, 1);
  assert.equal(group.cost_usd, null);
  assert.equal(group.all_attempts_duration_ms, null);
  assert.equal(group.cost_per_successful_task, null);
  assert.equal(summarize(dataset, [trace({ cost_usd: 1 })]).summaries[0].cost_per_successful_task, null);
});

test('pairing requires identical actual model, effort, budget, data access, task, repeat, and fixture', () => {
  const base = trace({ run_id: 'base', arm: 'same_model', duration_ms: 700 });
  assert.equal(summarize(dataset, [trace(), base]).paired[0].latency_delta_ms, 300);
  for (const changed of [{ model: 'other' }, { effort: 'low' }, { budget: { ...configuration.budget, timeout_ms: 5000 } },
    { data_access: { ...configuration.data_access, external: 'enabled' } }, { repeat: 2 }, { task_id: 'D02' }, { dataset_hash: 'other' }]) {
    assert.equal(summarize(dataset, [trace(), trace({ ...base, ...changed, comparison_config_hash: undefined })]).paired.length, 0, JSON.stringify(changed));
  }
  assert.equal(comparisonConfigHash(configuration), comparisonConfigHash({ effort: 'high', data_access: { external: 'disabled', fixture: 'supplied' }, budget: { timeout_ms: 900000, max_output_tokens: null } }));
  assert.throws(() => summarize(dataset, [trace({ comparison_config_hash: 'forged' })]), /config hash/u);
});

test('revisit pairs only the same phase and requires exact first-use lineage', () => {
  const prior = trace();
  const revisit = trace({ run_id: 'revisit', phase: 'revisit', prior_run_id: prior.run_id, started_at: '2026-10-03T00:01:00Z' });
  const base = trace({ run_id: 'base', arm: 'same_model' });
  const report = summarize(dataset, [prior, revisit, base]);
  assert.equal(report.paired.length, 1);
  assert.equal(report.paired[0].phase, 'first_use');
  assert.equal(report.summaries.length, 3);
  assert.throws(() => summarize(dataset, [revisit]), /matching completed first-use/u);
  assert.throws(() => summarize(dataset, [prior, trace({ ...revisit, repeat: 2 })]), /matching completed first-use/u);
  assert.throws(() => summarize(dataset, [prior, trace({ ...revisit, started_at: '2026-10-02T00:00:00Z' })]), /matching completed first-use/u);
  assert.throws(() => summarize(dataset, [trace({ duration_ms: 120000 }), revisit]), /ended before/u);
});

test('learned Memory requires real disjoint authoring provenance and paired controls', () => {
  assert.throws(() => summarize(dataset, [trace({ condition: 'learned_lens' })]), /provenance/u);
  assert.throws(() => summarize(dataset, [trace({ condition: 'learned_lens', condition_evidence: { learned_lens: { authoring_run_id: 'training-run', training_ids: ['D01'] } } })]), /held-out/u);
  const learned = trace({ run_id: 'learned', condition: 'learned_lens', condition_evidence: { learned_lens: { authoring_run_id: 'training-run', training_ids: ['train-separate-001'] } } });
  assert.equal(summarize(dataset, [trace(), learned]).memory_pairs.length, 1);
});

test('reviews are complete and attempt IDs unique; duplicate logical cells are retained', () => {
  assert.throws(() => summarize(dataset, [trace()], [{ ...review(), scores: {} }]), /Incomplete/u);
  assert.throws(() => summarize(dataset, [trace(), trace()]), /Duplicate run_id/u);
  assert.equal(summarize(dataset, [trace(), trace({ run_id: 'retry' })]).measured_runs, 2);
  assert.throws(() => summarize(dataset, [trace()], [review('unknown')]), /unknown run_id/u);
  assert.equal(quantile([1, 2, 3, 4], .95), 4);
});

test('actual product captures remain separate and match only identical comparison configurations', async (t) => {
  const report = summarize(dataset, [trace(), productTrace('codex'), productTrace('claude')]);
  assert.equal(report.product_pairs.length, 2);
  assert.equal(report.paired.length, 0);
  assert.deepEqual(report.product_pairs.map((pair) => pair.control_product), ['codex', 'claude']);
  assert.throws(() => validateTrace(productTrace('codex', { product_evidence: null }), dataset), /capture evidence/u);
  assert.equal(planRuns(dataset, { arm: 'product', product: 'codex' }).length, 576);
  const directory = await temporary(t);
  const source = join(directory, 'captures.jsonl');
  const output = join(directory, 'imported.jsonl');
  await writeFile(source, `${JSON.stringify(productTrace())}\n`);
  const imported = await main(['import', '--traces', source, '--output', output]);
  assert.equal(imported[0].run_id, 'codex');
  assert.equal(JSON.parse((await readFile(output, 'utf8')).trim()).arm, 'product');
  await assert.rejects(main(['run', '--arm', 'product', '--product', 'codex', '--adapter', 'not-called', '--config', 'not-read', '--traces', source]), /external/u);
});

test('pending journal finalization deduplicates the same attempt but retains unfinished attempts', async (t) => {
  const directory = await temporary(t);
  const path = join(directory, 'traces.jsonl');
  const complete = trace({ cost_usd: 1 });
  const pending = trace({ status: 'interrupted', duration_ms: null });
  const orphan = trace({ ...pending, run_id: 'orphan' });
  await writeFile(path, `${JSON.stringify(complete)}\n`);
  await writeFile(`${path}.pending.jsonl`, [pending, orphan].map(JSON.stringify).join('\n') + '\n');
  const attempts = await readTraceAttempts(path);
  assert.equal(attempts.length, 2);
  assert.equal(attempts.find((attempt) => attempt.run_id === 'run-a').status, 'completed');
  assert.equal(attempts.find((attempt) => attempt.run_id === 'orphan').status, 'interrupted');
  const report = await main(['report', '--traces', path, '--output', join(directory, 'report.json')]);
  assert.equal(report.summaries[0].cost_usd, null);
});

test('runner preserves explicit retry attempts and real prior output without invoking any provider', async (t) => {
  const directory = await temporary(t);
  const adapter = join(directory, 'local-test-adapter.mjs');
  const config = join(directory, 'config.json');
  const counter = join(directory, 'counter.json');
  const path = join(directory, 'attempts.jsonl');
  await writeFile(config, JSON.stringify({ model: 'model-a', ...configuration, counter }));
  await writeFile(adapter, `import {readFile,writeFile} from 'node:fs/promises';
export async function open(config) { return { async run(request) {
  let rows=[]; try { rows=JSON.parse(await readFile(config.counter,'utf8')); } catch {}
  rows.push(request); await writeFile(config.counter, JSON.stringify(rows));
  return {status:rows.length===1?'failed':'completed',answer:'{"metrics":{"total_revenue":300,"company_count":2}}',observed_model:config.model,cost_usd:.25,usage:null};
}}; }`);
  const args = ['run', '--adapter', adapter, '--config', config, '--traces', path, '--ids', 'D01', '--conditions', 'memory_off', '--repeats', '1', '--phases', 'first_use,revisit'];
  await main(args);
  assert.equal((await readTraceAttempts(path)).length, 1);
  await assert.rejects(main(args), /explicit retry/u);
  await main([...args, '--retry', 'true']);
  const attempts = await readTraceAttempts(path);
  assert.equal(attempts.length, 3);
  assert.deepEqual(attempts.map((attempt) => [attempt.phase, attempt.status]), [['first_use', 'failed'], ['first_use', 'completed'], ['revisit', 'completed']]);
  assert.equal(attempts[2].prior_run_id, attempts[1].run_id);
  const calls = JSON.parse(await readFile(counter, 'utf8'));
  assert.equal(calls.length, 3);
  assert.equal(calls[2].reuse.answer, attempts[1].answer);
  assert.equal(calls[2].reuse.prior_run_id, attempts[1].run_id);
  assert.equal('expected' in calls[2], false);
  await main(args);
  assert.equal(JSON.parse(await readFile(counter, 'utf8')).length, 3);
  const report = summarize(dataset, attempts, [review(attempts[1].run_id), review(attempts[2].run_id)]);
  assert.equal(report.summaries.find((entry) => entry.phase === 'first_use').cost_per_successful_task, .5);
  assert.equal(report.summaries.find((entry) => entry.phase === 'revisit').cost_per_successful_task, .25);
});

test('resuming a different fixture refuses before importing an adapter', async (t) => {
  const directory = await temporary(t);
  const config = join(directory, 'config.json');
  const path = join(directory, 'traces.jsonl');
  await writeFile(config, JSON.stringify({ model: 'model-a', ...configuration }));
  await writeFile(path, `${JSON.stringify(trace({ dataset_hash: 'different-fixture' }))}\n`);
  await assert.rejects(main(['run', '--adapter', join(directory, 'must-not-import.mjs'), '--config', config,
    '--traces', path, '--conditions', 'memory_off', '--ids', 'D01']), /Existing trace dataset hash/u);
});

test('runner retains unverified-model response costs but excludes it from completed quality pairs', async (t) => {
  const directory = await temporary(t);
  const config = join(directory, 'config.json');
  const adapter = join(directory, 'unverified-local-test.mjs');
  const path = join(directory, 'traces.jsonl');
  await writeFile(config, JSON.stringify({ model: 'model-a', ...configuration }));
  await writeFile(adapter, `export async function open() { return {run:async()=>({status:'completed',answer:'unverified answer',cost_usd:1,usage:null})}; }`);
  await main(['run', '--adapter', adapter, '--config', config, '--traces', path, '--conditions', 'memory_off', '--ids', 'D01', '--repeats', '1']);
  const attempts = await readTraceAttempts(path);
  assert.equal(attempts[0].status, 'failed');
  assert.equal(attempts[0].model_identity_verified, false);
  assert.equal(attempts[0].cost_usd, 1);
  assert.equal(summarize(dataset, [...attempts, trace({ run_id: 'baseline', arm: 'same_model' })]).paired.length, 0);
});
