import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { grade } from './grade.mjs';
import { botQuickLanguageSuite } from './fixtures/r5-bot-quick.mjs';

const load = async name => JSON.parse(await readFile(new URL(name, import.meta.url), 'utf8'));
const suiteOf = name => name === 'r5-bot-quick' ? botQuickLanguageSuite() : load('./suites/' + name + '-v1.json');
const manifest = () => load('./fixtures/r5-manifest.json');
const scenarioOf = async (name, id) => (await suiteOf(name)).scenarios.find(item => item.id === id);

async function turns(stage) {
  let count = 0;
  for (const arm of stage.arms) {
    const suite = await suiteOf(arm.suite);
    for (const id of arm.scenarios) {
      const scenario = suite.scenarios.find(item => item.id === id);
      assert.ok(scenario, id);
      count += scenario.sessions.reduce((sum, session) => sum + session.turns.length, 0) * arm.trials;
    }
  }
  return count;
}

test('R5 pairs each stage with its product revisions and counts fresh controls', async () => {
  const { pins, stages } = await manifest();
  assert.deepEqual(Object.values(pins).map(pin => pin.evalBase.slice(0, 8)), ['0e8e8c38', 'ef5d59b9', '707bc506']);
  const byPin = Object.fromEntries(Object.keys(pins).map(pin => [pin, 0]));
  for (const stage of stages) {
    assert.deepEqual([stage.control, stage.candidate], stage.name.startsWith('B ')
      ? ['p3-n3', 'p3-n3-b'] : ['pre-wave', 'p3-n3']);
    const count = await turns(stage);
    byPin[stage.control] += count;
    byPin[stage.candidate] += count;
  }
  assert.deepEqual(Object.values(byPin), [47, 183, 136]);
});

test('P3 runs each affected research workflow once on both revisions', async () => {
  const { stages } = await manifest();
  const arm = stages.find(stage => stage.name === 'P3').arms.find(item => item.suite === 'tool-use');
  assert.equal(arm.trials, 1);
  for (const id of ['T-041', 'T-042', 'T-047', 'T-048', 'T-049', 'T-050', 'T-051', 'T-057']) {
    assert.ok(arm.scenarios.includes(id), id);
  }
});

test('scheduled valuation runs the T-050 question on identical replay and market inputs', async () => {
  const research = await scenarioOf('tool-use', 'T-050');
  const scheduled = await scenarioOf('scheduled-valuation', 'SV-001');
  const [session] = scheduled.sessions;
  const [turn] = session.turns;
  assert.deepEqual([session.id, session.role, session.memory, turn.scheduled.kind], ['bot', 'probe', 'remember', 'custom']);
  assert.equal(scheduled.replay, research.replay);
  assert.deepEqual(scheduled.connections, research.connections);
  assert.equal(scheduled.clock, research.clock);
  assert.equal(turn.user, research.sessions[0].turns[0].user);
  assert.ok(turn.expect.trace.some(item => item.assert === 'scheduled_delivery'));
  assert.ok(turn.expect.trace.some(item => item.operation === 'reverse_dcf'));
  const { stages } = await manifest();
  assert.deepEqual(stages[0].arms.find(item => item.suite === 'scheduled-valuation'),
    { suite: 'scheduled-valuation', scenarios: ['SV-001'], trials: 3 });
});

test('move attribution accepts inference before the measured move', async () => {
  const scenario = await scenarioOf('tool-use', 'T-049');
  const rubric = scenario.sessions[0].turns[0].expect.rubric;
  assert.doesNotMatch(rubric[0].criterion, /four parts in order|four sections/i);
  assert.match(rubric[0].criterion, /inference/);
});

test('the N3 insufficient-evidence probe withholds the needed same-period cash flow', async () => {
  const caseId = 'P-R10';
  const scenario = await scenarioOf('principles', caseId);
  const [session] = scenario.sessions;
  const [turn] = session.turns;
  assert.equal(session.memory, 'read_only');
  assert.deepEqual(scenario.connections, []);
  assert.ok(scenario.seed.principles.length === 1);
  assert.match(turn.user, /17 million/);
  assert.doesNotMatch(turn.user, /cash flow of [0-9]/i);
  assert.match(turn.expect.rubric[0].criterion, /insufficient/i);
  assert.ok((await manifest()).stages.find(stage => stage.name === 'N3').arms[0].scenarios.includes(caseId));
});

async function answerChecks(id, answer) {
  const suite = await suiteOf('principles');
  const scenario = suite.scenarios.find(item => item.id === id);
  const session = scenario.sessions.at(-1);
  const turn = session.turns[0];
  const trace = { schema: 'loupist-agent-trace/1', run_id: 'private-synthetic', suite: suite.suite,
    trial: 0, turn_index: 0, status: 'completed', scenario_id: id, session_id: session.id,
    session_role: session.role, memory_mode: session.memory, user: turn.user, answer };
  return grade(suite, [trace]).results.filter(row => turn.expect.answer.some(item => item.assert === row.assert));
}

test('private evidence can discuss a principle without claiming to apply a saved checklist', async () => {
  const evidence = 'Cash flow of 6 million fell short of profit of 17 million.';
  for (const [lead, pass] of [
    ['Earnings quality', true], ['Principles of cash accounting', true],
    ['I cannot access your saved principles.', true], ['I have not checked against your principles.', true],
    ['Checked against your principles:', false], ['Under your saved rule,', false],
    ['I applied your research checklist.', false],
  ]) {
    const rows = await answerChecks('P-R07', lead + ' ' + evidence);
    assert.equal(rows.every(row => row.pass === true), pass, lead);
  }
});

test('ordinary Session turns fail an attempted screen label read even after a directions question', async () => {
  const suite = await suiteOf('session-language');
  for (const scenario of suite.scenarios.filter(item => item.id.startsWith('F3-'))) {
    const [session] = scenario.sessions;
    for (const [index, turn] of session.turns.entries()) {
      if (turn.expect.answer.some(item => item.assert === 'visible_labels')) continue;
      const trace = { schema: 'loupist-agent-trace/1', run_id: 'synthetic', suite: suite.suite,
        trial: 0, turn_index: index, status: 'completed', scenario_id: scenario.id, session_id: session.id,
        session_role: session.role, memory_mode: session.memory, user: turn.user, answer: 'Example answer.',
        tool_calls: [{ name: 'interface_labels', outcome: 'error', error_code: 'tool_not_found' }] };
      assert.equal(grade(suite, [trace]).results.find(row => row.assert === 'tool_not_called')?.pass, false);
    }
  }
});

test('briefings and the Korean Source follow-up have content criteria for the judge', async () => {
  for (const id of ['F3-RUN-BRIEF-EN', 'F3-RUN-BRIEF-KO']) {
    const scenario = await scenarioOf('scheduled-language', id);
    const rubric = scenario.sessions[0].turns[0].expect.rubric;
    assert.match(rubric?.[0]?.criterion ?? '', /empty library/);
  }
  const ui = await scenarioOf('session-language', 'F3-UI-02');
  assert.match(ui.sessions[0].turns[2].expect.rubric?.[0]?.criterion ?? '', /18 million tonnes/);
});

test('scheduled content criteria read the full session reply while the preview carries the language check', async () => {
  for (const scenario of (await suiteOf('scheduled-language')).scenarios) {
    const [turn] = scenario.sessions[0].turns;
    assert.match(turn.expect.rubric[0].criterion, /full scheduled reply/);
    assert.ok(turn.expect.answer.some(item => item.assert === 'answer_language'));
  }
});

test('both appearances admit a Korean saved request with and without an explicit override', async () => {
  const suite = await suiteOf('scheduled-language');
  const select = (kind, appearance) => suite.scenarios.find(item => item.id === `F3-KO-RUN-${kind}-${appearance.toUpperCase()}`);
  const defaults = [];
  for (const appearance of ['ko', 'en']) {
    const getTurn = kind => select(kind, appearance).sessions[0].turns[0];
    const base = getTurn('CUSTOM');
    const override = getTurn('OVERRIDE');
    assert.match(base.user, /설비/);
    assert.doesNotMatch(base.user, /영어|한국어/);
    assert.ok(override.user.startsWith(base.user + ' '));
    assert.equal(base.expect.answer[0].language, 'ko');
    assert.equal(override.expect.answer[0].language, appearance === 'en' ? 'ko' : 'en');
    defaults.push(base.user);
  }
  assert.equal(defaults[0], defaults[1]);
});

test('the comparison scenarios name the requested table and the optional publication accurately', async () => {
  for (const id of ['T-041', 'T-042']) {
    const scenario = await scenarioOf('tool-use', id);
    assert.ok(scenario.tags.includes('comparison'));
    assert.doesNotMatch(scenario.title, /is one grid/);
  }
});

test('the implied-growth review credits a range presented through a linked sensitivity card', async () => {
  const scenario = await scenarioOf('tool-use', 'T-050');
  assert.match(scenario.sessions[0].turns[0].expect.rubric[0].criterion, /displayed sensitivity card linked to the reverse DCF result/);
});
