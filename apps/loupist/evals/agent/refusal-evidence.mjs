/**
 * Tool refusals an agent walks into without having been told the rule.
 *
 * A refusal is first-attempt when the turn had not yet returned that reason for that tool: the
 * model could only have known the rule from the tool's description. Each distinct (tool, reason)
 * pair becomes one judge item asking whether the description states the rule; the pairs judged
 * "not stated" are the `hidden_rule_refusal` count. Counting needs no model call.
 */
import { createHash } from 'node:crypto';
import { canonicalJson, isRecord, toolCalls, turnFields } from './eval-contract.mjs';
import { isInvalidToolCall } from './tool-evidence.mjs';
import { cachedVerdict, DEFAULT_REASONING_EFFORT } from './judge.mjs';

export const REFUSAL_PROMPT_VERSION = 'refusal-rule/1';
export const RULE_STATED_CRITERION_ID = 'rule-stated';
export const RULE_STATED_CRITERION = 'The tool description, or the description of one of its parameters, states the rule this refusal enforces, clearly enough that a reader of the description alone would not have made the refused call.';

// Outcomes of the data or of the transport, not rules of the host: a description cannot state them.
const NOT_A_RULE = new Set(['empty_result', 'no_data', 'offline', 'timeout', 'interrupted', 'cancelled',
  'provider_error', 'provider_timeout', 'rate_limited', 'eval_replay_unmatched', 'credential_locked',
  'credential_unavailable', 'keychain_busy', 'keychain_unavailable', 'internal']);

const code = (value) => String(value ?? '').trim().toLowerCase().replace(/[\s-]+/gu, '_');

/** The host's typed reason when the trace carries one, else the error code. */
export function refusalReason(call) {
  const typed = [call?.error?.reason, call?.error_reason].find((value) => typeof value === 'string' && value.trim());
  if (typed) return { reason: code(typed), typed: true };
  const fallback = code(call?.error_code);
  return fallback ? { reason: fallback, typed: false } : null;
}

/** A known tool the host refused. Argument-schema failures belong to `tool_validity`. */
export function isRefusal(call, tools) {
  if (call?.outcome !== 'error' || !tools.has(call.name) || isInvalidToolCall(call)) return false;
  const found = refusalReason(call);
  return found !== null && (found.typed || !NOT_A_RULE.has(found.reason));
}

/**
 * Per turn, the first refusal of each (tool, reason). `repeats` counts the later ones: the turn
 * had already been told. `unmeasured` lists turns whose call list is missing.
 */
export function firstAttemptRefusals(traces, catalog) {
  const tools = new Set(catalog.tools.map((tool) => tool.name));
  const refusals = [];
  const unmeasured = [];
  let repeats = 0;
  for (const trace of traces) {
    const calls = toolCalls(trace);
    if (calls === null) { unmeasured.push({ run_id: trace.run_id ?? null, ...turnFields(trace) }); continue; }
    const told = new Set();
    calls.forEach((call, index) => {
      if (!isRefusal(call, tools)) return;
      const { reason, typed } = refusalReason(call);
      const key = canonicalJson([call.name, reason]);
      if (told.has(key)) { repeats += 1; return; }
      told.add(key);
      refusals.push({ run_id: trace.run_id ?? null, ...turnFields(trace), call_index: index, tool: call.name, reason, typed,
        message: typeof call.error_message === 'string' ? call.error_message : null });
    });
  }
  return { refusals, repeats, unmeasured };
}

/** Distinct (tool, reason) pairs, most frequent first, each with the scenarios that met it. */
export function refusalPairs(refusals) {
  const pairs = new Map();
  for (const row of refusals) {
    const key = canonicalJson([row.tool, row.reason]);
    const pair = pairs.get(key) ?? { tool: row.tool, reason: row.reason, typed: row.typed, count: 0, scenarios: new Set(), messages: new Set() };
    pair.count += 1;
    pair.typed = pair.typed && row.typed;
    pair.scenarios.add(row.scenario_id);
    if (row.message) pair.messages.add(row.message);
    pairs.set(key, pair);
  }
  return [...pairs.values()]
    .map((pair) => ({ ...pair, scenarios: [...pair.scenarios].sort(), messages: [...pair.messages].sort() }))
    .sort((left, right) => right.count - left.count || left.tool.localeCompare(right.tool) || left.reason.localeCompare(right.reason));
}

export function refusalItemId(tool, reason) {
  return canonicalJson(['refusal_rule', tool, reason]);
}

/**
 * One judge item per pair. The input holds what the model was given (the description and the
 * parameters of the frozen catalog) and what the host answered. It is the same for every run
 * that meets the pair, so the verdict is cached until the description or the refusal changes.
 */
export function refusalJudgeItems(pairs, catalog) {
  const byName = new Map(catalog.tools.map((tool) => [tool.name, tool]));
  return pairs.map((pair) => {
    const tool = byName.get(pair.tool);
    return {
      item_id: refusalItemId(pair.tool, pair.reason), tool: pair.tool, reason: pair.reason, typed: pair.typed, count: pair.count,
      scenarios: pair.scenarios,
      input: {
        tool: pair.tool, description: tool?.description ?? null, parameters: tool?.parameters ?? null,
        refusal: { reason: pair.reason, message: pair.messages[0] ?? null },
        criterion: RULE_STATED_CRITERION,
      },
    };
  });
}

export function renderRefusalPrompt(input) {
  return [
    'You are a binary judge of a tool description written for an AI research agent.',
    'The agent called the tool and the host refused the call for the reason shown. The refusal is correct: the host enforces a rule.',
    'Decide whether the description the agent had already states that rule.',
    'Return JSON matching the schema: {"pass": boolean, "reason": string}.',
    'pass is true only when the description or a parameter description states the rule; quote the words that state it. A rule that can only be inferred, or that only the refusal message states, is not stated.',
    'Do not use tools, web search, or outside knowledge.',
    '',
    `Criterion: ${input.criterion}`,
    '',
    'Observed input:',
    JSON.stringify(input),
  ].join('\n');
}

/** Judges every item once. An item whose tool has no description in the catalog stays unjudged. */
export async function judgeRefusalItems({ items, model, cacheDir, backend, reasoningEffort = DEFAULT_REASONING_EFFORT, concurrency = 1 }) {
  if (!Number.isInteger(concurrency) || concurrency < 1) throw new Error('Judge concurrency must be a positive integer');
  const judged = items.filter((item) => typeof item.input.description === 'string' && item.input.description);
  const rows = new Array(judged.length);
  let next = 0;
  const worker = async () => {
    while (next < judged.length) {
      const index = next++;
      const item = judged[index];
      const verdict = await cachedVerdict({ input: item.input, criterion: RULE_STATED_CRITERION, prompt: renderRefusalPrompt(item.input),
        model, promptVersion: REFUSAL_PROMPT_VERSION, reasoningEffort, cacheDir, backend });
      rows[index] = { item_id: item.item_id, kind: 'refusal_rule', criterion_id: RULE_STATED_CRITERION_ID, criterion: RULE_STATED_CRITERION,
        tool: item.tool, refusal_reason: item.reason, ...verdict, model, prompt_version: REFUSAL_PROMPT_VERSION, reasoning_effort: reasoningEffort };
    }
  };
  await Promise.all(Array.from({ length: Math.min(concurrency, judged.length) }, worker));
  return rows;
}

/**
 * The report of one run. `hidden_rule_refusal` is the number of pairs judged "not stated"; it is
 * null until every pair has a verdict, because an unjudged pair is not a stated rule.
 */
export function refusalReport(traces, catalog, judgments = []) {
  if (!isRecord(catalog) || !Array.isArray(catalog.tools)) throw new Error('A tool catalog is required');
  const { refusals, repeats, unmeasured } = firstAttemptRefusals(traces, catalog);
  const verdicts = new Map(judgments.map((row) => [row.item_id, row]));
  const pairs = refusalJudgeItems(refusalPairs(refusals), catalog).map(({ input, item_id, ...pair }) => {
    const verdict = verdicts.get(item_id);
    return { ...pair, message: input.refusal.message, described: input.description !== null,
      stated: typeof verdict?.pass === 'boolean' ? verdict.pass : null, judge_reason: verdict?.reason ?? null };
  });
  const judged = pairs.every((pair) => pair.stated !== null);
  return {
    catalog_sha256: createHash('sha256').update(canonicalJson(catalog.tools.map(({ name, description, parameters }) => ({ name, description, parameters })))).digest('hex'),
    turns: traces.length, unmeasured_turns: unmeasured,
    first_attempt_refusals: refusals.length, repeated_refusals: repeats,
    pairs,
    hidden_rule_refusal: unmeasured.length === 0 && judged ? pairs.filter((pair) => pair.stated === false).length : null,
    refusals,
  };
}
