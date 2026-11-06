import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { suite, traces, HIDDEN_SENTINEL } from './fixtures/sample.mjs';
import { agreement, labelViews } from './labels.mjs';
import { buildJudgeInput, modelFamily, DEFAULT_REASONING_EFFORT, grokBackend, grokJudgeArgs, itemId, judgeAll, judgeCacheKey, judgeItems, openAIBackend, parseGrokJudgeStdout, parseJudgeResult, renderJudgePrompt } from './judge.mjs';

async function cacheDir(t) {
  const directory = await mkdtemp(join(tmpdir(), 'agent-judge-test-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  return directory;
}

test('judge input includes the prior session and omits hidden data', () => {
  const items = judgeItems(suite, traces);
  assert.equal(items.filter((item) => item.kind === 'save_precision').length, 1);
  assert.equal(items.filter((item) => item.kind === 'save_precision' && item.trace.session_role === 'probe').length, 0);
  const probe = items.find((item) => item.criterion_id === 'applies-diluted-shares' && item.trace.run_id === 'probe-0');
  const input = buildJudgeInput(suite, probe, traces.map((trace) => ({ ...trace, run_id: probe.trace.run_id })));
  const prompt = renderJudgePrompt(input);
  assert.match(prompt, /120 million/);
  assert.equal(prompt.includes(HIDDEN_SENTINEL), false);
  assert.equal(JSON.stringify(input).includes(HIDDEN_SENTINEL), false);
  assert.equal(input.prior_sessions[0].session_id, 'teach-1');
  assert.equal(input.prior_sessions[0].turns[0].answer.includes('120 million'), true);
  const train = items.find((item) => item.trace.run_id === 'train-0' && item.kind === 'rubric');
  assert.equal(buildJudgeInput(suite, train, traces).prior_sessions.length, 0);
  const foreignHistory = traces.map((trace) => ({ ...trace, run_id: 'different-run' }));
  assert.equal(buildJudgeInput(suite, probe, foreignHistory).prior_sessions[0].turns.length, 0);
  assert.equal(judgeItems(suite, [{ ...traces[0], suite: 'different-suite' }]).length, 0);
});

test('judge preserves answer tails, tool values and committed content in both evidence and cache identity', () => {
  const original = structuredClone(traces[0]);
  original.user = 'Context. '.repeat(300) + 'Check the current period.';
  original.answer = 'Supported result. '.repeat(300) + 'The value is 7.';
  original.tool_calls = [{ name: 'connector_query', args: { period: '2026-Q2' }, outcome: 'ok',
    result: { value: 7, unit: 'USD million', period: '2026-Q2' }, result_redacted: false }];
  original.memory_commits[0].summary = 'Method detail. '.repeat(300) + 'Only use diluted shares.';
  const packet = (trace) => buildJudgeInput(suite, judgeItems(suite, [trace])[0], [trace]);
  const input = packet(original);
  assert.equal(input.user, original.user);
  assert.equal(input.answer, original.answer);
  assert.deepEqual(input.trace_summary.tools[0].result, original.tool_calls[0].result);
  assert.equal(input.trace_summary.memory_commits[0].summary, original.memory_commits[0].summary);
  const save = judgeItems(suite, [original]).find((item) => item.kind === 'save_precision');
  assert.equal(save.memory_item.summary, original.memory_commits[0].summary);
  const key = (value) => judgeCacheKey({ input: value, criterion: value.criterion, model: 'fixture' });
  for (const mutate of [
    (trace) => { trace.answer += 'Final correction: the value is 700.'; },
    (trace) => { trace.tool_calls[0].args.period = '2026-Q1'; },
    (trace) => { trace.tool_calls[0].result.unit = 'KRW million'; },
    (trace) => { trace.memory_commits[0].summary += 'Except this unsupported override.'; },
  ]) {
    const changed = structuredClone(original); mutate(changed);
    assert.notEqual(key(packet(changed)), key(input));
  }
});

test('judge follow-ups retain earlier turns from the current session and exclude future or foreign turns', () => {
  const current = { ...traces[1], run_id: 'same-run', turn_index: 1, user: 'Correct the previous result.', answer: 'Corrected.' };
  const first = { ...current, turn_index: 0, user: 'Original request.', answer: 'Earlier result.' };
  const future = { ...current, turn_index: 2, answer: 'FUTURE_SENTINEL' };
  const foreign = { ...first, run_id: 'different-run', answer: 'FOREIGN_SENTINEL' };
  const input = buildJudgeInput(suite, { trace: current, criterion: 'The correction is supported.' }, [first, current, future, foreign]);
  const history = input.prior_sessions.find((session) => session.session_id === current.session_id);
  assert.equal(history.turns.length, 1);
  assert.equal(history.turns[0].answer, first.answer);
  assert.equal(JSON.stringify(input).includes('FUTURE_SENTINEL'), false);
  assert.equal(JSON.stringify(input).includes('FOREIGN_SENTINEL'), false);
});

test('one run id still distinguishes turns when labels join judgments', async (t) => {
  const runId = 'eeeb9bb9-b84c-4c40-92c1-6a4d8949bc8c';
  const smokeSuite = {
    schema: 'loupist-agent-suite/1', suite: 'learning', version: 1,
    scenarios: [{
      id: 'L-SMOKE-001',
      sessions: [
        { id: 'teach-1', role: 'train', memory: 'remember', turns: [{ user: 'Save the method.', expect: { rubric: [{ id: 'states-method', criterion: 'The answer states the saved method.', metric: 'save_recall' }] } }] },
        { id: 'probe-1', role: 'probe', memory: 'read_only', turns: [{ user: 'Value Aurora.', expect: { rubric: [{ id: 'states-method', criterion: 'The answer states the saved method.', metric: 'carry_forward' }] } }] },
      ],
    }],
  };
  const shared = {
    schema: 'loupist-agent-trace/1', run_id: runId, suite: 'learning', scenario_id: 'L-SMOKE-001',
    turn_index: 0, trial: 0, tool_calls: [], memory_commits: [], memory_reads: [],
  };
  const smokeTraces = [
    {
      ...shared, session_id: 'teach-1', session_role: 'train', memory_mode: 'remember', user: 'Save the method.', answer: 'Saved the diluted-share rule.',
      tool_calls: [{ name: 'capability_load', args: { id: 'memory-save' }, outcome: 'ok', error_code: null, error_message: null }],
    },
    { ...shared, session_id: 'probe-1', session_role: 'probe', memory_mode: 'read_only', user: 'Value Aurora.', answer: 'Fair value per share is 40.' },
  ];
  const directory = await cacheDir(t);
  const judgments = await judgeAll({
    suite: smokeSuite, traces: smokeTraces, cacheDir: directory, model: 'fixture-judge',
    backend: { model: 'fixture-judge', judge: async () => ({ pass: true, reason: 'The answer states the method.' }) },
  });
  const teachId = itemId(smokeTraces[0], 'states-method');
  const probeId = itemId(smokeTraces[1], 'states-method');
  assert.notEqual(teachId, probeId);
  assert.equal(judgments.filter((row) => row.criterion_id === 'states-method').length, 2);
  const views = labelViews(judgments, smokeTraces);
  assert.equal(views.find((view) => view.item_id === teachId).answer, 'Saved the diluted-share rule.');
  assert.equal(views.find((view) => view.item_id === probeId).answer, 'Fair value per share is 40.');
  const labels = views.map((view) => ({ item_id: view.item_id, criterion_id: view.criterion_id, human_pass: view.item_id === probeId, note: '' }));
  const scored = agreement(judgments, labels);
  assert.equal(scored.counts.matched, judgments.length);
  assert.equal(scored.counts.unmatched, 0);
  assert.equal(scored.counts.tp, labels.filter((label) => label.human_pass).length);
});

test('judge identities keep repeated commits and delimiter-bearing IDs distinct', async (t) => {
  const trace = structuredClone(traces[0]);
  trace.memory_commits.push({ ...trace.memory_commits[0], summary: 'A revised method.' });
  const commits = judgeItems(suite, [trace]).filter((item) => item.kind === 'save_precision');
  assert.equal(commits.length, 2);
  assert.notEqual(itemId(trace, commits[0].criterion_id), itemId(trace, commits[1].criterion_id));
  assert.notEqual(itemId({ ...trace, run_id: 'one:A', scenario_id: 'B' }, 'quality'),
    itemId({ ...trace, run_id: 'one', scenario_id: 'A:B' }, 'quality'));
  assert.notEqual(itemId(trace, 'quality'), itemId({ ...trace, suite: 'another-suite' }, 'quality'));
  let calls = 0;
  await assert.rejects(judgeAll({ suite, traces: [trace, trace], cacheDir: await cacheDir(t),
    backend: { judge: async () => { calls += 1; return { pass: true, reason: 'known' }; } },
  }), /Ambiguous judge item identity/u);
  assert.equal(calls, 0);
});

test('judge rejects absent answer evidence before making any backend request', async (t) => {
  const directory = await cacheDir(t);
  let calls = 0;
  const backend = { judge: async () => { calls += 1; return { pass: true, reason: 'known' }; } };
  for (const answer of [undefined, null, 0, false, {}]) {
    const missing = { ...traces[0], run_id: 'absent-answer', answer };
    const item = judgeItems(suite, [missing])[0];
    assert.throws(() => buildJudgeInput(suite, item, [missing]), /requires observed answer text/u);
    await assert.rejects(judgeAll({ suite, traces: [traces[0], missing], cacheDir: directory, backend }),
      /requires observed answer text/u);
  }
  assert.equal(calls, 0);
  const empty = { ...traces[0], answer: '' };
  assert.equal(buildJudgeInput(suite, judgeItems(suite, [empty])[0], [empty]).answer, '');
});

test('cache hits skip the backend and invalid results are not cached', async (t) => {
  const directory = await cacheDir(t);
  let calls = 0;
  const backend = { model: 'grok-4.7', judge: async () => {
    calls += 1;
    if (calls === 1) return { pass: 'yes', reason: 'bad' };
    return { pass: true, reason: 'The answer states 120 million diluted shares.' };
  } };
  const one = [traces[0]];
  await assert.rejects(() => judgeAll({ suite, traces: one, cacheDir: directory, backend, model: 'grok-4.7' }), /boolean pass/u);
  assert.equal(calls, 1);
  const first = await judgeAll({ suite, traces: one, cacheDir: directory, backend, model: 'grok-4.7' });
  const second = await judgeAll({ suite, traces: one, cacheDir: directory, backend, model: 'grok-4.7' });
  assert.equal(calls, 3);
  assert.equal(first.every((row) => row.cache === 'miss'), true);
  assert.equal(second.every((row) => row.cache === 'hit'), true);
  assert.equal(second[0].pass, true);
  assert.equal(second[0].reasoning_effort, DEFAULT_REASONING_EFFORT);
});

test('concurrent judging bounds in-flight calls and keeps item order', async (t) => {
  const directory = await cacheDir(t);
  let inFlight = 0;
  let peak = 0;
  const backend = { model: 'grok-4.7', judge: async (_prompt, { item }) => {
    inFlight += 1;
    peak = Math.max(peak, inFlight);
    await new Promise((resolve) => setTimeout(resolve, 5));
    inFlight -= 1;
    return { pass: true, reason: `judged ${item.criterion_id}` };
  } };
  const sequential = await judgeAll({ suite, traces, cacheDir: await cacheDir(t), backend, model: 'grok-4.7' });
  assert.equal(peak, 1);
  peak = 0;
  const concurrent = await judgeAll({ suite, traces, cacheDir: directory, backend, model: 'grok-4.7', concurrency: 2 });
  assert.ok(sequential.length > 2);
  assert.equal(peak, 2);
  assert.deepEqual(concurrent.map((row) => row.item_id), sequential.map((row) => row.item_id));
  assert.deepEqual(concurrent.map((row) => row.reason), sequential.map((row) => row.reason));
});

test('reasoning effort is stored on each judgment and splits the cache', async (t) => {
  const directory = await cacheDir(t);
  let calls = 0;
  const backend = { model: 'grok-4.7', judge: async () => {
    calls += 1;
    return { pass: true, reason: 'The answer states 120 million diluted shares.' };
  } };
  const one = [traces[0]];
  const medium = await judgeAll({ suite, traces: one, cacheDir: directory, backend, model: 'grok-4.7' });
  const low = await judgeAll({ suite, traces: one, cacheDir: directory, backend, model: 'grok-4.7', reasoningEffort: 'low' });
  const again = await judgeAll({ suite, traces: one, cacheDir: directory, backend, model: 'grok-4.7' });
  assert.equal(calls, 4);
  assert.equal(medium.every((row) => row.cache === 'miss' && row.reasoning_effort === 'medium'), true);
  assert.equal(low.every((row) => row.cache === 'miss' && row.reasoning_effort === 'low'), true);
  assert.equal(again.every((row) => row.cache === 'hit' && row.reasoning_effort === 'medium'), true);
});

test('grok stdout and OpenAI responses validate against the judge schema', async () => {
  assert.deepEqual(parseGrokJudgeStdout(JSON.stringify({ text: '{"pass":false,"reason":"missing shares"}', stopReason: 'end_turn' })), { pass: false, reason: 'missing shares' });
  assert.deepEqual(parseJudgeResult({ pass: true, reason: 'kept' }), { pass: true, reason: 'kept' });
  assert.throws(() => parseJudgeResult({ pass: true, reason: 'kept', extra: 1 }), /unexpected field/u);
  const args = grokJudgeArgs({ prompt: 'judge this', model: 'grok-4.7', cwd: '/tmp/empty-judge', reasoningEffort: 'low' });
  assert.equal(args[0], '-p');
  for (const flag of ['--json-schema', '-m', '--disable-web-search', '--cwd', '--reasoning-effort']) assert.ok(args.includes(flag));
  assert.equal(args[args.indexOf('-m') + 1], 'grok-4.7');
  assert.equal(args[args.indexOf('--cwd') + 1], '/tmp/empty-judge');
  assert.equal(args[args.indexOf('--reasoning-effort') + 1], 'low');
  const defaults = grokJudgeArgs({ prompt: 'judge this', model: 'grok-4.7', cwd: '/tmp/empty-judge' });
  assert.equal(defaults[defaults.indexOf('--reasoning-effort') + 1], 'medium');
  let seenEffort = null;
  const local = grokBackend({
    run: async (_command, argv) => {
      seenEffort = argv[argv.indexOf('--reasoning-effort') + 1];
      return JSON.stringify({ structured_output: { pass: true, reason: 'kept' } });
    },
  });
  assert.deepEqual(await local.judge('criterion'), { pass: true, reason: 'kept' });
  assert.equal(seenEffort, 'medium');
  const seen = {};
  const backend = openAIBackend({
    endpoint: 'https://judge.example/v1/chat/completions', apiKeyEnv: 'AGENT_EVAL_TEST_KEY', model: 'judge-model',
    reasoningEffort: 'high',
    fetchImpl: async (url, request) => {
      seen.url = url;
      seen.body = JSON.parse(request.body);
      seen.authorization = request.headers.authorization;
      return { ok: true, json: async () => ({ choices: [{ message: { content: '{"pass":true,"reason":"reusable"}' } }] }) };
    },
  });
  const previous = process.env.AGENT_EVAL_TEST_KEY;
  process.env.AGENT_EVAL_TEST_KEY = 'test-key-not-a-secret';
  try {
    assert.deepEqual(await backend.judge('criterion'), { pass: true, reason: 'reusable' });
    assert.equal(seen.url, 'https://judge.example/v1/chat/completions');
    assert.equal(seen.body.model, 'judge-model');
    assert.equal(seen.body.reasoning_effort, 'high');
    await backend.judge('criterion', { reasoningEffort: 'low' });
    assert.equal(seen.body.reasoning_effort, 'low');
    assert.equal(Object.hasOwn(seen.body, 'api_key'), false);
    assert.equal(seen.authorization, 'Bearer test-key-not-a-secret');
    const failing = openAIBackend({
      endpoint: 'https://judge.example/v1/chat/completions', apiKeyEnv: 'AGENT_EVAL_TEST_KEY',
      fetchImpl: async () => ({ ok: false, status: 401, json: async () => ({ error: 'test-key-not-a-secret' }) }),
    });
    await assert.rejects(failing.judge('criterion'), (error) => {
      assert.equal(error.message, 'Judge endpoint failed with status 401');
      assert.equal(error.message.includes('test-key-not-a-secret'), false);
      return true;
    });
  } finally {
    if (previous == null) delete process.env.AGENT_EVAL_TEST_KEY;
    else process.env.AGENT_EVAL_TEST_KEY = previous;
  }
  assert.throws(() => openAIBackend({ endpoint: 'http://judge.example', apiKeyEnv: 'AGENT_EVAL_TEST_KEY' }), /https/u);
});


test('model families ignore routing prefixes and leave unknown IDs unclassified', () => {
  for (const [id, family] of [
    ['gpt-6', 'openai'], ['openai-codex/gpt-6-luna', 'openai'], ['o3', 'openai'], ['openai/o4-mini', 'openai'],
    ['codex-mini', 'openai'], ['anthropic/claude-opus-4', 'anthropic'], ['grok-4.7', 'xai'],
    ['google/gemini-2.5-pro', 'google'], ['deepseek-chat', 'deepseek'], ['kimi-k2', 'moonshot'],
    ['minimax-m2', 'minimax'], ['openai/custom', 'unknown'], ['other', 'unknown'], ['opus', 'unknown'],
    ['', 'unknown'], [null, 'unknown'], [undefined, 'unknown'],
  ]) assert.equal(modelFamily(id), family, String(id));
});
