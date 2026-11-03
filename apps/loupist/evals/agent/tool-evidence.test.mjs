import test from 'node:test';
import assert from 'node:assert/strict';
import { analyzeToolCalls } from './tool-evidence.mjs';

const call = (name, outcome, args = {}) => ({ name, outcome, args });

test('trajectory counts successful duplicates and recovers each earlier error once by tool name', () => {
  const calls = [
    call('read', 'error', { path: 'a' }),
    call('read', 'error', { path: 'b' }),
    call('search', 'ok', { query: 'c' }),
    call('read', 'ok', { path: 'different' }),
    call('read', 'ok', { path: 'different' }),
    call('search', 'error', { query: 'c' }),
    call('search', 'ok', { query: 'c' }),
    call('read', 'error', { path: 'last' }),
  ];
  const result = analyzeToolCalls(calls);
  assert.equal(result.steps, 8);
  assert.equal(result.calls, 8);
  assert.deepEqual(result.redundant_ok, [4, 6]);
  assert.equal(result.errors, 4);
  assert.equal(result.recovered_errors, 3);
  assert.deepEqual(result.repeated_indexes, []);
});

test('successful equality is canonical and excludes missing, truncated and redacted arguments', () => {
  const calls = [call('read', 'ok', { a: 1, b: 2 }), call('read', 'ok', { b: 2, a: 1 }), call('other', 'ok', { a: 1, b: 2 })];
  for (const args of [null, { value: '[REDACTED]' }, { nested: { _truncated: true } }]) {
    calls.push(call('read', 'ok', args), call('read', 'ok', args));
  }
  assert.deepEqual(analyzeToolCalls(calls).redundant_ok, [1]);
  assert.deepEqual(analyzeToolCalls([calls[1]]).redundant_ok, []);
  assert.equal(analyzeToolCalls([call('read', 'error'), call('other', 'ok')]).recovered_errors, 0);
  assert.equal(analyzeToolCalls([call('read', 'error'), call('read', 'unknown')]).recovered_errors, 0);
  assert.equal(analyzeToolCalls([]).steps, 0);
  assert.equal(analyzeToolCalls(null).unknown_call_lists, 1);
});
