import test from 'node:test';
import assert from 'node:assert/strict';
import { open } from './adapters/provider.mjs';

const config = { endpoint: 'https://example.invalid/chat/completions', api_key_env: 'LOUPIST_FINANCE_EVAL_UNIT_TEST_KEY', model: 'exact-test-model',
  effort: 'high', budget: { max_output_tokens: 1024, timeout_ms: 1000 }, data_access: { fixture: 'supplied', external: 'disabled' } };

test('provider rejects request effort/budget/data access mismatches before any network request', async () => {
  await assert.rejects(open({ ...config, request_options: { reasoning_effort: 'low' } }), /effort/u);
  await assert.rejects(open({ ...config, request_options: { max_completion_tokens: 2048 } }), /output limit/u);
  await assert.rejects(open({ ...config, timeout_ms: 2000 }), /comparison budget/u);
  await assert.rejects(open({ ...config, data_access: { fixture: 'supplied', external: 'enabled' } }), /external access disabled/u);
});

test('provider sends pinned model/prompt/effort/budget and reports actual response identity with local-only fetch stub', async (t) => {
  const previous = process.env[config.api_key_env];
  process.env[config.api_key_env] = 'unit-test-placeholder';
  t.after(() => { if (previous == null) delete process.env[config.api_key_env]; else process.env[config.api_key_env] = previous; });
  let body;
  t.mock.method(globalThis, 'fetch', async (_url, init) => {
    body = JSON.parse(init.body);
    return { ok: true, json: async () => ({ model: 'observed-test-model', id: 'test-response',
      choices: [{ message: { content: 'measured test answer' } }], usage: { prompt_tokens: 100, completion_tokens: 20 } }) };
  });
  const session = await open({ ...config, request_options: { model: 'cannot-override', messages: [{ role: 'user', content: 'cannot-override' }] } });
  const response = await session.run({ arm: 'same_model', condition: 'memory_off', prompt: 'real evaluation prompt' });
  assert.equal(body.model, config.model);
  assert.equal(body.reasoning_effort, config.effort);
  assert.equal(body.max_completion_tokens, config.budget.max_output_tokens);
  assert.deepEqual(body.messages, [{ role: 'user', content: 'real evaluation prompt' }]);
  assert.equal(response.observed_model, 'observed-test-model');
  assert.equal(response.cost_usd, null);
  assert.deepEqual(response.usage, { input_tokens: 100, output_tokens: 20, cached_input_tokens: null });
});

test('missing cache split keeps billed cost unknown and missing execution identity cannot complete', async (t) => {
  const previous = process.env[config.api_key_env];
  process.env[config.api_key_env] = 'unit-test-placeholder';
  t.after(() => { if (previous == null) delete process.env[config.api_key_env]; else process.env[config.api_key_env] = previous; });
  let cached;
  let model = 'actual-model';
  t.mock.method(globalThis, 'fetch', async () => ({ ok: true, json: async () => ({ model,
    choices: [{ message: { content: 'test answer' } }], usage: { prompt_tokens: 100, completion_tokens: 20,
      ...(cached != null ? { prompt_tokens_details: { cached_tokens: cached } } : {}) } }) }));
  const session = await open({ ...config, pricing: { as_of: '2026-10-03', input_per_million: 10, output_per_million: 20, cached_input_per_million: 1 } });
  const request = { arm: 'same_model', condition: 'memory_off', prompt: 'test prompt' };
  assert.equal((await session.run(request)).cost_usd, null);
  cached = 0;
  assert.equal((await session.run(request)).cost_usd, .0014);
  model = null;
  const missingIdentity = await session.run(request);
  assert.equal(missingIdentity.status, 'failed');
  assert.equal(missingIdentity.observed_model, null);
  assert.equal(missingIdentity.cost_usd, .0014);
});
