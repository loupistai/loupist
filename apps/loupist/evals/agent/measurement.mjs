/** Suite coverage and judgment ownership. Never infer missing execution evidence. */
import { validateSuite, validateTraceRecord, positiveTrialCount, turnFields, turnKey, trialKey, turnIdentity, turnCompleted,
  executionMismatches, answerEvidence, traceEvidenceIssues } from './eval-contract.mjs';
import { judgeItems, itemId } from './judge.mjs';

/** Build the expected matrix from the suite, never from the surviving traces. */
export function measuredTurns(suite, traces, trials) {
  validateSuite(suite);
  positiveTrialCount(trials);
  const expected = new Map();
  for (const scenario of suite.scenarios) for (let trial = 0; trial < trials; trial += 1) {
    for (const session of scenario.sessions) session.turns.forEach((turn, turn_index) => {
      const trace = { schema: 'loupist-agent-trace/1', run_id: 'unmeasured', suite: suite.suite ?? null,
        scenario_id: scenario.id, session_id: session.id, session_role: session.role, memory_mode: session.memory,
        turn_index, trial, user: turn.user, status: 'missing', answer: '', memory_commits: [], tool_calls: [], memory_reads: [] };
      expected.set(turnKey(trace), { trace, session, turn, rows: [] });
    });
  }
  const coverage = { trials, expected_turns: expected.size, observed_turns: traces.length,
    completed_turns: 0, failed_turns: 0, missing_turns: [], duplicate_turns: [], unexpected_turns: [], unknown_status_turns: [],
    incoherent_trials: [], mismatched_turns: [], incomplete_evidence: [] };
  for (const trace of traces) {
    validateTraceRecord(trace);
    const cell = expected.get(turnKey(trace));
    if (cell) cell.rows.push(trace);
    else coverage.unexpected_turns.push(turnFields(trace));
  }
  const units = new Map();
  const measured = [...expected.values()].map(({ trace, session, turn, rows }) => {
    if (!rows.length) { coverage.missing_turns.push(turnFields(trace)); return trace; }
    if (rows.length !== 1) { coverage.duplicate_turns.push({ ...turnFields(trace), attempts: rows.length }); return { ...trace, status: 'ambiguous' }; }
    const row = rows[0];
    const mismatches = executionMismatches(row, session, turn);
    if (mismatches.length) coverage.mismatched_turns.push({ ...turnFields(row), fields: mismatches });
    const unavailable = traceEvidenceIssues(row);
    if (unavailable.length) coverage.incomplete_evidence.push({ ...turnFields(row), fields: unavailable });
    if (row.status === 'completed') coverage.completed_turns += 1;
    else if (turnCompleted(row) === false) coverage.failed_turns += 1;
    else coverage.unknown_status_turns.push(turnFields(row));
    const key = trialKey(row);
    const unit = units.get(key) ?? { scenario_id: row.scenario_id, trial: row.trial, runs: new Set() };
    unit.runs.add(row.run_id);
    units.set(key, unit);
    return mismatches.length ? { ...row, status: 'mismatched' } : row;
  });
  const incoherent = new Set();
  for (const [key, unit] of units) if (unit.runs.size !== 1) {
    incoherent.add(key);
    coverage.incoherent_trials.push({ scenario_id: unit.scenario_id, trial: unit.trial, runs: unit.runs.size });
  }
  return { measured: measured.map((trace) => incoherent.has(trialKey(trace)) ? { ...trace, status: 'incoherent' } : trace), coverage };
}

export function measuredJudgments(suite, traces, judgments, coverage) {
  const expected = new Map(judgeItems(suite, traces).map((item) => [itemId(item.trace, item.criterion_id), { item, rows: [] }]));
  coverage.missing_judgments = [];
  coverage.duplicate_judgments = [];
  coverage.unmatched_judgments = [];
  coverage.invalid_judgments = [];
  for (const row of judgments) {
    const target = expected.get(row.item_id);
    if (target && target.item.metric === row.metric) target.rows.push(row);
    else coverage.unmatched_judgments.push(row.item_id ?? null);
  }
  return [...expected.entries()].map(([id, { item, rows }]) => {
    const trace = item.trace;
    const result = { ...turnFields(trace), run_id: trace.run_id, item_id: id, metric: item.metric, pass: null };
    // A failed answer cannot satisfy its rubric. Saved content remains evidence,
    // but does not turn an unsuccessful end-to-end trial into a successful one.
    if (turnCompleted(trace) !== true) return { ...result, pass: turnCompleted(trace) };
    if (answerEvidence(trace) === null) return result;
    if (!rows.length) coverage.missing_judgments.push(id);
    else if (rows.length !== 1) coverage.duplicate_judgments.push(id);
    else if (typeof rows[0].pass !== 'boolean' || turnIdentity(rows[0]) !== turnIdentity(trace)
      || rows[0].criterion_id !== item.criterion_id) coverage.invalid_judgments.push(id);
    else return { ...result, pass: rows[0].pass };
    return result;
  });
}
