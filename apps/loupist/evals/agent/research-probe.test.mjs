import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { inflateRawSync } from 'node:zlib';
import { validateSuite } from './eval-contract.mjs';
import { assertScenarioContract, expectations, isKoreanUserTurn } from './suite-contract.mjs';
import { grade } from './grade.mjs';

const filePath = fileURLToPath(new URL('./suites/research-probe-v1.json', import.meta.url));
const suite = JSON.parse(await readFile(filePath, 'utf8'));
const scenario = (id) => suite.scenarios.find((item) => item.id === id);
const replayOf = async (id) => JSON.parse(await readFile(new URL(`./suites/${scenario(id).replay}`, import.meta.url), 'utf8'));

// Both arms of the research-protocol comparison run this probe, and the candidate removes
// or narrows plan_update, passage reads, unasked results capture and Lens filing. A check on
// any of those steps would pass or fail an arm for the step itself, so the probe reads only
// outcomes; the steps are measured from the traces instead.
const OUTCOME_CHECKS = new Set(['numbers_linked', 'grid_cells_linked', 'no_repeated_failed_call', 'no_false_save_claim']);

test('the probe is two one-turn research sessions on replayed fictional companies', () => {
  validateSuite(suite);
  assert.equal(suite.suite, 'research-probe');
  assert.deepEqual(suite.scenarios.map((item) => item.id), ['RP-001', 'RP-002']);
  for (const item of suite.scenarios) {
    assertScenarioContract(item, { suite: suite.suite, filePath });
    assert.equal(item.split, 'capability');
    assert.ok(item.tags.includes('needs-replay'), item.id);
    assert.match(item.clock, /^2026-11-16T/u, item.id);
    assert.equal(item.sessions.length, 1, item.id);
    const [session] = item.sessions;
    assert.notEqual(session.id, 'bot', item.id);
    assert.equal(session.memory, 'remember', item.id);
    assert.equal(session.turns.length, 1, item.id);
    assert.equal(item.language, isKoreanUserTurn(session.turns[0].user) ? 'ko' : 'en', item.id);
    assert.ok(item.seed.watchlist.length > 0, `${item.id} follows the companies it asks about`);
  }
  assert.deepEqual(scenario('RP-001').connections, ['opendart']);
  assert.deepEqual(scenario('RP-002').connections, ['opendart', 'web-research']);
});

test('checks read outcomes and name nothing either arm is asked to change', () => {
  assert.doesNotMatch(JSON.stringify(suite), /plan_update|plan_progress|lens_file|note_save/u);
  for (const item of suite.scenarios) {
    const [turn] = item.sessions[0].turns;
    const checks = expectations(turn).filter(({ bucket }) => bucket !== 'rubric');
    assert.ok(checks.length > 0, item.id);
    for (const { item: check } of checks) assert.ok(OUTCOME_CHECKS.has(check.assert), `${item.id} ${check.assert}`);
    assert.ok(checks.some(({ item: check }) => check.assert === 'numbers_linked'), item.id);
  }
  assert.ok(expectations(scenario('RP-001').sessions[0].turns[0]).some(({ item }) => item.assert === 'grid_cells_linked'));
});

const evidence = (letter) => `#evidence-ev_${letter.repeat(32)}`;
const DART = evidence('d');
const CALC = evidence('c');
const NEWS = evidence('e');

function traceFor(id, overrides) {
  const item = scenario(id);
  const session = item.sessions[0];
  return {
    schema: 'loupist-agent-trace/1', run_id: `${id}-fixture`, suite: suite.suite, scenario_id: id,
    session_id: session.id, session_role: session.role, memory_mode: session.memory, turn_index: 0, trial: 0,
    status: 'completed', user: session.turns[0].user, tool_calls: [], memory_commits: [], number_checks: [], ...overrides,
  };
}
const rows = (trace) => grade(suite, [trace]).results.map((row) => [row.assert, row.pass]);
const passes = (trace, name) => rows(trace).some(([assertName, pass]) => assertName === name && pass === true);
const fails = (trace, name) => rows(trace).some(([assertName, pass]) => assertName === name && pass === false);
const check = (text, status = 'matched') => ({ link_index: 0, text, evidence_id: 'ev', kind: 'data', status });

const lookup = { name: 'dart_company_lookup', args: { query: 'Solsup Foods' }, outcome: 'ok' };
const statement = (company) => ({ name: 'dart_financials', outcome: 'ok', result: { citation_url: DART },
  args: { company, fiscal_year: 2026, report_period: 'q3', basis: 'consolidated' } });
const noAnnual = { name: 'dart_financials', outcome: 'error', error_code: 'no_data', error_message: 'OpenDART returned no matching data',
  args: { company: 'Solsup Foods', fiscal_year: 2026, report_period: 'annual', basis: 'consolidated' } };
const cell = (value, url = DART) => ({ value, citation_url: url });
const grid = (statuses) => {
  const cells = [[cell('₩126.4bn'), cell('₩12.0bn'), cell('9.5%', CALC), cell('+10.7%', CALC)],
    [cell('₩67.5bn'), cell('₩5.2bn'), cell('7.7%', CALC), cell('-6.6%', CALC)]];
  return { name: 'grid_publish', outcome: 'ok',
    args: { mode: 'create', layout: 'assets', title: 'Q3 2026',
      rows: [{ symbol: '990221', market: 'KRX', label: 'Solsup Foods' }, { symbol: '990777', market: 'KRX', label: 'Hanbit Chem' }],
      columns: ['Revenue', 'Operating income', 'Operating margin', 'Revenue change'].map((label) => ({ label, period: 'Q3 2026', basis: 'consolidated' })),
      cells },
    result: { status: 'staged', layout: 'assets', artifact: { artifact_id: 'quarter-comparison' }, cell_checks: cells.flatMap((row, r) => row.map((item, c) => (
      { cell: `cells[${r}][${c}]`, value: item.value, status: statuses[r * 4 + c] ?? 'matched' }))) } };
};

test('RP-001: a linked, checked comparison passes and an unlinked, mismatched or repeated-failure one fails', () => {
  const answer = `In the third quarter of 2026 Solsup Foods grew revenue to [₩126.4bn](${DART}), [+10.7%](${CALC}) on a year earlier, `
    + `with operating income of [₩12.0bn](${DART}) and a margin of [9.5%](${CALC}). Hanbit Chem's revenue fell to [₩67.5bn](${DART}), `
    + `[-6.6%](${CALC}), and its margin narrowed to [7.7%](${CALC}).`;
  const good = traceFor('RP-001', {
    answer,
    tool_calls: [lookup, statement('Solsup Foods'), statement('Hanbit Chem'), grid([])],
    number_checks: ['₩126.4bn', '+10.7%', '₩12.0bn', '9.5%', '₩67.5bn', '-6.6%', '7.7%'].map((text) => check(text, 'rounded')),
  });
  assert.ok(rows(good).every(([, pass]) => pass === true), JSON.stringify(rows(good)));

  // numbers_linked: a figure written without its link, or a link the host found to differ, fails.
  assert.ok(fails({ ...good, answer: answer.replace(`[9.5%](${CALC})`, '9.5%') }, 'numbers_linked'));
  assert.ok(fails({ ...good, number_checks: [...good.number_checks.slice(1), check('₩126.4bn', 'differs')] }, 'numbers_linked'));

  // grid_cells_linked: a table with a cell that differs from its source fails; correcting it in a
  // later publish of the same reply (the revision wave 4 makes possible) passes. A linked prose comparison can stand on its own.
  const wrong = grid(['matched', 'differs']);
  assert.ok(fails({ ...good, tool_calls: [lookup, statement('Solsup Foods'), wrong] }, 'grid_cells_linked'));
  const correction = grid([]);
  correction.args = { ...correction.args, mode: 'revise', artifact_id: 'quarter-comparison' };
  assert.ok(passes({ ...good, tool_calls: [lookup, statement('Solsup Foods'), wrong, correction] }, 'grid_cells_linked'));
  assert.ok(passes({ ...good, tool_calls: good.tool_calls.filter(call => call.name !== 'grid_publish') }, 'grid_cells_linked'));

  // no_repeated_failed_call: one failed read followed by a different request passes; the same failed request again fails.
  assert.ok(passes({ ...good, tool_calls: [lookup, noAnnual, statement('Solsup Foods'), grid([])] }, 'no_repeated_failed_call'));
  assert.ok(fails({ ...good, tool_calls: [lookup, noAnnual, noAnnual, statement('Solsup Foods'), grid([])] }, 'no_repeated_failed_call'));

  // no_false_save_claim: a save claim with a commit passes; the same claim with nothing committed fails.
  const claim = `${answer} I saved this comparison to your notes for later.`;
  assert.ok(passes({ ...good, answer: claim, memory_commits: [{ kind: 'note', title: 'Solsup Foods and Hanbit Chem, Q3 2026' }] }, 'no_false_save_claim'));
  assert.ok(fails({ ...good, answer: claim }, 'no_false_save_claim'));
});

test('RP-002: an answer with linked filing and article figures passes and an unlinked or falsely saved one fails', () => {
  const answer = `한빛화학의 3분기 영업이익률은 [7.7%](${CALC})로 전년 동기 [11.1%](${CALC})보다 낮습니다. `
    + `여수공장 정기보수로 판매량이 [약 9%](${NEWS}) 줄었고, 일회성 비용 [약 14억원](${NEWS})이 반영되었습니다. `
    + `스프레드는 톤당 [212달러](${NEWS})로 2분기 [248달러](${NEWS})보다 좁아졌습니다.`;
  const good = traceFor('RP-002', {
    answer,
    tool_calls: [statement('한빛화학'), { name: 'web_fetch', outcome: 'ok', args: { url: 'https://fictional.example/news/hanbit-chem-q3-2026' }, result: { citation_url: NEWS } }],
    memory_commits: [{ kind: 'note', title: '한빛화학 IR 통화' }],
    number_checks: ['7.7%', '11.1%', '약 9%', '약 14억원', '212달러', '248달러'].map((text) => check(text)),
  });
  assert.ok(rows(good).every(([, pass]) => pass === true), JSON.stringify(rows(good)));
  assert.ok(fails({ ...good, answer: answer.replace(`[약 14억원](${NEWS})`, '약 14억원') }, 'numbers_linked'));
  assert.ok(fails({ ...good, number_checks: [...good.number_checks.slice(0, 5), check('248달러', 'not_found')] }, 'numbers_linked'));
  const fetchAgain = { name: 'web_fetch', outcome: 'error', error_code: 'eval_replay_unmatched', args: { url: 'https://fictional.example/news/hanbit-chem' } };
  assert.ok(passes({ ...good, tool_calls: [fetchAgain, ...good.tool_calls] }, 'no_repeated_failed_call'));
  assert.ok(fails({ ...good, tool_calls: [fetchAgain, fetchAgain, ...good.tool_calls] }, 'no_repeated_failed_call'));
  const claim = `${answer} 통화 내용은 메모에 저장했습니다.`;
  assert.ok(passes({ ...good, answer: claim }, 'no_false_save_claim'));
  assert.ok(fails({ ...good, answer: claim, memory_commits: [] }, 'no_false_save_claim'));
});

/** Income-statement amounts by corp_code, year and report code, as the replay serves them. */
function statements(replay) {
  const out = new Map();
  for (const entry of replay.requests) {
    const url = new URL(entry.request.url ?? 'https://invalid.example/');
    if (url.pathname !== '/api/fnlttSinglAcntAll.json' || entry.response.body?.status !== '000') continue;
    const key = ['corp_code', 'bsns_year', 'reprt_code'].map((name) => url.searchParams.get(name)).join(':');
    out.set(key, Object.fromEntries(entry.response.body.list.map((row) => [row.account_id, Number(row.thstrm_amount)])));
  }
  return out;
}
const bn = (value) => (value / 1e9).toFixed(1);
const pct = (value) => value.toFixed(1);
const signed = (value) => `${value >= 0 ? '+' : '-'}${Math.abs(value).toFixed(1)}%`;
const REVENUE = 'ifrs-full_Revenue';
const OPERATING = 'dart_OperatingIncomeLoss';

test('the criteria state the figures the replay serves', async () => {
  const comparison = statements(await replayOf('RP-001'));
  const criterion = scenario('RP-001').sessions[0].turns[0].expect.rubric[0].criterion;
  for (const corp of ['99022100', '99077700']) {
    const latest = comparison.get(`${corp}:2026:11014`);
    const yearEarlier = comparison.get(`${corp}:2025:11014`);
    assert.ok(latest && yearEarlier && comparison.get(`${corp}:2026:11012`), corp);
    for (const account of [REVENUE, OPERATING]) {
      assert.ok(criterion.includes(`KRW ${bn(latest[account])}bn against ${bn(yearEarlier[account])}bn (${signed((latest[account] / yearEarlier[account] - 1) * 100)})`), `${corp} ${account}`);
    }
    assert.ok(criterion.includes(`${pct(latest[OPERATING] / latest[REVENUE] * 100)}% against ${pct(yearEarlier[OPERATING] / yearEarlier[REVENUE] * 100)}%`), corp);
  }

  const margin = await replayOf('RP-002');
  const hanbit = statements(margin);
  const [cause, kept] = scenario('RP-002').sessions[0].turns[0].expect.rubric.map((item) => item.criterion);
  const ratio = (key) => pct(hanbit.get(key)[OPERATING] / hanbit.get(key)[REVENUE] * 100);
  assert.ok(cause.includes(`fell to ${ratio('99077700:2026:11014')}%`));
  assert.ok(cause.includes(`against ${ratio('99077700:2026:11012')}% in the second quarter and ${ratio('99077700:2025:11014')}% a year earlier`));
  const article = margin.requests.find((entry) => entry.provider === 'web');
  assert.ok(scenario('RP-002').sessions[0].turns[0].user.includes(article.request.url), 'the user turn links the replayed article');
  for (const [said, printed] of [['about 9% lower', '약 9% 줄었'], ['about KRW 1.4bn', '약 14억원'], ['USD 212 a tonne', '톤당 212달러'], ['against USD 248', '248달러']]) {
    assert.ok(cause.includes(said), said);
    assert.ok(article.response.text.includes(printed), printed);
  }
  const report = margin.requests.find((entry) => entry.request.url?.endsWith('rcept_no=20261112990777'));
  const archive = Buffer.from(report.response.body_base64, 'base64');
  // One stored or deflated member: read the local header and its data.
  const nameLength = archive.readUInt16LE(26);
  const extraLength = archive.readUInt16LE(28);
  const size = archive.readUInt32LE(18);
  const data = archive.subarray(30 + nameLength + extraLength, 30 + nameLength + extraLength + size);
  const text = (archive.readUInt16LE(8) === 8 ? inflateRawSync(data) : data).toString('utf8');
  assert.ok(cause.includes('38 days of the quarter') && text.includes('38일간'));
  assert.ok(text.includes('정기보수는 진행 중') && !article.response.text.includes('끝났'), 'only the user says the maintenance ended');
  assert.match(kept, /ended at the end of October/u);
});
