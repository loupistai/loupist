import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { grade } from './grade.mjs';
import { buildJudgeInput, judgeItems, SAVE_PRECISION_CRITERION } from './judge.mjs';

// D4 (rc1): material the user pastes into a Chat is kept as a user-provided Source
// (`pasted_capture`, then `source_save`). These are the traces the six learning
// scenarios expect once the agent does that, in the shape the eval runner records:
// a `source` commit whose summary names `kind: user_provided`, and for L-033 a
// subject page whose fact cites that Source's span.
const suite = JSON.parse(await readFile(new URL('./suites/learning-v1.json', import.meta.url), 'utf8'));

/** Per train turn: the passage the agent captures, the link the user typed and the title it saves. */
const PASTES = {
  'L-014/train-1': { body: '솔숲식품은 2025년 3월 12일 현재 수주잔고 수치가 184.6이고 단위는 십억 원이라고 알렸다.',
    url: 'https://fictional.example/solsup/backlog-2025', title: '솔숲식품 수주잔고 공시 (2025-03-12)' },
  'L-017/train-1': { body: 'Sable Orchard packed 318.2 thousand boxes in the FY2025 harvest.',
    url: 'https://fictional.example/sable/harvest-2025', title: 'Sable Orchard FY2025 harvest note' },
  'L-023/train-1': { body: 'Byeolmuri Semiconductor, ticker BYMR, KRX 990308, Korean name 별무리반도체, reported FY2025 capex of 73.4 billion KRW.',
    url: 'https://fictional.example/byeolmuri/alias-capex', title: 'Byeolmuri Semiconductor FY2025 capex' },
  'L-024/train-1': { body: '노을제지, 영문명 Noeul Paper, 티커 NOUL, 종목코드 990184. FY2025 영업이익 수치는 47.3이고 단위는 십억 원이다.',
    url: 'https://fictional.example/noeul/op-2025', title: '노을제지 FY2025 영업이익' },
  'L-032/train-1': { body: '솔숲식품은 2025-03-12 현재 수주잔고 수치가 184.6이고 단위는 십억 원이라고 알렸다.',
    url: 'https://fictional.example/solsup/backlog-2025', title: 'Solsup backlog' },
  'L-033/train-1': { body: 'Marrow Glass original FY2024 gross margin was 40.0 percent.',
    url: 'https://fictional.example/marrow/fy2024', title: 'Marrow margin original' },
  'L-033/train-2': { body: 'Marrow Glass restated FY2024 gross margin to 33.0 percent.',
    url: 'https://fictional.example/marrow/fy2024-restatement', title: 'Marrow margin restatement' },
};

const turnOf = (scenarioId, sessionId) => {
  const scenario = suite.scenarios.find((item) => item.id === scenarioId);
  const session = scenario.sessions.find((item) => item.id === sessionId);
  return { scenario, session, turn: session.turns[0] };
};

function trace(scenarioId, sessionId, fields) {
  const { session, turn } = turnOf(scenarioId, sessionId);
  return { schema: 'loupist-agent-trace/1', run_id: 'pasted-source', suite: suite.suite, scenario_id: scenarioId, session_id: sessionId,
    session_role: session.role, memory_mode: session.memory, turn_index: 0, trial: 0, status: 'completed', user: turn.user,
    answer: 'Saved.', tool_calls: [], memory_commits: [], memory_reads: [], ...fields };
}

/** What the runner records after `pasted_capture` and `source_save` commit a user-provided Source. */
function pastedSourceTurn(key) {
  const [scenarioId, sessionId] = key.split('/');
  const { body, url, title } = PASTES[key];
  const source = { kind: 'source', record_id: `src_${scenarioId}_${sessionId}`, title, operation: 'create',
    change_summary: 'Retained the exact delivered material.',
    summary: `title: ${title}\norigin: ${url}\nkind: user_provided\ncoverage: ["included_excerpt"]\ntext: ${body}` };
  const tool_calls = [
    { name: 'pasted_capture', args: { text: body, url }, outcome: 'ok', result: { content: [{ type: 'text', text: JSON.stringify({ status: 'captured', source_class: 'user_provided', content: body, result_ref: 'result:paste', citation_url: `#evidence-ev_${'a'.repeat(32)}` }) }] } },
    { name: 'source_save', args: { result_ref: 'result:paste', title, coverage: 'included_excerpt' }, outcome: 'ok', result: { content: [{ type: 'text', text: '{"status":"staged","saveRef":"save_paste","durable":false}' }] } },
  ];
  if (scenarioId !== 'L-033') return trace(scenarioId, sessionId, { tool_calls, memory_commits: [source] });
  const margin = sessionId === 'train-1' ? '40.0%' : '33.0%, restated from 40.0%';
  const lens = { kind: 'lens', record_id: 'rec_marrow_view', title: 'Marrow margin view', operation: sessionId === 'train-1' ? 'create' : 'revise', summary: 'title: Marrow margin view' };
  return trace(scenarioId, sessionId, {
    tool_calls: [...tool_calls, { name: 'lens_file', args: {}, outcome: 'ok' }],
    memory_commits: [source, lens],
    lens: { pages: [{ record_id: 'rec_marrow_view', kind: 'subject', title: 'Marrow margin view', archived: false, summary: null,
      sections: [{ key: 'facts', heading: 'FY2024 gross margin', markdown: `Marrow Glass FY2024 gross margin: ${margin}.`, basis: ['span'] }],
      ops_this_turn: [{ op: 'put', outcome: sessionId === 'train-1' ? 'added' : 'replaced' }] }] },
  });
}

/** Only the Memory commit checks of each turn, so the six scenarios are judged on what D4 changed. */
function memoryRows(traces) {
  const { results, unmatched } = grade(suite, traces);
  assert.deepEqual(unmatched, []);
  return results.filter((row) => row.assert === 'committed');
}

test('every pasted body is verbatim in its user turn, as pasted_capture requires', () => {
  for (const [key, { body, url }] of Object.entries(PASTES)) {
    const [scenarioId, sessionId] = key.split('/');
    const { turn } = turnOf(scenarioId, sessionId);
    assert.ok(turn.user.includes(body), key);
    assert.ok(turn.user.includes(url), key);
  }
});

test('a user-provided Source counts as the source commit L-014, L-017, L-023, L-024, L-032 and L-033 expect', () => {
  const rows = memoryRows(Object.keys(PASTES).map(pastedSourceTurn));
  const scenarios = new Set(rows.map((row) => row.scenario_id));
  assert.deepEqual([...scenarios].sort(), ['L-014', 'L-017', 'L-023', 'L-024', 'L-032', 'L-033']);
  for (const row of rows) assert.equal(row.pass, true, `${row.scenario_id} ${row.session_id} ${row.metric}: ${row.detail}`);
  // L-033 train turns check four commits each: the titled Source and the Lens, for recall and precision.
  assert.equal(rows.filter((row) => row.scenario_id === 'L-033').length, 8);
});

test('the working-note traces rc1 observed still fail where a Source is required', () => {
  const noteOnly = ['L-032/train-1', 'L-033/train-1', 'L-033/train-2'].map((key) => {
    const passed = pastedSourceTurn(key);
    const note = { kind: 'note', record_id: 'rsh_note', title: PASTES[key].title, operation: 'create', summary: `title: ${PASTES[key].title}` };
    return { ...passed, memory_commits: passed.memory_commits.map((commit) => commit.kind === 'source' ? note : commit) };
  });
  const rows = memoryRows(noteOnly);
  const source = rows.filter((row) => /\bsource commits\b/u.test(row.detail));
  assert.equal(source.length, 6);
  for (const row of source) assert.equal(row.pass, false, `${row.scenario_id} ${row.session_id}: ${row.detail}`);
  // A Source under another title is not the one the user named.
  const retitled = pastedSourceTurn('L-032/train-1');
  retitled.memory_commits[0].title = 'Solsup Foods order backlog';
  assert.ok(memoryRows([retitled]).every((row) => row.pass === false));
});

test('the judge sees the user-provided Source and is told pasted originals belong in a Source', () => {
  assert.match(SAVE_PRECISION_CRITERION, /original material the user pasted into the Chat .* user_provided Source/u);
  assert.match(SAVE_PRECISION_CRITERION, /facts the user stated or this Chat's progress in a note/u);
  const traces = Object.keys(PASTES).map(pastedSourceTurn);
  const items = judgeItems(suite, traces);
  const precision = items.filter((item) => item.kind === 'save_precision' && item.memory_item.kind === 'source');
  assert.equal(precision.length, Object.keys(PASTES).length);
  for (const item of precision) assert.match(item.memory_item.summary, /\nkind: user_provided\n/u);
  const view = items.find((item) => item.kind === 'save_precision' && item.memory_item.kind === 'lens' && item.trace.session_id === 'train-1');
  assert.equal(view.memory_item.page_kind, 'subject');
  assert.deepEqual(view.memory_item.page_basis, ['span']);
  const keeps = items.find((item) => item.criterion_id === 'keeps-source' && item.trace.scenario_id === 'L-014');
  const input = buildJudgeInput(suite, keeps, traces);
  assert.deepEqual(input.trace_summary.memory_commits.map((commit) => commit.kind), ['source']);
  assert.match(input.trace_summary.memory_commits[0].summary, /kind: user_provided/u);
});

test('a probe answered from the saved user-provided Source carries the figure forward', () => {
  for (const [scenarioId, metrics] of [['L-014', { backlog: 184.6 }], ['L-017', { boxes: 318.2 }], ['L-023', { capex: 73.4 }],
    ['L-024', { operating_profit: 47.3 }], ['L-032', { backlog: 184.6 }], ['L-033', { fy2024_gross_margin: 33 }]]) {
    const answer = `From the saved source.\n\n${JSON.stringify({ metrics })}`;
    const { results } = grade(suite, [trace(scenarioId, 'probe-1', { answer })]);
    const row = results.find((item) => item.assert === 'metrics');
    assert.equal(row.pass, true, `${scenarioId}: ${row.detail}`);
    const nothing = grade(suite, [trace(scenarioId, 'probe-1', { answer: `I couldn't verify it.\n\n${JSON.stringify({ metrics: Object.fromEntries(Object.keys(metrics).map((key) => [key, null])) })}` })]);
    assert.equal(nothing.results.find((item) => item.assert === 'metrics').pass, false, scenarioId);
  }
});
