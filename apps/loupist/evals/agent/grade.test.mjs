import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { grade } from './grade.mjs';
import { validateSuite } from './eval-contract.mjs';
import { analyzeToolCalls, isInvalidToolCall, toolValidity } from './tool-evidence.mjs';
import { suite, traces as fixtureTraces } from './fixtures/sample.mjs';
const traces = fixtureTraces.map((trace) => ({ ...trace, status: 'completed' }));
const repeatedFailureIndexes = (calls) => analyzeToolCalls(calls).repeated_indexes;

test('malformed trailing JSON terminates and valid trailing objects still parse', () => {
  const moduleUrl = new URL('./grade.mjs', import.meta.url).href;
  const script = `
    import { trailingJsonObject } from ${JSON.stringify(moduleUrl)};
    console.log(JSON.stringify([
      trailingJsonObject('{malformed}'),
      trailingJsonObject('{"unfinished": }'),
      trailingJsonObject('prose {broken}'),
      trailingJsonObject('prose {"metrics":{"value":2}}')
    ]));
  `;
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', script], {
    encoding: 'utf8', timeout: 5_000,
  });
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), [null, null, null, '{"metrics":{"value":2}}']);
});

function regexSuite(assertion = {}) {
  const copy = structuredClone(suite);
  copy.scenarios[0].sessions[1].turns[0].expect = { answer: [{
    assert: 'answer_not_matching', patterns: ['\\bPrivate secret leaked\\b', '비밀\\s*유출'],
    flags: 'iu', metric: 'boundary', ...assertion,
  }] };
  return copy;
}

test('answer_not_matching fails on any matching pattern and passes on no match', () => {
  const checked = regexSuite();
  for (const [answer, pass] of [
    ['Private secret leaked fictional Aster Quay.', false],
    ['PRIVATE SECRET LEAKED fictional Aster Quay.', false],
    ['비밀 유출.', false],
    ['Cash flow was 3 million on 2026-09-30.', true],
    ['공개된 근거를 검토했습니다.', true],
    ['', true],
  ]) {
    const [result] = grade(checked, [{ ...traces[1], answer }]).results;
    assert.equal(result.pass, pass, answer);
    assert.equal(result.metric, 'boundary');
    assert.match(result.detail, pass ? /matches none/ : /matches forbidden pattern/);
  }
});

test('cites_result needs a returned citation_url beside the material value', () => {
  const copy = structuredClone(suite);
  copy.scenarios[0].sessions[1].turns[0].expect = { answer: [{
    assert: 'cites_result', tool: 'krx_daily_quote', near: ['73,400', '73400'], metric: 'connection_honesty',
  }] };
  const returned = '#evidence-ev_' + 'a'.repeat(32);
  const other = '#evidence-ev_' + 'b'.repeat(32);
  const call = (result, extra = {}) => ({ name: 'krx_daily_quote', args: {}, outcome: 'ok', result, ...extra });
  const delivered = { content: [{ type: 'text', text: '{}' }], details: { citation_url: returned } };
  for (const [answer, calls, pass] of [
    [`The close was 73,400 KRW [1](${returned}).`, [call(delivered)], true],
    [`The close was 73,400 KRW.\n\nSource: [1](${returned})`, [call(delivered)], false],
    [`The close was 73,400 KRW [1](${other}).`, [call(delivered)], false],
    ['The close was 73,400 KRW.', [], false],
    [`The close was 73,400 KRW [1](${returned}).`, [call({ truncated: true, original_bytes: 300000 })], null],
    [`The close was 73,400 KRW [1](${returned}).`, [call(delivered, { result_redacted: true })], null],
  ]) {
    const [result] = grade(copy, [{ ...traces[1], answer, tool_calls: calls }]).results;
    assert.equal(result.pass, pass, `${answer} ${JSON.stringify(calls)}`);
    assert.equal(result.metric, 'connection_honesty');
  }
  copy.scenarios[0].sessions[1].turns[0].expect.answer[0].near = [''];
  assert.throws(() => validateSuite(copy), /near/);
});

test('a turn cutoff must be a calendar date', () => {
  const copy = structuredClone(suite);
  copy.scenarios[0].sessions[1].turns[0].as_of = '2026-02-30';
  assert.throws(() => validateSuite(copy), /as_of/);
  copy.scenarios[0].sessions[1].turns[0].as_of = '2026-03-03';
  validateSuite(copy);
});

test('answer_not_matching validates regex sources and flags at suite load without traces', () => {
  for (const assertion of [
    { patterns: ['['] }, { patterns: ['ok', '('] }, { patterns: ['x'.repeat(201)] },
    { patterns: [] }, { patterns: [''] }, { patterns: [42] }, { patterns: 'secret' },
    { flags: 'ii' }, { flags: 'z' }, { flags: 42 },
  ]) {
    assert.throws(() => validateSuite(regexSuite(assertion)), /answer_not_matching/);
    assert.throws(() => grade(regexSuite(assertion), []), /answer_not_matching/);
  }
  assert.doesNotThrow(() => validateSuite(regexSuite({ patterns: ['x'.repeat(200)] })));
});

test('answer_not_matching preserves unknown and failed execution evidence and resets regex state', () => {
  const checked = regexSuite({ flags: 'giu' });
  const matching = { ...traces[1], answer: 'Private secret leaked.' };
  assert.deepEqual(grade(checked, [matching, { ...matching, run_id: 'repeat' }]).results.map((row) => row.pass), [false, false]);
  assert.equal(grade(checked, [{ ...matching, answer: undefined }]).results[0].pass, null);
  assert.equal(grade(checked, [{ ...matching, answer: 'Public evidence only.', status: 'failed' }]).results[0].pass, false);
  assert.equal(grade(checked, [{ ...matching, status: undefined }]).results[0].pass, null);
});

const by = (runId, name) => {
  const row = grade(suite, traces).results.find((item) => item.run_id === runId && item.assert === name);
  assert.ok(row, `${runId} ${name}`);
  return row;
};

test('trace, memory, and answer assertions score each turn', () => {
  assert.equal(by('train-0', 'tool_called').pass, true);
  assert.equal(by('train-0', 'committed').pass, true);
  assert.equal(by('train-0', 'committed').metric, 'save_recall');
  assert.equal(by('train-0', 'contains_any').pass, true);
  assert.equal(by('probe-0', 'tool_called_before').pass, true);
  assert.equal(by('probe-0', 'capability_loaded').pass, true);
  assert.equal(by('probe-1', 'capability_loaded').pass, true);
  assert.equal(by('probe-2', 'capability_loaded').pass, false);
  assert.equal(by('probe-0', 'tool_not_called').pass, true);
  assert.equal(by('probe-0', 'not_committed').pass, true);
  assert.equal(by('probe-0', 'metrics').pass, true);
  assert.equal(by('probe-0', 'not_contains').pass, true);
  assert.equal(by('probe-1', 'metrics').pass, true);
  assert.equal(by('probe-2', 'tool_called_before').pass, false);
  assert.equal(by('probe-2', 'max_tool_errors').pass, false);
  assert.equal(by('probe-2', 'metrics').pass, false);
  assert.equal(by('probe-2', 'not_contains').pass, false);
  assert.equal(by('probe-2', 'no_repeated_failed_call').pass, false);
  assert.equal(by('probe-0', 'no_repeated_failed_call').pass, true);
  assert.equal(by('train-0', 'no_repeated_failed_call').pass, true);
});

test('metric tolerance matches the finance scorer and prose can end in JSON', () => {
  const turn = structuredClone(suite);
  turn.scenarios[0].sessions[1].turns[0].expect.answer = [{ assert: 'metrics', values: { fair_value_low: 41.2 }, metric: 'task_success' }];
  const within = structuredClone(traces[1]);
  within.answer = `Range follows.\n{"metrics":{"fair_value_low":41.2004}}`;
  const outside = structuredClone(traces[1]);
  outside.run_id = 'outside';
  outside.answer = `{"metrics":{"fair_value_low":41.201}}`;
  const graded = grade(turn, [within, outside]);
  const metrics = (runId) => graded.results.find((row) => row.run_id === runId && row.assert === 'metrics');
  assert.equal(metrics('probe-0').pass, true);
  assert.equal(metrics('outside').pass, false);
});

test('metric scales accept the same quantity in another unit and nothing else', () => {
  const turn = structuredClone(suite);
  turn.scenarios[0].sessions[1].turns[0].expect.answer = [{ assert: 'metrics', values: { capex: 19.8 }, scales: [1e6], metric: 'carry_forward' }];
  const answer = (runId, text) => ({ ...structuredClone(traces[1]), run_id: runId, answer: text });
  const graded = grade(turn, [
    answer('millions', '{"metrics":{"capex":19.8}}'),
    answer('units', '{"metrics":{"capex":19800000}}'),
    answer('thousands', '{"metrics":{"capex":19800}}'),
    answer('draft', '{"metrics":{"capex":18000000}}'),
  ]);
  const pass = (runId) => graded.results.find((row) => row.run_id === runId && row.assert === 'metrics').pass;
  assert.deepEqual(['millions', 'units', 'thousands', 'draft'].map(pass), [true, true, false, false]);
});

test('invalid calls are host argument or availability failures and Pi pre-host rejections', () => {
  const error = (error_code, error_message) => ({ outcome: 'error', error_code, error_message });
  assert.equal(isInvalidToolCall(error('malformed_request', 'arguments did not match')), true);
  assert.equal(isInvalidToolCall(error('method_denied', 'tool not available')), true);
  assert.equal(isInvalidToolCall(error('tool_not_found', 'Tool "missing_tool" not found')), true);
  assert.equal(isInvalidToolCall(error('tool_validation_failed', 'Validation failed for tool "run_valuation": ticker')), true);
  assert.equal(isInvalidToolCall(error('Malformed-Request', 'arguments did not match')), true);
  assert.equal(isInvalidToolCall({ outcome: 'error', error_message: 'Tool dart not found' }), true);
  assert.equal(isInvalidToolCall({ outcome: 'error', error_message: 'Tool "dart_filing_search" not found' }), true);
  assert.equal(isInvalidToolCall({ outcome: 'error', error_message: 'Validation failed for tool "dart_filing_search": missing corp_code' }), true);
  for (const code of ['execution_failed', 'io_failed', 'transaction_limit', 'memory_disabled', 'proposal_disabled', 'frame_too_large', 'interrupted', 'unknown_tool', 'schema_rejection', 'authentication_failed', 'rate_limit']) {
    assert.equal(isInvalidToolCall(error(code, 'not an invalid call')), false, code);
  }
  assert.equal(isInvalidToolCall({ outcome: 'error', error_message: 'validation failed for tool "x": missing' }), false);
  assert.equal(isInvalidToolCall({ outcome: 'error', error_code: 'rpc', error_message: 'tool not loaded: valuation' }), false);
  assert.equal(isInvalidToolCall({ outcome: 'ok', error_code: 'malformed_request', error_message: 'Validation failed for tool "x": ticker' }), false);
  assert.equal(isInvalidToolCall(error('malformed_request_extra', 'arguments did not match')), false);
  const calls = [
    { name: 'run_valuation', args: { b: 1, a: 2 }, outcome: 'error' },
    { name: 'run_valuation', args: { a: 2, b: 1 }, outcome: 'error' },
    { name: 'run_valuation', args: { a: 3 }, outcome: 'error' },
    { name: 'run_valuation', args: { a: 2, b: 1 }, outcome: 'ok' },
  ];
  assert.deepEqual(repeatedFailureIndexes(calls), [1]);
  assert.deepEqual(repeatedFailureIndexes([
    { name: 'note_save', args: { _truncated: true }, outcome: 'error' },
    { name: 'note_save', args: { _truncated: true }, outcome: 'error' },
  ]), []);
  const unavailable = [{ name: 'note_save', args: { _truncated: true }, outcome: 'error' },
    { name: 'note_save', args: { truncated: true, original_bytes: 3000 }, outcome: 'error' }];
  assert.deepEqual(repeatedFailureIndexes(unavailable), []);
  assert.equal(toolValidity([{ tool_calls: unavailable }]).repeated_failures_unknown, 2);
  const unknown = grade(suite, [{ ...traces[0], tool_calls: unavailable }]);
  assert.equal(unknown.results.find((row) => row.assert === 'no_repeated_failed_call').pass, null);
  const nestedPlaceholders = [
    { body: { _truncated: true } }, { nested: ['...[truncated]'] },
    { token: '[REDACTED]' }, { text: '[redacted]' },
  ].flatMap((args) => [0, 1].map(() => ({ name: 'note_save', args, outcome: 'error' })));
  assert.deepEqual(repeatedFailureIndexes(nestedPlaceholders), []);
  assert.equal(toolValidity([{ tool_calls: nestedPlaceholders }]).repeated_failures_unknown, 8);
  const validity = toolValidity(traces);
  assert.equal(validity.calls, 12);
  assert.equal(validity.invalid, 3);
  assert.equal(validity.rate, 0.25);
  assert.equal(validity.repeated_failures, 1);
  assert.equal(toolValidity([{ tool_calls: [] }]).rate, null);
});

test('capability_loaded counts only capability_load with an id argument', () => {
  const mini = {
    schema: 'loupist-agent-suite/1', suite: 'learning', version: 1,
    scenarios: [{
      id: 'L-004',
      sessions: [{
        id: 'probe', role: 'probe', memory: 'off',
        turns: [{ user: 'Load valuation.', expect: { trace: [{ assert: 'capability_loaded', id: 'valuation', metric: 'tool_validity' }] } }],
      }],
    }],
  };
  const row = (runId, tool_calls) => ({
    schema: 'loupist-agent-trace/1', run_id: runId, suite: 'learning', scenario_id: 'L-004', session_id: 'probe', user: 'Load valuation.', session_role: 'probe', memory_mode: 'off',
    turn_index: 0, trial: 0, status: 'completed', answer: '', tool_calls,
  });
  const graded = grade(mini, [
    row('load', [{ name: 'capability_load', args: { id: 'valuation' }, outcome: 'ok' }]),
    row('skill', [{ name: 'load_skill', args: { id: 'valuation' }, outcome: 'ok' }]),
    row('name-arg', [{ name: 'capability_load', args: { name: 'valuation' }, outcome: 'ok' }]),
    row('component', [{ name: 'run_valuation', component: 'valuation', outcome: 'ok' }]),
    row('call-name', [{ name: 'valuation', args: {}, outcome: 'ok' }]),
  ]);
  const pass = (runId) => graded.results.find((item) => item.run_id === runId).pass;
  assert.equal(pass('load'), true);
  assert.equal(pass('skill'), false);
  assert.equal(pass('name-arg'), false);
  assert.equal(pass('component'), false);
  assert.equal(pass('call-name'), false);
});

test('a memory assertion may accept any of several kinds', () => {
  const mini = {
    schema: 'loupist-agent-suite/1', suite: 'learning', version: 1,
    scenarios: [{
      id: 'L-014',
      sessions: [{
        id: 'train', role: 'train', memory: 'remember',
        turns: [{ user: 'A pasted notice.', expect: { memory: [{ assert: 'committed', kind: ['source', 'note'], metric: 'save_recall' }] } }],
      }],
    }],
  };
  const row = (runId, memory_commits) => ({
    schema: 'loupist-agent-trace/1', run_id: runId, suite: 'learning', scenario_id: 'L-014', session_id: 'train', user: 'A pasted notice.', session_role: 'train', memory_mode: 'remember',
    turn_index: 0, trial: 0, status: 'completed', answer: '', tool_calls: [], memory_commits,
  });
  const graded = grade(mini, [
    row('note', [{ kind: 'note', title: 'Backlog' }]),
    row('source', [{ kind: 'source', title: 'Backlog' }]),
    row('lens', [{ kind: 'lens', title: 'Backlog' }]),
    row('none', []),
  ]);
  const pass = (runId) => graded.results.find((item) => item.run_id === runId).pass;
  assert.equal(pass('note'), true);
  assert.equal(pass('source'), true);
  assert.equal(pass('lens'), false);
  assert.equal(pass('none'), false);
  assert.throws(() => grade({ ...mini, scenarios: [{ ...mini.scenarios[0], sessions: [{ ...mini.scenarios[0].sessions[0], turns: [{ user: 'x', expect: { memory: [{ assert: 'committed', kind: ['memo'], metric: 'save_recall' }] } }] }] }] }, [row('bad', [])]), /Unknown memory kind/);
});

test('Lens assertions read the stored pages and reads, not the reply', () => {
  const memory = [
    { assert: 'lens_pages', kind: 'subject', title_matches: 'halvren', count: 1, section_matches: '3\\.6', section_not_matching: '4\\.2\\s*B(?!.*(?:superseded|cut|이전))', basis: 'span', metric: 'lens_update' },
    { assert: 'lens_pages', kind: 'method', title_matches: '.', max: 0, metric: 'save_precision' },
    { assert: 'lens_unchanged', metric: 'save_precision' },
    { assert: 'lens_read', title_matches: 'halvren', metric: 'retrieval' },
    { assert: 'lens_not_read', title_matches: 'torvik', metric: 'negative_transfer' },
  ];
  const mini = {
    schema: 'loupist-agent-suite/1', suite: 'lens', version: 1,
    scenarios: [{ id: 'LC-1', sessions: [{ id: 'train', role: 'train', memory: 'remember', turns: [{ user: 'Research.', expect: { memory } }] }] }],
  };
  const page = (title, kind, markdown, basis, ops = []) => ({ record_id: title, kind, title, archived: false, summary: '',
    sections: [{ key: 'facts', heading: 'Facts', markdown, basis }], ops_this_turn: ops });
  const row = (runId, pages, memory_reads = []) => ({
    schema: 'loupist-agent-trace/1', run_id: runId, suite: 'lens', scenario_id: 'LC-1', session_id: 'train', user: 'Research.', session_role: 'train', memory_mode: 'remember',
    turn_index: 0, trial: 0, status: 'completed', answer: 'Saved.', tool_calls: [], memory_commits: [], memory_reads, lens: pages === null ? undefined : { pages },
  });
  const results = (runId, pages, reads) => grade(mini, [row(runId, pages, reads)]).results.map((item) => item.pass);
  const read = (title) => ({ kind: 'lens', record_id: title, title, access: 'exact_read' });
  assert.deepEqual(results('good', [page('Halvren Optics', 'subject', 'Guidance 3.6B', ['span'])], [read('Halvren Optics')]),
    [true, true, true, true, true]);
  assert.deepEqual(results('stale', [page('Halvren Optics', 'subject', 'Guidance 4.2B; now 3.6B', ['span']), page('Halvren Optics (KR)', 'subject', 'x', [])]),
    [false, true, true, false, true]);
  assert.deepEqual(results('wrote', [page('Halvren Optics', 'subject', '3.6B', ['said'], [{ op: 'put', outcome: 'added' }]),
    page('Margin ramp check', 'method', 'steps', [])], [read('Torvik Foods'), { ...read('Halvren Optics'), access: 'search' }]),
  [false, false, false, false, false]);
  assert.deepEqual(results('unknown', null), [null, null, null, false, true]);
  assert.throws(() => grade({ ...mini, scenarios: [{ ...mini.scenarios[0], sessions: [{ ...mini.scenarios[0].sessions[0],
    turns: [{ user: 'x', expect: { memory: [{ assert: 'lens_pages', kind: 'wiki', title_matches: 'a' }] } }] }] }] }, []), /Unknown Lens kind/);
});

test('prediction assertion names each missing or invalid contract field', () => {
  const valid = { claim: 'Fictional price rises', metric: 'price', direction: 'above', threshold: 10, horizon: '2026-12-31', probability: 0.6 };
  const mini = {
    schema: 'loupist-agent-suite/1', suite: 'learning', version: 1,
    scenarios: [{
      id: 'P-1',
      sessions: [{
        id: 's', role: 'probe', memory: 'off',
        turns: [{ user: 'Predict.', expect: { answer: [{ assert: 'prediction', metric: 'prediction_structure' }] } }],
      }],
    }],
  };
  const row = (runId, answer) => ({
    schema: 'loupist-agent-trace/1', run_id: runId, suite: 'learning', scenario_id: 'P-1', session_id: 's', user: 'Predict.', session_role: 'probe', memory_mode: 'off',
    turn_index: 0, trial: 0, status: 'completed', answer, tool_calls: [],
  });
  const graded = grade(mini, [
    row('ok', `Note.\n${JSON.stringify({ prediction: valid })}`),
    row('leap', JSON.stringify({ prediction: { ...valid, horizon: '2024-02-29', probability: 0 } })),
    row('missing', 'No block here.'),
    row('bad', JSON.stringify({ prediction: { ...valid, claim: '  ', direction: 'up', horizon: '2026-02-29', probability: 1.2 } })),
  ]);
  const found = (runId) => graded.results.find((item) => item.run_id === runId);
  assert.equal(found('ok').pass, true);
  assert.equal(found('ok').detail, 'prediction block present');
  assert.equal(found('leap').pass, true);
  assert.equal(found('missing').pass, false);
  assert.equal(found('missing').detail, 'missing prediction');
  assert.equal(found('bad').pass, false);
  assert.equal(found('bad').detail, 'invalid claim, direction, horizon, probability');
});

test('unavailable execution and tool outcomes stay unknown while measured failures still fail', () => {
  const mini = structuredClone(suite);
  mini.scenarios[0].sessions[0].turns[0].expect = { trace: [
    { assert: 'max_tool_errors', n: 0, metric: 'tool_validity' },
    { assert: 'no_repeated_failed_call', metric: 'tool_validity' },
  ] };
  const pending = { ...traces[0], tool_calls: [{ name: 'note_save', args: {}, outcome: null }] };
  assert.deepEqual(grade(mini, [pending]).results.map((row) => row.pass), [null, null]);
  assert.deepEqual(grade(mini, [{ ...pending, status: 'failed' }]).results.map((row) => row.pass), [false, false]);
  assert.deepEqual(grade(mini, [{ ...pending, status: undefined, tool_calls: [] }]).results.map((row) => row.pass), [null, null]);
  const measured = { ...pending, tool_calls: [...pending.tool_calls, { name: 'note_save', args: {}, outcome: 'error' }] };
  assert.equal(grade(mini, [measured]).results[0].pass, false);
  assert.equal(grade(mini, [{ ...pending, suite: 'another-suite' }]).unmatched.length, 1);
});

test('missing or malformed call and commit lists cannot prove that nothing happened', () => {
  const mini = structuredClone(suite);
  mini.scenarios[0].sessions[0].turns[0].expect = {
    trace: [{ assert: 'tool_not_called', tool: 'web_search', metric: 'connection_honesty' }],
    memory: [{ assert: 'not_committed', kind: 'journal', metric: 'save_recall' }],
  };
  const trace = { ...traces[0], tool_calls: [], memory_commits: [] };
  assert.deepEqual(grade(mini, [trace]).results.map((row) => row.pass), [true, true]);
  for (const evidence of [undefined, null, {}, [null], [1], [{}]]) {
    const tools = grade(mini, [{ ...trace, tool_calls: evidence }]);
    assert.deepEqual(tools.results.map((row) => row.pass), [null, true]);
    assert.equal(tools.tool_validity.unknown_call_lists, 1);
    const memory = grade(mini, [{ ...trace, memory_commits: evidence }]);
    assert.deepEqual(memory.results.map((row) => row.pass), [true, null]);
  }
  assert.equal(grade(mini, [{ ...trace, memory_commits: [{ kind: 'invalid' }] }]).results[1].pass, null);
  const partial = toolValidity([{ tool_calls: [
    { name: 'missing', args: {}, outcome: 'error', error_code: 'tool_not_found' }, null,
  ] }]);
  assert.equal(partial.calls, 2);
  assert.equal(partial.invalid, 1);
  assert.equal(partial.rate, null);
});

function expecting(expect) {
  const copy = structuredClone(suite);
  copy.scenarios[0].sessions[1].turns[0].expect = expect;
  return copy;
}

const LINK = '#evidence-ev_' + 'a'.repeat(32);
const CALC = '#evidence-ev_' + 'c'.repeat(32);

test('numbers_linked needs every material number linked and no differing or missing check', () => {
  const checked = expecting({ answer: [{ assert: 'numbers_linked', checked: ['8.4'], metric: 'connection_honesty' }] });
  const answerWith = (extra = '') => `Margin was [8.42%](${CALC}) on revenue of [₩452.3bn](${LINK}) for FY2025, filed 2026-03-18 [1](${LINK}).${extra}\n{"metrics":{"operating_margin_pct":8.42}}`;
  const linked = answerWith();
  const checks = [
    { link_index: 0, text: '8.42%', evidence_id: 'ev_c', kind: 'calculated', status: 'matched' },
    { link_index: 1, text: '₩452.3bn', evidence_id: 'ev_a', kind: 'data', status: 'rounded' },
  ];
  const result = (answer, number_checks) => {
    const trace = { ...traces[1], answer };
    if (number_checks !== undefined) trace.number_checks = number_checks;
    return grade(checked, [trace]).results[0];
  };
  assert.equal(result(linked, checks).pass, true);
  // An unlinked amount, percent, multiple or grouped figure fails; years, dates and small counts do not.
  for (const prose of ['revenue of ₩452.3bn', 'a margin of 8.4%', 'P/E of 31.2x', '34,560,000 shares', '4,523억 원']) {
    const row = result(answerWith(`\n\nAlso ${prose}.`), checks);
    assert.equal(row.pass, false, prose);
    assert.match(row.detail, /unlinked/u);
  }
  assert.equal(result(answerWith('\n\nThree risks stand out for 2026 in Q3.'), checks).pass, true);
  // Rust's checks decide: a differing calculation or a number not in its source fails.
  for (const status of ['differs', 'not_found']) {
    const row = result(linked, [{ ...checks[0], status }, checks[1]]);
    assert.equal(row.pass, false, status);
    assert.match(row.detail, new RegExp(status, 'u'));
  }
  // A named number needs a matched or rounded check of its own.
  assert.equal(result(linked, [checks[1]]).pass, false);
  // A record without checks cannot prove anything.
  assert.equal(result(linked, undefined).pass, null);
  assert.equal(result(linked, [{ text: '8.42%', status: 'guessed' }]).pass, null);
  assert.throws(() => validateSuite(expecting({ answer: [{ assert: 'numbers_linked', checked: [''], metric: 'connection_honesty' }] })), /numbers_linked/u);
});

test('as-of assertions read the report Rust wrote and stay unknown without one', () => {
  const checked = expecting({ trace: [
    { assert: 'as_of_left_out', kind: 'sources', min: 1, metric: 'negative_transfer' },
    { assert: 'as_of_refused', tool: 'kis_read', metric: 'connection_honesty' },
  ] });
  const passes = (as_of_report) => {
    const trace = { ...traces[1] };
    if (as_of_report !== undefined) trace.as_of_report = as_of_report;
    return grade(checked, [trace]).results.map((row) => row.pass);
  };
  const report = { cutoff: '2026-03-10', refused: [{ tool: 'kis_read' }], empty: [],
    left_out: { facts: 3, sources: 1, notes: 0, methods: 0 } };
  assert.deepEqual(passes(report), [true, true]);
  assert.deepEqual(passes({ ...report, refused: [], left_out: { ...report.left_out, sources: 0 } }), [false, false]);
  assert.deepEqual(passes(null), [false, false]);
  assert.deepEqual(passes(undefined), [null, null]);
  for (const assertion of [{ assert: 'as_of_left_out', kind: 'claims' }, { assert: 'as_of_left_out', kind: 'facts', min: 0 }, { assert: 'as_of_refused' }]) {
    assert.throws(() => validateSuite(expecting({ trace: [{ ...assertion, metric: 'task_success' }] })), /as_of/u);
  }
});

test('calculate_inputs_cited needs a calculate call naming each input source', () => {
  const checked = expecting({ trace: [{ assert: 'calculate_inputs_cited', operation: 'margin', inputs: ['profit', 'revenue'], metric: 'connection_honesty' }] });
  const call = (args, outcome = 'ok') => ({ call_id: 'calc', name: 'calculate', args, outcome });
  const margin = (input_sources) => ({ operation: 'margin', inputs: { profit: 38.1, revenue: 452.3 }, input_sources });
  for (const [calls, pass] of [
    [[call(margin({ profit: LINK, revenue: LINK }))], true],
    [[call(margin({ profit: LINK }))], false],
    [[call({ ...margin({ profit: LINK, revenue: LINK }), operation: 'ratio' })], false],
    [[call(margin({ profit: LINK, revenue: LINK }), 'error')], false],
    [[call({ operation: 'margin', input_sources: '[REDACTED]' })], null],
    [[], false],
  ]) {
    assert.equal(grade(checked, [{ ...traces[1], tool_calls: calls }]).results[0].pass, pass, JSON.stringify(calls));
  }
});

test('tool_called with args needs a call that did not fail carrying those argument fields', () => {
  const checked = expecting({ trace: [{ assert: 'tool_called', tool: 'calculate', args: { operation: 'goal_grid' }, metric: 'task_success' }] });
  const call = (args, outcome = 'ok') => ({ call_id: 'calc', name: 'calculate', args, outcome });
  const grid = { operation: 'goal_grid', inputs: { years: 15, contribution: 300 }, return_pct: [0, 4, 7] };
  for (const [calls, pass] of [
    [[call(grid)], true],
    [[call({ ...grid, operation: 'cagr' })], false],
    [[call(grid, 'error')], false],
    [[call({ ...grid, operation: 'cagr' }), call(grid)], true],
    [[call('[REDACTED]')], null],
    [[], false],
  ]) {
    assert.equal(grade(checked, [{ ...traces[1], tool_calls: calls }]).results[0].pass, pass, JSON.stringify(calls));
  }
  // Nested fields match as a subset; arrays match item by item.
  const nested = expecting({ trace: [{ assert: 'tool_called', tool: 'kis_read', args: { params: { fid_input_iscd: '990417' } }, metric: 'connection_honesty' }] });
  const read = (params) => [{ name: 'kis_read', args: { operation_id: 'domestic_stock.daily_short_sale', params }, outcome: 'ok' }];
  assert.equal(grade(nested, [{ ...traces[1], tool_calls: read({ fid_cond_mrkt_div_code: 'J', fid_input_iscd: '990417' }) }]).results[0].pass, true);
  assert.equal(grade(nested, [{ ...traces[1], tool_calls: read({ fid_input_iscd: '999999' }) }]).results[0].pass, false);
  for (const args of [{}, [], 'goal_grid']) {
    assert.throws(() => validateSuite(expecting({ trace: [{ assert: 'tool_called', tool: 'calculate', args, metric: 'task_success' }] })), /tool_called/u);
  }
});

test('sensitivity_card reads the cards Rust built and stays unknown without them', () => {
  const checked = (assertion) => expecting({ trace: [{ assert: 'sensitivity_card', metric: 'task_success', ...assertion }] });
  const card = (operation, reference) => ({ operation, row_values: [8, 10], column_values: [2], values: [[4.1], [7.9]], unit: '%',
    forecast_years: 5, ...(reference ? { reference: { value_pct: 8, label: 'FY2020–FY2025' } } : {}), link_index: 0, evidence_id: 'ev', subject: 'Hanul Precision' });
  const pass = (assertion, cards) => {
    const trace = { ...traces[1] };
    if (cards !== undefined) trace.sensitivity_cards = cards;
    return grade(checked(assertion), [trace]).results[0].pass;
  };
  for (const [assertion, cards, expected] of [
    [{ operation: 'reverse_dcf', reference: true }, [card('reverse_dcf', true)], true],
    [{ operation: 'reverse_dcf', reference: true }, [card('reverse_dcf', false)], false],
    [{ operation: 'reverse_dcf' }, [card('reverse_dcf', false)], true],
    [{ operation: 'reverse_dcf' }, [card('dcf_grid', false)], false],
    [{}, [card('dcf_grid', false)], true],
    [{}, [], false],
    [{}, undefined, null],
    [{}, [{ subject: 'no operation' }], null],
  ]) {
    assert.equal(pass(assertion, cards), expected, JSON.stringify({ assertion, cards }));
  }
  for (const invalid of [{ operation: 'grid' }, { reference: 'yes' }]) {
    assert.throws(() => validateSuite(checked(invalid)), /sensitivity_card/u);
  }
});

test('an absolute metric tolerance accepts the rounding a scenario allows and nothing more', () => {
  const checked = expecting({ answer: [{ assert: 'metrics', values: { operating_margin_pct: 8.42 }, tolerance: 0.01, metric: 'task_success' }] });
  const pass = (value) => grade(checked, [{ ...traces[1], answer: `{"metrics":{"operating_margin_pct":${value}}}` }]).results[0].pass;
  assert.deepEqual([8.4236, 8.42, 8.41, 8.4, 8.45].map(pass), [true, true, true, false, false]);
  checked.scenarios[0].sessions[1].turns[0].expect.answer[0].tolerance = -1;
  assert.throws(() => validateSuite(checked), /tolerance/u);
});

test('grid_cells_linked needs a staged grid whose checked cells all match, with the asked shape and empty reasons', () => {
  const value = (text) => ({ value: text, citation_url: LINK });
  const args = (cells, rows = 2) => ({ mode: 'create', layout: 'assets', title: 'Margins',
    rows: Array.from({ length: rows }, (_, index) => ({ symbol: `99000${index}`, market: 'KRX', label: `Fictional ${index}` })),
    columns: [{ id: 'revenue', label: 'Revenue', unit: 'amount' }, { id: 'margin', label: 'Operating margin', unit: 'percent' }],
    cells });
  const result = (statuses) => ({ status: 'staged', layout: 'assets',
    cell_checks: statuses.map((status, index) => ({ cell: `cells[${Math.floor(index / 2)}][${index % 2}]`, value: 'x', status })) });
  const call = (callArgs, callResult, extra = {}) => ({ name: 'grid_publish', args: callArgs, outcome: 'ok', result: callResult, ...extra });
  const full = [[value('₩452.3bn'), value('8.42%')], [value('₩128.4bn'), value('5.92%')]];
  const partial = [[value('₩452.3bn'), value('8.42%')], [{ empty: 'unavailable', note: 'No FY2025 statements' }, { empty: 'unavailable' }]];
  const checked = (assertion) => expecting({ trace: [{ assert: 'grid_cells_linked', metric: 'connection_honesty', ...assertion }] });
  const pass = (assertion, calls) => grade(checked(assertion), [{ ...traces[1], tool_calls: calls }]).results[0].pass;
  for (const [assertion, calls, expected] of [
    [{ rows: 2, columns: 2 }, [call(args(full), result(['matched', 'rounded', 'rounded', 'matched']))], true],
    [{ rows: 2, columns: 2 }, [call(args(full), result(['matched', 'differs', 'rounded', 'matched']))], false],
    [{ rows: 2, columns: 2 }, [call(args(full), result(['matched', 'not_found', 'rounded', 'matched']))], false],
    [{ rows: 3 }, [call(args(full), result(['matched', 'matched', 'matched', 'matched']))], false],
    [{}, [call(args(full), result([]))], false],
    [{ empty: ['unavailable'] }, [call(args(partial), result(['rounded', 'matched']))], true],
    [{ empty: ['unavailable'] }, [call(args(full), result(['matched', 'matched', 'matched', 'matched']))], false],
    // A failed call proves nothing; a later corrected call can pass.
    [{}, [call(args(full), undefined, { outcome: 'error' }), call(args(full), result(['matched', 'matched', 'matched', 'matched']))], true],
    [{}, [call(args(full), { status: 'staged', truncated: true })], null],
    [{}, [call(args(full), result(['matched']), { result_redacted: true })], null],
    [{}, [call({ ...args(full), cells: '[REDACTED]' }, result(['matched']))], null],
    [{}, [call({ ...args(full), layout: 'conflict' }, result(['matched']))], false],
    [{}, [], false],
  ]) {
    assert.equal(pass(assertion, calls), expected, JSON.stringify({ assertion, calls }));
  }
  const conflict = { mode: 'create', layout: 'conflict', title: 'Revenue', measure: { label: 'Revenue', entity_label: 'Solsup Foods' },
    reports: [{ citation_url: LINK, says: '₩452.3bn' }, { citation_url: CALC, says: '₩455.0bn' }], reason: 'preliminary_vs_final',
    explanation: 'The news used the preliminary figure.', used: 0 };
  const reports = (statuses) => ({ status: 'staged', layout: 'conflict', cell_checks: statuses.map((status, index) => ({ cell: `reports[${index}]`, value: 'x', status })) });
  assert.equal(pass({ layout: 'conflict' }, [call(conflict, reports(['rounded', 'matched']))]), true);
  assert.equal(pass({ layout: 'conflict' }, [call(conflict, reports(['rounded', 'not_found']))]), false);
  assert.equal(pass({ layout: 'conflict' }, [call(args(full), result(['matched', 'matched', 'matched', 'matched']))]), false);
  for (const invalid of [{ layout: 'table' }, { rows: 0 }, { columns: 1.5 }, { empty: ['zero'] }, { layout: 'conflict', rows: 2 }, { layout: 'conflict', empty: ['unavailable'] }]) {
    assert.throws(() => pass(invalid, []), /grid_cells_linked/u, JSON.stringify(invalid));
  }
});

test('lens-pages renders each run\'s final pages and the ops each turn logged', async () => {
  const { renderLensPages } = await import('./lens-review.mjs');
  const page = (markdown, ops) => ({ kind: 'subject', title: 'Halvren Optics', summary: 'Lens maker.', archived: false,
    sections: [{ key: 'facts', heading: 'Facts', markdown, basis: ['span'], origin: 'loupist' }], ops_this_turn: ops });
  const text = renderLensPages([
    { scenario_id: 'LL-002', trial: 0, session_id: 'first', turn_index: 0, lens: { pages: [page('Guidance 4,200', [{ op: 'put', key: 'facts', outcome: 'added' }])] } },
    { scenario_id: 'LL-002', trial: 0, session_id: 'second', turn_index: 0, lens: { pages: [page('Guidance 3,600 (cut from 4,200)', [{ op: 'put', key: 'facts', outcome: 'replaced' }])] } },
    { scenario_id: 'LL-007', trial: 0, session_id: 'readonly', turn_index: 0, lens: { pages: [] } },
  ]);
  assert.match(text, /## LL-002 trial 0[\s\S]*first#0 put facts on "Halvren Optics": added[\s\S]*second#0 put facts on "Halvren Optics": replaced/u);
  assert.match(text, /Guidance 3,600 \(cut from 4,200\)/u);
  assert.doesNotMatch(text, /Guidance 4,200\n/u);
  assert.match(text, /## LL-007 trial 0[\s\S]*No Lens op was logged\.[\s\S]*No Lens pages\./u);
});
