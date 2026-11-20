import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { grade } from './grade.mjs';
import { buildJudgeInput, judgeItems } from './judge.mjs';

// Unedited answers and the trace fields their checks read, from the rc1 pre-release
// regression (`rc1` and its `rc1-r2` rerun). Each case pairs an observed correct answer
// with a counterpart that must still fail.
const load = async (path) => JSON.parse(await readFile(new URL(path, import.meta.url), 'utf8'));
const observed = await load('./fixtures/rc1-answers.json');
const suites = Object.fromEntries(await Promise.all(['tool-use', 'lens-lifecycle', 'learning'].map(async (name) => (
  [name, await load(`./suites/${name}-v1.json`)]
))));

const answers = (scenarioId, sessionId) => {
  const items = observed.filter((item) => item.scenario_id === scenarioId && item.session_id === sessionId);
  assert.deepEqual(items.map((item) => item.attempt), ['rc1', 'rc1-r2'], `${scenarioId} ${sessionId}`);
  return items;
};

function turnOf(suiteName, scenarioId, sessionId) {
  const suite = structuredClone(suites[suiteName]);
  const scenario = suite.scenarios.find((item) => item.id === scenarioId);
  const session = scenario.sessions.find((item) => item.id === sessionId);
  return { suite, scenario, session, turn: session.turns[0] };
}

/** Grade one observed turn against only the named assertions of its current suite turn. */
function gradeObserved(item, asserts, fields = {}) {
  const { suite, scenario, session, turn } = turnOf(item.suite, item.scenario_id, item.session_id);
  const pick = (list) => (list ?? []).filter((check) => asserts.includes(check.assert));
  turn.expect = { trace: pick(turn.expect.trace), memory: pick(turn.expect.memory), answer: pick(turn.expect.answer) };
  const { attempt: _attempt, ...recorded } = item;
  const trace = { schema: 'loupist-agent-trace/1', trial: 0, status: 'completed',
    session_role: session.role, memory_mode: session.memory, ...recorded, ...fields };
  const { results, unmatched } = grade(suite, [trace]);
  assert.deepEqual(unmatched, []);
  assert.ok(results.length > 0, `${scenario.id}: no ${asserts.join(', ')} check`);
  return results;
}
const passes = (item, asserts, fields) => gradeObserved(item, asserts, fields).every((row) => row.pass === true);

test('T-029 accepts the harvest count written out in boxes and still rejects another count', () => {
  for (const item of answers('T-029', 'task-1')) {
    assert.match(item.answer, /\{"metrics":\{"boxes":88100000\}\}$/u);
    assert.equal(passes(item, ['metrics']), true, item.answer);
    for (const wrong of [88100, 8810000, 881000000, 88.2, 318.2]) {
      const answer = item.answer.replace('88100000', String(wrong));
      assert.equal(passes(item, ['metrics'], { answer }), false, answer);
    }
  }
  const [item] = answers('T-029', 'task-1');
  assert.equal(passes(item, ['metrics'], { answer: '{"metrics":{"boxes":88.1}}' }), true);
});

const swapRetailers = (answer) => answer.replace(/Hearthlane Grocery|Larkspur Retail|Hearthlane|Larkspur/gu, (name) => ({
  'Hearthlane Grocery': 'Larkspur Retail', 'Larkspur Retail': 'Hearthlane Grocery', Hearthlane: 'Larkspur', Larkspur: 'Hearthlane',
})[name]);

test('T-031 accepts the bold retailer conclusion and rejects the other retailer or the bare figures', () => {
  for (const item of answers('T-031', 'task-1')) {
    assert.match(item.answer, /^\*\*Hearthlane Grocery\*\* grew more in its established stores/u);
    assert.equal(passes(item, ['contains_any']), true, item.answer);
    assert.equal(passes(item, ['contains_any'], { answer: swapRetailers(item.answer) }), false);
    assert.equal(passes(item, ['contains_any'], { answer: item.user }), false, 'echoing the user is not a conclusion');
    const figures = item.answer.split('\n\n').at(-1);
    assert.equal(passes(item, ['contains_any'], { answer: figures }), false, figures);
  }
});

test('T-050 leaves the chosen discount and terminal growth rates plain but not the grid results', () => {
  for (const item of answers('T-050', 'task-1')) {
    const [row] = gradeObserved(item, ['numbers_linked']);
    assert.equal(row.pass, true, row.detail);
    assert.deepEqual(item.tool_calls[0].args, { operation: 'reverse_dcf', wacc_pct: [8, 9, 10], terminal_growth_pct: [1, 2, 3] });
    // Without the calculation that took those rates, they are unlinked percents again.
    const [bare] = gradeObserved(item, ['numbers_linked'], { tool_calls: [] });
    assert.equal(bare.pass, false);
    assert.match(bare.detail, /^unlinked 9%, 2%, 1%, 2%, 3%/u);
    // An implied growth cell, the historical growth or a sourced figure written without its link still fails.
    for (const text of ['2.08%', '10.03%', '₩21,950']) {
      const link = new RegExp(String.raw`\[([^\]\n]*${text.replace('.', '\\.')}[^\]\n]*)\]\(#evidence-ev_[0-9a-f]{32}\)`, 'u');
      assert.match(item.answer, link, text);
      assert.equal(passes(item, ['numbers_linked'], { answer: item.answer.replace(link, '$1') }), false, text);
    }
    assert.equal(passes(item, ['numbers_linked'], { answer: `${item.answer}\n\nAt an 11% discount rate the price implies more.` }), false);
    const flagged = item.number_checks.map((check, index) => (index === 1 ? { ...check, status: 'differs' } : check));
    assert.equal(passes(item, ['numbers_linked'], { number_checks: flagged }), false);
  }
});

const RECEIPT = '20260318990221';
const rubricOf = (turn, id) => turn.expect.rubric.find((item) => item.id === id).criterion;
const judgeInput = (item, criterionId, fields = {}, earlier = []) => {
  const { suite, session } = turnOf(item.suite, item.scenario_id, item.session_id);
  const record = (entry, extra = {}) => {
    const { attempt: _attempt, ...recorded } = entry;
    const own = turnOf(entry.suite, entry.scenario_id, entry.session_id).session;
    return { schema: 'loupist-agent-trace/1', trial: 0, status: 'completed', session_role: own.role, memory_mode: own.memory, ...recorded, ...extra };
  };
  const trace = record(item, fields);
  assert.equal(trace.session_role, session.role);
  const traces = [...earlier.map((entry) => record(entry)), trace];
  const judged = judgeItems(suite, [trace]).find((entry) => entry.criterion_id === criterionId);
  return buildJudgeInput(suite, judged, traces);
};

test('T-038 and T-039 keep the filing as the replay serves it: its dart_financials result', async () => {
  for (const id of ['T-038', 'T-039']) {
    const { scenario, turn } = turnOf('tool-use', id, 'setup');
    const replay = await load(`./suites/${scenario.replay}`);
    // The replay serves the statements of that receipt and no filing document.
    const served = replay.requests.filter((entry) => JSON.stringify(entry.response.body ?? '').includes(RECEIPT));
    assert.deepEqual(served.map((entry) => new URL(entry.request.url).pathname), ['/api/fnlttSinglAcntAll.json']);
    assert.ok(!replay.requests.some((entry) => entry.request.url?.includes('/api/document.xml')));
    assert.match(replay.otherwise.find((entry) => entry.url_prefix.includes('/api/document.xml')).response.text, /<status>013<\/status>/u);
    assert.match(scenario.replay_needs, /no filing document/u);
    const criterion = rubricOf(turn, 'keeps-the-filing');
    assert.match(criterion, new RegExp(`receipt ${RECEIPT}`, 'u'));
    assert.match(criterion, /dart_financials result for that receipt passes; the full filing document is not required/u);
    assert.match(criterion, /Fail if no such Source was committed or the answer claims more was saved/u);

    for (const item of answers(id, 'setup')) {
      // The judge sees the committed Source of that receipt, so the criterion is decidable.
      const input = judgeInput(item, 'keeps-the-filing');
      const [source] = input.trace_summary.memory_commits.filter((commit) => commit.kind === 'source');
      assert.match(source.summary, /origin: tool:dart_financials/u);
      assert.ok(source.summary.includes(`"filing_id":"${RECEIPT}"`));
      assert.equal(passes(item, ['committed']), true);
      // Without a Source commit the setup fails its save checks.
      assert.equal(passes(item, ['committed'], { memory_commits: item.memory_commits.filter((commit) => commit.kind !== 'source') }), false);
      assert.equal(passes(item, ['committed'], { memory_commits: [] }), false);
    }
  }
});

test('T-038 cutoff honesty accepts not being able to establish a figure and still rejects the later filing', () => {
  const { turn } = turnOf('tool-use', 'T-038', 'probe');
  const criterion = rubricOf(turn, 'cutoff-honesty');
  assert.match(criterion, /no FY2025 revenue figure for Solsup Foods can be established as of 2026-03-10/u);
  assert.match(criterion, /had not been reported by then, or the more careful claim that nothing available by that date shows a reported figure/u);
  assert.match(criterion, /uses nothing from the annual filing published on 2026-03-18 and gives no FY2025 revenue figure or estimate/u);
  for (const item of answers('T-038', 'probe')) {
    assert.match(item.answer, /doesn[’']t (?:establish that|prove) the company hadn[’']t/u);
    assert.equal(passes(item, ['answer_not_matching', 'as_of_left_out']), true, item.answer);
    for (const leak of ['Revenue was ₩452.3bn.', '매출은 4,523억 원이었습니다.', 'Revenue was 452,300,000,000 KRW.', 'It was ₩4.523조.']) {
      assert.equal(passes(item, ['answer_not_matching'], { answer: `${item.answer}\n\n${leak}` }), false, leak);
    }
    const hidden = { ...item.as_of_report, left_out: { ...item.as_of_report.left_out, sources: 0 } };
    assert.equal(passes(item, ['as_of_left_out'], { as_of_report: hidden }), false);
  }
});

test('LL-005 checks that the update keeps what the first session filed, not detail it never filed', () => {
  const { turn } = turnOf('lens-lifecycle', 'LL-005', 'second');
  const criterion = rubricOf(turn, 'page-current');
  assert.match(criterion, /every other July detail the page held before this turn, as the first session filed it in prior_sessions/u);
  assert.match(criterion, /A July detail the first session never filed, such as the mill footprint, is not required/u);
  const firsts = answers('LL-005', 'first');
  for (const [index, item] of answers('LL-005', 'second').entries()) {
    // The judge sees what the first session filed; in rc1 that was never the mill footprint.
    const input = judgeInput(item, 'page-current', {}, [firsts[index]]);
    const filed = JSON.stringify(input.prior_sessions[0].turns[0].trace_summary.tools.filter((tool) => tool.name === 'lens_file'));
    assert.match(filed, /packaging board (?:is )?61%/iu);
    assert.match(filed, /2\.6[x×]/u);
    assert.doesNotMatch(filed, /Savonlinna|Kemi|Duluth|mill footprint/iu);
    assert.ok(input.lens_pages_after_turn.length === 1);

    assert.equal(passes(item, ['lens_pages']), true);
    // A page collapsed to the new dividend fails the retention checks.
    const [page] = item.lens.pages;
    const collapsed = { ...page, sections: page.sections.map((section) => (section.key === 'facts'
      ? { ...section, markdown: 'The 2 October 2026 notice cuts the quarterly dividend from USD 0.18 to USD 0.10 per share.' }
      : { ...section, markdown: 'Check coverage at the next results.' })) };
    assert.equal(passes(item, ['lens_pages'], { lens: { pages: [collapsed] } }), false);
  }
});

test('L-022 states a termination that took effect, so a cautious reading has one right answer', () => {
  const { turn } = turnOf('learning', 'L-022', 'train-2');
  assert.match(turn.user, /terminated the contract effective 2026-04-09\.$/u);
  assert.doesNotMatch(turn.user, /filed to terminate/u);
  assert.match(rubricOf(turn, 'records-termination'), /terminated on 2026-04-09 rather than renewed through 2028/u);
  // rc1 answered the ambiguous "filed to terminate"; those turns no longer match the fixture
  // and cannot pass.
  for (const item of answers('L-022', 'train-2')) {
    assert.match(item.user, /filed to terminate/u);
    assert.ok(gradeObserved(item, ['committed']).every((row) => row.pass === null && row.detail === 'turn completion unavailable'));
  }
  // The probe still needs renewal_in_force 0: rc1's first run gave 0, the rerun -1 ("unknown").
  const [first, rerun] = answers('L-022', 'probe-1');
  assert.equal(passes(first, ['metrics']), true, first.answer);
  assert.equal(passes(rerun, ['metrics']), false, rerun.answer);
  for (const value of ['1', 'null', '0.5']) {
    assert.equal(passes(first, ['metrics'], { answer: first.answer.replace('"renewal_in_force":0', `"renewal_in_force":${value}`) }), false, value);
  }
});

test('L-019 asks for the figure that replaced 40 percent and why, not a three-part format', () => {
  const { turn } = turnOf('learning', 'L-019', 'probe-1');
  const criterion = rubricOf(turn, 'discloses-status');
  assert.equal(criterion, 'The answer gives 33 percent as the current gross margin and says that it replaces the earlier 40 percent figure and why.');
  for (const item of answers('L-019', 'probe-1')) {
    assert.match(item.answer, /The restatement you reported supersedes the original 40\.0%/u);
    assert.equal(passes(item, ['metrics']), true, item.answer);
    assert.equal(passes(item, ['metrics'], { answer: item.answer.replace('"current_gross_margin":33.0', '"current_gross_margin":40.0') }), false);
  }
});

test('L-033 names the FY2024 margin it asks for, so the rc1 view answers 33 and the old key fails', () => {
  const { scenario, turn } = turnOf('learning', 'L-033', 'probe-1');
  const train = scenario.sessions.find((session) => session.id === 'train-1').turns[0];
  assert.match(train.user, /records 40\.0 percent as the FY2024 gross margin\.$/u);
  assert.match(turn.user, /\{"metrics":\{"fy2024_gross_margin": <number>\}\}/u);
  assert.match(rubricOf(turn, 'reads-and-discloses'), /gives 33 percent as the FY2024 gross margin it now uses/u);
  for (const item of answers('L-033', 'probe-1')) {
    // rc1 asked for current_gross_margin and answered null with 33.0% in the prose; that turn no longer matches.
    assert.match(item.answer, /\{"metrics":\{"current_gross_margin":null\}\}$/u);
    assert.ok(gradeObserved(item, ['metrics']).every((row) => row.pass === null && row.detail === 'turn completion unavailable'));
    const asked = { user: turn.user };
    const corrected = item.answer.replace('{"metrics":{"current_gross_margin":null}}', '{"metrics":{"fy2024_gross_margin":33.0}}');
    assert.equal(passes(item, ['metrics'], { ...asked, answer: corrected }), true, corrected);
    for (const wrong of ['{"metrics":{"fy2024_gross_margin":40.0}}', '{"metrics":{"fy2024_gross_margin":null}}', '{"metrics":{"current_gross_margin":33.0}}']) {
      const answer = item.answer.replace('{"metrics":{"current_gross_margin":null}}', wrong);
      assert.equal(passes(item, ['metrics'], { ...asked, answer }), false, wrong);
    }
  }
});
