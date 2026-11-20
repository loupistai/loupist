import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { grade } from './grade.mjs';

const load = async (path) => JSON.parse(await readFile(new URL(path, import.meta.url), 'utf8'));
const answers = await load('./fixtures/polish-r1-answers.json');
const suites = Object.fromEntries(await Promise.all(['tool-use', 'principles'].map(async (name) => (
  [name, await load(`./suites/${name}-v1.json`)]
))));

// Actual, unedited answers from the polish-r1 traces. Only the answers and their
// identities are retained; these tests never need the original run or a model.
// Where the observed answer failed the intent, the positive is an explicitly
// corrected counterpart, not an invented successful observation.
const corrected = {
  'T-011': (answer) => `${answer}\n\nWorld Bank(세계은행) 연결을 켜 주세요. 연결 후 해당 지표와 연도의 값을 확인하겠습니다.`,
  'T-012': (answer) => `${answer.split('\n\n')[0]}\n\n한국투자(Korea Investment) 연결을 설정해 주세요. 연결 후 확인된 호가만 가져오겠습니다.`,
  'T-034': () => '2026년 3월 15일까지 공시된 자료에서는 3월 5일 공시의 검사 장비 2대 투자가 확인됩니다.',
};

const swapRetailers = (answer) => answer.replace(/Hearthlane Grocery|Larkspur Retail|Hearthlane|Larkspur/gu, (name) => ({
  'Hearthlane Grocery': 'Larkspur Retail', 'Larkspur Retail': 'Hearthlane Grocery',
  Hearthlane: 'Larkspur', Larkspur: 'Hearthlane',
})[name]);

function rejectedCounterpart(id, answer) {
  if (id === 'T-007' || id === 'T-031') return swapRetailers(answer);
  if (id === 'P-R05b') return answer.replaceAll('8%', '12%').replaceAll('below', 'above')
    .replace('rule is triggered', 'rule is not triggered').replace('condition is met', 'condition is not met');
  if (id === 'P-R03' || id === 'P-R09') {
    // Keep the entire historical answer, including its date and verdict, but unlink the rule.
    return answer.replace(/\[([^\]]+)\]\(lens:rec_[a-f0-9]+\)/gu, '$1');
  }
  throw new Error(`Missing counterexample for ${id}`);
}

for (const observed of answers) {
  test(`${observed.scenario_id}: polish-r1 answer contract accepts the intent and rejects its counterexample`, () => {
    const suite = structuredClone(suites[observed.suite]);
    const scenario = suite.scenarios.find((item) => item.id === observed.scenario_id);
    const session = scenario.sessions.find((item) => item.id === observed.session_id);
    const turn = session.turns[observed.turn_index];
    // Isolate lexical assertions: replay/tool execution and semantic rubrics are
    // independent, and no synthetic tool evidence should make these examples pass.
    const assertions = turn.expect.answer.filter((item) => ['contains_any', 'contains_number_any', 'answer_matching', 'answer_not_matching'].includes(item.assert));
    assert.ok(assertions.some((item) => item.assert === 'contains_any'));
    turn.expect = { answer: assertions };
    const passes = (answer) => {
      const rows = grade(suite, [{
        schema: 'loupist-agent-trace/1', run_id: observed.run_id, suite: observed.suite,
        scenario_id: scenario.id, session_id: session.id, turn_index: observed.turn_index,
        trial: 0, status: 'completed', session_role: session.role, memory_mode: session.memory,
        user: turn.user, answer,
      }]).results;
      assert.equal(rows.length, assertions.length);
      return rows.every((row) => row.pass === true);
    };

    const fix = corrected[scenario.id];
    const accepted = fix ? fix(observed.answer) : observed.answer;
    const rejected = fix ? observed.answer : rejectedCounterpart(scenario.id, observed.answer);
    assert.equal(passes(accepted), true, `accepted: ${accepted}`);
    assert.equal(passes(rejected), false, `rejected: ${rejected}`);
    assert.equal(passes(turn.user), false, 'echoing the user is not a conclusion');
    assert.equal(passes(''), false, 'an idle turn is not a conclusion');

    if (observed.suite === 'principles') {
      const rule = observed.answer.match(/lens:rec_[a-f0-9]+/u)[0];
      const inline = {
        'P-R03': `On September 29, 2026, only 6 million of the 17 million credits of profit appeared in operating cash flow. Under [your cash flow rule](${rule}), that weakens confidence in earnings.`,
        'P-R05b': `The 8% margin on September 29, 2026 falls below the 10% threshold in [your sell rule](${rule}), triggering the condition.`,
        'P-R09': `2026년 9월 29일 영업현금흐름은 6억 원으로 보고 이익 17억 원에 못 미쳤습니다. [저장한 원칙](${rule})에 비춰 이익을 그대로 신뢰하기 어렵습니다.`,
      }[scenario.id];
      assert.equal(passes(inline), true, inline);
      if (scenario.id === 'P-R05b') {
        assert.equal(passes(inline.replace('8%', 'eight percent')), true);
        for (const wrong of ['18%', '28%', '18 percent']) {
          assert.equal(passes(inline.replace('8%', wrong)), false, wrong);
        }
      }
      if (scenario.id === 'P-R03' || scenario.id === 'P-R09') {
        const linked = `The 6 of 17 weakens confidence under [the rule](${rule}).`;
        for (const date of ['2026', '2026-01-01', 'September 29, 2025', '9/29', '2026-09-290']) {
          assert.equal(passes(date + ' ' + linked), false, date);
        }
        for (const date of ['2026-09-29', '29 September 2026', 'September 29th, 2026',
          'Sept. 29, 2026', '29/09/2026', '9/29/26', '2026. 9. 29.', '2026년 9월 29일',
          'the twenty-ninth of September, twenty twenty-six']) {
          assert.equal(passes(date + ' ' + linked), true, date);
        }
      }
      assert.equal(passes('Checked against your principles'), false);
      assert.equal(passes(`See [the rule](${rule}).`), false, 'a link alone supplies no evidence');
    }

    if (['T-007', 'T-031'].includes(scenario.id)) {
      for (const answer of [
        'Put __Hearthlane Grocery__ first: its comparable-store growth is higher.',
        'Hearthlane Grocery has higher same-store growth, so it takes priority.',
        'Hearthlane Grocery의 기존 점포 매출 성장률이 더 높습니다. 6% 대 4%이므로 Hearthlane Grocery를 먼저 둡니다.',
        '**Hearthlane Grocery**가 1위입니다. 동일점포 매출 성장률이 더 높기 때문입니다.',
      ]) {
        assert.equal(passes(answer), true, answer);
        assert.equal(passes(swapRetailers(answer)), false, `wrong retailer: ${answer}`);
      }
      for (const answer of [
        'Hearthlane Grocery has 6 percent same-store and 2 percent total sales growth. Larkspur Retail has 4 and 11 percent.',
        'Put Larkspur Retail first because its total sales growth is 11 percent.',
        'Larkspur Retail을 먼저 둡니다. 총매출 성장률이 11%로 더 높습니다.',
      ]) assert.equal(passes(answer), false, answer);
    }
  });
}
