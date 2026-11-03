/** Aggregate agent-eval metrics. Unknown measurements stay null. */
import { quantile } from '../evaluate.mjs';
import { grade } from './grade.mjs';
import { groupBy, turnIdentity, turnKey, turnFields, trialKey, positiveTrialCount } from './eval-contract.mjs';
import { analyzeToolCalls, toolValidity } from './tool-evidence.mjs';
import { measuredTurns, measuredJudgments } from './measurement.mjs';

const ITEM_METRICS = new Set(['save_precision', 'save_recall']);
const SCENARIO_METRICS = new Set(['carry_forward', 'negative_transfer', 'revision_disclosure', 'connection_honesty', 'task_success', 'prediction_structure']);

const Z95 = 1.959963984540054;

export function wilsonInterval(successes, total, z = Z95) {
  if (!Number.isInteger(total) || total <= 0 || !Number.isInteger(successes) || successes < 0 || successes > total) return null;
  const p = successes / total;
  const z2 = z * z;
  const denom = 1 + z2 / total;
  const center = (p + z2 / (2 * total)) / denom;
  const margin = (z * Math.sqrt((p * (1 - p) + z2 / (4 * total)) / total)) / denom;
  return { low: center - margin, high: center + margin };
}

function rate(pass, total) {
  return { pass, total, rate: total === 0 ? null : pass / total };
}

export function latencySummary(traces) {
  const values = traces.map((trace) => trace.duration_ms).filter((value) => typeof value === 'number' && Number.isFinite(value) && value >= 0);
  return { n: values.length, unknown: traces.length - values.length, p50: quantile(values, 0.5), p95: quantile(values, 0.95) };
}

export function tokenTotals(traces) {
  const fields = ['input', 'output', 'cache_read', 'cache_write'];
  const totals = {};
  for (const field of fields) {
    let sum = 0;
    let known = traces.length > 0;
    for (const trace of traces) {
      const value = trace.usage?.[field];
      if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) { known = false; break; }
      sum += value;
    }
    totals[field] = known ? sum : null;
  }
  totals.total = fields.every((field) => totals[field] != null) ? fields.reduce((sum, field) => sum + totals[field], 0) : null;
  return totals;
}

/** Observed attempts only; diagnostics never contribute to completion goals. */
export function trajectorySummary(traces) {
  const analyses = traces.map((trace) => analyzeToolCalls(trace.tool_calls));
  const known = analyses.filter((row) => !row.unknown_call_lists);
  const steps = known.map((row) => row.steps);
  const sum = (field) => analyses.reduce((total, row) => total + row[field], 0);
  const calls = sum('calls');
  const errors = sum('errors');
  const successes = calls - errors - sum('unknown_outcomes');
  const redundant = analyses.reduce((total, row) => total + row.redundant_ok.length, 0);
  const complete = known.length === traces.length && sum('unknown_outcomes') === 0;
  return {
    turns: traces.length,
    steps: { n: steps.length, unknown: traces.length - steps.length,
      mean: steps.length ? steps.reduce((total, value) => total + value, 0) / steps.length : null,
      p90: quantile(steps, 0.9) },
    calls, successes, errors, redundant_ok: redundant, recovered_errors: sum('recovered_errors'),
    redundant_success_rate: complete && successes ? redundant / successes : null,
    error_recovery_rate: complete && errors ? sum('recovered_errors') / errors : null,
    invalid_call_rate: toolValidity(traces).rate,
  };
}

function trajectory(suite, traces) {
  const splits = new Map(suite.scenarios.map((scenario) => [scenario.id, scenario.split ?? 'unscoped']));
  return { ...trajectorySummary(traces), by_split: Object.fromEntries([...new Set(splits.values())].sort().map((split) =>
    [split, trajectorySummary(traces.filter((trace) => trace.suite === suite.suite && splits.get(trace.scenario_id) === split))])) };
}

function checksFrom(assertions, judgments) {
  return [...assertions, ...judgments].filter((row) => row.metric);
}

export function aggregateMetrics(checks, { k = 3 } = {}) {
  positiveTrialCount(k);
  const grouped = groupBy(checks, (check) => check.metric);
  const trialMetrics = aggregateTrialMap(checks);
  const metrics = {};
  for (const metric of [...grouped.keys()].sort()) {
    const rows = grouped.get(metric);
    const trialRows = trialMetrics.get(metric);
    const byScenario = groupBy(trialRows, (trial) => trial.scenario_id);
    // A partial scenario stays in the denominator; absent trials cannot pass.
    const eligible = [...byScenario.values()].map((list) => [...list.map((trial) => trial.pass), ...Array(Math.max(0, k - list.length)).fill(null)]);
    const trialPass = trialRows.filter((trial) => trial.pass).length;
    const checks = rate(rows.filter((row) => row.pass).length, rows.length);
    const trialRate = { ...rate(trialPass, trialRows.length), wilson_95: wilsonInterval(trialPass, trialRows.length) };
    const majority = rate(eligible.filter((list) => list.filter(Boolean).length * 2 > k).length, eligible.length);
    const level = ITEM_METRICS.has(metric) ? 'item' : SCENARIO_METRICS.has(metric) ? 'scenario' : 'trial';
    const headline = level === 'item' ? checks : level === 'scenario' ? majority : trialRate;
    metrics[metric] = {
      level, headline, checks, trials: trialRate, majority_of_k: { k, ...majority },
      majority_of_3: k === 3 ? majority : rate(0, 0),
      unknown_checks: rows.filter((row) => row.pass == null).length,
      pass_k: { k, ...rate(eligible.filter((list) => list.every(Boolean)).length, eligible.length) },
    };
    if (metric === 'negative_transfer') metrics[metric].transfer_rate = majority.rate == null ? null : 1 - majority.rate;
  }
  return metrics;
}

function goalRow(target, observed, pass) {
  return { target, observed: observed ?? null, pass: observed == null ? null : pass };
}

export function completionGoals(metrics, toolValidity, coverage = null, trials = 3) {
  const hasCalls = (toolValidity?.calls ?? 0) > 0;
  const invalid = hasCalls ? toolValidity.rate : null;
  const repeated = hasCalls ? toolValidity.repeated_failures : null;
  const carry = metrics.carry_forward?.headline?.rate ?? null;
  const transfer = metrics.negative_transfer?.transfer_rate ?? null;
  const recall = metrics.save_recall?.headline?.rate ?? null;
  const precision = metrics.save_precision?.headline?.rate ?? null;
  const structure = metrics.prediction_structure?.headline?.rate ?? null;
  const goals = {
    carry_forward: goalRow('>=0.8', carry, carry >= 0.8),
    negative_transfer: goalRow('<=0.1', transfer, transfer <= 0.1),
    save_recall: goalRow('>=0.8', recall, recall >= 0.8),
    save_precision: goalRow('>=0.8', precision, precision >= 0.8),
    invalid_tool_calls: goalRow('<=0.05', invalid, invalid <= 0.05),
    repeated_failed_calls: goalRow('==0', repeated, repeated === 0),
    prediction_structure: goalRow('>=0.9', structure, structure >= 0.9),
  };
  for (const [name, goal] of Object.entries(goals)) {
    if ((metrics[name]?.unknown_checks ?? 0) > 0) {
      goal.pass = null;
      goal.reason = 'unmeasured_checks';
    }
  }
  if ((toolValidity?.repeated_failures_unknown ?? 0) > 0 && goals.repeated_failed_calls.pass !== false) {
    goals.repeated_failed_calls.pass = null;
    goals.repeated_failed_calls.reason = 'failed_call_arguments_unavailable';
  }
  if ((toolValidity?.unknown_outcomes ?? 0) > 0) {
    for (const name of ['invalid_tool_calls', 'repeated_failed_calls']) {
      if (goals[name].pass !== false) {
        goals[name].pass = null;
        goals[name].reason = 'tool_outcome_unavailable';
      }
    }
  }
  if ((toolValidity?.unknown_call_lists ?? 0) > 0) {
    goals.invalid_tool_calls.pass = null;
    goals.invalid_tool_calls.reason = 'tool_call_evidence_unavailable';
    if (goals.repeated_failed_calls.pass !== false) {
      goals.repeated_failed_calls.pass = null;
      goals.repeated_failed_calls.reason = 'tool_call_evidence_unavailable';
    }
  }
  if (trials !== 3 || (coverage && !coverage.complete)) {
    for (const goal of Object.values(goals)) {
      if (goal.pass === true) goal.pass = null;
      goal.reason = trials !== 3 ? 'completion_requires_three_trials' : 'incomplete_measurement';
    }
  }
  return goals;
}

function scenarioPasses(checks, k = 3) {
  const metrics = aggregateTrialMap(checks);
  const decisions = {};
  for (const [metric, trials] of metrics) {
    const byScenario = groupBy(trials, (trial) => trial.scenario_id);
    decisions[metric] = {};
    for (const [scenarioId, list] of byScenario) {
      decisions[metric][scenarioId] = list.length === k && list.every((trial) => trial.pass != null)
        ? list.filter((trial) => trial.pass).length * 2 > k : null;
    }
  }
  return decisions;
}

function aggregateTrialMap(checks) {
  const grouped = new Map();
  for (const check of checks) {
    const trials = grouped.get(check.metric) ?? new Map();
    const key = trialKey(check);
    const entry = trials.get(key) ?? { scenario_id: check.scenario_id, trial: check.trial, pass: true };
    if (check.pass === false) entry.pass = false;
    else if (check.pass !== true && entry.pass !== false) entry.pass = null;
    trials.set(key, entry);
    grouped.set(check.metric, trials);
  }
  return new Map([...grouped].map(([metric, trials]) => [metric, [...trials.values()]]));
}

function measureReport({ suite, traces, judgments = [], trials = 3 }) {
  const { measured, coverage } = measuredTurns(suite, traces, trials);
  const graded = grade(suite, measured);
  const scoredJudgments = measuredJudgments(suite, measured, judgments, coverage);
  coverage.complete = ['missing_turns', 'duplicate_turns', 'unexpected_turns', 'unknown_status_turns', 'incoherent_trials',
    'mismatched_turns', 'incomplete_evidence', 'missing_judgments', 'duplicate_judgments', 'unmatched_judgments', 'invalid_judgments'].every((key) => coverage[key].length === 0);
  const checks = checksFrom(graded.results, scoredJudgments);
  const metrics = aggregateMetrics(checks, { k: trials });
  // Preserve observed calls, costs, tokens and latency for every attempt.
  const observedTools = toolValidity(traces);
  const report = {
    schema: 'loupist-agent-report/1',
    suite: suite.suite ?? null,
    version: suite.version ?? null,
    traces: traces.length,
    agent_snapshot_sha256s: [...new Set(traces.map(trace => trace.harness?.agent_snapshot_sha256 ?? null))].sort(),
    assertions: graded.results.length,
    unmatched: coverage.unexpected_turns.length,
    coverage,
    metrics,
    goals: completionGoals(metrics, observedTools, coverage, trials),
    tool_validity: observedTools,
    trajectory: trajectory(suite, traces),
    latency_ms: latencySummary(traces),
    tokens: tokenTotals(traces),
  };
  return { report, checks };
}

export function buildReport(options) {
  const report = measureReport(options).report;
  report.trials = options.trials ?? 3;
  report.by_split = Object.fromEntries([...new Set(options.suite.scenarios.map(s => s.split ?? 'unscoped'))].map(split => {
    const scenarios = options.suite.scenarios.filter(s => (s.split ?? 'unscoped') === split);
    const ids = new Set(scenarios.map(s => s.id));
    return [split, measureReport({ ...options, suite: { ...options.suite, scenarios },
      traces: options.traces.filter(t => ids.has(t.scenario_id)),
      judgments: (options.judgments ?? []).filter(t => ids.has(t.scenario_id)) }).report];
  }));
  return report;
}

export function pairReason(left, right) {
  if (!left.model?.model || !right.model?.model || left.model.provider !== right.model.provider || left.model.model !== right.model.model) return 'model_mismatch';
  if ([left, right].some((trace) => !trace.model.provider
    || trace.observed_model !== `${trace.model.provider}/${trace.model.model}`)) return 'model_unverified';
  if ([left, right].some((trace) => typeof trace.model.thinking !== 'string' || !trace.model.thinking
    || !['off', 'read_only', 'remember'].includes(trace.memory_mode))) return 'settings_unverified';
  if ((left.model.thinking ?? null) !== (right.model.thinking ?? null) || (left.memory_mode ?? null) !== (right.memory_mode ?? null)) return 'settings_mismatch';
  if ([left, right].some((trace) => typeof trace.harness?.scenario_input_sha256 !== 'string'
    || !/^[a-f0-9]{64}$/u.test(trace.harness.scenario_input_sha256))) return 'inputs_unverified';
  if (left.harness.scenario_input_sha256 !== right.harness.scenario_input_sha256) return 'inputs_mismatch';
  return null;
}

export function pairTraces(baseline, candidate) {
  const left = groupBy(baseline, turnKey);
  const right = groupBy(candidate, turnKey);
  const pairs = [];
  const unpaired = [];
  for (const key of new Set([...left.keys(), ...right.keys()])) {
    const baselineRows = left.get(key) ?? [];
    const candidateRows = right.get(key) ?? [];
    const sample = baselineRows[0] ?? candidateRows[0];
    if (baselineRows.length !== 1 || candidateRows.length !== 1) {
      const reason = baselineRows.length === 0 ? 'missing_baseline' : candidateRows.length === 0 ? 'missing_candidate' : 'duplicate';
      unpaired.push({ ...turnFields(sample), reason });
      continue;
    }
    const reason = pairReason(baselineRows[0], candidateRows[0]);
    if (reason) unpaired.push({ ...turnFields(sample), reason });
    else pairs.push({ baseline: baselineRows[0], candidate: candidateRows[0] });
  }
  return { pairs, unpaired };
}

function nullableDelta(left, right) {
  return left == null || right == null ? null : right - left;
}

function flipsFor(baselineChecks, candidateChecks) {
  const left = scenarioPasses(baselineChecks);
  const right = scenarioPasses(candidateChecks);
  const rows = [];
  for (const metric of new Set([...Object.keys(left), ...Object.keys(right)])) {
    const scenarios = new Set([...Object.keys(left[metric] ?? {}), ...Object.keys(right[metric] ?? {})]);
    for (const scenarioId of [...scenarios].sort()) {
      const baselinePass = left[metric]?.[scenarioId] ?? null;
      const candidatePass = right[metric]?.[scenarioId] ?? null;
      if (baselinePass == null || candidatePass == null || baselinePass === candidatePass) continue;
      rows.push({ scenario_id: scenarioId, metric, baseline_pass: baselinePass, candidate_pass: candidatePass });
    }
  }
  return rows;
}

export function compareRuns({ suite, baseline, candidate, baselineJudgments = [], candidateJudgments = [], trials = 3 }) {
  const { pairs, unpaired } = pairTraces(baseline, candidate);
  const pairedIds = {
    baseline: new Set(pairs.map((pair) => turnIdentity(pair.baseline))),
    candidate: new Set(pairs.map((pair) => turnIdentity(pair.candidate))),
  };
  const baselineTraces = pairs.map((pair) => pair.baseline);
  const candidateTraces = pairs.map((pair) => pair.candidate);
  const baselineMeasured = measureReport({ suite, traces: baselineTraces, judgments: baselineJudgments.filter((row) => pairedIds.baseline.has(turnIdentity(row))), trials });
  const candidateMeasured = measureReport({ suite, traces: candidateTraces, judgments: candidateJudgments.filter((row) => pairedIds.candidate.has(turnIdentity(row))), trials });
  const baselineReport = baselineMeasured.report;
  const candidateReport = candidateMeasured.report;
  const baselineChecks = baselineMeasured.checks.filter((row) => pairedIds.baseline.has(turnIdentity(row)));
  const candidateChecks = candidateMeasured.checks.filter((row) => pairedIds.candidate.has(turnIdentity(row)));
  const baselineMetrics = aggregateMetrics(baselineChecks, { k: trials });
  const candidateMetrics = aggregateMetrics(candidateChecks, { k: trials });
  const originalBaseline = buildReport({ suite, traces: baseline, judgments: baselineJudgments, trials });
  const originalCandidate = buildReport({ suite, traces: candidate, judgments: candidateJudgments, trials });
  const names = new Set([...Object.keys(baselineMetrics), ...Object.keys(candidateMetrics)]);
  const metrics = {};
  for (const name of [...names].sort()) {
    const left = baselineMetrics[name]?.headline ?? { pass: 0, total: 0, rate: null };
    const right = candidateMetrics[name]?.headline ?? { pass: 0, total: 0, rate: null };
    metrics[name] = {
      baseline_pass: left.pass, baseline_total: left.total, baseline_rate: left.rate,
      candidate_pass: right.pass, candidate_total: right.total, candidate_rate: right.rate,
      delta: nullableDelta(left.rate, right.rate),
    };
  }
  return {
    schema: 'loupist-agent-compare/1',
    suite: suite.suite ?? null,
    paired: pairs.length,
    unpaired,
    coverage: { baseline: originalBaseline.coverage, candidate: originalCandidate.coverage },
    goals: { baseline: originalBaseline.goals, candidate: originalCandidate.goals },
    metrics,
    flips: trials === 3 ? flipsFor(baselineChecks, candidateChecks) : [],
    tool_validity: {
      baseline: baselineReport.tool_validity, candidate: candidateReport.tool_validity,
      rate_delta: nullableDelta(baselineReport.tool_validity.rate, candidateReport.tool_validity.rate),
    },
    latency_ms: {
      baseline: baselineReport.latency_ms, candidate: candidateReport.latency_ms,
      p50_delta: nullableDelta(baselineReport.latency_ms.p50, candidateReport.latency_ms.p50),
      p95_delta: nullableDelta(baselineReport.latency_ms.p95, candidateReport.latency_ms.p95),
    },
    tokens: {
      baseline: baselineReport.tokens, candidate: candidateReport.tokens,
      total_delta: nullableDelta(baselineReport.tokens.total, candidateReport.tokens.total),
    },
  };
}
