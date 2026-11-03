import test from 'node:test';
import assert from 'node:assert/strict';
import { suite, traces as fixtureTraces } from './fixtures/sample.mjs';
const traces = fixtureTraces.map((trace) => ({ ...trace, status: 'completed', observed_model: `${trace.model.provider}/${trace.model.model}` }));
import { aggregateMetrics, buildReport, compareRuns, completionGoals, pairTraces, tokenTotals, wilsonInterval } from './report.mjs';
import { itemId, judgeItems } from './judge.mjs';

test('Wilson intervals, majority-of-3, and pass^k keep their denominators', () => {
  const interval = wilsonInterval(8, 10);
  assert.ok(Math.abs(interval.low - 0.49016247153664183) < 1e-12);
  assert.ok(Math.abs(interval.high - 0.9433178485456247) < 1e-12);
  assert.equal(wilsonInterval(0, 0), null);
  const checks = [];
  for (const [scenarioId, trials] of [['L-001', [true, true, false]], ['L-002', [true, false]], ['L-003', [false, false, false]]]) {
    trials.forEach((pass, trial) => checks.push({ scenario_id: scenarioId, trial, metric: 'task_success', pass }));
  }
  const metrics = aggregateMetrics(checks);
  assert.deepEqual(metrics.task_success.trials, {
    pass: 3, total: 8, rate: 3 / 8, wilson_95: wilsonInterval(3, 8),
  });
  assert.deepEqual(metrics.task_success.majority_of_3, { pass: 1, total: 3, rate: 1 / 3 });
  assert.deepEqual(metrics.task_success.pass_k, { k: 3, pass: 0, total: 3, rate: 0 });
  assert.equal(metrics.task_success.level, 'scenario');
  assert.equal(metrics.task_success.headline.rate, 1 / 3);
});

test('unknown latency and tokens stay null, including when any trace omits usage', () => {
  const report = buildReport({ suite, traces });
  assert.equal(report.latency_ms.p50, 1000);
  assert.equal(report.latency_ms.p95, 3000);
  assert.equal(report.latency_ms.unknown, 1);
  assert.equal(report.tokens.input, null);
  assert.equal(report.tokens.total, null);
  const zeros = tokenTotals([{ usage: { input: 0, output: 0, cache_read: 0, cache_write: 0 } }]);
  assert.equal(zeros.total, 0);
  assert.equal(tokenTotals([]).total, null);
  assert.equal(report.tool_validity.calls, 12);
  assert.equal(report.tool_validity.invalid, 3);
  assert.equal(report.tool_validity.rate, 0.25);
  assert.equal(report.tool_validity.repeated_failures, 1);
  assert.equal(report.goals.invalid_tool_calls.observed, 0.25);
  assert.equal(report.goals.invalid_tool_calls.pass, false);
  assert.equal(report.goals.repeated_failed_calls.observed, 1);
  assert.equal(report.goals.repeated_failed_calls.pass, false);
  assert.equal(report.goals.carry_forward.observed, 0);
  assert.equal(report.goals.carry_forward.pass, null);
  assert.equal(report.goals.negative_transfer.observed, 0);
  assert.equal(report.goals.negative_transfer.pass, null);
  assert.equal(report.goals.save_recall.pass, null);
  assert.equal(report.goals.save_precision.pass, null);
  assert.equal(report.goals.prediction_structure.pass, null);
});

test('comparison pairs matched turns and drops a mismatched model', () => {
  const baseline = traces.map((row) => ({ ...row, run_id: `base-${row.run_id}` }));
  const candidate = traces.map((row) => ({ ...row, run_id: `cand-${row.run_id}`, harness: { ...row.harness, git_commit: 'def' } }));
  candidate[1] = { ...candidate[1], model: { ...candidate[1].model, model: 'other-model' } };
  const pairing = pairTraces(baseline, candidate);
  assert.equal(pairing.pairs.length, 3);
  assert.equal(pairing.unpaired.length, 1);
  assert.equal(pairing.unpaired[0].reason, 'model_mismatch');
  assert.equal(pairing.unpaired[0].run_id, undefined);
  assert.equal(pairing.pairs.some((pair) => pair.baseline.trial === 0 && pair.baseline.session_id === 'probe-1'), false);
  const shifted = { ...candidate[2], model: { ...candidate[2].model, thinking: 'high' } };
  assert.equal(pairTraces([baseline[2]], [shifted]).unpaired[0].reason, 'settings_mismatch');
  const comparison = compareRuns({ suite, baseline, candidate });
  assert.equal(comparison.paired, 3);
  assert.equal(comparison.metrics.task_success.delta, 0);
  const headlineSuite = {
    schema: 'loupist-agent-suite/1', suite: 'learning', version: 1,
    scenarios: [{ id: 'L-010', sessions: [{ id: 'only', role: 'probe', memory: 'off', turns: [{ user: 'Say alpha.', expect: { answer: [{ assert: 'contains_any', values: ['alpha'], metric: 'task_success' }] } }] }] }],
  };
  const headlineRow = (side, trial, answer) => ({
    schema: 'loupist-agent-trace/1', run_id: `${side}-${trial}`, suite: 'learning', scenario_id: 'L-010', session_id: 'only',
    harness: { scenario_input_sha256: '1'.repeat(64) },
    turn_index: 0, trial, status: 'completed', observed_model: 'openai-codex/fixture-model', model: { provider: 'openai-codex', model: 'fixture-model', thinking: 'low' }, memory_mode: 'off', session_role: 'probe', user: 'Say alpha.', answer, tool_calls: [], memory_commits: [],
  });
  const headline = compareRuns({
    suite: headlineSuite,
    baseline: [0, 1, 2].map((trial) => headlineRow('b', trial, 'alpha')),
    candidate: [0, 1, 2].map((trial) => headlineRow('c', trial, trial === 2 ? 'beta' : 'alpha')),
  });
  assert.equal(headline.metrics.task_success.baseline_rate, 1);
  assert.equal(headline.metrics.task_success.candidate_rate, 1);
  assert.equal(headline.metrics.task_success.delta, 0);
  const mini = {
    schema: 'loupist-agent-suite/1', suite: 'learning', version: 1,
    scenarios: [{ id: 'L-009', sessions: [{ id: 'only', role: 'probe', memory: 'off', turns: [{ user: 'Say alpha.', expect: { answer: [{ assert: 'contains_any', values: ['alpha'], metric: 'task_success' }] } }] }] }],
  };
  const row = (trial, answer) => ({
    schema: 'loupist-agent-trace/1', run_id: `m-${trial}-${answer}`, suite: 'learning', scenario_id: 'L-009', session_id: 'only',
    harness: { scenario_input_sha256: '2'.repeat(64) },
    turn_index: 0, trial, status: 'completed', observed_model: 'openai-codex/fixture-model', model: { provider: 'openai-codex', model: 'fixture-model', thinking: 'low' }, memory_mode: 'off', session_role: 'probe', user: 'Say alpha.', answer, tool_calls: [], memory_commits: [],
  });
  const flips = compareRuns({
    suite: mini,
    baseline: [0, 1, 2].map((trial) => row(trial, 'alpha')),
    candidate: [0, 1, 2].map((trial) => row(trial, trial === 0 ? 'alpha' : 'beta')),
  }).flips;
  assert.deepEqual(flips, [{ scenario_id: 'L-009', metric: 'task_success', baseline_pass: true, candidate_pass: false }]);
});

function scenarioChecks(metric, passed, failed) {
  const checks = [];
  for (let scenario = 0; scenario < passed + failed; scenario += 1) {
    const scenarioPass = scenario < passed;
    for (let trial = 0; trial < 3; trial += 1) {
      checks.push({ scenario_id: `${metric}-${scenario}`, trial, metric, pass: scenarioPass ? trial < 2 : false });
    }
  }
  return checks;
}

function itemChecks(metric, passed, total) {
  return Array.from({ length: total }, (_, index) => ({ scenario_id: 'items', trial: 0, metric, pass: index < passed }));
}

test('completion goals use item checks, scenario majorities, and null when there is no data', () => {
  const metrics = aggregateMetrics([
    ...scenarioChecks('carry_forward', 4, 1),
    ...scenarioChecks('negative_transfer', 9, 1),
    ...scenarioChecks('prediction_structure', 9, 1),
    ...itemChecks('save_recall', 8, 10),
    ...itemChecks('save_precision', 7, 10),
    ...itemChecks('retrieval', 1, 1),
    ...itemChecks('efficiency', 0, 1),
  ]);
  assert.equal(metrics.save_recall.level, 'item');
  assert.equal(metrics.save_recall.headline.rate, 0.8);
  assert.equal(metrics.save_recall.trials.rate, 0);
  assert.equal(metrics.negative_transfer.level, 'scenario');
  assert.ok(Math.abs(metrics.negative_transfer.transfer_rate - 0.1) < 1e-12);
  const goals = completionGoals(metrics, { calls: 20, invalid: 1, rate: 0.05, repeated_failures: 0 });
  assert.equal(goals.carry_forward.pass, true);
  assert.equal(goals.carry_forward.observed, 0.8);
  assert.equal(goals.negative_transfer.target, '<=0.1');
  assert.ok(Math.abs(goals.negative_transfer.observed - 0.1) < 1e-12);
  assert.equal(goals.negative_transfer.pass, true);
  assert.equal(goals.save_recall.pass, true);
  assert.equal(goals.save_precision.pass, false);
  assert.equal(goals.prediction_structure.observed, 0.9);
  assert.equal(goals.prediction_structure.pass, true);
  assert.equal(goals.invalid_tool_calls.pass, true);
  assert.equal(goals.repeated_failed_calls.pass, true);
  assert.equal(metrics.retrieval.checks.rate, 1);
  assert.equal(metrics.efficiency.checks.rate, 0);
  assert.equal(Object.hasOwn(goals, 'retrieval'), false);
  assert.equal(Object.hasOwn(goals, 'efficiency'), false);
  const missed = completionGoals(aggregateMetrics([
    ...scenarioChecks('negative_transfer', 8, 2),
    ...scenarioChecks('prediction_structure', 8, 2),
    ...itemChecks('save_recall', 7, 10),
  ]), { calls: 20, invalid: 2, rate: 0.1, repeated_failures: 1 });
  assert.ok(Math.abs(missed.negative_transfer.observed - 0.2) < 1e-12);
  assert.equal(missed.negative_transfer.pass, false);
  assert.equal(missed.prediction_structure.pass, false);
  assert.equal(missed.save_recall.pass, false);
  assert.equal(missed.invalid_tool_calls.pass, false);
  assert.equal(missed.repeated_failed_calls.pass, false);
  const empty = completionGoals({}, { calls: 0, invalid: 0, rate: null, repeated_failures: 0 });
  for (const goal of Object.values(empty)) assert.equal(goal.pass, null);
  assert.equal(empty.invalid_tool_calls.observed, null);
  assert.equal(empty.repeated_failed_calls.observed, null);
  const blank = buildReport({ suite, traces: [] });
  assert.equal(blank.goals.carry_forward.observed, 0);
  assert.equal(blank.coverage.missing_turns.length, 6);
  assert.equal(blank.goals.invalid_tool_calls.pass, null);
  assert.equal(blank.goals.repeated_failed_calls.pass, null);
});

function measuredFixture() {
  const suite = { schema: 'loupist-agent-suite/1', suite: 'coverage', version: 1,
    scenarios: ['D-A', 'D-B'].map((id) => ({ id, sessions: [{ id: 'ask', role: 'probe', memory: 'off', turns: [{
      user: 'Say alpha.', expect: { answer: [{ assert: 'contains_any', values: ['alpha'], metric: 'prediction_structure' }] },
    }] }] })) };
  const traces = suite.scenarios.flatMap((scenario) => [0, 1, 2].map((trial) => ({
    schema: 'loupist-agent-trace/1', suite: suite.suite, run_id: 'one-run', scenario_id: scenario.id,
    harness: { scenario_input_sha256: '3'.repeat(64) },
    session_id: 'ask', session_role: 'probe', memory_mode: 'off', user: 'Say alpha.',
    turn_index: 0, trial, status: 'completed', answer: 'alpha', tool_calls: [], memory_commits: [],
  })));
  return { suite, traces };
}

test('a timeout and skipped trials remain in the suite denominator and cannot certify a goal', () => {
  const { suite, traces } = measuredFixture();
  traces[3] = { ...traces[3], status: 'failed', error: 'timeout', answer: '' };
  traces.splice(4);
  const report = buildReport({ suite, traces });
  assert.equal(report.coverage.expected_turns, 6);
  assert.equal(report.coverage.failed_turns, 1);
  assert.equal(report.coverage.missing_turns.length, 2);
  assert.equal(report.coverage.complete, false);
  assert.deepEqual(report.metrics.prediction_structure.headline, { pass: 1, total: 2, rate: 0.5 });
  assert.equal(report.goals.prediction_structure.pass, null);
  assert.equal(report.metrics.prediction_structure.unknown_checks, 2);

  // Even an otherwise passing majority cannot certify an unmeasured third trial.
  const partial = measuredFixture();
  partial.traces.pop();
  const mostlyPassing = buildReport(partial);
  assert.equal(mostlyPassing.metrics.prediction_structure.headline.rate, 1);
  assert.equal(mostlyPassing.goals.prediction_structure.pass, null);
  assert.equal(mostlyPassing.goals.prediction_structure.reason, 'incomplete_measurement');
});

test('complete executed trials can meet the threshold despite a recorded failed turn', () => {
  const fixture = measuredFixture();
  fixture.traces[0] = { ...fixture.traces[0], status: 'failed', answer: 'alpha' };
  const report = buildReport(fixture);
  assert.equal(report.coverage.complete, true);
  assert.equal(report.coverage.failed_turns, 1);
  assert.equal(report.metrics.prediction_structure.checks.pass, 5);
  assert.equal(report.goals.prediction_structure.pass, true);
  fixture.traces[0].status = 'aborted';
  const aborted = buildReport(fixture);
  assert.equal(aborted.coverage.failed_turns, 1);
  assert.equal(aborted.coverage.unknown_status_turns.length, 0);
  assert.equal(aborted.metrics.prediction_structure.checks.pass, 5);
});

test('k=1 remains a diagnostic measurement and cannot pass k=3 completion goals', () => {
  const fixture = measuredFixture();
  fixture.traces = fixture.traces.filter((row) => row.trial === 0);
  const report = buildReport({ ...fixture, trials: 1 });
  assert.equal(report.coverage.complete, true);
  assert.equal(report.metrics.prediction_structure.headline.rate, 1);
  assert.equal(report.metrics.prediction_structure.majority_of_3.rate, null);
  assert.equal(report.goals.prediction_structure.pass, null);
  assert.equal(report.goals.prediction_structure.reason, 'completion_requires_three_trials');
  for (const trials of [0, -1, 1.5, NaN]) assert.throws(() => buildReport({ ...fixture, trials }), /positive integer/u);
});

test('ambiguous attempts, mixed run histories, and absent judgments never manufacture success', () => {
  const duplicate = measuredFixture();
  duplicate.traces.push({ ...duplicate.traces[0], run_id: 'retry' });
  const repeated = buildReport(duplicate);
  assert.equal(repeated.coverage.duplicate_turns.length, 1);
  assert.notEqual(repeated.goals.prediction_structure.pass, true);

  const mixed = measuredFixture();
  mixed.suite.scenarios[0].sessions[0].turns.push({ user: 'Again.' });
  mixed.traces.push(...mixed.traces.filter((row) => row.scenario_id === 'D-A').map((row) => ({ ...row, turn_index: 1, user: 'Again.', run_id: 'different-loupist' })));
  const incoherent = buildReport(mixed);
  assert.equal(incoherent.coverage.incoherent_trials.length, 3);
  assert.notEqual(incoherent.goals.prediction_structure.pass, true);

  const unjudged = measuredFixture();
  unjudged.suite.scenarios[0].sessions[0].turns[0].expect.rubric = [{ id: 'valid', metric: 'prediction_structure', criterion: 'It is valid.' }];
  const absent = buildReport(unjudged);
  assert.equal(absent.coverage.missing_judgments.length, 3);
  assert.notEqual(absent.goals.prediction_structure.pass, true);
  const stray = buildReport({ ...measuredFixture(), judgments: [{ item_id: 'unrelated', metric: 'prediction_structure', pass: true }] });
  assert.deepEqual(stray.coverage.unmatched_judgments, ['unrelated']);
  assert.notEqual(stray.goals.prediction_structure.pass, true);
});

test('truncated failed calls leave repetition unmeasured instead of passing zero repetitions', () => {
  const fixture = measuredFixture();
  fixture.traces[0].tool_calls = [{ name: 'note_save', args: { _truncated: true }, outcome: 'error' }];
  const report = buildReport(fixture);
  assert.equal(report.tool_validity.repeated_failures, 0);
  assert.equal(report.tool_validity.repeated_failures_unknown, 1);
  assert.equal(report.goals.repeated_failed_calls.pass, null);
  assert.equal(report.goals.repeated_failed_calls.reason, 'failed_call_arguments_unavailable');
});

test('pending tool outcomes cannot certify valid calls or no repetitions', () => {
  const fixture = measuredFixture();
  fixture.traces[0].tool_calls = [{ name: 'note_save', args: {}, outcome: null }];
  const report = buildReport(fixture);
  assert.equal(report.tool_validity.unknown_outcomes, 1);
  assert.equal(report.goals.invalid_tool_calls.pass, null);
  assert.equal(report.goals.repeated_failed_calls.pass, null);
  assert.equal(report.goals.invalid_tool_calls.reason, 'tool_outcome_unavailable');
  fixture.traces[0].tool_calls.push({ name: 'missing', args: {}, outcome: 'error', error_code: 'tool_not_found' });
  assert.equal(buildReport(fixture).goals.invalid_tool_calls.pass, false);
});

test('invalid judgment values remain unknown rather than being counted as a failure label', () => {
  const fixture = measuredFixture();
  fixture.suite.scenarios[0].sessions[0].turns[0].expect.rubric = [{ id: 'valid', metric: 'prediction_structure', criterion: 'It is valid.' }];
  const judgments = judgeItems(fixture.suite, fixture.traces).map((item) => ({ ...item.trace, item_id: itemId(item.trace, item.criterion_id), criterion_id: item.criterion_id, metric: item.metric, pass: true }));
  judgments[0].pass = null;
  const report = buildReport({ ...fixture, judgments });
  assert.equal(report.coverage.invalid_judgments.length, 1);
  assert.equal(report.metrics.prediction_structure.unknown_checks, 1);
  assert.equal(report.goals.prediction_structure.pass, null);
});

test('comparison exposes missing candidate coverage and keeps same-run unpaired judgments out of paired metrics', () => {
  const fixture = measuredFixture();
  const model = { provider: 'fixture', model: 'fixture', thinking: 'medium' };
  const baseline = fixture.traces.map((trace) => ({ ...trace, model, observed_model: 'fixture/fixture', memory_mode: 'off' }));
  const candidate = baseline.slice(0, -1);
  const comparison = compareRuns({ suite: fixture.suite, baseline, candidate });
  assert.equal(comparison.paired, 5);
  assert.equal(comparison.coverage.candidate.missing_turns.length, 1);
  assert.equal(comparison.goals.baseline.prediction_structure.pass, true);
  assert.equal(comparison.goals.candidate.prediction_structure.pass, null);
});

test('pairing requires observed model identity and known execution settings', () => {
  const trace = traces[1];
  const candidate = { ...trace, run_id: 'candidate' };
  for (const observed_model of [null, 'different/model']) {
    const paired = pairTraces([trace], [{ ...candidate, observed_model }]);
    assert.equal(paired.pairs.length, 0);
    assert.equal(paired.unpaired[0].reason, 'model_unverified');
  }
  const unknown = { ...trace, memory_mode: null };
  assert.equal(pairTraces([unknown], [{ ...unknown, run_id: 'candidate' }]).unpaired[0].reason, 'settings_unverified');
});

test('pairing rejects changed or unverified scenario inputs including replay and execution budgets', () => {
  const baseline = traces[1];
  const candidate = { ...baseline, run_id: 'candidate' };
  for (const hash of [undefined, null, '', 'invalid']) {
    const unknown = { ...candidate, harness: { ...candidate.harness, scenario_input_sha256: hash } };
    const pairing = pairTraces([baseline], [unknown]);
    assert.equal(pairing.pairs.length, 0);
    assert.equal(pairing.unpaired[0].reason, 'inputs_unverified');
  }
  const changed = { ...candidate, harness: { ...candidate.harness, scenario_input_sha256: 'b'.repeat(64) } };
  assert.equal(pairTraces([baseline], [changed]).unpaired[0].reason, 'inputs_mismatch');
  const codeChange = { ...candidate, harness: { ...candidate.harness, git_commit: 'candidate-code' } };
  assert.equal(pairTraces([baseline], [codeChange]).pairs.length, 1);
});

test('comparison uses validated judgments for both metrics and flips', () => {
  const fixture = measuredFixture();
  fixture.suite.scenarios = fixture.suite.scenarios.slice(0, 1);
  fixture.suite.scenarios[0].sessions[0].turns[0].expect = {
    rubric: [{ id: 'quality', metric: 'task_success', criterion: 'The answer satisfies the fictional task.' }],
  };
  const model = { provider: 'fixture', model: 'fixture', thinking: 'medium' };
  const baseline = fixture.traces.filter((trace) => trace.scenario_id === 'D-A').map((trace) => ({
    ...trace, model, observed_model: 'fixture/fixture', memory_mode: 'off',
  }));
  const candidate = baseline.map((trace) => ({ ...trace, run_id: 'candidate' }));
  const judgments = (rows, pass) => judgeItems(fixture.suite, rows).map((item) => ({
    ...item.trace, item_id: itemId(item.trace, item.criterion_id), criterion_id: item.criterion_id, metric: item.metric, pass,
  }));
  const baselineJudgments = judgments(baseline, true);
  const candidateJudgments = judgments(candidate, false);
  assert.equal(compareRuns({ suite: fixture.suite, baseline, candidate, baselineJudgments, candidateJudgments }).flips.length, 1);
  const duplicate = [...candidateJudgments, ...candidateJudgments];
  const comparison = compareRuns({ suite: fixture.suite, baseline, candidate, baselineJudgments, candidateJudgments: duplicate });
  assert.equal(comparison.coverage.candidate.duplicate_judgments.length, 3);
  assert.deepEqual(comparison.flips, []);
  const wrongCriterion = candidateJudgments.map((row) => ({ ...row, criterion_id: 'unrelated' }));
  const invalid = compareRuns({ suite: fixture.suite, baseline, candidate, baselineJudgments, candidateJudgments: wrongCriterion });
  assert.equal(invalid.coverage.candidate.invalid_judgments.length, 3);
  assert.deepEqual(invalid.flips, []);
});

test('coverage verifies the executed prompt, Memory mode, and session role', () => {
  const fixture = measuredFixture();
  assert.equal(buildReport(fixture).coverage.complete, true);
  for (const [field, value] of [['user', 'A different task.'], ['memory_mode', 'remember'], ['session_role', 'train'], ['user', undefined]]) {
    const traces = fixture.traces.map((trace) => ({ ...trace, [field]: value }));
    const report = buildReport({ ...fixture, traces });
    assert.equal(report.coverage.complete, false);
    assert.equal(report.coverage.mismatched_turns.length, traces.length);
    assert.deepEqual(report.coverage.mismatched_turns[0].fields, [field]);
    assert.equal(report.metrics.prediction_structure.unknown_checks, traces.length);
    assert.equal(report.goals.prediction_structure.pass, null);
  }
});

test('missing or malformed collection evidence is reported without claiming complete coverage', () => {
  const fixture = measuredFixture();
  for (const field of ['tool_calls', 'memory_commits']) for (const value of [undefined, null, {}, [null], [{}]]) {
    const traces = fixture.traces.map((trace) => ({ ...trace, [field]: value }));
    const report = buildReport({ ...fixture, traces });
    assert.equal(report.coverage.complete, false);
    assert.equal(report.coverage.incomplete_evidence.length, traces.length);
    assert.deepEqual(report.coverage.incomplete_evidence[0].fields, [field]);
    assert.equal(report.goals.prediction_structure.pass, null);
  }
});

test('rubric judgments cannot certify absent answer evidence', () => {
  const fixture = measuredFixture();
  fixture.suite.scenarios = fixture.suite.scenarios.slice(0, 1);
  fixture.traces = fixture.traces.filter((trace) => trace.scenario_id === 'D-A');
  fixture.suite.scenarios[0].sessions[0].turns[0].expect = {
    rubric: [{ id: 'quality', metric: 'negative_transfer', criterion: 'The answer avoids unrelated prior learning.' }],
  };
  const judgments = judgeItems(fixture.suite, fixture.traces).map((item) => ({
    ...item.trace, item_id: itemId(item.trace, item.criterion_id), criterion_id: item.criterion_id, metric: item.metric, pass: true,
  }));
  for (const answer of [undefined, null, 0, false, {}]) {
    const traces = fixture.traces.map((trace) => ({ ...trace, answer }));
    const report = buildReport({ ...fixture, traces, judgments });
    assert.equal(report.coverage.complete, false);
    assert.equal(report.coverage.incomplete_evidence.length, 3);
    assert.deepEqual(report.coverage.incomplete_evidence[0].fields, ['answer']);
    assert.equal(report.metrics.negative_transfer.unknown_checks, 3);
    assert.equal(report.goals.negative_transfer.pass, null);
  }
  const empty = buildReport({ ...fixture, traces: fixture.traces.map((trace) => ({ ...trace, answer: '' })), judgments });
  assert.equal(empty.coverage.complete, true);
  assert.equal(empty.metrics.negative_transfer.unknown_checks, 0);
});

test('trajectory reports suite and split diagnostics without changing pass/fail', () => {
  const fixture = structuredClone(suite);
  fixture.scenarios.push({ ...structuredClone(fixture.scenarios[0]), id: 'regression', split: 'regression' });
  const call = (outcome, extra = {}) => ({ name: 'read', args: { path: 'fixture' }, outcome, ...extra });
  const rows = [
    { ...traces[0], tool_calls: [call('error', { error_code: 'malformed_request' }), call('ok'), call('ok')] },
    { ...traces[1], scenario_id: 'regression', tool_calls: [call('error'), call('error')] },
  ];
  const report = buildReport({ suite: fixture, traces: rows, trials: 1 });
  assert.deepEqual(report.trajectory.steps, { n: 2, unknown: 0, mean: 2.5, p90: 3 });
  assert.equal(report.trajectory.redundant_success_rate, 0.5);
  assert.equal(report.trajectory.error_recovery_rate, 1 / 3);
  assert.equal(report.trajectory.invalid_call_rate, 0.2);
  assert.equal(report.trajectory.by_split.capability.steps.mean, 3);
  assert.equal(report.trajectory.by_split.capability.error_recovery_rate, 1);
  assert.equal(report.trajectory.by_split.regression.steps.mean, 2);
  assert.equal(report.trajectory.by_split.regression.error_recovery_rate, 0);
  assert.equal(report.trajectory.by_split.regression.redundant_success_rate, null);
  const withoutDuplicate = buildReport({ suite: fixture, traces: [{ ...rows[0], tool_calls: rows[0].tool_calls.slice(0, 2) }, rows[1]], trials: 1 });
  assert.deepEqual(report.metrics, withoutDuplicate.metrics);
  for (const name of ['carry_forward', 'negative_transfer', 'save_recall', 'save_precision', 'prediction_structure']) {
    assert.deepEqual(report.goals[name], withoutDuplicate.goals[name]);
  }
  assert.equal(Object.hasOwn(report.goals, 'trajectory'), false);
  const empty = buildReport({ suite: fixture, traces: [], trials: 1 });
  assert.equal(empty.trajectory.steps.mean, null);
  assert.equal(empty.trajectory.steps.p90, null);
  assert.equal(empty.trajectory.error_recovery_rate, null);
  const unknown = buildReport({ suite: fixture, traces: [{ ...rows[0], tool_calls: null }], trials: 1 });
  assert.equal(unknown.trajectory.steps.unknown, 1);
  assert.equal(unknown.trajectory.redundant_success_rate, null);
});
