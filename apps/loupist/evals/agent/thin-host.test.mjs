import assert from 'node:assert/strict';
import test from 'node:test';
import assets from './suites/assets-v1.json' with { type: 'json' };
import scheduling from './suites/scheduling-v1.json' with { type: 'json' };
import alertReplay from './suites/replay/krx-alert.json' with { type: 'json' };
import { grade } from './grade.mjs';

function trace(suite, id, turnIndex, fields = {}) {
  const { sessions: [session] } = suite.scenarios.find(scenario => scenario.id === id);
  return { schema: 'loupist-agent-trace/1', run_id: 'batch', suite: suite.suite, scenario_id: id,
    session_id: session.id, session_role: session.role, memory_mode: session.memory, trial: 0,
    turn_index: turnIndex, user: session.turns[turnIndex].user, status: 'completed', answer: 'Done.', tool_calls: [], ...fields };
}

test('both typed names must be followed in the store, together or separately, in either order', () => {
  const names = ['Zircon Loom', 'Quartz Ferry'];
  const rows = names.map(name => ({ name, reference: null }));
  const submit = follow => ({ name: 'loupist_setup', args: { follow }, outcome: 'ok' });
  const check = (tool_calls, followed_assets) => grade(assets, [trace(assets, 'A-010', 0, { tool_calls, followed_assets })])
    .results.find(row => row.assert === 'followed_names');
  assert.equal(check(rows.map(row => submit([row])), rows.toReversed()).pass, true);
  const lookup = check([{ name: 'asset_lookup', outcome: 'ok' }, submit(rows)], rows);
  assert.equal(lookup.pass, true);
  assert.match(lookup.detail, /asset_lookup called: true/);
  assert.equal(check([submit(rows)], undefined).pass, null);
  assert.equal(check([submit(rows)], rows.slice(1)).pass, false);
  assert.equal(check([submit(rows.slice(1))], rows).pass, false);
});

test('a successful save or an old stored price cannot stand in for the host replay', () => {
  const record = trace(assets, 'A-011', 0, { tool_calls: [{ name: 'price_read_save', outcome: 'ok' }] });
  const check = replayed_price => grade(assets, [{ ...record, replayed_price }]).results.find(row => row.assert === 'replayed_price').pass;
  const price = { state: 'ready', value: 73400, date: '2026-03-03', asset: 'Noeul Paper' };
  assert.equal(check(price), true);
  assert.equal(check(undefined), null);
  for (const change of [{ asset: 'Wrong name' }, { state: 'failed' }, { value: '73400' }, { value: 74000 }, { date: '2026-03-02' }]) {
    assert.equal(check({ ...price, ...change }), false);
  }
  const followup = trace(assets, 'A-011', 1, { tool_calls: record.tool_calls });
  assert.equal(grade(assets, [followup]).results[0].pass, false);
});

test('a quiet alert needs its own successful history entry and neither a Bot nor an inbox delivery', () => {
  const task = { id: 'custom-1', kind: 'custom', prompt: 'Compare both closes to the threshold. Use schedule_quiet when neither qualifies.' };
  const identity = { task_id: task.id, run_id: 'occurrence-2', thread_id: 'task-thread' };
  const created = trace(scheduling, 'S-009', 0, { scheduled_tasks: [{ task_id: task.id, prompt: task.prompt, status: 'active' }] });
  const first = trace(scheduling, 'S-009', 1, { admission: { run_id: 'occurrence-1' } });
  const quiet = trace(scheduling, 'S-009', 2, { answer: '', saved_task: task, resolved_appearance: 'en',
    home_thread_id: 'bot-thread', admission: { ...identity, prompt: task.prompt, surface: 'scheduled' },
    runs: [{ ...identity, kind: 'custom', status: 'succeeded', scheduled_for_ms: 100, started_at_ms: 101, finished_at_ms: 110, message_id: 'reply' }],
    terminal_event: { status: 'completed', message_id: 'reply' },
    session_answer: { message_id: 'reply', status: 'complete', host_composed: false, thread_id: identity.thread_id },
    deliveries: [], inbox_run_ids: ['occurrence-1'], tool_calls: [{ name: 'schedule_quiet', outcome: 'ok' }],
  });
  const check = (record, setup = created, earlier = first) => grade(scheduling, [setup, earlier, record]).results
    .find(row => row.assert === 'scheduled_quiet').pass;
  assert.equal(check(quiet), true);
  assert.equal(check({ ...quiet, inbox_run_ids: undefined }), null);
  assert.equal(check(quiet, { ...created, trial: 1 }), null);
  assert.equal(check(quiet, { ...created, scheduled_tasks: [{ task_id: 'other', prompt: task.prompt }] }), false);
  assert.equal(check(quiet, created, { ...first, admission: { run_id: identity.run_id } }), false);
  for (const change of [{ runs: [] }, { runs: [{ ...quiet.runs[0], status: 'failed' }] },
    { inbox_run_ids: [identity.run_id] }, { deliveries: [{ run_id: identity.run_id }] },
    { session_answer: { ...quiet.session_answer, thread_id: 'other' } }]) {
    assert.equal(check({ ...quiet, ...change }), false);
  }
});

test('the alert replay covers a qualifying close, a quiet check and a provider outage', () => {
  const scenario = scheduling.scenarios.find(item => item.id === 'S-009');
  const tickers = scenario.seed.watchlist.map(row => row.reference.ticker).sort();
  const dates = scenario.sessions[0].turns.slice(1).map(turn => turn.scheduled.clock.slice(0, 10).replaceAll('-', ''));
  const closes = alertReplay.requests.filter(entry => entry.response.status === 200)
    .flatMap(entry => entry.response.body.OutBlock_1);
  const above = dates.slice(0, 2).map(date => {
    const rows = closes.filter(row => row.BAS_DD === date);
    assert.deepEqual(rows.map(row => row.ISU_SRT_CD).sort(), tickers);
    return rows.filter(row => Number(row.TDD_CLSPRC) > 20000).map(row => [row.ISU_SRT_CD, Number(row.TDD_CLSPRC)]);
  });
  assert.deepEqual(above, [[['990221', 21950]], []]);
  const outage = alertReplay.requests.filter(entry => entry.request.url.endsWith(`basDd=${dates[2]}`));
  assert.deepEqual(outage.map(entry => entry.response.status), [503, 503]);
  assert.deepEqual(scenario.sessions[0].turns[3].expect.trace.map(item => item.assert), ['tool_called', 'scheduled_delivery']);
});
