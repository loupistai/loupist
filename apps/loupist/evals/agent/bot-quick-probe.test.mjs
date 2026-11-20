import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { answerPatterns, validateSuite } from './eval-contract.mjs';
import { assertScenarioContract } from './suite-contract.mjs';
import { grade } from './grade.mjs';

const filePath = fileURLToPath(new URL('./suites/bot-quick-probe-v1.json', import.meta.url));
const suite = JSON.parse(await readFile(filePath, 'utf8'));

test('live Bot timing probe is five ungraded turns with a marketless followed NVDA', () => {
  validateSuite(suite);
  assert.equal(suite.suite, 'bot-quick-probe');
  assert.equal(suite.scenarios.length, 1);
  const [scenario] = suite.scenarios;
  assertScenarioContract(scenario, { suite: suite.suite, filePath });
  assert.deepEqual(scenario.connections, ['openbb.platform']);
  assert.equal(scenario.replay, undefined);
  assert.deepEqual(scenario.seed.watchlist, [{ name: 'NVIDIA', reference: { ticker: 'NVDA' } }]);
  const [session] = scenario.sessions;
  assert.equal(session.id, 'bot');
  assert.equal(session.memory, 'remember');
  assert.deepEqual(session.turns.map(turn => turn.user), [
    'NVDA 간단 브리핑해 줘', 'PER은?', 'AAPL 지금 어때?', 'Quick take on MSFT?', '엔비디아 왜 빠졌어?',
  ]);
  assert.deepEqual(session.turns.map(turn => turn.expect.trace.find(check => check.assert === 'max_tool_calls').n), [5, 1, 5, 5, 4]);
  for (const index of [0, 2, 3]) {
    const expect = session.turns[index].expect;
    assert.deepEqual(expect.trace.filter(check => check.assert === 'tool_not_called').map(check => check.tool), [
      'lens_file', 'source_save', 'web_fetch', 'world_search', 'lens_search', 'conversation_memory',
    ]);
    assert.ok(expect.answer.some(check => check.assert === 'numbers_linked'));
    assert.ok(expect.answer.some(check => check.assert === 'no_false_save_claim'));
    assert.ok(expect.memory.some(check => check.assert === 'lens_unchanged'));
    assert.ok(expect.memory.some(check => check.assert === 'committed' && check.kind === 'source' && check.count === 0));
  }
  assert.match(session.turns[4].expect.rubric[0].criterion, /publisher.*date/iu);
});

test('live runtime exception stays specific to the timing probe', () => {
  const [scenario] = suite.scenarios;
  assert.throws(() => assertScenarioContract(scenario, { suite: 'tool-use', filePath }));
  assert.throws(() => assertScenarioContract({ ...scenario, connections: ['unknown'] }, { suite: suite.suite, filePath }));
});

test('quick answer checks catch extra calls, filing and a false save claim, not the answer form', () => {
  const scenario = suite.scenarios[0];
  const session = scenario.sessions[0];
  const trace = {
    schema: 'loupist-agent-trace/1', run_id: 'scripted-probe', suite: suite.suite,
    scenario_id: scenario.id, session_id: session.id, turn_index: 0, trial: 0,
    status: 'completed', session_role: session.role, memory_mode: session.memory, user: session.turns[0].user,
    answer: '간단한 브리핑입니다.', tool_calls: [{ name: 'price_read_save', outcome: 'ok' }],
    memory_commits: [], lens: { pages: [], ops_this_turn: [] }, number_checks: [],
  };
  const results = changes => grade(suite, [{ ...trace, ...changes }]).results.filter(row => row.turn_index === 0);
  const failed = (changes, assertion) => results(changes).some(row => row.assert === assertion && row.pass === false);
  assert.ok(results({}).every(row => row.pass), JSON.stringify(results({})));
  const formed = `## 요약\n\n| 항목 | 내용 |\n| --- | --- |\n| 흐름 | 견조함 |\n\n${'설명 '.repeat(400)}`;
  assert.ok(results({ answer: formed }).every(row => row.pass), JSON.stringify(results({ answer: formed })));
  assert.ok(failed({ answer: '브리핑을 메모리에 저장했어요.' }, 'no_false_save_claim'));
  assert.ok(failed({ tool_calls: [...trace.tool_calls, { name: 'lens_file', outcome: 'ok' }] }, 'tool_not_called'));
  assert.ok(failed({ tool_calls: Array.from({ length: 6 }, () => trace.tool_calls[0]) }, 'max_tool_calls'));
  assert.ok(failed({ tool_calls: [{ name: 'price_read_save', outcome: 'error' }] }, 'tool_called'));
  assert.ok(failed({ memory_commits: [{ kind: 'source' }] }, 'committed'));
});

/** Every answer of the paid baseline's probe run (three trials of the five turns, in order), verbatim. */
const BASELINE = [
  // trial 0, turn 0
  "지금은 NVIDIA(NVDA) 시세와 실적 데이터를 불러오지 못해 브리핑을 드릴 수 없습니다. 잠시 후 다시 요청해 주시면 확인하겠습니다.",
  // trial 0, turn 1
  "NVDA의 주가수익비율(PER)은 2026년 11월 16일 종가 기준 [19.7배](#evidence-ev_b8034c4699b611e30fcb56059ba5f4a7)입니다. 선행 PER은 [9.9배](#evidence-ev_b8034c4699b611e30fcb56059ba5f4a7)입니다.",
  // trial 0, turn 2
  "애플(AAPL)은 2026년 11월 16일 종가 [$352.78](#evidence-ev_dd261954dabbb30f9a988e516362a985)로, 52주 고점 대비 [-2.2%](#evidence-ev_dd261954dabbb30f9a988e516362a985)입니다. 당일은 [-1.0%](#evidence-ev_dd261954dabbb30f9a988e516362a985) 하락했고, 주가수익비율은 [39.3배](#evidence-ev_dd261954dabbb30f9a988e516362a985)입니다.",
  // trial 0, turn 3
  "Microsoft closed at [$517.99](#evidence-ev_0f7bac65fb61a6660606ba167303221b) on 16 November 2026, down [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b) and [-7.4%](#evidence-ev_0f7bac65fb61a6660606ba167303221b) below its 52-week high. The share is essentially on its 50-day average, and the trailing price-to-earnings ratio is [27.0x](#evidence-ev_0f7bac65fb61a6660606ba167303221b).",
  // trial 0, turn 4
  "2026년 11월 16일 하락의 원인은 확인되지 않았습니다. 엔비디아는 그날 종가 [$156.14](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)로 전일 대비 [-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)였고, 원인을 더 좁히려면 리서치 세션으로 넘기면 됩니다.",
  // trial 1, turn 0
  "엔비디아(NVDA)는 2026년 11월 16일 종가 [$156.14](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)로 전일 대비 [-4.1%](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)이며, 52주 고점 [$243.37](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 대비 [-35.8%](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)로 52주 저점 [$148.80](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)에 가깝습니다. 50일선 [$214.65](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)와 200일선 [$204.19](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 모두 아래에 있습니다.\n\n시가총액은 [$3.77조](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 달러, 주가수익비율은 후행 [19.7배](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be), 선행 [9.9배](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)입니다. 2026년 7월 31일 마감 분기 매출은 [$962억](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 달러, 영업이익률은 [66.2%](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be), 전분기 대비 매출 증가율은 [17.9%](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)입니다. 같은 날 보도는 증권 집단소송 원고 모집 공고이며, 하락 원인으로 확인된 것은 아닙니다.",
  // trial 1, turn 1
  "엔비디아의 주가수익비율은 2026년 11월 16일 종가 기준으로 후행 [19.7배](#evidence-ev_9ca464578c94c9872f88325b34558fb5), 선행 [9.9배](#evidence-ev_9ca464578c94c9872f88325b34558fb5)입니다.",
  // trial 1, turn 2
  "애플(AAPL)은 2026년 11월 16일 종가 [$352.78](#evidence-ev_6b2e3e3954e653439c5e698489c87af4)로 전일 대비 [-1.0%](#evidence-ev_6b2e3e3954e653439c5e698489c87af4)이며, 52주 고점 대비 [-2.2%](#evidence-ev_6b2e3e3954e653439c5e698489c87af4)로 고점 부근에 있습니다. 50일선과 200일선 위에 있고, 후행 주가수익비율은 [39.3배](#evidence-ev_6b2e3e3954e653439c5e698489c87af4)입니다. 2026년 9월 30일 마감 분기 영업이익률은 [31.1%](#evidence-ev_6b2e3e3954e653439c5e698489c87af4)입니다.",
  // trial 1, turn 3
  "Microsoft closed at [$517.99](#evidence-ev_4a552de294c9e82a5c1534860138f08a) on 16 November 2026, down [-1.1%](#evidence-ev_4a552de294c9e82a5c1534860138f08a), and stands [-7.4%](#evidence-ev_4a552de294c9e82a5c1534860138f08a) below its 52-week high, essentially on the 50-day average and still well above the 200-day. The trailing price-to-earnings ratio is [27.0 times](#evidence-ev_4a552de294c9e82a5c1534860138f08a).",
  // trial 1, turn 4
  "하락 원인은 아직 확인되지 않았습니다. 엔비디아는 2026년 11월 16일 종가 [$156.14](#evidence-ev_ad89dfaf0123385fc2a6b2dc8936d901)로 전일 대비 [-4.1%](#evidence-ev_ad89dfaf0123385fc2a6b2dc8936d901)였고, 같은 날 보도는 증권 집단소송 원고 모집 공고일 뿐 원인으로 확인된 것은 아닙니다. 원인을 리서치 세션 「엔비디아 11월 16일 하락 원인」에서 확인하겠습니다.",
  // trial 2, turn 0
  "엔비디아(NVDA)는 2026년 11월 16일 [156.14달러](#evidence-ev_d208d04275abd36fbead619f0fbd080f)로 마감해 전일보다 [-4.1%](#evidence-ev_d208d04275abd36fbead619f0fbd080f) 낮고, 52주 고점 대비 [-35.8%](#evidence-ev_d208d04275abd36fbead619f0fbd080f)로 52주 저점 부근이며 50일·200일 이동평균 아래에 있습니다. 2026년 7월 31일 마감 분기의 영업이익률은 [66.2%](#evidence-ev_d208d04275abd36fbead619f0fbd080f)로, 주가 약세와 달리 최근 보고된 수익성은 아직 매우 높습니다.",
  // trial 2, turn 1
  "엔비디아(NVDA)의 주가수익배율(PER)은 2026년 11월 16일 종가 기준 [19.7배](#evidence-ev_1f2d5fe8c74e683046b470bfd76e891a)이고, 선행 PER은 [9.9배](#evidence-ev_1f2d5fe8c74e683046b470bfd76e891a)입니다.",
  // trial 2, turn 2
  "애플(AAPL)은 2026년 11월 16일 [352.78달러](#evidence-ev_31aea5f4faaf5523ce19718d23d4fa88)로 마감해 전일보다 [-1.0%](#evidence-ev_31aea5f4faaf5523ce19718d23d4fa88) 낮지만, 52주 고점 대비 [-2.2%](#evidence-ev_31aea5f4faaf5523ce19718d23d4fa88)로 고점 부근에 있고 50일·200일 이동평균 위에 있습니다. 주가수익배율은 [39.3배](#evidence-ev_31aea5f4faaf5523ce19718d23d4fa88)로, 가격은 강하지만 밸류에이션은 비싼 편입니다.",
  // trial 2, turn 3
  "Microsoft closed at [$517.99](#evidence-ev_10bab1aaef6c3c7268d58803639ef475) on November 16, 2026, down [-1.1%](#evidence-ev_10bab1aaef6c3c7268d58803639ef475) on the day and [-7.4%](#evidence-ev_10bab1aaef6c3c7268d58803639ef475) from its 52-week high, sitting on its 50-day average and still well above the 200-day. The trailing price-to-earnings ratio is [27.0x](#evidence-ev_10bab1aaef6c3c7268d58803639ef475), so the pullback is modest and the multiple is not stretched.",
  // trial 2, turn 4
  "하락 원인은 아직 확인되지 않았습니다. 엔비디아는 2026년 11월 16일 [156.14달러](#evidence-ev_c99489fd1f3725a3456d72c59b429322)로 마감해 전일보다 [-4.1%](#evidence-ev_c99489fd1f3725a3456d72c59b429322) 낮고, 52주 고점 대비로는 [-35.8%](#evidence-ev_c99489fd1f3725a3456d72c59b429322)입니다. 같은 날 헤드라인은 집단소송 원고 모집 안내일 뿐이라 원인으로 볼 수 없고, 확인은 리서치 세션 「엔비디아 하락 원인」에서 이어가겠습니다.",
];
// The answers among them that write a linked figure's sign, or its currency, a second time.
const DOUBLED_SIGN = new Set([2, 3, 8, 10, 12, 13, 14]);
const DOUBLED_CURRENCY = new Set([5]);

test('number-writing checks find a sign or currency said twice beside a linked figure in the baseline answers', () => {
  const [scenario] = suite.scenarios;
  const [session] = scenario.sessions;
  const checks = session.turns.map(turn => turn.expect.answer.filter(check => check.metric === 'number_writing'));
  for (const list of checks) assert.deepEqual(list, checks[0]);
  assert.equal(checks[0].length, 2);
  const [sign, currency] = checks[0];
  const matches = (check, text) => answerPatterns(check).some(pattern => pattern.test(text));

  for (const text of [
    'down [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b)',
    '[-7.4%](#evidence-ev_0f7bac65fb61a6660606ba167303221b) below its 52-week high',
    'stands [-7.4%](#evidence-ev_4a552de294c9e82a5c1534860138f08a) below its 52-week high',
    'down [-1.1%](#evidence-ev_10bab1aaef6c3c7268d58803639ef475) on the day',
    '당일은 [-1.0%](#evidence-ev_dd261954dabbb30f9a988e516362a985) 하락했고',
    '전일보다 [-4.1%](#evidence-ev_d208d04275abd36fbead619f0fbd080f) 낮고',
    '전일보다 [-1.0%](#evidence-ev_31aea5f4faaf5523ce19718d23d4fa88) 낮지만',
    // The baseline notes' Korean form.
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) 하락',
    // A magnitude or subject particle between the signed figure and the direction word.
    '전일보다 [-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)만큼 하락했습니다',
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)나 하락',
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)가 하락했다',
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)은 하락했습니다',
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)만 하락했습니다.',
    // A Korean direction word before the link.
    '당일 하락 [-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)를 기록했습니다.',
    '전일 대비 하락한 [-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)입니다.',
    // Controls the recheck confirmed.
    'Shares were off [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b) today.',
    'It fell [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b).', 'It dropped by [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b).',
    'It slid [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b).', 'Revenue declined [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b).',
    '[-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b) under its average',
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) 낮아졌습니다', '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)만큼 감소했습니다',
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)가 떨어졌습니다', '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)나 줄었습니다',
  ]) assert.ok(matches(sign, text), text);
  for (const text of [
    // A signed figure with no direction word beside it.
    '전일 대비 [-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)였고',
    '52주 고점 대비 [-2.2%](#evidence-ev_dd261954dabbb30f9a988e516362a985)입니다',
    '[-7.4%](#evidence-ev_10bab1aaef6c3c7268d58803639ef475) from its 52-week high',
    '전일 대비 [-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)',
    '[-1.1%](#evidence-ev_10bab1aaef6c3c7268d58803639ef475) on the day',
    // An unsigned figure beside a direction word.
    'down [1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b)',
    '[1.0%](#evidence-ev_dd261954dabbb30f9a988e516362a985) 하락했고',
    // A negated direction, a lower bound, and signed levels after 에서 and 로 as after "from … to".
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) 낮지 않습니다',
    'A [−3.2%](#evidence-ev_0f7bac65fb61a6660606ba167303221b) lower bound is consistent with these observations.',
    'the lower [-3.2%](#evidence-ev_0f7bac65fb61a6660606ba167303221b) bound',
    '[-2%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)에서 [-5%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)로 하락했습니다',
    'fell from [-2%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) to [-5%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)',
    // A hyphenated "-off", English and Korean negation, and the levels after 로.
    'a one-off [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b) charge',
    'The goodwill write-off [-$1.2bn](#evidence-ev_0f7bac65fb61a6660606ba167303221b) was booked in Q3.',
    'It did not fall [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b) on the day.',
    'The stock was not down [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b) today.',
    "It didn't fall [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b).",
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) 낮지는 않습니다.', '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) 하락하진 않았습니다.',
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) 하락하지는 않았습니다.', '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) 떨어지지는 않았습니다.',
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) 감소하진 않았습니다.', '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) 줄어들진 않았습니다.',
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)은 낮은 수준이 아닙니다.',
    '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)로 하락했습니다.', '[-4.1%](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)로 낮습니다.',
    // Controls the recheck confirmed: rose or up, and a word between the direction and the link.
    'rose [+1.4%](#evidence-ev_0f7bac65fb61a6660606ba167303221b)', 'up [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b)',
    'down about [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b)', 'fell sharply [-1.1%](#evidence-ev_0f7bac65fb61a6660606ba167303221b)',
  ]) assert.ok(!matches(sign, text), text);
  for (const text of [
    '[$3.77조](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 달러',
    '[$962억](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 달러',
    // Both markers inside the link, a comma between, and the same for won.
    '[$3.77조 달러](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)',
    '[$3.77조](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be), 달러',
    '[₩1,200억](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 원',
    '[₩1,200억 원](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)',
    // Controls the recheck confirmed.
    '[$962억](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)달러', '[$517.99](#evidence-ev_0f7bac65fb61a6660606ba167303221b) dollars',
    '[$3.77조, 달러](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)', '[₩80](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) won',
    '[$4.2bn dollars](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)', '[$962억](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 달러화',
    '[₩100](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) won.',
  ]) assert.ok(matches(currency, text), text);
  for (const text of [
    // The currency word with no symbol, and a symbol with no word.
    '[156.14달러](#evidence-ev_d208d04275abd36fbead619f0fbd080f)로',
    '[$156.14](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3)로',
    '[$517.99](#evidence-ev_0f7bac65fb61a6660606ba167303221b) on 16 November 2026',
    // Words that only begin like 엔 or 원.
    '[$156.14](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) 엔비디아',
    '[$156.14](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) 원인',
    // A word of another currency, or "won" as a verb, is not the same currency said twice.
    'The close of [$517.99](#evidence-ev_0f7bac65fb61a6660606ba167303221b) won back half the drop.',
    '[$3.77조](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 원화 환산',
    // "won" as a verb after a won figure, and 원화 inside the link.
    'The contract [₩2.4bn](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) won board approval.',
    'After the dip, [₩52,000](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) won back most of the loss.',
    '[₩1,200억 원화](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be)',
    // Controls the recheck confirmed.
    '[₩1,200억](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 원화 환산입니다.', '[₩80](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 원유',
    '[₩52,000](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 원래', '[₩100](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 원금',
    '[₩100](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) 원가', '[₩156.14](#evidence-ev_ae87f10c9cfe3bff0b09070d228e62e3) 원인',
    'The [₩2.4bn](#evidence-ev_8bfed6f7e0653b33b9f2f88ef2bfc7be) contract won approval yesterday.',
    '[$517.99](#evidence-ev_0f7bac65fb61a6660606ba167303221b)',
  ]) assert.ok(!matches(currency, text), text);

  // Through the grader, every baseline answer with a doubled case fails and every answer free of both passes.
  assert.equal(BASELINE.length, 15);
  BASELINE.forEach((answer, index) => {
    const turn = index % 5;
    const trace = {
      schema: 'loupist-agent-trace/1', run_id: 'baseline-answers', suite: suite.suite,
      scenario_id: scenario.id, session_id: session.id, turn_index: turn, trial: Math.floor(index / 5),
      status: 'completed', session_role: session.role, memory_mode: session.memory, user: session.turns[turn].user,
      answer, tool_calls: [], memory_commits: [], lens: { pages: [], ops_this_turn: [] }, number_checks: [],
    };
    const rows = grade(suite, [trace]).results.filter(row => row.metric === 'number_writing');
    assert.deepEqual(rows.map(row => row.pass), [!DOUBLED_SIGN.has(index), !DOUBLED_CURRENCY.has(index)],
      `trial ${trace.trial} turn ${turn}: ${JSON.stringify(rows.map(row => row.detail))}`);
  });
});
