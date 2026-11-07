/** Shared identities and the current agent evaluation contract. No I/O or scoring. */
import { fromMarkdown } from 'mdast-util-from-markdown';

export const TRACE_SCHEMA = 'loupist-agent-trace/1';
export const SUITE_SCHEMA = 'loupist-agent-suite/1';
export const TRACE_ASSERTS = new Set(['tool_called', 'tool_not_called', 'tool_called_before', 'max_tool_calls', 'max_tool_errors', 'no_repeated_failed_call', 'capability_loaded', 'scheduled_tasks', 'tool_args_not_matching', 'calculate_inputs_cited', 'as_of_left_out', 'as_of_refused', 'grid_cells_linked', 'sensitivity_card', 'plan_progress']);
export const LENS_ASSERTS = new Set(['lens_pages', 'lens_unchanged', 'lens_read', 'lens_not_read']);
export const MEMORY_ASSERTS = new Set(['committed', 'not_committed', ...LENS_ASSERTS]);
const LENS_KINDS = new Set(['principle', 'perspective', 'method', 'subject', 'idea']);
const BASIS_KINDS = new Set(['span', 'claim', 'said', 'page']);
export const MEMORY_KINDS = new Set(['source', 'note', 'lens', 'lens_proposal', 'principle', 'journal']);
export const ANSWER_ASSERTS = new Set(['metrics', 'contains_any', 'not_contains', 'answer_not_matching', 'prediction', 'cites_result', 'numbers_linked', 'calculation_chip']);
/** Saved Memory an as-of answer left out, as Rust counts it in `as_of_report.left_out`. */
export const AS_OF_LEFT_OUT_KINDS = new Set(['facts', 'sources', 'notes', 'methods']);
const NUMBER_CHECK_STATUSES = new Set(['matched', 'rounded', 'not_found', 'differs']);
const CITATION_URL = /#evidence-ev_[0-9a-f]{32}/gu;

export function planProgressAssertion(assertion) {
  const min = assertion.min_items ?? 2;
  if (!Number.isInteger(min) || min < 2 || min > 6) throw new Error('plan_progress min_items must be 2 to 6');
  return { min_items: min };
}

/** A checked answer link to a formula computed from pinned dataset cells. */
export function calculationChipAssertion(assertion) {
  for (const field of ['operation', 'formula', 'text']) {
    if (typeof assertion[field] !== 'string' || !assertion[field].trim()) throw new Error(`calculation_chip requires ${field}`);
  }
  if (!isRecord(assertion.inputs) || !Object.keys(assertion.inputs).length
    || Object.values(assertion.inputs).some(value => !Number.isFinite(value))) throw new Error('calculation_chip inputs must contain finite expected values');
  return assertion;
}

/** `cites_result` names one tool and optional nearby strings (the material value as written). */
export function citationAssertion(assertion) {
  if (typeof assertion.tool !== 'string' || !assertion.tool) throw new Error('cites_result requires a tool');
  const near = assertion.near ?? [];
  if (!Array.isArray(near) || near.some((value) => typeof value !== 'string' || !value || value.length > 80)) {
    throw new Error('cites_result near must be nonempty strings of at most 80 characters');
  }
  return { tool: assertion.tool, near };
}

/** `numbers_linked` may name printed numbers that must have a matched or rounded check. */
export function numbersLinkedAssertion(assertion) {
  const checked = assertion.checked ?? [];
  if (!Array.isArray(checked) || checked.some((value) => typeof value !== 'string' || !value.trim() || value.length > 40)) {
    throw new Error('numbers_linked checked must be nonempty strings of at most 40 characters');
  }
  return { checked };
}

/** `as_of_left_out` reads one kind of the as-of report's left-out counts. */
export function asOfLeftOutAssertion(assertion) {
  if (!AS_OF_LEFT_OUT_KINDS.has(assertion.kind)) throw new Error('as_of_left_out kind must be facts, sources, notes, or methods');
  const min = assertion.min ?? 1;
  if (!Number.isSafeInteger(min) || min < 1) throw new Error('as_of_left_out min must be a positive integer');
  return { kind: assertion.kind, min };
}

/** `as_of_refused` names the tool the as-of report must list as refused. */
export function asOfRefusedAssertion(assertion) {
  if (typeof assertion.tool !== 'string' || !assertion.tool) throw new Error('as_of_refused requires a tool');
  return { tool: assertion.tool };
}

/**
 * `tool_called` names one tool and, optionally, `args`: fields a call that did
 * not fail must carry (a subset match, so `{"operation": "goal_grid"}` matches
 * any goal_grid calculation).
 */
export function toolCalledAssertion(assertion) {
  if (typeof assertion.tool !== 'string' || !assertion.tool) throw new Error('tool_called requires a tool');
  const args = assertion.args;
  if (args !== undefined && (!isRecord(args) || Object.keys(args).length === 0)) {
    throw new Error('tool_called args must be a nonempty object of expected argument fields');
  }
  return { tool: assertion.tool, args: args ?? null };
}

/** Whether `actual` carries every field of `expected`; arrays match item by item. */
export function argumentsInclude(actual, expected) {
  if (Array.isArray(expected)) {
    return Array.isArray(actual) && actual.length === expected.length
      && expected.every((item, index) => argumentsInclude(actual[index], item));
  }
  if (isRecord(expected)) {
    return isRecord(actual) && Object.entries(expected).every(([key, value]) => argumentsInclude(actual[key], value));
  }
  return actual === expected;
}

const SENSITIVITY_OPERATIONS = new Set(['reverse_dcf', 'dcf_grid']);

/**
 * `sensitivity_card` asks for a grid card Rust built from a paragraph that is
 * only a link to a `reverse_dcf` or `dcf_grid` result; `reference: true` also
 * needs the past growth the grid is read against.
 */
export function sensitivityCardAssertion(assertion) {
  if (assertion.operation !== undefined && !SENSITIVITY_OPERATIONS.has(assertion.operation)) {
    throw new Error('sensitivity_card operation must be reverse_dcf or dcf_grid');
  }
  if (assertion.reference !== undefined && typeof assertion.reference !== 'boolean') {
    throw new Error('sensitivity_card reference must be a boolean');
  }
  return { operation: assertion.operation ?? null, reference: assertion.reference ?? false };
}

/** The answer's sensitivity cards: undefined when the record predates them, null when malformed. */
export function sensitivityCards(trace) {
  if (!('sensitivity_cards' in trace)) return undefined;
  const cards = trace.sensitivity_cards;
  return Array.isArray(cards) && cards.every((card) => isRecord(card) && typeof card.operation === 'string') ? cards : null;
}

/** `calculate_inputs_cited` names inputs a calculate call must link to evidence with input_sources. */
export function calculateInputsAssertion(assertion) {
  const inputs = assertion.inputs;
  if (!Array.isArray(inputs) || inputs.length === 0 || inputs.some((name) => typeof name !== 'string' || !name)) {
    throw new Error('calculate_inputs_cited requires input names');
  }
  if (assertion.operation !== undefined && (typeof assertion.operation !== 'string' || !assertion.operation)) {
    throw new Error('calculate_inputs_cited operation must be a nonempty string');
  }
  return { inputs, operation: assertion.operation ?? null };
}

/** Reasons a grid cell may stay empty (finance-grid-answers.md §3.2). */
export const GRID_EMPTY_REASONS = new Set(['not_reported', 'not_applicable', 'unavailable', 'not_checked']);
const GRID_LAYOUTS = new Set(['assets', 'conflict']);

/**
 * `grid_cells_linked` asks for a `grid_publish` call of one layout whose every
 * checked cell matched or rounded against its evidence; optional exact row and
 * column counts and empty-cell reasons are read from the call's arguments.
 */
export function gridCellsAssertion(assertion) {
  const layout = assertion.layout ?? 'assets';
  if (!GRID_LAYOUTS.has(layout)) throw new Error('grid_cells_linked layout must be assets or conflict');
  for (const key of ['rows', 'columns']) {
    const value = assertion[key];
    if (value !== undefined && (!Number.isSafeInteger(value) || value < 1)) throw new Error(`grid_cells_linked ${key} must be a positive integer`);
  }
  const empty = assertion.empty ?? [];
  if (!Array.isArray(empty) || empty.some((reason) => !GRID_EMPTY_REASONS.has(reason))) {
    throw new Error('grid_cells_linked empty must list not_reported, not_applicable, unavailable or not_checked');
  }
  if (layout === 'conflict' && (assertion.rows !== undefined || assertion.columns !== undefined || empty.length)) {
    throw new Error('grid_cells_linked rows, columns and empty apply to the assets layout');
  }
  return { layout, rows: assertion.rows ?? null, columns: assertion.columns ?? null, empty };
}

/** An absolute metric tolerance, when the scenario accepts rounding. */
export function metricTolerance(assertion) {
  if (assertion.tolerance === undefined) return null;
  if (typeof assertion.tolerance !== 'number' || !Number.isFinite(assertion.tolerance) || assertion.tolerance <= 0) {
    throw new Error('metrics tolerance must be a positive finite number');
  }
  return assertion.tolerance;
}

/** Rust's checks of the answer's number links, or null when the record does not carry them. */
export function numberChecks(trace) {
  const checks = trace.number_checks;
  return Array.isArray(checks) && checks.every((check) => isRecord(check) && typeof check.text === 'string'
    && NUMBER_CHECK_STATUSES.has(check.status)) ? checks : null;
}

/** The as-of report: undefined when the record predates it, null for an answer without a cutoff. */
export function asOfReport(trace) {
  if (!('as_of_report' in trace)) return undefined;
  return isRecord(trace.as_of_report) ? trace.as_of_report : null;
}

/** citation_url values a tool returned, or null when its delivered results are unavailable. */
export function returnedCitationUrls(calls, tool) {
  const urls = new Set();
  let known = false;
  for (const call of calls.filter((item) => item.name === tool && item.outcome !== 'error')) {
    // Redacted or size-truncated deliveries cannot show which citation they carried.
    if (!isRecord(call.result) || call.result_redacted === true || call.result.truncated === true) continue;
    known = true;
    for (const match of JSON.stringify(call.result).match(CITATION_URL) ?? []) urls.add(match);
  }
  return known ? urls : null;
}

/** Answer paragraphs that cite one of `urls`. */
export function citingParagraphs(answer, urls) {
  const markdown = String(answer ?? '');
  const tree = fromMarkdown(markdown);
  const definitions = new Map();
  const collectDefinitions = (node) => {
    // CommonMark resolves duplicate reference definitions to the first one.
    if (node.type === 'definition' && !definitions.has(node.identifier)) definitions.set(node.identifier, node.url);
    node.children?.forEach(collectDefinitions);
  };
  collectDefinitions(tree);
  const hasCitation = (node) => {
    const href = node.type === 'link' ? node.url
      : node.type === 'linkReference' ? definitions.get(node.identifier) : undefined;
    // Match the full href, as the answer renderer does. Bare handles, code and
    // image destinations are not link nodes and cannot stand in for a citation.
    return urls.has(href) || (node.children?.some(hasCitation) ?? false);
  };
  const paragraphs = [];
  const visit = (node) => {
    if (node.type === 'paragraph' && hasCitation(node)) {
      paragraphs.push(markdown.slice(node.position.start.offset, node.position.end.offset));
    }
    node.children?.forEach(visit);
  };
  visit(tree);
  return paragraphs;
}

/** Compile at load time, including when no traces are available to grade. */
export function answerPatterns(assertion) {
  if (!Array.isArray(assertion.patterns) || assertion.patterns.length === 0
    || assertion.patterns.some((pattern) => typeof pattern !== 'string' || pattern.length === 0 || pattern.length > 200)) {
    throw new Error('answer_not_matching requires nonempty patterns of at most 200 characters');
  }
  const flags = assertion.flags ?? 'iu';
  if (typeof flags !== 'string') throw new Error('answer_not_matching flags must be a string');
  try {
    return assertion.patterns.map((pattern) => new RegExp(pattern, flags));
  } catch (error) {
    throw new Error(`answer_not_matching has an invalid regular expression or flags: ${error.message}`);
  }
}
const canonical = (value) => Array.isArray(value) ? value.map(canonical) : value && typeof value === 'object'
  ? Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])])) : value;

export function canonicalJson(value) {
  return JSON.stringify(canonical(value ?? null));
}

/** One runner writes one run_id for every turn, so a turn needs the rest of this key. */
export function turnIdentity(row) {
  return canonicalJson([row.run_id, ...turnCoordinates(row)]);
}

export function hasTurnIdentity(row) {
  return row != null && ['run_id', 'suite', 'scenario_id', 'session_id'].every((field) => typeof row[field] === 'string' && row[field])
    && Number.isInteger(row.turn_index) && row.turn_index >= 0 && Number.isInteger(row.trial) && row.trial >= 0;
}

export function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

export function toolCalls(trace) {
  const calls = trace.tool_calls;
  return Array.isArray(calls) && calls.every((call) => isRecord(call) && typeof call.name === 'string' && call.name)
    ? calls : null;
}

export function memoryCommits(trace) {
  const commits = trace.memory_commits;
  return Array.isArray(commits) && commits.every((commit) => isRecord(commit) && MEMORY_KINDS.has(commit.kind))
    ? commits : null;
}

/** Canonical Lens pages the runner read from the store after the turn. */
export function lensPages(trace) {
  const pages = trace.lens?.pages;
  return Array.isArray(pages) && pages.every((page) => isRecord(page) && typeof page.title === 'string'
    && Array.isArray(page.sections) && Array.isArray(page.ops_this_turn)) ? pages : null;
}

export function memoryReads(trace) {
  const reads = trace.memory_reads;
  return Array.isArray(reads) && reads.every(isRecord) ? reads : null;
}

function lensPattern(value, field) {
  if (value === undefined) return null;
  if (typeof value !== 'string' || !value) throw new Error(`Lens ${field} must be a non-empty pattern`);
  return new RegExp(value, 'iu');
}

export function lensAssertion(assertion) {
  const { kind, basis } = assertion;
  if (kind !== undefined && !LENS_KINDS.has(kind)) throw new Error(`Unknown Lens kind ${kind}`);
  if (basis !== undefined && !BASIS_KINDS.has(basis)) throw new Error(`Unknown basis kind ${basis}`);
  for (const field of ['count', 'min', 'max']) {
    const value = assertion[field];
    if (value !== undefined && (!Number.isSafeInteger(value) || value < 0)) throw new Error(`Lens ${field} must be a non-negative integer`);
  }
  if (assertion.assert !== 'lens_unchanged' && assertion.title_matches === undefined) throw new Error(`${assertion.assert} needs title_matches`);
  return {
    kind, basis, count: assertion.count, min: assertion.min, max: assertion.max,
    title: lensPattern(assertion.title_matches, 'title_matches'),
    sectionMatches: lensPattern(assertion.section_matches, 'section_matches'),
    sectionNotMatching: lensPattern(assertion.section_not_matching, 'section_not_matching'),
  };
}

/** An observed empty answer is data; an absent or non-text answer is not. */
export function answerEvidence(trace) {
  return typeof trace.answer === 'string' ? trace.answer : null;
}

export function traceEvidenceIssues(trace) {
  return [toolCalls(trace) === null && 'tool_calls', memoryCommits(trace) === null && 'memory_commits',
    answerEvidence(trace) === null && 'answer'].filter(Boolean);
}

export function validDate(value) {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}$/u.test(value)) return false;
  const [year, month, day] = value.split('-').map(Number);
  const date = new Date(`${value}T00:00:00.000Z`);
  return date.getUTCFullYear() === year && date.getUTCMonth() === month - 1 && date.getUTCDate() === day;
}

export function executionMismatches(trace, session, turn) {
  const expected = { user: turn.user, memory_mode: session.memory, session_role: session.role };
  return Object.keys(expected).filter((field) => trace[field] !== expected[field]);
}

export function validateSuite(suite) {
  if (suite?.schema !== SUITE_SCHEMA || typeof suite.suite !== 'string' || !suite.suite || !Array.isArray(suite.scenarios)) throw new Error('Unsupported agent suite');
  if (suite.budget !== undefined) {
    for (const key of ['maxInputTokensPerTurn', 'maxTurns']) {
      if (!Number.isSafeInteger(suite.budget?.[key]) || suite.budget[key] < 1) throw new Error(`budget.${key} must be a positive integer`);
    }
  }
  const ids = new Set();
  for (const scenario of suite.scenarios) {
    if (typeof scenario.id !== 'string' || !scenario.id || ids.has(scenario.id) || !Array.isArray(scenario.sessions) || !scenario.sessions.length) throw new Error('Invalid or duplicate scenario');
    ids.add(scenario.id);
    const sessions = new Set();
    for (const session of scenario.sessions) {
      if (typeof session.id !== 'string' || !session.id || sessions.has(session.id) || !Array.isArray(session.turns) || !session.turns.length) throw new Error(`Scenario ${scenario.id} has an invalid session`);
      if (!['train', 'probe'].includes(session.role) || !['off', 'read_only', 'remember'].includes(session.memory)) throw new Error(`Session ${session.id} must declare role and memory`);
      sessions.add(session.id);
      for (const turn of session.turns) {
        if (typeof turn.user !== 'string' || !turn.user.trim()) throw new Error(`Session ${session.id} has an invalid user prompt`);
        if (turn.as_of !== undefined && !validDate(turn.as_of)) throw new Error(`Session ${session.id} has an invalid as_of date`);
        for (const assertion of turn.expect?.answer ?? []) {
          if (assertion.assert === 'answer_not_matching') answerPatterns(assertion);
          if (assertion.assert === 'cites_result') citationAssertion(assertion);
          if (assertion.assert === 'numbers_linked') numbersLinkedAssertion(assertion);
          if (assertion.assert === 'calculation_chip') calculationChipAssertion(assertion);
          if (assertion.assert === 'metrics') metricTolerance(assertion);
        }
        for (const assertion of turn.expect?.trace ?? []) {
          if (assertion.assert === 'tool_args_not_matching') answerPatterns(assertion);
          if (assertion.assert === 'as_of_left_out') asOfLeftOutAssertion(assertion);
          if (assertion.assert === 'as_of_refused') asOfRefusedAssertion(assertion);
          if (assertion.assert === 'calculate_inputs_cited') calculateInputsAssertion(assertion);
          if (assertion.assert === 'tool_called') toolCalledAssertion(assertion);
          if (assertion.assert === 'sensitivity_card') sensitivityCardAssertion(assertion);
          if (assertion.assert === 'plan_progress') planProgressAssertion(assertion);
        }
        for (const assertion of turn.expect?.memory ?? []) {
          if (assertion.count !== undefined && (!Number.isSafeInteger(assertion.count) || assertion.count < 0)) throw new Error('Memory count must be a non-negative integer');
          if (LENS_ASSERTS.has(assertion.assert)) lensAssertion(assertion);
        }
        const criteria = new Set();
        for (const rubric of turn.expect?.rubric ?? []) {
          if (typeof rubric.id !== 'string' || !rubric.id || criteria.has(rubric.id)) throw new Error(`Session ${session.id} has an invalid or duplicate rubric`);
          criteria.add(rubric.id);
        }
      }
    }
  }
  return suite;
}

export function validateTraceRecord(trace) {
  if (trace?.schema !== TRACE_SCHEMA) throw new Error('Unsupported agent trace schema');
  for (const field of ['run_id', 'suite', 'scenario_id', 'session_id']) {
    if (typeof trace[field] !== 'string' || !trace[field]) throw new Error(`Trace missing ${field}`);
  }
  if (!Number.isInteger(trace.turn_index) || trace.turn_index < 0) throw new Error('Trace turn_index must be a non-negative integer');
  if (!Number.isInteger(trace.trial) || trace.trial < 0) throw new Error('Trace trial must be a non-negative integer');
  return trace;
}

export function locateTurn(suite, trace) {
  if (trace.suite !== suite.suite) return { scenario: null, session: null, turn: null };
  const scenario = suite.scenarios.find((item) => item.id === trace.scenario_id) ?? null;
  const session = scenario?.sessions.find((item) => item.id === trace.session_id) ?? null;
  const turn = session && Number.isInteger(trace.turn_index) ? session.turns[trace.turn_index] ?? null : null;
  return { scenario, session, turn };
}

/** A logical cell has no run ID; an attempt identity does. Use both deliberately. */
function turnCoordinates(row) {
  return [row.suite ?? null, row.scenario_id, row.session_id, row.turn_index, row.trial];
}

export function turnKey(row) {
  return canonicalJson(turnCoordinates(row));
}

export function turnFields(row) {
  return { suite: row.suite ?? null, scenario_id: row.scenario_id, session_id: row.session_id,
    turn_index: row.turn_index, trial: row.trial };
}

export function trialKey(row) {
  return canonicalJson([row.suite ?? null, row.scenario_id, row.trial]);
}

export function groupBy(rows, keyOf) {
  const grouped = new Map();
  for (const row of rows) {
    const key = keyOf(row);
    const group = grouped.get(key) ?? [];
    group.push(row);
    grouped.set(key, group);
  }
  return grouped;
}

export function positiveTrialCount(value = 3) {
  if (!Number.isInteger(value) || value < 1) throw new Error('trials must be a positive integer');
  return value;
}

export const FAILED_TURN_STATUSES = new Set(['failed', 'cancelled', 'interrupted', 'aborted']);

/** Missing execution evidence is unknown; only a recorded failure is false. */
export function turnCompleted(trace) {
  return trace.status === 'completed' ? true : FAILED_TURN_STATUSES.has(trace.status) ? false : null;
}
