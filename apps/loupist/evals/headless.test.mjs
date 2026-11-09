import test from 'node:test';
import assert from 'node:assert/strict';
import { validateConfig, translateRequest, mapResult } from './adapters/headless.mjs';
import { taskRequest } from './evaluate.mjs';
const config = { model: 'openai-codex/gpt-6-luna', effort: 'medium', pi_agent_dir: '/fixture/pi', budget: { timeout_ms: 600000, max_output_tokens: null }, data_access: { fixture: 'supplied', external: 'runtime_tools' } };
test('headless translates exact finance prompt and supplied facts into one isolated memory-off probe', () => {
  const task = { id: 'T3', language: 'en', prompt: 'Compute return.', facts: { prices: [10, 12] }, expected: { metrics: { return: .2 } } };
  const request = { ...taskRequest(task, { task_id: task.id, arm: 'app', condition: 'memory_off', phase: 'first_use' }, 'hash'), run_id: 'attempt-one' };
  const suite = translateRequest(request);
  assert.equal(suite.scenarios.length, 1);
  const scenario = suite.scenarios[0];
  assert.deepEqual(scenario.connections, []);
  assert.equal(scenario.sessions.length, 1);
  assert.deepEqual(scenario.sessions[0], { id: 'attempt', role: 'probe', memory: 'off', turns: [{ user: request.prompt }] });
  assert.ok(request.prompt.includes(JSON.stringify(task.facts)));
  assert.equal(scenario.seed, undefined);
  assert.equal(scenario.hidden, undefined);
  assert.notEqual(translateRequest({ ...request, run_id: 'attempt-two' }).scenarios[0].id, scenario.id);
  for (const patch of [{ arm: 'same_model' }, { condition: 'learned_lens' }]) assert.throws(() => translateRequest({ ...request, ...patch }));
});
test('headless refuses unenforceable comparison settings before any model call', () => {
  assert.doesNotThrow(() => validateConfig(config));
  for (const patch of [{ effort: '' }, { thinking: 'high' }, { budget: { ...config.budget, max_output_tokens: 100 } }, { budget: { ...config.budget, max_cost: 1 } }, { data_access: { fixture: 'supplied', external: 'disabled' } }, { data_access: { ...config.data_access, network: 'off' } }, { pi_agent_dir: 'relative' }, { request_options: {} }, { connections: ['web'] }]) assert.throws(() => validateConfig({ ...config, ...patch }));
});
test('headless maps observed execution, usage, latency, and retained trace without inventing measurements', () => {
  const record = { status: 'completed', answer: 'answer', observed_model: 'openai-codex/gpt-6-luna', model: { provider: 'configured', model: 'not-observed' }, usage: { input: 10, output: 5, cache_read: 2 }, duration_ms: 123, tool_calls: [{ name: 'run_python' }], harness: { dirty: true } };
  const response = mapResult(record, '/trace.jsonl');
  assert.equal(response.status, 'completed'); assert.equal(response.observed_model, record.observed_model);
  assert.deepEqual(response.execution, record.tool_calls); assert.equal(response.answer, 'answer');
  assert.equal(response.duration_ms, 123); assert.equal(response.trace_ref, '/trace.jsonl');
  assert.deepEqual(response.usage, { input_tokens: 10, output_tokens: 5, cached_input_tokens: 2, cache_write_tokens: null, input_includes_cache: false });
  assert.equal(response.cost_usd, null); assert.equal(response.harness.dirty, true);
  assert.equal(mapResult({ ...record, observed_model: null }, '/trace').status, 'failed');
  const failed = mapResult({ status: 'failed', error: 'timeout' }, '/trace');
  assert.equal(failed.error, 'timeout'); assert.equal(failed.usage.input_tokens, null); assert.equal(failed.duration_ms, null);
  assert.equal(mapResult({ status: 'aborted' }, '/trace').status, 'cancelled');
});
