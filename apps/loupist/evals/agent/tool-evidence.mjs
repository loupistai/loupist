/** Tool evidence classification shared by deterministic checks and reports. */
import { canonicalJson, toolCalls } from './eval-contract.mjs';
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
