/** Tool evidence classification shared by deterministic checks and reports. */
import { canonicalJson, toolCalls, skillReadAssertion } from './eval-contract.mjs';
import catalog from '../../agent/tool-catalog.json' with { type: 'json' };
const INVALID_CODES = new Set(['malformed_request', 'method_denied', 'tool_not_found', 'tool_validation_failed']);
const INVALID_MESSAGE = /Tool (?:"[^"]+"|\S+) not found|Validation failed for tool "[^"]+":/u;

function normCode(code) {
  return String(code ?? '').trim().toLowerCase().replace(/[\s-]+/gu, '_');
}

/** Invalid calls are host argument/availability failures or Pi's pre-host rejection. Other errors are not. */
export function isInvalidToolCall(call) {
  if (call?.outcome !== 'error') return false;
  if (INVALID_CODES.has(normCode(call.error_code))) return true;
  return INVALID_MESSAGE.test(String(call.error_message ?? ''));
}

// These codes establish a pre-execution host refusal. Execution, offline and
// authentication errors do not prove a request was stopped before execution.
const REFUSED_CODES = new Set(['permission_denied', 'outbound_paused', 'untrusted_text_paused',
  'protected_path', 'outside_workspace', 'needs_approval', 'memory_disabled', 'proposal_disabled',
  'frame_too_large']);

export function toolExecutionState(call) {
  if (call.outcome === 'ok') return true;
  if (call.outcome !== 'error') return null;
  return !(isInvalidToolCall(call) || REFUSED_CODES.has(normCode(call.error_code)));
}

export function capabilityLoadEvidence(id, calls) {
  let unknown = false;
  for (const call of calls.filter(call => call.name === 'capability_load' && call.outcome !== 'error')) {
    if (call.args?.id === id) {
      if (call.outcome === 'ok') return { pass: true, detail: `loaded ${id}` };
      unknown = true;
    } else if (comparableArgs(call.args) === null) unknown = true;
  }
  return { pass: unknown ? null : false, detail: unknown ? `${id} load evidence unavailable` : `${id} not successfully loaded` };
}

export function firstPackUseEvidence(id, tools, calls) {
  const packTools = new Set(catalog.tools.filter(tool => tool.component === id).map(tool => tool.name));
  if (!tools.slice(1).every(tool => packTools.has(tool) && calls.some(call => call.name === tool))) {
    return { pass: false, detail: `no requested ${id} tool use` };
  }
  // Every attempted use counts, including a tool_not_found before a later load.
  const firstUse = calls.findIndex(call => packTools.has(call.name));
  const loaded = capabilityLoadEvidence(id, calls.slice(0, firstUse));
  return { ...loaded, detail: `${loaded.detail} before first ${id} use at ${firstUse}` };
}

export function skillReadEvidence(assertion, calls) {
  const { id } = skillReadAssertion(assertion);
  let unknown = false;
  for (const call of calls.filter(call => call.name === 'read')) {
    if (call.outcome === 'error') continue;
    if (comparableArgs(call.args) === null) { unknown = true; continue; }
    const path = call.args.path;
    if (typeof path !== 'string') continue;
    const parts = path.replaceAll('\\', '/').split('/');
    const matches = !parts.includes('..') && parts.slice(-3).join('/') === `skills/${id}/SKILL.md`;
    if (!matches) continue;
    if (call.outcome === 'ok') return { pass: true, detail: `read skills/${id}/SKILL.md` };
    unknown = true;
  }
  return { pass: unknown ? null : false, detail: unknown ? 'Skill read evidence unavailable' : `no successful read of skills/${id}/SKILL.md` };
}

export function comparableArgs(args) {
  // Host and Pi trace caps/redaction can replace nested values too. Equal
  // placeholders do not establish equality of the original arguments.
  const unavailable = (value) => {
    if (typeof value === 'string') return /\[(?:redacted|truncated)\]/iu.test(value);
    if (!value || typeof value !== 'object') return false;
    if (!Array.isArray(value) && (value._truncated === true
      || (value.truncated === true && Object.keys(value).every((key) => ['truncated', 'original_bytes'].includes(key))))) return true;
    return Object.values(value).some(unavailable);
  };
  if (args == null || unavailable(args)) return null;
  return canonicalJson(args);
}

export function analyzeToolCalls(calls) {
  const seen = new Set();
  const successful = new Set();
  const pendingErrors = new Map();
  const valid = toolCalls({ tool_calls: calls });
  const list = Array.isArray(calls) ? calls : [];
  const analysis = { calls: list.length, errors: 0, invalid: 0, unknown_outcomes: 0, unknown_call_lists: Number(valid === null),
    repeated_indexes: [], repeated_failures_unknown: 0, redundant_ok: [], recovered_errors: 0, steps: list.length };
  list.forEach((call, index) => {
    if (toolCalls({ tool_calls: [call] }) === null) { analysis.unknown_outcomes += 1; return; }
    if (call?.outcome === 'ok') {
      analysis.recovered_errors += pendingErrors.get(call.name) ?? 0;
      pendingErrors.delete(call.name);
      const args = comparableArgs(call.args);
      if (args !== null) {
        const key = canonicalJson([call.name, args]);
        if (successful.has(key)) analysis.redundant_ok.push(index);
        successful.add(key);
      }
      return;
    }
    if (call?.outcome !== 'error') { analysis.unknown_outcomes += 1; return; }
    analysis.errors += 1;
    pendingErrors.set(call.name, (pendingErrors.get(call.name) ?? 0) + 1);
    if (isInvalidToolCall(call)) analysis.invalid += 1;
    const args = comparableArgs(call.args);
    if (args === null || typeof call.name !== 'string' || !call.name) {
      analysis.repeated_failures_unknown += 1;
      return;
    }
    const key = canonicalJson([call.name, args]);
    if (seen.has(key)) analysis.repeated_indexes.push(index);
    seen.add(key);
  });
  return analysis;
}

export function toolValidity(traces) {
  let calls = 0;
  let invalid = 0;
  let repeated = 0;
  let unknown = 0;
  let unknownOutcomes = 0;
  let unknownLists = 0;
  for (const trace of traces) {
    const analysis = analyzeToolCalls(trace.tool_calls);
    calls += analysis.calls;
    invalid += analysis.invalid;
    repeated += analysis.repeated_indexes.length;
    unknown += analysis.repeated_failures_unknown;
    unknownOutcomes += analysis.unknown_outcomes;
    unknownLists += analysis.unknown_call_lists;
  }
  return { calls, invalid, rate: calls === 0 || unknownLists ? null : invalid / calls, unknown_outcomes: unknownOutcomes, unknown_call_lists: unknownLists,
    repeated_failures: repeated, repeated_failures_unknown: unknown };
}
