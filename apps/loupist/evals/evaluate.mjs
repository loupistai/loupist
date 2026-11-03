#!/usr/bin/env node
/** Held-out finance evaluation. No model calls occur for plan/import/report. */
import { readFile, writeFile, mkdir, appendFile } from 'node:fs/promises';
import { createHash, randomUUID } from 'node:crypto';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const DEFAULT_DATASET = fileURLToPath(new URL('./finance-heldout-48.json', import.meta.url));
export const CONDITIONS = ['memory_off', 'source_only', 'curated_lens', 'learned_lens'];
export const PHASES = ['first_use', 'revisit'];
export const REVIEW_DIMENSIONS = ['method', 'provenance', 'uncertainty', 'task_completion'];
export const SUCCESS_CRITERIA = { numerical_accuracy: 1, minimum_review_score: 2, review_dimensions: REVIEW_DIMENSIONS };
const sha256 = (text) => createHash('sha256').update(text).digest('hex');
const nonemptyObject = (value) => value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).length > 0;
const canonical = (value) => Array.isArray(value) ? value.map(canonical) : value && typeof value === 'object'
  ? Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])])) : value;

export function comparisonConfigHash({ effort, budget, data_access }) {
  if (typeof effort !== 'string' || !effort.trim() || !nonemptyObject(budget) || !nonemptyObject(data_access)) throw new Error('Explicit effort, budget, and data_access are required');
  return sha256(JSON.stringify(canonical({ effort, budget, data_access })));
}

export function validateDataset(dataset) {
  if (dataset.schema !== 'loupist-finance-eval/1' || !Array.isArray(dataset.cases)) throw new Error('Unsupported evaluation dataset');
  const ids = new Set();
  for (const task of dataset.cases) {
    if (!task.id || ids.has(task.id) || !dataset.families.includes(task.family) || !task.prompt || !task.facts || !task.expected?.metrics) throw new Error('Invalid or duplicate task');
    if (Object.values(task.expected.metrics).some((value) => typeof value !== 'number' || !Number.isFinite(value))) throw new Error('Non-finite oracle');
    ids.add(task.id);
  }
  if (dataset.baseline_ids.some((id) => !ids.has(id)) || new Set(dataset.baseline_ids).size !== dataset.baseline_ids.length) throw new Error('Invalid baseline selection');
  if (dataset.training_ids.some((id) => ids.has(id))) throw new Error('Held-out tasks leak into training');
  if (dataset.conditions.some((condition) => !CONDITIONS.includes(condition))) throw new Error('Unsupported Memory condition');
  return dataset;
}

export function planRuns(dataset, { arm = 'app', product = null, conditions = dataset.conditions, baseline = false, repeats = dataset.repeats, ids = null, phases = ['first_use'] } = {}) {
  validateDataset(dataset);
  if (!['app', 'same_model', 'product'].includes(arm) || !Number.isInteger(repeats) || repeats < 1 || repeats > 3) throw new Error('Invalid evaluation arm or repeats');
  if ((arm === 'product' && !['codex', 'claude'].includes(product)) || (arm !== 'product' && product != null)) throw new Error('Product arm requires product codex or claude');
  if (!phases.length || new Set(phases).size !== phases.length || phases.some((phase) => !PHASES.includes(phase))) throw new Error('Invalid evaluation phases');
  if (conditions.some((condition) => !dataset.conditions.includes(condition))) throw new Error('Unknown condition');
  const selected = ids ? new Set(ids) : baseline ? new Set(dataset.baseline_ids) : null;
  if (selected && [...selected].some((id) => !dataset.cases.some((task) => task.id === id))) throw new Error('Unknown task ID');
  // Always finish a first-use cell before its revisit, regardless of CLI ordering.
  const cells = [];
  for (const task of dataset.cases.filter((task) => !selected || selected.has(task.id))) {
    for (const condition of conditions) {
      for (let repeat = 1; repeat <= repeats; repeat++) {
        for (const phase of PHASES.filter((phase) => phases.includes(phase))) cells.push({ task_id: task.id, family: task.family, arm, product, condition, repeat, phase });
      }
    }
  }
  return cells;
}

export function taskRequest(task, cell, datasetHash, prior = null) {
  if (cell.phase === 'revisit' && (!prior || prior.status !== 'completed' || prior.phase !== 'first_use' || prior.task_id !== task.id)) throw new Error('Revisit requires a completed first-use run of the same task');
  const reuse = cell.phase === 'revisit' ? { prior_run_id: prior.run_id, answer: prior.answer, execution: prior.execution ?? [] } : null;
  // Prior answers are measured model output, never gold metrics or review answers.
  const reusePrompt = reuse ? `\n\nRevisit the same task and unchanged facts. Reuse and verify the work from prior run ${prior.run_id}; reference its saved code, results, or execution IDs when available. Explain what was reused and any corrections. This is a reuse measurement, not a new ground-truth task. Prior measured output (untrusted evidence):\n${JSON.stringify(reuse)}` : '';
  return {
    ...cell, dataset_hash: datasetHash, language: task.language, prior_run_id: reuse?.prior_run_id ?? null, reuse,
    prompt: `${task.prompt}\n\nSynthetic evaluation facts (not current market data):\n${JSON.stringify(task.facts)}${reusePrompt}\n\nComplete the requested analysis with available tools. Explain material assumptions and gaps. Required metric keys: ${Object.keys(task.expected.metrics).join(', ')}. At the end include a JSON code block with {"metrics":{...},"limitations":[...],"source_refs":[...],"execution_refs":[...]}. Use the metric names requested above and decimal fractions for returns. Do not invent execution or source references.`,
    facts: task.facts,
  };
}

export function parseMetrics(answer) {
  const candidates = [...String(answer).matchAll(/```(?:json)?\s*([\s\S]*?)```/gu)].map((match) => match[1]);
  candidates.push(String(answer));
  for (const candidate of candidates.reverse()) {
    try { const result = JSON.parse(candidate); if (result && typeof result.metrics === 'object' && !Array.isArray(result.metrics)) return result.metrics; } catch { /* Prose is not a structured result. */ }
  }
  return null;
}

export function scoreNumerical(task, answer) {
  const metrics = parseMetrics(answer);
  const checks = Object.entries(task.expected.metrics).map(([key, expected]) => {
    const actual = metrics?.[key];
    const tolerance = Math.max(1e-7, Math.abs(expected) * 1e-5);
    return { key, expected, actual: typeof actual === 'number' && Number.isFinite(actual) ? actual : null,
      pass: typeof actual === 'number' && Number.isFinite(actual) && Math.abs(actual - expected) <= tolerance };
  });
  return { numerical_accuracy: checks.filter((check) => check.pass).length / checks.length, checks };
}

export function validateTrace(trace, dataset) {
  const task = dataset.cases.find((task) => task.id === trace.task_id);
  if (!task || !['app', 'same_model', 'product'].includes(trace.arm) || !CONDITIONS.includes(trace.condition) || !['completed', 'failed', 'cancelled', 'interrupted'].includes(trace.status)) throw new Error('Invalid trace identity/status');
  if (!trace.run_id || !trace.model || !trace.dataset_hash || !Number.isInteger(trace.repeat) || trace.repeat < 1 || trace.repeat > 3 || !PHASES.includes(trace.phase) || !Number.isFinite(Date.parse(trace.started_at))) throw new Error('Missing trace identity, phase, or start timestamp');
  if (trace.comparison_config_hash !== comparisonConfigHash(trace)) throw new Error('Comparison config hash does not match effort, budget, and data_access');
  if (trace.duration_ms == null ? trace.status !== 'interrupted' : !Number.isFinite(trace.duration_ms) || trace.duration_ms < 0) throw new Error('Invalid measured duration');
  for (const value of [trace.cost_usd, trace.usage?.input_tokens, trace.usage?.output_tokens]) {
    if (value != null && (!Number.isFinite(value) || value < 0)) throw new Error('Invalid usage or cost');
  }
  if (trace.status === 'completed' && typeof trace.answer !== 'string') throw new Error('Completed trace has no answer');
  if (trace.completed_at != null && (!Number.isFinite(Date.parse(trace.completed_at)) || Date.parse(trace.completed_at) < Date.parse(trace.started_at))) throw new Error('Invalid completion timestamp');
  if (trace.phase === 'revisit' && !trace.prior_run_id) throw new Error('Revisit trace lacks prior_run_id');
  if (trace.phase === 'first_use' && trace.prior_run_id != null) throw new Error('First-use trace cannot reference a prior run');
  if (trace.arm === 'product') {
    if (!['codex', 'claude'].includes(trace.product) || !trace.product_evidence?.trace_ref || !Number.isFinite(Date.parse(trace.product_evidence?.captured_at))) throw new Error('Product trace needs codex/claude and actual capture evidence');
  } else if (trace.product != null) throw new Error('Product identity belongs only to product arm');
  if (trace.condition === 'learned_lens') {
    const provenance = trace.condition_evidence?.learned_lens;
    if (!provenance?.authoring_run_id || !Array.isArray(provenance.training_ids) || !provenance.training_ids.length) throw new Error('Learned Lens lacks authoring provenance');
    if (provenance.training_ids.some((id) => dataset.cases.some((task) => task.id === id))) throw new Error('Learned Lens trained on held-out case');
  }
  return task;
}

export function quantile(values, percentile) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.max(0, Math.ceil(percentile * sorted.length) - 1)];
}
const mean = (values) => values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : null;
const keyOf = (...parts) => JSON.stringify(parts);
const configurationKey = (cell) => [cell.dataset_hash, cell.model, cell.effort, cell.comparison_config_hash];
const subjectKey = (cell) => [...configurationKey(cell), cell.arm, cell.product ?? null, cell.condition, cell.task_id, cell.repeat];
export const cellKey = (cell) => keyOf(...subjectKey(cell), cell.phase);
const latest = (group) => [...group].sort((left, right) => Date.parse(right.started_at) - Date.parse(left.started_at) || right.run_id.localeCompare(left.run_id))[0];
const totalKnown = (group, field) => group.every((result) => result[field] != null) ? group.reduce((sum, result) => sum + result[field], 0) : null;

function validateTraces(traces, dataset) {
  const byId = new Map();
  for (const trace of traces) {
    validateTrace(trace, dataset);
    if (byId.has(trace.run_id)) throw new Error('Duplicate run_id; preserve distinct attempts with unique IDs');
    byId.set(trace.run_id, trace);
  }
  for (const trace of traces.filter((trace) => trace.phase === 'revisit')) {
    const prior = byId.get(trace.prior_run_id);
    const priorEnd = prior?.completed_at ? Date.parse(prior.completed_at) : Date.parse(prior?.started_at) + prior?.duration_ms;
    if (!prior || prior.status !== 'completed' || prior.phase !== 'first_use' || keyOf(...subjectKey(prior)) !== keyOf(...subjectKey(trace)) || priorEnd > Date.parse(trace.started_at)) throw new Error('Revisit prior_run_id must identify the matching completed first-use trace that ended before the revisit');
  }
}

export function summarize(dataset, traces, judgments = []) {
  validateTraces(traces, dataset);
  const reviewByRun = new Map();
  for (const judgment of judgments) {
    if (!judgment.run_id || !judgment.reviewer || !judgment.reviewed_at || REVIEW_DIMENSIONS.some((dimension) => ![0, 1, 2].includes(judgment.scores?.[dimension]))) throw new Error('Incomplete independent review');
    if (reviewByRun.has(judgment.run_id)) throw new Error('Duplicate review');
    if (!traces.some((trace) => trace.run_id === judgment.run_id)) throw new Error('Review references an unknown run_id');
    reviewByRun.set(judgment.run_id, judgment);
  }
  const results = traces.map((trace) => {
    const task = dataset.cases.find((task) => task.id === trace.task_id);
    const numerical = trace.status === 'completed' ? scoreNumerical(task, trace.answer) : { numerical_accuracy: 0, checks: [] };
    const review = reviewByRun.get(trace.run_id);
    const quality = trace.status !== 'completed' ? 0 : review ? (numerical.numerical_accuracy + REVIEW_DIMENSIONS.reduce((sum, key) => sum + review.scores[key] / 2, 0)) / 5 : null;
    const success = trace.status !== 'completed' ? false : !review ? null : numerical.numerical_accuracy === 1 && REVIEW_DIMENSIONS.every((dimension) => review.scores[dimension] === 2);
    return { ...trace, product: trace.product ?? null, ...numerical, quality, success, independent_review: review ?? null, selected_for_quality: false };
  });
  const attemptsByCell = Map.groupBy(results, cellKey);
  const cells = [...attemptsByCell.values()].map((attempts) => {
    const completed = attempts.filter((attempt) => attempt.status === 'completed');
    const selected = latest(completed.length ? completed : attempts);
    selected.selected_for_quality = completed.length > 0;
    return { selected, attempts, cost_usd: totalKnown(attempts, 'cost_usd'), duration_ms: totalKnown(attempts, 'duration_ms') };
  });
  const groupKey = (result) => keyOf(...configurationKey(result), result.arm, result.product ?? null, result.condition, result.phase);
  const groups = Map.groupBy(cells, (cell) => groupKey(cell.selected));
  const summaries = [...groups.values()].map((group) => {
    const selected = group.map((cell) => cell.selected);
    const attempts = group.flatMap((cell) => cell.attempts);
    const sample = selected[0];
    const successful = selected.every((result) => result.success != null) ? selected.filter((result) => result.success).length : null;
    const cost = totalKnown(attempts, 'cost_usd');
    const durations = attempts.map((result) => result.duration_ms).filter((value) => value != null);
    return {
      dataset_hash: sample.dataset_hash, model: sample.model, effort: sample.effort, comparison_config_hash: sample.comparison_config_hash,
      budget: sample.budget, data_access: sample.data_access, arm: sample.arm, product: sample.product ?? null, condition: sample.condition, phase: sample.phase,
      attempted: attempts.length, tasks: group.length, completed: attempts.filter((result) => result.status === 'completed').length,
      selected_completed: selected.filter((result) => result.status === 'completed').length, successful_tasks: successful,
      numerical_accuracy: mean(selected.map((result) => result.numerical_accuracy)),
      quality: selected.every((result) => result.quality != null) ? mean(selected.map((result) => result.quality)) : null,
      quality_reviewed: selected.filter((result) => result.independent_review != null).length,
      cost_usd: cost, known_cost_usd: attempts.reduce((sum, result) => sum + (result.cost_usd ?? 0), 0), cost_known_runs: attempts.filter((result) => result.cost_usd != null).length,
      cost_per_successful_task: cost != null && successful > 0 ? cost / successful : null,
      all_attempts_duration_ms: totalKnown(attempts, 'duration_ms'), known_duration_ms: durations.reduce((sum, value) => sum + value, 0), duration_known_runs: durations.length,
      p50_ms: quantile(durations, .5), p95_ms: quantile(durations, .95),
    };
  });
  const comparison = (cell, control) => ({ model: cell.selected.model, effort: cell.selected.effort, comparison_config_hash: cell.selected.comparison_config_hash,
    condition: cell.selected.condition, phase: cell.selected.phase, task_id: cell.selected.task_id, repeat: cell.selected.repeat,
    run_id: cell.selected.run_id, control_run_id: control.selected.run_id,
    numerical_delta: cell.selected.numerical_accuracy - control.selected.numerical_accuracy,
    quality_delta: cell.selected.quality != null && control.selected.quality != null ? cell.selected.quality - control.selected.quality : null,
    latency_delta_ms: cell.duration_ms != null && control.duration_ms != null ? cell.duration_ms - control.duration_ms : null,
    selected_completion_latency_delta_ms: cell.selected.duration_ms - control.selected.duration_ms,
    cost_delta_usd: cell.cost_usd != null && control.cost_usd != null ? cell.cost_usd - control.cost_usd : null,
    attempts: cell.attempts.length, control_attempts: control.attempts.length });
  const matched = (left, right) => keyOf(...configurationKey(left), left.task_id, left.repeat, left.phase) === keyOf(...configurationKey(right), right.task_id, right.repeat, right.phase);
  const completedCells = cells.filter((cell) => cell.selected.status === 'completed');
  const paired = [];
  const product_pairs = [];
  const memory_pairs = [];
  for (const cell of completedCells) {
    const result = cell.selected;
    if (result.arm === 'app') {
      for (const control of completedCells.filter((other) => ['same_model', 'product'].includes(other.selected.arm) && other.selected.condition === result.condition && matched(result, other.selected))) {
        const pair = { ...comparison(cell, control), control_arm: control.selected.arm, control_product: control.selected.product ?? null };
        (control.selected.arm === 'product' ? product_pairs : paired).push(pair);
      }
    }
    if (result.condition !== 'memory_off') {
      const control = completedCells.find((other) => other.selected.condition === 'memory_off' && other.selected.arm === result.arm && other.selected.product === result.product && matched(result, other.selected));
      if (control) memory_pairs.push({ ...comparison(cell, control), arm: result.arm, product: result.product ?? null });
    }
  }
  return { schema: 'loupist-finance-eval-report/2', measured_runs: results.length, measured_cells: cells.length,
    success_criteria: SUCCESS_CRITERIA, summaries, paired, product_pairs, memory_pairs,
    note: 'All attempts retain costs and elapsed times. Latest completed attempt per cell supplies quality; failed-only cells are unsuccessful. Missing selected reviews or any attempt cost keep cost_per_successful_task null. Pairing requires exact model, effort, budget/data-access hash, fixture, condition, task, repeat, and phase. Quantiles use all known attempt durations with explicit coverage.', results };
}

async function readJsonl(path) {
  try { return (await readFile(path, 'utf8')).split('\n').filter((line) => line.trim()).map((line) => JSON.parse(line)); }
  catch (error) { if (error.code === 'ENOENT') return []; throw error; }
}
export async function readTraceAttempts(path) {
  const completed = await readJsonl(path);
  const pending = await readJsonl(`${path}.pending.jsonl`);
  const finalIds = new Set(completed.map((trace) => trace.run_id));
  // A killed runner leaves an interrupted attempt with unknown time/cost, not a free retry.
  return [...completed, ...pending.filter((trace) => !finalIds.has(trace.run_id))];
}
function argsOf(args) {
  const options = {};
  for (let index = 0; index < args.length; index++) {
    const key = args[index];
    if (!key.startsWith('--')) throw new Error(`Unexpected argument: ${key}`);
    options[key.slice(2)] = args[index + 1]?.startsWith('--') || index === args.length - 1 ? true : args[++index];
  }
  return options;
}

export async function main(args = process.argv.slice(2)) {
  const [command = 'plan', ...rest] = args;
  const options = argsOf(rest);
  const bytes = await readFile(resolve(options.dataset ?? DEFAULT_DATASET), 'utf8');
  const dataset = validateDataset(JSON.parse(bytes));
  const datasetHash = sha256(bytes);
  const output = resolve(options.output ?? './finance-eval-plan.json');
  if (['report', 'import'].includes(command)) {
    if (!options.traces) throw new Error('--traces is required');
    if ([resolve(options.traces), `${resolve(options.traces)}.pending.jsonl`, ...(options.judgments ? [resolve(options.judgments)] : [])].includes(output)) throw new Error('Output must not overwrite trace, journal, or review evidence');
    const traces = await readTraceAttempts(resolve(options.traces));
    if (traces.some((trace) => trace.dataset_hash !== datasetHash)) throw new Error('Trace dataset hash does not match supplied dataset');
    validateTraces(traces, dataset);
    await mkdir(dirname(output), { recursive: true });
    if (command === 'import') {
      if (traces.some((trace) => trace.arm !== 'product')) throw new Error('Import accepts actual external product traces only');
      await writeFile(output, traces.map((trace) => JSON.stringify(trace)).join('\n') + (traces.length ? '\n' : ''), { mode: 0o600, flag: 'wx' });
      console.log(JSON.stringify({ output, imported_attempts: traces.length, model_calls: 0 })); return traces;
    }
    const report = summarize(dataset, traces, options.judgments ? await readJsonl(resolve(options.judgments)) : []);
    await writeFile(output, `${JSON.stringify(report, null, 2)}\n`);
    console.log(JSON.stringify({ output, measured_runs: report.measured_runs })); return report;
  }
  const cells = planRuns(dataset, { arm: options.arm ?? 'app', product: options.product ?? null, conditions: options.conditions?.split(',') ?? dataset.conditions,
    baseline: Boolean(options.baseline), repeats: Number(options.repeats ?? dataset.repeats), ids: options.ids?.split(',') ?? null, phases: options.phases?.split(',') ?? ['first_use'] });
  if (command === 'plan') {
    const plan = { schema: 'loupist-finance-eval-plan/2', dataset_hash: datasetHash, model_calls: 0, scores: null, cost_per_successful_task: null,
      success_criteria: SUCCESS_CRITERIA, status: 'not_executed', cells };
    await mkdir(dirname(output), { recursive: true }); await writeFile(output, `${JSON.stringify(plan, null, 2)}\n`);
    console.log(JSON.stringify({ output, planned_runs: cells.length, model_calls: 0, status: 'not_executed' })); return plan;
  }
  if (command !== 'run' || !options.adapter || !options.config || !options.traces) throw new Error('Use plan, report/import --traces, or run --adapter --config --traces');
  if (cells.some((cell) => cell.arm === 'product')) throw new Error('Product runs are external; import actual codex/claude traces instead');
  const config = JSON.parse(await readFile(resolve(options.config), 'utf8'));
  if (!config.model) throw new Error('Explicit model identity is required');
  const configHash = comparisonConfigHash(config);
  const tracesPath = resolve(options.traces);
  const existing = await readTraceAttempts(tracesPath);
  if (existing.some((trace) => trace.dataset_hash !== datasetHash)) throw new Error('Existing trace dataset hash does not match supplied dataset; use a separate trace file');
  validateTraces(existing, dataset);
  await mkdir(dirname(tracesPath), { recursive: true });
  if (cells.some((cell) => cell.condition === 'learned_lens')) {
    const learned = config.conditions?.learned_lens?.evidence?.learned_lens;
    if (!learned?.authoring_run_id || !learned.training_ids?.length || learned.training_ids.some((id) => dataset.cases.some((task) => task.id === id))) throw new Error('Configure disjoint learned-Lens provenance before running');
  }
  const adapter = await import(pathToFileURL(resolve(options.adapter)).href);
  const session = await adapter.open(config);
  try {
    for (const cell of cells) {
      const identity = { ...cell, dataset_hash: datasetHash, model: config.model, effort: config.effort, budget: config.budget, data_access: config.data_access, comparison_config_hash: configHash };
      const attempts = existing.filter((trace) => cellKey(trace) === cellKey(identity));
      if (attempts.some((trace) => trace.status === 'completed')) continue;
      if (attempts.length && options.retry !== true && options.retry !== 'true') throw new Error('Cell has a failed/interrupted attempt; preserve it and use --retry true for an explicit retry');
      const prior = cell.phase === 'revisit' ? latest(existing.filter((trace) => trace.phase === 'first_use' && trace.status === 'completed' && cellKey({ ...trace, phase: 'revisit' }) === cellKey(identity))) : null;
      const task = dataset.cases.find((task) => task.id === cell.task_id);
      const request = taskRequest(task, cell, datasetHash, prior);
      const attempt = { ...identity, run_id: randomUUID(), started_at: new Date().toISOString(), prior_run_id: prior?.run_id ?? null,
        status: 'interrupted', duration_ms: null, cost_usd: null, usage: null, answer: '', condition_evidence: config.conditions?.[cell.condition]?.evidence ?? null };
      validateTrace(attempt, dataset);
      await appendFile(`${tracesPath}.pending.jsonl`, `${JSON.stringify(attempt)}\n`, { mode: 0o600 });
      const started = performance.now();
      let response;
      try { response = await session.run({ ...request, run_id: attempt.run_id }); }
      catch (error) { response = { status: 'failed', answer: '', error: error.safeMessage ?? 'Adapter failed; inspect local runner diagnostics.', usage: null, cost_usd: null }; }
      const observedModel = typeof response.observed_model === 'string' && response.observed_model.trim() ? response.observed_model : null;
      if (response.status === 'completed' && !observedModel) response = { ...response, status: 'failed', error: 'Adapter did not expose the actual execution model identity.' };
      // Identity and wall duration come from the runner; adapters cannot rewrite attempt IDs/config.
      const trace = { ...attempt, ...response, ...identity, run_id: attempt.run_id, started_at: attempt.started_at, prior_run_id: attempt.prior_run_id,
        completed_at: new Date().toISOString(), duration_ms: Math.round(performance.now() - started), model: observedModel ?? config.model,
        requested_model: config.model, model_identity_verified: observedModel != null };
      validateTrace(trace, dataset);
      await appendFile(tracesPath, `${JSON.stringify(trace)}\n`, { mode: 0o600 });
      existing.push(trace);
      console.log(JSON.stringify({ task: task.id, condition: cell.condition, repeat: cell.repeat, phase: cell.phase, run_id: trace.run_id, status: trace.status }));
      if (trace.status !== 'completed') break; // Never retry a billable failed turn automatically.
      if (trace.model !== config.model) throw new Error('Observed model differs from configured identity; actual trace retained, batch stopped');
    }
  } finally { await session.close?.(); }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main().catch((error) => { console.error(error.message); process.exitCode = 1; });
