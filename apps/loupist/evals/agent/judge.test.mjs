import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, rm, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { suite, traces, HIDDEN_SENTINEL } from './fixtures/sample.mjs';
import { agreement, labelViews } from './labels.mjs';
import sessionLanguage from './suites/session-language-v1.json' with { type: 'json' };
import { buildJudgeInput, modelFamily, DEFAULT_REASONING_EFFORT, JUDGE_SCHEMA, claudeBackend, claudeJudgeArgs, claudeJudgeEnv, codexBackend, grokBackend, grokJudgeArgs, itemId, judgeAll, judgeCacheKey, judgeItems, openAIBackend, parseClaudeJudgeStdout, parseGrokJudgeStdout, parseJudgeResult, renderJudgePrompt } from './judge.mjs';

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

test('Claude receives the turn’s exact screen labels for directions even without tool calls', async (t) => {
  const rows = [
    ['F3-UI-01', 'ko', 0, '**봇** opens chat. **월드** opens relationships. **예약** opens schedules.',
      { bot: '봇', map: '월드', schedule: '예약' }, true],
    ['F3-UI-02', 'en', 1, '사이드바 제목은 **Research sessions**, 연결 그룹은 **Market data**입니다.',
      { research_sessions_heading: 'Research sessions', market_data_group: 'Market data' }, true],
    ['F3-UI-02', 'en', 1, '사이드바 제목은 **Research sessions**, 연결 그룹은 **Market data platform**입니다.',
      { research_sessions_heading: 'Research sessions', market_data_group: 'Market data' }, false],
  ];
  const directions = rows.map(([id, appearance, index, answer], trial) => {
    const scenario = sessionLanguage.scenarios.find(scenario => scenario.id === id);
    const [session] = scenario.sessions;
    return { ...traces[0], suite: sessionLanguage.suite, scenario_id: id, trial,
      session_id: session.id, session_role: session.role, memory_mode: session.memory,
      turn_index: index, user: session.turns[index].user, answer,
      resolved_appearance: appearance, tool_calls: [], memory_reads: [], memory_commits: [] };
  });
  let call = 0;
  const backend = claudeBackend({ env: {}, run: async (_command, _args, { input }) => {
    const packet = JSON.parse(input.split('\nObserved input:\n')[1]);
    const labels = rows[call++][4];
    assert.deepEqual(packet.screen_labels, labels);
    assert.deepEqual(packet.trace_summary.tools, []);
    assert.match(input, /screen_labels.*no tool call/u);
    const pass = Object.values(packet.screen_labels).every(label => packet.answer.includes(`**${label}**`));
    return claudeResult({ structured_output: { pass, reason: 'Compared with the screen labels.' } });
  } });
  const judgments = await judgeAll({ suite: sessionLanguage, traces: directions, cacheDir: await cacheDir(t), backend });
  assert.equal(call, rows.length);
  assert.deepEqual(judgments.map(row => row.pass), rows.map(row => row[5]));

  const followUp = { ...directions[0], turn_index: 2, user: 'Summarize the note.', answer: 'Summary.' };
  const [item] = judgeItems(sessionLanguage, [followUp]);
  assert.equal(Object.hasOwn(buildJudgeInput(sessionLanguage, item, [followUp]), 'screen_labels'), false);
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

test('codex judge runs tool-free with the verdict schema and reads the last message', async () => {
  const { writeFile, readFile } = await import('node:fs/promises');
  let seen;
  const backend = codexBackend({ model: 'gpt-6-astra', run: async (command, args, { input }) => {
    seen = { command, args, input, schema: JSON.parse(await readFile(args[args.indexOf('--output-schema') + 1], 'utf8')) };
    await writeFile(args[args.indexOf('--output-last-message') + 1], '{"pass":false,"reason":"stale value"}');
    return '';
  } });
  assert.deepEqual(await backend.judge('Judge this.', { reasoningEffort: 'low' }), { pass: false, reason: 'stale value' });
  assert.equal(seen.command, 'codex');
  assert.equal(seen.args.at(-1), '-');
  assert.equal(seen.input, 'Judge this.');
  for (const flag of ['features.shell_tool=false', 'web_search="disabled"', 'model_reasoning_effort="low"', 'read-only']) assert.ok(seen.args.includes(flag), flag);
  assert.deepEqual(seen.schema.required, ['pass', 'reason']);
  assert.equal(modelFamily(backend.model), 'openai');
});

test('codex judge sends a Stage A sized prompt intact over stdin', async (t) => {
  const { writeFile } = await import('node:fs/promises');
  const bin = await cacheDir(t);
  const fake = join(bin, 'fake-codex.mjs');
  await writeFile(fake, `#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { writeFile } from 'node:fs/promises';
const args = process.argv.slice(2);
const hash = createHash('sha256');
for await (const chunk of process.stdin) hash.update(chunk);
const output = args[args.indexOf('--output-last-message') + 1];
await writeFile(output, JSON.stringify({ pass: args.at(-1) === '-', reason: hash.digest('hex') }));
`, { mode: 0o755 });
  const prompt = 'Index close: 2,750. Record advances and declines.\n'.repeat(30_000);
  assert.ok(Buffer.byteLength(prompt) > 1_309_413);
  assert.deepEqual(await codexBackend({ command: fake }).judge(prompt),
    { pass: true, reason: createHash('sha256').update(prompt).digest('hex') });
});

const claudeResult = (fields) => JSON.stringify({ type: 'result', subtype: 'success', is_error: false, stop_reason: 'end_turn',
  num_turns: 1, result: '', session_id: 'fixture', ...fields });

test('claude judge arguments pin a tool-free, context-free single turn', () => {
  // A security pin: every flag here is required to keep untrusted trace text from reaching a tool or loaded context.
  assert.deepEqual(claudeJudgeArgs({ model: 'claude-opus-5-5', reasoningEffort: 'low' }), [
    '-p', '--model', 'claude-opus-5-5', '--effort', 'low',
    '--tools', '', '--strict-mcp-config', '--disallowedTools', 'mcp__*', '--safe-mode',
    '--setting-sources', '', '--disable-slash-commands', '--no-session-persistence', '--max-turns', '1',
    '--output-format', 'json', '--json-schema', JSON.stringify(JUDGE_SCHEMA),
  ]);
  const defaults = claudeJudgeArgs();
  assert.equal(defaults[defaults.indexOf('--model') + 1], 'claude-opus-5-5');
  assert.equal(defaults[defaults.indexOf('--effort') + 1], 'medium');
  for (const flag of ['--mcp-config', '--allowedTools', '--allowed-tools', '--add-dir', '--plugin-dir', '--settings', '--agents',
    '--bare', '--dangerously-skip-permissions', '--allow-dangerously-skip-permissions', '--continue', '--resume']) {
    assert.equal(defaults.includes(flag), false, flag);
  }
  assert.equal(modelFamily(claudeBackend().model), 'anthropic');
});

test('claude judge child environment keeps the subscription login and drops API keys and parent session state', () => {
  const env = claudeJudgeEnv({
    PATH: '/usr/bin', HOME: '/home/fixture', CLAUDE_CONFIG_DIR: '/home/fixture/.claude', CLAUDE_CODE_OAUTH_TOKEN: 'oauth-fixture',
    ANTHROPIC_API_KEY: 'key-fixture', ANTHROPIC_AUTH_TOKEN: 'token-fixture', CLAUDECODE: '1', CLAUDE_CODE_SIMPLE: '1',
    ANTHROPIC_PROFILE: 'work', ANTHROPIC_FEDERATION_RULE_ID: 'rule', ANTHROPIC_ORGANIZATION_ID: 'org',
    ANTHROPIC_CUSTOM_HEADERS: 'Authorization: Bearer x', ANTHROPIC_BASE_URL: 'https://gateway.example',
    ANTHROPIC_AWS_API_KEY: 'aws', ANTHROPIC_FOUNDRY_API_KEY: 'foundry',
    CLAUDE_CODE_SESSION_ID: 'parent', CLAUDE_CODE_MESSAGING_SOCKET: '/tmp/socket', CLAUDE_CODE_USE_BEDROCK: '1', CLAUDE_EFFORT: 'max',
  });
  assert.deepEqual(Object.keys(env).sort(), ['CLAUDE_CODE_OAUTH_TOKEN', 'CLAUDE_CONFIG_DIR', 'HOME', 'PATH']);
});

test('claude judge sends the prompt on stdin from a new directory that is removed afterwards', async () => {
  const seen = [];
  const backend = claudeBackend({ env: { PATH: '/usr/bin', ANTHROPIC_API_KEY: 'key-fixture' }, run: async (command, args, options) => {
    seen.push({ command, args, ...options, existed: (await stat(options.cwd)).isDirectory() });
    await new Promise((resolve) => setTimeout(resolve, 5));
    return claudeResult({ structured_output: { pass: true, reason: `verdict for ${options.input}` } });
  } });
  const verdicts = await Promise.all([backend.judge('first prompt', { reasoningEffort: 'high' }), backend.judge('second prompt')]);
  assert.deepEqual(verdicts.map((verdict) => verdict.reason), ['verdict for first prompt', 'verdict for second prompt']);
  assert.equal(seen.length, 2);
  assert.notEqual(seen[0].cwd, seen[1].cwd);
  for (const call of seen) {
    assert.equal(call.command, 'claude');
    assert.equal(call.existed, true);
    assert.equal(call.args.includes(call.input), false, 'the prompt is not an argument');
    assert.deepEqual(call.env, { PATH: '/usr/bin' });
    await assert.rejects(stat(call.cwd), { code: 'ENOENT' });
  }
  const effort = (input) => { const { args } = seen.find((call) => call.input === input); return args[args.indexOf('--effort') + 1]; };
  assert.equal(effort('first prompt'), 'high');
  assert.equal(effort('second prompt'), 'medium');
});

test('claude judge process receives the prompt on stdin, its own directory and a filtered environment', async (t) => {
  const { writeFile, realpath } = await import('node:fs/promises');
  const bin = await cacheDir(t);
  const fake = join(bin, 'fake-claude.mjs');
  await writeFile(fake, `#!/usr/bin/env node
let input = '';
process.stdin.setEncoding('utf8').on('data', (chunk) => { input += chunk; }).on('end', () => {
  const reason = JSON.stringify({ input, cwd: process.cwd(), apiKey: process.env.ANTHROPIC_API_KEY ?? null, args: process.argv.slice(2) });
  if (input === 'fail') { process.stdout.write(JSON.stringify({ type: 'result', subtype: 'error_during_execution', is_error: true, errors: ['Not logged in'] })); process.exit(1); }
  process.stdout.write(JSON.stringify({ type: 'result', subtype: 'success', is_error: false, stop_reason: 'end_turn', structured_output: { pass: true, reason } }));
});
`, { mode: 0o755 });
  const backend = claudeBackend({ command: fake, env: { PATH: process.env.PATH, ANTHROPIC_API_KEY: 'key-fixture' } });
  const observed = JSON.parse((await backend.judge('Judge "this" criterion.\nSecond line.')).reason);
  assert.equal(observed.input, 'Judge "this" criterion.\nSecond line.');
  assert.equal(observed.apiKey, null);
  assert.deepEqual(observed.args, claudeJudgeArgs());
  assert.match(observed.cwd, /agent-judge-claude-/u);
  assert.notEqual(observed.cwd, await realpath(process.cwd()));
  await assert.rejects(stat(observed.cwd), { code: 'ENOENT' });
  await assert.rejects(backend.judge('fail'), /exited 1.*error_during_execution.*Not logged in/u);
});

test('claude judge accepts only a successful structured verdict and fails malformed or refused answers', async (t) => {
  assert.deepEqual(parseClaudeJudgeStdout(claudeResult({ structured_output: { pass: false, reason: 'stale value' } })),
    { pass: false, reason: 'stale value' });
  for (const [stdout, error] of [
    ['I think it passes.', /not JSON/u],
    [JSON.stringify({ pass: true, reason: 'bare verdict without a result message' }), /not a result message/u],
    [claudeResult({ result: '{"pass":true,"reason":"text instead of the schema tool"}' }), /no structured verdict/u],
    [claudeResult({ structured_output: '{"pass":true,"reason":"string"}' }), /no structured verdict/u],
    [claudeResult({ structured_output: { pass: 'yes', reason: 'not boolean' } }), /boolean pass/u],
    [claudeResult({ structured_output: { pass: true, reason: 'kept', score: 1 } }), /unexpected field/u],
    [claudeResult({ structured_output: { pass: true, reason: ' ' } }), /non-empty reason/u],
    [claudeResult({ stop_reason: 'refusal', result: 'I cannot help with that.' }), /refused/u],
    [claudeResult({ stop_reason: 'refusal', is_error: true, structured_output: { pass: true, reason: 'retracted' } }), /refused/u],
    [claudeResult({ is_error: true, result: 'API Error: 500', structured_output: { pass: true, reason: 'kept' } }), /failed \(success\): API Error/u],
    [claudeResult({ subtype: 'error_max_turns', is_error: true, errors: ['Reached maximum number of turns (1)'] }), /error_max_turns.*maximum number of turns/u],
    [claudeResult({ subtype: 'error_max_structured_output_retries', is_error: true, errors: [] }), /error_max_structured_output_retries/u],
  ]) assert.throws(() => parseClaudeJudgeStdout(stdout), error, stdout);

  const failed = claudeBackend({ env: {}, run: async () => {
    throw Object.assign(new Error('claude exited 1: '), { stdout: claudeResult({ subtype: 'error_during_execution', is_error: true, errors: ['Not logged in'] }) });
  } });
  await assert.rejects(failed.judge('prompt'), /claude exited 1.*Not logged in/u);
  const passingButNonzero = claudeBackend({ env: {}, run: async () => {
    throw Object.assign(new Error('claude exited 1: '), { stdout: claudeResult({ structured_output: { pass: true, reason: 'kept' } }) });
  } });
  await assert.rejects(passingButNonzero.judge('prompt'), /claude exited 1/u);

  const directory = await cacheDir(t);
  const refusing = claudeBackend({ env: {}, run: async () => claudeResult({ stop_reason: 'refusal', result: 'I cannot help with that.' }) });
  await assert.rejects(judgeAll({ suite, traces: [traces[0]], cacheDir: directory, backend: refusing, model: refusing.model }), /refused/u);
  const judged = claudeBackend({ env: {}, run: async () => claudeResult({ structured_output: { pass: false, reason: 'The answer omits diluted shares.' } }) });
  const rows = await judgeAll({ suite, traces: [traces[0]], cacheDir: directory, backend: judged, model: judged.model });
  assert.equal(rows.every((row) => row.cache === 'miss' && row.pass === false && row.model === 'claude-opus-5-5'), true, 'a refusal leaves no cached verdict');
});

test('a judged Lens commit carries the stored page kind and basis', () => {
  const suite = { schema: 'loupist-agent-suite/1', suite: 'lens', version: 1, scenarios: [{ id: 'LC', sessions: [{ id: 'train', role: 'train', memory: 'remember', turns: [{ user: 'Research.' }] }] }] };
  const trace = { schema: 'loupist-agent-trace/1', run_id: 'r', suite: 'lens', scenario_id: 'LC', session_id: 'train', session_role: 'train', memory_mode: 'remember', turn_index: 0, trial: 0, user: 'Research.', status: 'completed', answer: 'Filed.', tool_calls: [],
    memory_commits: [{ kind: 'lens', record_id: 'rec_a', title: 'Halvren Optics', operation: 'create' }, { kind: 'source', record_id: 'src_b', title: 'Release' }],
    lens: { pages: [{ record_id: 'rec_a', kind: 'subject', title: 'Halvren Optics', sections: [{ key: 'facts', basis: ['span'] }, { key: 'views', basis: ['span', 'page'] }], ops_this_turn: [] }] } };
  const items = judgeItems(suite, [trace]).filter((item) => item.kind === 'save_precision');
  assert.deepEqual(items.map((item) => [item.memory_item.page_kind ?? null, item.memory_item.page_basis ?? null]), [['subject', ['page', 'span']], [null, null]]);
});

test('Journal judge packets preserve committed checks in both callers without inventing absent legacy fields', () => {
  const suite = { schema: 'loupist-agent-suite/1', suite: 'journal', version: 1, scenarios: [{ id: 'J', sessions: [{ id: 'train', role: 'train', memory: 'remember', turns: [{ user: 'Record my check.', expect: { rubric: [{ id: 'recorded', criterion: 'The user check was recorded.' }] } }] }] }] };
  const check = { claim: 'Customer share stays below 60%.', metric: 'customer share', direction: 'below', threshold: 60, unit: 'percent', byDate: '2026-12-15', confidence: 70 };
  const trace = { schema: 'loupist-agent-trace/1', run_id: 'r', suite: 'journal', scenario_id: 'J', session_id: 'train', session_role: 'train', memory_mode: 'remember', turn_index: 0, trial: 0, user: 'Record my check.', status: 'completed', answer: 'Recorded.',
    tool_calls: [{ name: 'journal_save', args: { check: structuredClone(check) }, outcome: 'ok', result: { status: 'staged', durable: false } }],
    memory_commits: [{ kind: 'journal', record_id: 'jnl_a', operation: 'create', judgment: 'I will hold.', reasoning: 'User supplied evidence.', check }] };
  const packet = (row) => buildJudgeInput(suite, judgeItems(suite, [row]).find(item => item.kind === 'rubric'), [row]);
  const evidence = packet(trace).trace_summary.memory_commits[0];
  assert.deepEqual(evidence.check, check);
  assert.equal(evidence.judgment, 'I will hold.');
  assert.equal(evidence.reasoning, 'User supplied evidence.');
  assert.equal(Object.hasOwn(evidence, 'prediction'), false);
  assert.equal(Object.hasOwn(evidence, 'outcome'), false);
  const saved = judgeItems(suite, [trace]).find(item => item.kind === 'save_precision').memory_item;
  assert.deepEqual(saved.check, check);
  const changed = structuredClone(trace); changed.memory_commits[0].check.threshold = 61;
  const key = row => judgeCacheKey({ input: packet(row), criterion: 'recorded', model: 'fixture' });
  assert.notEqual(key(changed), key(trace));
  const absent = structuredClone(trace); delete absent.memory_commits[0].check;
  assert.equal(Object.hasOwn(packet(absent).trace_summary.memory_commits[0], 'check'), false, 'staged tool arguments do not replace missing commit evidence');
  absent.memory_commits[0].prediction = { claim: 'Legacy observation' };
  absent.memory_commits[0].outcome = null;
  assert.deepEqual(packet(absent).trace_summary.memory_commits[0].prediction, { claim: 'Legacy observation' });
  assert.equal(packet(absent).trace_summary.memory_commits[0].outcome, null);
});

test('judge input carries the stored Lens pages so a stale current value is visible', () => {
  const suite = { schema: 'loupist-agent-suite/1', suite: 'lens', version: 1, scenarios: [{ id: 'LC', sessions: [{ id: 'train', role: 'train', memory: 'remember', turns: [{ user: 'Update.', expect: { rubric: [{ id: 'current', criterion: 'The page states 14 ships as current.' }] } }] }] }] };
  const page = (markdown) => ({ record_id: 'rec_a', kind: 'subject', title: 'Dongjin', summary: '', archived: false,
    sections: [{ key: 'facts', heading: 'Fleet', markdown, basis: ['span'], contested: false }], ops_this_turn: [{ op: 'put', key: 'facts', outcome: 'replaced' }] });
  const trace = (markdown) => ({ schema: 'loupist-agent-trace/1', run_id: 'r', suite: 'lens', scenario_id: 'LC', session_id: 'train', session_role: 'train', memory_mode: 'remember', turn_index: 0, trial: 0, user: 'Update.', status: 'completed', answer: 'Done.', tool_calls: [], memory_commits: [], lens: { pages: [page(markdown)] } });
  const input = (markdown) => { const traces = [trace(markdown)]; return buildJudgeInput(suite, judgeItems(suite, traces)[0], traces); };
  assert.equal(input('Current fleet: 14 ships.').lens_pages_after_turn[0].sections[0].markdown, 'Current fleet: 14 ships.');
  assert.notDeepEqual(input('Current fleet: 18 ships. A rumor of 14 was rejected.'), input('Current fleet: 14 ships.'));
});

test('a scheduled rubric sees the full verified reply and the exact Bot preview', () => {
  const user = 'Give an implied growth range for Fictional Forge.';
  const kind = 'custom';
  const scheduledSuite = { schema: 'loupist-agent-suite/1', suite: 'scheduled', scenarios: [{
    id: 'VALUATION', language: 'en', sessions: [{ id: 'bot', role: 'probe', memory: 'remember', turns: [
      { user, scheduled: { kind }, expect: { rubric: [{ id: 'range', criterion: 'Give an implied growth range, not a single value.', metric: 'task_success' }] } },
    ] }],
  }] };
  const answer = 'I compared the valuations.';
  const full = answer + '\n\nThe implied growth range is 8% to 12%.';
  const identity = { run_id: 'scheduled-run', task_id: 'scheduled-task' };
  const reply = { thread_id: 'research-thread', message_id: 'originating-reply' };
  const trace = { ...traces[0], suite: 'scheduled', scenario_id: 'VALUATION', session_id: 'bot',
    turn_index: 0, user, answer, resolved_appearance: 'en',
    saved_task: { id: identity.task_id, prompt: user, kind },
    admission: { ...identity, prompt: user, surface: 'scheduled', thread_id: reply.thread_id },
    home_thread_id: 'home-thread',
    runs: [{ ...identity, ...reply, kind, status: 'succeeded', summary: answer, scheduled_for_ms: 10, started_at_ms: 11, finished_at_ms: 12 }],
    deliveries: [{ ...identity, kind, status: 'succeeded', finished_at_ms: 12, thread_id: 'home-thread', run_thread_id: reply.thread_id,
      message_id: 'bot-delivery', content: answer }],
    session_answer: { ...reply, status: 'complete', host_composed: false, content: full },
    terminal_event: { status: 'completed', message_id: reply.message_id },
  };
  const input = row => buildJudgeInput(scheduledSuite, judgeItems(scheduledSuite, [row])[0], [row]);
  assert.equal(input(trace).answer, full);
  assert.equal(input(trace).bot_preview, answer);
  for (const changed of [
    { ...trace, terminal_event: { ...trace.terminal_event, message_id: 'other-reply' } },
    { ...trace, session_answer: { ...trace.session_answer, host_composed: true } },
    { ...trace, deliveries: [] },
  ]) {
    assert.equal(input(changed).answer, answer);
    assert.equal(Object.hasOwn(input(changed), 'bot_preview'), false);
  }
  const interactive = structuredClone(scheduledSuite);
  delete interactive.scenarios[0].sessions[0].turns[0].scheduled;
  assert.equal(buildJudgeInput(interactive, judgeItems(interactive, [trace])[0], [trace]).answer, answer);
});
