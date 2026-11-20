/** Deterministic graders for agent-suite traces. No model calls. */
import { scoreNumerical } from '../evaluate.mjs';

import { TRACE_ASSERTS, MEMORY_ASSERTS, MEMORY_KINDS, ANSWER_ASSERTS,
  turnFields, turnCompleted, validateSuite, validateTraceRecord, locateTurn,
  toolCalls, memoryCommits, answerEvidence, visibleAnswerText, withoutEmphasis, numberSpellings, answerPatterns, validDate, executionMismatches,
  citationAssertion, returnedCitationUrls, citingParagraphs, numbersLinkedAssertion, asOfLeftOutAssertion,
  asOfRefusedAssertion, calculateInputsAssertion, metricTolerance, numberChecks, asOfReport,
  toolCalledAssertion, argumentsInclude, isRecord,
  LENS_ASSERTS, lensAssertion, lensPages, memoryReads, isNegativeAssertion, toolOrderAssertion } from './eval-contract.mjs';
import { analyzeToolCalls, toolValidity, comparableArgs, toolExecutionState, skillReadEvidence, capabilityLoadEvidence, firstPackUseEvidence, hostResult } from './tool-evidence.mjs';
import { noFalseSaveClaimEvidence } from './memory-evidence.mjs';
import { scheduledDeliveryEvidence } from './scheduled-evidence.mjs';
import { visibleLabelsEvidence, visibleLabels } from './label-evidence.mjs';
import { answerLanguageEvidence } from './language-evidence.mjs';
import { figuresPresentEvidence, matchingNumber } from './figure-evidence.mjs';
import { gridCellsEvidence } from './grid-evidence.mjs';
import { sourceSpanEvidence } from './source-evidence.mjs';
import { calculationChipEvidence } from './research-evidence.mjs';

export function trailingJsonObject(text) {
  const trimmed = String(text ?? '').trimEnd();
  if (!trimmed.endsWith('}')) return null;
  for (let index = trimmed.lastIndexOf('{'); index >= 0; index = index === 0 ? -1 : trimmed.lastIndexOf('{', index - 1)) {
    const slice = trimmed.slice(index);
    try {
      const value = JSON.parse(slice);
      if (value && typeof value === 'object' && !Array.isArray(value)) return slice;
    } catch { /* An earlier brace starts the trailing object. */ }
  }
  return null;
}

export function parsePredictionBlock(answer) {
  const text = String(answer ?? '');
  const fenced = [...text.matchAll(/```(?:json)?\s*([\s\S]*?)```/gu)].map((match) => match[1]).reverse();
  const trailing = trailingJsonObject(text);
  for (const candidate of [trailing, ...fenced, text].filter(Boolean)) {
    try {
      const value = JSON.parse(String(candidate).trim());
      if (value?.prediction && typeof value.prediction === 'object' && !Array.isArray(value.prediction)) return value.prediction;
    } catch { /* Prose and non-prediction objects are ignored. */ }
  }
  return null;
}

export function predictionIssues(prediction) {
  if (!prediction) return ['prediction'];
  const issues = [];
  if (typeof prediction.claim !== 'string' || !prediction.claim.trim()) issues.push('claim');
  if (typeof prediction.metric !== 'string' || !prediction.metric.trim()) issues.push('metric');
  if (prediction.direction !== 'above' && prediction.direction !== 'below') issues.push('direction');
  if (typeof prediction.threshold !== 'number' || !Number.isFinite(prediction.threshold)) issues.push('threshold');
  if (!validDate(prediction.horizon)) issues.push('horizon');
  if (typeof prediction.probability !== 'number' || !Number.isFinite(prediction.probability) || prediction.probability < 0 || prediction.probability > 1) issues.push('probability');
  return issues;
}

function predictionDetail(issues, passDetail) {
  if (!issues.length) return passDetail;
  return `${issues[0] === 'prediction' ? 'missing' : 'invalid'} ${issues.join(', ')}`;
}

const EVIDENCE_LINK = /\[[^\]\n]*\]\(#evidence-ev_[0-9a-f]{32}\)/gu;
const NUMBER = /\d+(?:,\d{3})*(?:\.\d+)?/gu;
const UNIT_AFTER = /^\s?(?:%p|%pt|%포인트|%|％|pp|ppt|bps?\b|basis points?|percentage points?|percent\b|pct\b|배|x\b|×|times\b|조|억|천만|백만|만|천|bn\b|billion\b|mn\b|million\b|tn\b|trillion\b|k\b|m\b|b\b)/iu;
const DATE_OR_ORDINAL_AFTER = /^\s?(?:년|월|일|번째|분기|기(?![가-힣])|st\b|nd\b|rd\b|th\b)/u;
const PERCENT_UNIT = /^\s?(?:%|％|percent|pct)$/iu;
const CURRENCY_CODE = /(?:krw|usd|eur|jpy|gbp|cny)/iu;
const CURRENCY_PREFIX = new RegExp(String.raw`[$₩€£¥￦]|\b${CURRENCY_CODE.source}`, 'iu');
const CURRENCY_BEFORE = new RegExp(String.raw`(?:${CURRENCY_PREFIX.source})\s?[-−]?$`, 'iu');
const CURRENCY_AFTER = new RegExp(String.raw`^\s?(?:원|달러|엔|위안|ドル|(?:${CURRENCY_CODE.source}|won|dollars?)\b)`, 'iu');
const FIGURE_AFTER = new RegExp(
  String.raw`^[ \t]*\[?(?:${CURRENCY_PREFIX.source})?[ \t]*[+−-]?${NUMBER.source}(?!\d|[.,/:-]\d|[-_][a-z]|[a-z]\d)`, 'iu',
);
// Keep UTF-16 offsets into the original answer, including links with astral characters.
const blank = (text) => text.replace(/[^\n]/g, ' ');

function isRateBase(answer, match, currencyUnit) {
  if (!currencyUnit || !/^1(?:\.00?)?$/u.test(match[0])) return false;
  const before = answer.slice(0, match.index);
  if (/[+−△▲-]$/u.test(before)) return false;
  if (/\b(?:per|for[ \t]+each)[ \t]+$/iu.test(before)) return true;
  const after = answer.slice(match.index + match[0].length + currencyUnit.length);
  if (/^(?:당(?![가-힣])|あたり)/u.test(after)) return true;
  // 에 can also introduce a purchase price. Require the quoted figure in that case.
  const marker = /^(?:에|[ \t]*=)/u.exec(after);
  return !!marker && FIGURE_AFTER.test(after.slice(marker[0].length));
}

/**
 * The percent axes of the DCF grids a calculation returned this turn: the discount and
 * terminal growth rates it was given, not figures read from a source.
 */
export function gridAxisPercents(calls) {
  const values = new Set();
  for (const call of calls ?? []) {
    if (call?.name !== 'calculate' || call.outcome !== 'ok') continue;
    const grid = hostResult(call.result)?.grid;
    for (const [name, axis] of [[grid?.rows, grid?.row_values], [grid?.columns, grid?.column_values]]) {
      if (typeof name === 'string' && name.endsWith('_pct') && Array.isArray(axis)) {
        for (const value of axis) if (Number.isFinite(value)) values.add(value);
      }
    }
  }
  return values;
}

/**
 * Material numbers in an answer that are not inside an evidence link
 * (finance-show-the-math.md §3.1): amounts with a currency or scale, percents,
 * points, multiples, grouped or decimal figures and large bare numbers. Years,
 * dates, periods glued to letters (FY2025, Q3), ordinals, small counts, code,
 * the answer's trailing JSON block and percents equal to a value in `axisPercents`
 * (a grid's assumed rates, sign included) stay out.
 */
export function unlinkedNumbers(answer, user = '', { axisPercents = new Set() } = {}) {
  // A prompt can name a listing as either 990221 or 솔숲식품(990221).
  // Keep only whole tokens, so a longer ID, decimal or grouped amount cannot supply a code.
  const userCodes = new Set(typeof user === 'string' ? user.match(/\b(?<!\d[.,])\d{6}\b(?![.,]\d)/gu) : []);
  const original = String(answer ?? '');
  let text = original;
  const trailing = trailingJsonObject(text);
  if (trailing) text = text.slice(0, text.trimEnd().length - trailing.length);
  text = text.replace(/```[\s\S]*?```/gu, blank).replace(/`[^`\n]*`/gu, blank)
    .replace(EVIDENCE_LINK, blank)
    .replace(/\]\([^)\s]*\)/gu, (target) => `]${blank(target.slice(1))}`)
    .replace(/https?:\/\/\S+/gu, blank)
    .replace(/\d+(?:[-/:]\d+)+/gu, blank)
    .replace(/\d+(?:\.\d+){2,}/gu, blank);
  const found = [];
  for (const match of text.matchAll(NUMBER)) {
    const start = match.index;
    const end = start + match[0].length;
    const previous = text[start - 1] ?? '';
    const before = text[start - 2] ?? '';
    if (/[A-Za-z0-9_]/u.test(previous) || (/[-/.:]/u.test(previous) && /[A-Za-z0-9_]/u.test(before))) continue;
    const rest = text.slice(end);
    // Match the ticker forms skipped by the host's text_numbers.
    if (/^\d{6}$/u.test(match[0]) && ((previous === '(' && rest.startsWith(')')) || /^\.[A-Z]/u.test(rest))) continue;
    if (/^[A-Za-z]\d/u.test(rest) || /^[-_][A-Za-z]/u.test(rest)) continue;
    if (DATE_OR_ORDINAL_AFTER.test(rest)) continue;
    const currencyUnit = CURRENCY_AFTER.exec(rest);
    if (isRateBase(original, match, currencyUnit?.[0])) continue;
    const unit = currencyUnit ?? UNIT_AFTER.exec(rest);
    const currency = CURRENCY_BEFORE.test(text.slice(Math.max(0, start - 6), start).trimEnd());
    if (userCodes.has(match[0]) && !unit && !currency
      && !/[+−-]/u.test(previous) && !/^(?:\w|\.[A-Za-z])/u.test(rest)) continue;
    const grouped = match[0].includes(',');
    const decimal = match[0].includes('.');
    const value = Number(match[0].replaceAll(',', ''));
    if (unit && PERCENT_UNIT.test(unit[0]) && axisPercents.has(/[-−]/u.test(previous) ? -value : value)) continue;
    const year = !grouped && !decimal && match[0].length === 4 && value >= 1900 && value <= 2100;
    const material = currency || unit || grouped || decimal || (value >= 1000 && !year);
    if (material) found.push(`${match[0]}${unit ? unit[0].trimStart() : ''}`);
  }
  return found;
}

const comparable = (text) => String(text).replace(/[\s,]/gu, '');

function numbersLinkedEvidence(assertion, trace, answer) {
  const { checked } = numbersLinkedAssertion(assertion);
  if (!('number_checks' in trace)) return { pass: null, detail: 'number checks unavailable' };
  const checks = numberChecks(trace);
  if (checks === null) return { pass: null, detail: 'number checks unavailable' };
  const problems = [];
  const flagged = checks.filter((check) => check.status === 'differs' || check.status === 'not_found');
  if (flagged.length) problems.push(`checks ${flagged.map((check) => `${check.text} ${check.status}`).join(', ')}`);
  const unlinked = unlinkedNumbers(answer, trace.user, { axisPercents: gridAxisPercents(toolCalls(trace)) });
  if (unlinked.length) problems.push(`unlinked ${unlinked.slice(0, 5).join(', ')}`);
  const missing = checked.filter((value) => !checks.some((check) => (check.status === 'matched' || check.status === 'rounded')
    && comparable(check.text).includes(comparable(value))));
  if (missing.length) problems.push(`no matched check for ${missing.join(', ')}`);
  return { pass: problems.length === 0, detail: problems.length ? problems.join('; ') : `${checks.length} number links checked` };
}

function asOfEvidence(assertion, trace) {
  const report = asOfReport(trace);
  if (report === undefined) return { pass: null, detail: 'as-of report unavailable' };
  if (assertion.assert === 'as_of_left_out') {
    const { kind, min } = asOfLeftOutAssertion(assertion);
    if (report === null) return { pass: false, detail: 'no as-of report' };
    const count = report.left_out?.[kind];
    if (!Number.isInteger(count)) return { pass: null, detail: `as-of ${kind} count unavailable` };
    return { pass: count >= min, detail: `${count} ${kind} left out, min ${min}` };
  }
  const { tool } = asOfRefusedAssertion(assertion);
  if (report === null) return { pass: false, detail: 'no as-of report' };
  const refused = Array.isArray(report.refused) ? report.refused : [];
  const pass = refused.some((item) => item?.tool === tool);
  return { pass, detail: pass ? `${tool} refused under the cutoff` : `${tool} not listed as refused` };
}

function calculateInputsEvidence(assertion, calls) {
  const { inputs, operation } = calculateInputsAssertion(assertion);
  const relevant = calls.filter((call) => call.name === 'calculate' && call.outcome !== 'error'
    && (operation === null || call.args?.operation === operation));
  if (!relevant.length) return { pass: false, detail: `no ${operation ?? ''} calculate call`.replace('  ', ' ') };
  const cites = (call) => inputs.every((name) => typeof call.args?.input_sources?.[name] === 'string'
    && call.args.input_sources[name].startsWith('#evidence-'));
  if (relevant.some(cites)) return { pass: true, detail: `calculate cites ${inputs.join(', ')}` };
  if (relevant.some((call) => comparableArgs(call.args) === null)) return { pass: null, detail: 'calculate arguments unavailable' };
  return { pass: false, detail: `calculate does not cite ${inputs.join(', ')}` };
}

function resultOf(trace, assertion, pass, detail) {
  const completed = turnCompleted(trace);
  if (isNegativeAssertion(assertion)) {
    const answer = answerEvidence(trace);
    if (answer === null && pass !== false) { pass = null; detail = 'answer evidence unavailable'; }
    else if (answer !== null && !answer.trim()) { pass = false; detail = 'negative assertion requires a non-empty answer'; }
  }
  return {
    run_id: trace.run_id, ...turnFields(trace), assert: assertion.assert,
    metric: typeof assertion.metric === 'string' ? assertion.metric : null,
    pass: completed === true ? pass : completed,
    detail: completed === true ? detail : completed === false ? 'turn did not complete' : 'turn completion unavailable',
  };
}

export function scheduleStateEvidence(assertion, trace) {
  if (!Number.isInteger(assertion.count) || assertion.count < 0 || !Array.isArray(assertion.tasks ?? [])) throw new Error('scheduled_tasks requires count and optional task patterns');
  const tasks = trace.scheduled_tasks;
  if (!Array.isArray(tasks) || tasks.some((task) => !task || typeof task.task_id !== 'string' || typeof task.status !== 'string')) return { pass: null, detail: 'saved schedule state unavailable' };
  const subset = (actual, expected) => Array.isArray(expected)
    ? Array.isArray(actual) && actual.length === expected.length && expected.every((value, i) => subset(actual[i], value))
    : expected && typeof expected === 'object'
      ? actual && typeof actual === 'object' && Object.entries(expected).every(([key, value]) => subset(actual[key], value))
      : actual === expected;
  const patterns = assertion.tasks ?? [];
  const matches = (task, pattern) => {
    const { prompt_includes, ...fields } = pattern;
    return subset(task, fields) && (prompt_includes === undefined ||
      (Array.isArray(prompt_includes) && typeof task.prompt === 'string' && prompt_includes.every((text) => task.prompt.includes(text))));
  };
  // Find distinct matches even when a broad pattern precedes a specific one.
  const owners = new Map();
  const assign = (patternIndex, visited) => {
    for (let taskIndex = 0; taskIndex < tasks.length; taskIndex++) {
      if (visited.has(taskIndex) || !matches(tasks[taskIndex], patterns[patternIndex])) continue;
      visited.add(taskIndex);
      if (!owners.has(taskIndex) || assign(owners.get(taskIndex), visited)) {
        owners.set(taskIndex, patternIndex);
        return true;
      }
    }
    return false;
  };
  const unmatched = [];
  for (const [index, expected] of patterns.entries()) {
    if (!assign(index, new Set())) unmatched.push(expected.title ?? JSON.stringify(expected));
  }
  const duplicates = new Set(tasks.map((task) => task.task_id)).size !== tasks.length;
  return { pass: tasks.length === assertion.count && !unmatched.length && !duplicates,
    detail: `${tasks.length} saved tasks, expected ${assertion.count}${unmatched.length ? `; unmatched: ${unmatched.join(', ')}` : ''}${duplicates ? '; duplicate task IDs' : ''}` };
}

function evaluateTraceAssertion(assertion, trace, scenario, turn, traces) {
  if (!TRACE_ASSERTS.has(assertion.assert)) throw new Error(`Unknown trace assertion ${assertion.assert}`);
  if (assertion.assert === 'scheduled_tasks') {
    const result = scheduleStateEvidence(assertion, trace);
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  if (assertion.assert === 'as_of_left_out' || assertion.assert === 'as_of_refused') {
    const result = asOfEvidence(assertion, trace);
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  if (assertion.assert === 'followed_names') {
    const rows = trace.followed_assets;
    const calls = toolCalls(trace);
    if (!Array.isArray(rows) || !calls) return resultOf(trace, assertion, null, 'persisted follows or tool evidence unavailable');
    const submitted = calls.filter(call => call.name === 'loupist_setup' && call.outcome === 'ok')
      .flatMap(call => Array.isArray(call.args?.follow) ? call.args.follow.map(asset => asset?.name) : []);
    const names = rows.map(row => row?.name);
    const pass = names.length === assertion.names.length && assertion.names.every(name => names.includes(name) && submitted.includes(name));
    const lookup = calls.some(call => call.name === 'asset_lookup');
    return resultOf(trace, assertion, pass, `compared submitted and persisted names; asset_lookup called: ${lookup}`);
  }
  if (assertion.assert === 'replayed_price') {
    const price = trace.replayed_price;
    if (!isRecord(price)) return resultOf(trace, assertion, null, 'host replay evidence unavailable');
    const pass = price.asset === assertion.asset && price.state === 'ready' && price.value === assertion.value && price.date === assertion.date;
    return resultOf(trace, assertion, pass, 'compared the replay target, state, stored price and provider date');
  }
  if (assertion.assert === 'scheduled_delivery' || assertion.assert === 'scheduled_quiet') {
    const result = scheduledDeliveryEvidence(trace, scenario, turn, traces, assertion.assert === 'scheduled_quiet');
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  const calls = toolCalls(trace);
  if (calls === null) return resultOf(trace, assertion, null, 'tool call evidence unavailable');
  if (assertion.assert === 'skill_read') {
    const result = skillReadEvidence(assertion, calls);
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  if (assertion.assert === 'calculate_inputs_cited') {
    const result = calculateInputsEvidence(assertion, calls);
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  if (assertion.assert === 'grid_cells_linked') {
    const result = gridCellsEvidence(assertion, calls);
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  if (assertion.assert === 'tool_args_not_matching') {
    const patterns = answerPatterns(assertion);
    const relevant = calls.filter((call) => (!assertion.tool || call.name === assertion.tool) && toolExecutionState(call) !== false);
    const hit = relevant.find((call) => toolExecutionState(call) === true && patterns.some((pattern) => pattern.test(JSON.stringify(call.args ?? null))));
    const unknown = relevant.some((call) => toolExecutionState(call) === null || comparableArgs(call.args) === null);
    return resultOf(trace, assertion, hit ? false : unknown ? null : true, hit ? `forbidden arguments in ${hit.name}` : unknown ? 'tool arguments unavailable' : 'no forbidden tool arguments');
  }
  if (assertion.assert === 'tool_called') {
    const { tool, args, successful } = toolCalledAssertion(assertion);
    if (successful) {
      const relevant = calls.filter(call => call.name === tool && call.outcome !== 'error');
      const matches = call => args === null || argumentsInclude(call.args, args);
      const pass = relevant.some(call => call.outcome === 'ok' && matches(call)) ? true
        : relevant.some(call => (call.outcome !== 'ok' && matches(call)) || (args !== null && comparableArgs(call.args) === null)) ? null : false;
      return resultOf(trace, assertion, pass, pass ? `successfully called ${tool}` : pass === null ? `${tool} evidence unavailable` : `no successful ${tool} call`);
    }
    if (args === null) {
      const pass = calls.some((call) => call.name === tool);
      return resultOf(trace, assertion, pass, pass ? `called ${tool}` : `not called ${tool}`);
    }
    // With expected arguments, only a call that did not fail counts.
    const relevant = calls.filter((call) => call.name === tool && call.outcome !== 'error');
    if (relevant.some((call) => argumentsInclude(call.args, args))) return resultOf(trace, assertion, true, `called ${tool} with ${JSON.stringify(args)}`);
    const unknown = relevant.some((call) => comparableArgs(call.args) === null);
    return resultOf(trace, assertion, unknown ? null : false, unknown ? `${tool} arguments unavailable` : `no ${tool} call with ${JSON.stringify(args)}`);
  }
  if (assertion.assert === 'tool_not_called') {
    if (assertion.tool === 'interface_labels') {
      const attempts = calls.filter(call => call.name === assertion.tool).length;
      return resultOf(trace, assertion, attempts === 0, `${attempts} interface_labels attempts`);
    }
    const states = calls.filter(call => call.name === assertion.tool).map(toolExecutionState);
    const pass = states.includes(true) ? false : states.includes(null) ? null : true;
    return resultOf(trace, assertion, pass, pass === null ? `${assertion.tool} execution unavailable` : pass ? `did not execute ${assertion.tool}` : `executed ${assertion.tool}`);
  }
  if (assertion.assert === 'tool_called_before') {
    const { tools, id } = toolOrderAssertion(assertion);
    if (tools[0] === 'capability_load') {
      const result = firstPackUseEvidence(id, tools, calls);
      return resultOf(trace, assertion, result.pass, result.detail);
    }
    const indexes = tools.map((tool) => calls.findIndex((call) => call.name === tool));
    const pass = indexes.every((index) => index >= 0) && indexes.every((index, position) => position === 0 || indexes[position - 1] < index);
    return resultOf(trace, assertion, pass, tools.map((tool, position) => `${tool}@${indexes[position]}`).join(' before '));
  }
  if (assertion.assert === 'max_tool_calls' || assertion.assert === 'max_tool_errors') {
    if (!Number.isInteger(assertion.n) || assertion.n < 0) throw new Error(`${assertion.assert} requires a non-negative integer n`);
    const evidence = analyzeToolCalls(calls);
    const observed = assertion.assert === 'max_tool_calls' ? evidence.calls : evidence.errors;
    const noun = assertion.assert === 'max_tool_calls' ? 'calls' : 'errors';
    const pass = observed > assertion.n ? false : noun === 'errors' && evidence.unknown_outcomes ? null : true;
    return resultOf(trace, assertion, pass, `${observed} ${noun}, max ${assertion.n}${pass === null ? '; tool outcomes unavailable' : ''}`);
  }
  if (assertion.assert === 'no_repeated_failed_call') {
    const evidence = analyzeToolCalls(calls);
    const repeated = evidence.repeated_indexes;
    const unknown = evidence.repeated_failures_unknown + evidence.unknown_outcomes;
    return resultOf(trace, assertion, repeated.length ? false : unknown ? null : true,
      repeated.length ? `repeated failed calls at ${repeated.join(',')}` : unknown ? `${unknown} calls have unavailable failure evidence` : 'no repeated failed call');
  }
  const loaded = capabilityLoadEvidence(assertion.id, calls);
  return resultOf(trace, assertion, loaded.pass, loaded.detail);
}

function memoryKinds(assertion) {
  const kinds = Array.isArray(assertion.kind) ? assertion.kind : [assertion.kind];
  if (kinds.length === 0 || kinds.some((kind) => !MEMORY_KINDS.has(kind))) throw new Error(`Unknown memory kind ${assertion.kind}`);
  return kinds;
}

function memoryMatches(commits, assertion) {
  const kinds = memoryKinds(assertion);
  return commits.filter((commit) => kinds.includes(commit.kind)
    && (assertion.title_contains == null || String(commit.title ?? '').includes(assertion.title_contains)));
}

function countPasses({ count, min, max }, found) {
  if (count !== undefined) return found === count;
  return found >= (min ?? (max === undefined ? 1 : 0)) && (max === undefined || found <= max);
}

const NON_LENS_READS = new Set(['source', 'note', 'journal', 'world']);

/** Lens state comes from the store after the turn; a reply's save claim is never evidence. */
function evaluateLensAssertion(assertion, trace) {
  const spec = lensAssertion(assertion);
  if (assertion.assert === 'lens_read' || assertion.assert === 'lens_not_read') {
    const reads = memoryReads(trace);
    if (reads === null) return resultOf(trace, assertion, null, 'memory read evidence unavailable');
    const kinds = new Map((lensPages(trace) ?? []).map((page) => [page.record_id, page.kind]));
    const hits = reads.filter((read) => read.access === 'exact_read' && !NON_LENS_READS.has(read.kind)
      && spec.title.test(String(read.title ?? '')) && (spec.kind === undefined || kinds.get(read.record_id) === spec.kind));
    const pass = assertion.assert === 'lens_read' ? countPasses(spec, hits.length) : hits.length === 0;
    return resultOf(trace, assertion, pass, hits.length ? `read ${hits.map((read) => read.title).join(' | ')}` : 'no matching Lens read');
  }
  const pages = lensPages(trace);
  if (pages === null) return resultOf(trace, assertion, null, 'Lens state unavailable');
  if (assertion.assert === 'lens_unchanged') {
    const touched = pages.filter((page) => page.ops_this_turn.length > 0);
    return resultOf(trace, assertion, touched.length === 0,
      touched.length ? `ops on ${touched.map((page) => page.title).join(' | ')}` : 'no Lens op this turn');
  }
  const matching = pages.filter((page) => !page.archived && (spec.kind === undefined || page.kind === spec.kind)
    && spec.title.test(page.title));
  const text = (page) => [page.summary ?? '', ...page.sections.map((section) => `${section.heading ?? ''}\n${section.markdown ?? ''}`)];
  const failures = matching.flatMap((page) => {
    const why = [];
    if (spec.sectionMatches && !text(page).some((part) => spec.sectionMatches.test(part))) why.push(`${page.title} lacks ${spec.sectionMatches}`);
    if (spec.sectionNotMatching && text(page).some((part) => spec.sectionNotMatching.test(part))) why.push(`${page.title} still has ${spec.sectionNotMatching}`);
    if (spec.basis && !page.sections.some((section) => (section.basis ?? []).includes(spec.basis))) why.push(`${page.title} has no ${spec.basis} basis`);
    if (spec.unchanged && page.ops_this_turn.length > 0) why.push(`${page.title} was changed this turn`);
    return why;
  });
  const counted = countPasses(spec, matching.length);
  const detail = [`${matching.length} matching page${matching.length === 1 ? '' : 's'}`, ...failures].join('; ');
  return resultOf(trace, assertion, counted && failures.length === 0, detail);
}

function evaluateMemoryAssertion(assertion, trace) {
  if (!MEMORY_ASSERTS.has(assertion.assert)) throw new Error(`Unknown memory assertion ${assertion.assert}`);
  if (LENS_ASSERTS.has(assertion.assert)) return evaluateLensAssertion(assertion, trace);
  const commits = memoryCommits(trace);
  if (commits === null) return resultOf(trace, assertion, null, 'Memory commit evidence unavailable');
  const matches = memoryMatches(commits, assertion);
  const min = assertion.min ?? 1;
  const pass = assertion.assert === 'committed' ? (assertion.count === undefined ? matches.length >= min : matches.length === assertion.count) : matches.length < min;
  return resultOf(trace, assertion, pass, `${matches.length} ${memoryKinds(assertion).join('|')} commits`);
}

function answerForMetrics(answer) {
  const text = String(answer ?? '');
  const trailing = trailingJsonObject(text);
  if (!trailing) return text;
  try {
    const value = JSON.parse(trailing);
    if (value?.metrics && typeof value.metrics === 'object' && !Array.isArray(value.metrics)) return trailing;
  } catch { /* Fenced metrics still go through scoreNumerical. */ }
  return text;
}

function evaluateAnswerAssertion(assertion, trace, scenario) {
  if (!ANSWER_ASSERTS.has(assertion.assert)) throw new Error(`Unknown answer assertion ${assertion.assert}`);
  const answer = answerEvidence(trace);
  if (answer === null) return resultOf(trace, assertion, null, 'answer evidence unavailable');
  if (assertion.assert === 'cites_source_span') {
    const result = sourceSpanEvidence(assertion, trace, answer, scenario);
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  if (assertion.assert === 'no_false_save_claim') {
    const result = noFalseSaveClaimEvidence(trace, answer);
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  if (assertion.assert === 'calculation_chip') {
    const result = calculationChipEvidence(assertion, trace, answer);
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  if (assertion.assert === 'visible_labels') {
    const result = visibleLabelsEvidence(assertion, answer, scenario, trace);
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  if (assertion.assert === 'answer_language') {
    const result = answerLanguageEvidence(assertion, answer, assertion.per_paragraph ? Object.values(visibleLabels(scenario.language)) : []);
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  if (assertion.assert === 'figures_present') {
    const result = figuresPresentEvidence(assertion, answer, toolCalls(trace));
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  if (assertion.assert === 'contains_number_any') {
    const hit = matchingNumber(numberSpellings(assertion.values), [visibleAnswerText(answer)]);
    return resultOf(trace, assertion, Boolean(hit), hit ? `contains number ${hit}` : 'contains none of the numbers');
  }
  if (assertion.assert === 'contains_any' || assertion.assert === 'not_contains') {
    // Bold or italic around part of a phrase does not change what it says.
    const text = withoutEmphasis(answer);
    const hit = (assertion.values ?? []).find((value) => text.includes(withoutEmphasis(value)));
    if (assertion.assert === 'not_contains') return resultOf(trace, assertion, !hit, hit ? `contains ${hit}` : 'contains none of the forbidden values');
    return resultOf(trace, assertion, Boolean(hit), hit ? `contains ${hit}` : 'contains none of the values');
  }
  if (assertion.assert === 'answer_matching') {
    const text = visibleAnswerText(answer);
    const hit = answerPatterns(assertion).find(pattern => pattern.test(text));
    return resultOf(trace, assertion, Boolean(hit), hit ? `matches ${hit}` : 'matches none of the required patterns');
  }
  if (assertion.assert === 'answer_not_matching') {
    const hit = answerPatterns(assertion).find((pattern) => pattern.test(answer));
    return resultOf(trace, assertion, !hit, hit ? `matches forbidden pattern ${hit}` : 'matches none of the forbidden patterns');
  }
  if (assertion.assert === 'cites_result') {
    const { tools, near } = citationAssertion(assertion);
    const calls = toolCalls(trace);
    if (calls === null) return resultOf(trace, assertion, null, 'tool call evidence unavailable');
    const called = tools.filter(tool => calls.some(call => call.name === tool));
    if (!called.length) return resultOf(trace, assertion, false, `${tools.join(' | ')} was not called`);
    const sets = called.map(tool => returnedCitationUrls(calls, tool));
    const unavailable = sets.some(urls => urls === null);
    if (sets.every(urls => urls === null)) return resultOf(trace, assertion, null, 'tool results unavailable');
    const urls = new Set(sets.filter(Boolean).flatMap(urls => [...urls]));
    const paragraphs = citingParagraphs(answer, urls);
    const cited = near.length === 0 ? paragraphs : paragraphs.filter(paragraph => near.some(value => paragraph.includes(value)));
    if (!cited.length && unavailable) return resultOf(trace, assertion, null, 'some tool results unavailable');
    return resultOf(trace, assertion, cited.length > 0, cited.length > 0
      ? `cites ${called.join(' | ')}${near.length ? ' beside the value' : ''}`
      : paragraphs.length > 0 ? `cites result but not beside ${near.join(' | ')}` : `no citation_url from ${called.join(' | ')}`);
  }
  if (assertion.assert === 'prediction') {
    const issues = predictionIssues(parsePredictionBlock(answer));
    return resultOf(trace, assertion, issues.length === 0, predictionDetail(issues, 'prediction block present'));
  }
  if (assertion.assert === 'numbers_linked') {
    const result = numbersLinkedEvidence(assertion, trace, answer);
    return resultOf(trace, assertion, result.pass, result.detail);
  }
  const values = assertion.values ?? {};
  if (Object.values(values).some((value) => typeof value !== 'number' || !Number.isFinite(value))) throw new Error('Metric expectations must be finite numbers');
  const scales = [1, ...(assertion.scales ?? [])];
  if (scales.some((scale) => typeof scale !== 'number' || !Number.isFinite(scale) || scale <= 0)) throw new Error('Metric scales must be positive finite numbers');
  const tolerance = metricTolerance(assertion);
  // A scale accepts the same quantity in another stated unit; never use it where the unit is what the scenario teaches.
  const attempts = scales.map((scale) => {
    const scored = scoreNumerical(
      { expected: { metrics: Object.fromEntries(Object.entries(values).map(([key, value]) => [key, value * scale])) } },
      answerForMetrics(answer),
    );
    // An absolute tolerance accepts a value rounded the way the scenario allows.
    if (tolerance === null) return scored;
    return { ...scored, checks: scored.checks.map((check) => ({ ...check,
      pass: check.actual !== null && Math.abs(check.actual - check.expected) <= tolerance * scale + 1e-12 })) };
  });
  const passes = (scored) => scored.checks.length > 0 && scored.checks.every((check) => check.pass);
  const scored = attempts.find(passes) ?? attempts[0];
  const detail = scored.checks.map((check) => `${check.key} expected ${check.expected} actual ${check.actual ?? 'null'}`).join('; ');
  return resultOf(trace, assertion, passes(scored), detail || 'no metrics');
}

export function grade(suite, traces) {
  validateSuite(suite);
  const results = [];
  const unmatched = [];
  for (const input of traces) {
    validateTraceRecord(input);
    const { scenario, session, turn } = locateTurn(suite, input);
    if (!turn) {
      unmatched.push({ run_id: input.run_id, ...turnFields(input), detail: 'No matching suite turn' });
      continue;
    }
    const trace = executionMismatches(input, session, turn).length ? { ...input, status: 'mismatched' } : input;
    const expect = turn.expect ?? {};
    for (const assertion of expect.trace ?? []) results.push(evaluateTraceAssertion(assertion, trace, scenario, turn, traces));
    for (const assertion of expect.memory ?? []) results.push(evaluateMemoryAssertion(assertion, trace));
    for (const assertion of expect.answer ?? []) results.push(evaluateAnswerAssertion(assertion, trace, scenario));
  }
  return { results, unmatched, tool_validity: toolValidity(traces) };
}
