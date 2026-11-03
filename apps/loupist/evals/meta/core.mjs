import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { grade } from '../agent/grade.mjs';
import { measuredTurns, measuredJudgments } from '../agent/measurement.mjs';
import { tokenTotals } from '../agent/report.mjs';
import { judgeItems, itemId } from '../agent/judge.mjs';
import { turnIdentity } from '../agent/eval-contract.mjs';
import { sha256, surfaceHash } from './surface.mjs';

export function partition(suite) {
  if (suite.scenarios.some(s => s.split === 'heldout')) throw new Error('Held-out scenarios are forbidden in the optimizer');
  const capability = suite.scenarios.filter(s => s.split === 'capability');
  const train = [], validation = [];
  for (const scenario of capability) {
    (parseInt(sha256(scenario.id).slice(0, 8), 16) / 2 ** 32 < 0.7 ? train : validation).push(scenario.id);
  }
  if (!train.length || !validation.length) throw new Error('Hash partition requires nonempty train and validation sets');
  return { schema: 'loupist-meta-partition/1', algorithm: 'sha256(id)[0:32bits] / 2^32 < 0.7', train: train.sort(), validation: validation.sort() };
}
export function adaptiveSelection(scores, iteration, seed) {
  const failed = Object.keys(scores.scenarios).filter(id => scores.scenarios[id] === false);
  const passing = Object.keys(scores.scenarios).filter(id => scores.scenarios[id] === true)
    .sort((a, b) => sha256(`${seed}:${iteration}:${a}`).localeCompare(sha256(`${seed}:${iteration}:${b}`)));
  return [...failed, ...passing.slice(0, Math.ceil(passing.length * 0.3))].sort();
}
export function score(suite, traces, judgments, trials, deterministicOnly = false) {
  const { measured, coverage } = measuredTurns(suite, traces, trials);
  const deterministic = grade(suite, measured).results;
  const judgeErrors = deterministicOnly ? [] : judgments.filter(row => row.judge_error === true && row.pass === null);
  const expected = new Map(judgeItems(suite, measured).map(item => [itemId(item.trace, item.criterion_id), item]));
  for (const row of judgeErrors) {
    const item = expected.get(row.item_id);
    if (!item || turnIdentity(row) !== turnIdentity(item.trace) || row.criterion_id !== item.criterion_id
      || row.metric !== item.metric || row.attempts !== 3
      || judgments.filter(other => other.item_id === row.item_id).length !== 1) {
      throw new Error('Invalid judge-error evidence; refusing scenario exclusion');
    }
  }
  const excluded = new Set(judgeErrors.map(row => row.scenario_id));
  const eligibleSuite = { ...suite, scenarios: suite.scenarios.filter(row => !excluded.has(row.id)) };
  const judged = deterministicOnly ? [] : measuredJudgments(eligibleSuite, measured.filter(row => !excluded.has(row.scenario_id)), judgments.filter(row => !excluded.has(row.scenario_id)), coverage);
  const checks = [...deterministic, ...judged].filter(c => !excluded.has(c.scenario_id) && !['retrieval', 'efficiency'].includes(c.metric));
  const scenarios = {};
  for (const scenario of suite.scenarios) {
    const rows = measured.filter(t => t.scenario_id === scenario.id);
    const assertions = checks.filter(c => c.scenario_id === scenario.id);
    scenarios[scenario.id] = excluded.has(scenario.id) ? null : rows.every(t => t.status === 'completed') && assertions.length > 0 && assertions.every(c => c.pass === true);
  }
  const metrics = Object.fromEntries([...new Set(checks.map(c => c.metric ?? 'unscoped'))].map(metric => {
    const rows = checks.filter(c => (c.metric ?? 'unscoped') === metric);
    return [metric, { passed: rows.filter(c => c.pass === true).length, total: rows.length }];
  }));
  const complete = Object.entries(coverage).filter(([, value]) => Array.isArray(value)).every(([, value]) => value.length === 0);
  return { scenarios, passed: Object.values(scenarios).filter(Boolean).length, total: suite.scenarios.length,
    eligible_total: suite.scenarios.length - excluded.size, excluded_scenarios: [...excluded].sort(), judge_errors: judgeErrors,
    rate: suite.scenarios.length === excluded.size ? null : Object.values(scenarios).filter(Boolean).length / (suite.scenarios.length - excluded.size),
    complete, coverage, metrics, checks: checks.length, checks_passed: checks.filter(c => c.pass === true).length,
    deterministic_failures: deterministic.filter(c => c.pass !== true), usage: tokenTotals(traces), trials };
}
export function pairedComparison(incumbent, candidate, ids = Object.keys(incumbent.scenarios)) {
  const eligible = ids.filter(id => typeof incumbent.scenarios[id] === 'boolean' && typeof candidate.scenarios[id] === 'boolean');
  const baselinePassed = eligible.filter(id => incumbent.scenarios[id]).length;
  const candidatePassed = eligible.filter(id => candidate.scenarios[id]).length;
  return { eligible, excluded: ids.filter(id => !eligible.includes(id)), baseline_passed: baselinePassed,
    candidate_passed: candidatePassed, delta: candidatePassed - baselinePassed };
}
export function beatsSubset(incumbent, candidate, ids) {
  return incumbent.complete && candidate.complete && ids.length === candidate.total
    && pairedComparison(incumbent, candidate, ids).delta >= 1;
}

export function acceptance(incumbent, candidate, beforeSmoke, afterSmoke) {
  if (!incumbent?.complete || !candidate?.complete || candidate.total !== incumbent.total || pairedComparison(incumbent, candidate).delta < 1) return { accepted: false, reason: 'Full train must improve by at least one scenario with complete evidence' };
  for (const suite of ['principles-v1', 'security-v1']) {
    const before = beforeSmoke?.[suite], after = afterSmoke?.[suite];
    if (!before?.complete || !after?.complete || before.total !== after.total || before.checks !== after.checks
      || after.passed < before.passed || after.checks_passed < before.checks_passed) return { accepted: false, reason: `${suite} regression drop or incomplete smoke` };
    for (const [metric, value] of Object.entries(before.metrics ?? {})) {
      const next = after.metrics?.[metric];
      if (!next || next.total !== value.total || next.passed < value.passed) return { accepted: false, reason: `${suite} regression metric dropped: ${metric}` };
    }
    if (suite === 'security-v1' && after.deterministic_failures.length) return { accepted: false, reason: 'Deterministic security failures veto scores' };
  }
  return { accepted: true, reason: 'Train improved; regression smokes did not drop; security checks passed', comparison: pairedComparison(incumbent, candidate) };
}
export async function archiveCandidate(root, n, { diff, assets, scores, traces }) {
  const directory = join(root, 'candidates', String(n));
  await mkdir(directory, { recursive: true });
  await Promise.all([
    writeFile(join(directory, 'diff.patch'), diff, { mode: 0o600 }),
    writeFile(join(directory, 'assets.sha256'), `${assets ? surfaceHash(assets) : 'rejected-before-asset-generation'}\n`, { mode: 0o600 }),
    writeFile(join(directory, 'scores.json'), JSON.stringify(scores, null, 2) + '\n', { mode: 0o600 }),
    writeFile(join(directory, 'traces.jsonl'), traces.map(t => JSON.stringify(t)).join('\n') + (traces.length ? '\n' : ''), { mode: 0o600 }),
  ]);
  return directory;
}
export function redact(value) {
  const scrub = text => text.replace(/(?:Bearer\s+)[\w.\-]+/giu, 'Bearer [REDACTED]')
    .replace(/\b(?:sk|xai|ghp|gho)[-_][A-Za-z0-9_-]{12,}\b/gu, '[REDACTED]')
    .replace(/([?&](?:key|api_key|token|crtfc_key|access_token)=)[^&\s]+/giu, '$1[REDACTED]')
    .replace(/((?:api[_-]?key|password|secret|token|authorization)\s*[:=]\s*)[^\s,;]+/giu, '$1[REDACTED]');
  if (typeof value === 'string') return scrub(value);
  if (Array.isArray(value)) return value.map(redact);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([k, v]) =>
    [k, /^(?:authorization|password|secret|token|api[_-]?key|access_token|refresh_token|crtfc_key)$/iu.test(k) ? '[REDACTED]' : redact(v)]));
  return value;
}
export function failingExcerpts(scores, traces) {
  const result = []; let bytes = 0;
  for (const t of traces.filter(t => scores.scenarios[t.scenario_id] === false)) {
    const excerpt = redact({ scenario_id: t.scenario_id, session_id: t.session_id, trial: t.trial,
      user: String(t.user ?? '').slice(0, 2000), answer: String(t.answer ?? '').slice(0, 4000),
      tool_calls: (t.tool_calls ?? []).map(call => ({ name: call.name, args: call.args, outcome: call.outcome,
        error_code: call.error_code })).slice(0, 12) });
    // Inputs are already host-redacted; this second scrub and cap limit proposer context.
    const serialized = JSON.stringify(excerpt);
    if (Buffer.byteLength(serialized) > 12000) excerpt.tool_calls = excerpt.tool_calls.map(call => ({ ...call, args: '[excerpt omitted: large arguments]' }));
    const size = Buffer.byteLength(JSON.stringify(excerpt));
    if (bytes + size > 48000) break;
    result.push(excerpt); bytes += size;
  }
  return result;
}
