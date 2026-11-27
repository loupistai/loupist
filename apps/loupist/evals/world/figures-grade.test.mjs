import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { amountOf, gradeFigures, MIN_PRECISION } from './figures-grade.mjs';

const suite = JSON.parse(await readFile(new URL('./figures-corpus-v1.json', import.meta.url), 'utf8'));
const PROFILES = ['us', 'korea', 'japan', 'germany', 'united_kingdom', 'india', 'brazil', 'hong_kong_china'];
const sourceOf = (scenario) => scenario.steps[0].sources[0];

test('the corpus has two Sources for each of eight market profiles, and labels each figure once', () => {
  assert.equal(suite.scenarios.length, 16);
  assert.deepEqual(suite.scenarios.map(({ profile }) => profile), PROFILES.flatMap((profile) => [profile, profile]));
  let cells = 0;
  for (const scenario of suite.scenarios) {
    assert.match(sourceOf(scenario).origin, /^https:\/\/fixtures\.invalid\//u);
    assert.ok(scenario.cells.length >= 3, scenario.id);
    const amounts = new Set();
    for (const cell of scenario.cells) {
      assert.deepEqual(Object.keys(cell), ['metric', 'period', 'period_end', 'basis', 'value', 'scale', 'unit', 'currency'], scenario.id);
      assert.ok(cell.metric.length <= 80 && sourceOf(scenario).text.includes(cell.metric), `${scenario.id} metric as stated: ${cell.metric}`);
      assert.match(cell.period_end, /^\d{4}-\d{2}(?:-\d{2})?$/u, scenario.id);
      assert.ok(['actual', 'forecast', 'guidance', 'estimate'].includes(cell.basis), scenario.id);
      assert.ok(['amount', 'percent', 'count'].includes(cell.unit), scenario.id);
      assert.equal(cell.unit === 'amount', cell.currency !== null, `${scenario.id} ${cell.metric}`);
      // One amount, one label: a kept structure can be matched to its cell by the amount alone.
      const amount = amountOf(cell);
      assert.equal(amounts.has(`${amount.amount}|${amount.currency}`), false, `${scenario.id} ${cell.metric}`);
      amounts.add(`${amount.amount}|${amount.currency}`);
      cells += 1;
    }
  }
  assert.equal(cells, 58);
  // Each profile's own number format, and both kinds of basis, are in the corpus.
  const text = Object.fromEntries(PROFILES.map((profile) => [profile, suite.scenarios.filter((item) => item.profile === profile).map((item) => sourceOf(item).text).join(' ')]));
  assert.match(text.korea, /1,234억 원[\s\S]*1조 2,345억 원/u);
  assert.match(text.japan, /86億5,000万円[\s\S]*2027年3月期/u);
  assert.match(text.germany, /1\.482,6 Mio\. EUR[\s\S]*2,85 Mrd\. EUR/u);
  assert.match(text.united_kingdom, /18\.6p[\s\S]*14\/01\/2027/u);
  assert.match(text.india, /₹642\.80 crore[\s\S]*lakh[\s\S]*FY2026-27/u);
  assert.match(text.brazil, /R\$ 1,84 bilhão[\s\S]*4,1 milhões de toneladas/u);
  assert.match(text.hong_kong_china, /21\.3亿元[\s\S]*9,500万港元/u);
  for (const basis of ['actual', 'forecast', 'guidance', 'estimate']) assert.ok(suite.scenarios.some((item) => item.cells.some((cell) => cell.basis === basis)), basis);
});

test('an amount is its value times its scale in its currency, whatever spelling states the scale', () => {
  assert.deepEqual(amountOf({ value: 1234, scale: 1e8, currency: 'KRW' }), { amount: 123400000000, currency: 'KRW' });
  assert.deepEqual(amountOf({ value: '1234', scale: '억', currency: 'krw' }), { amount: 123400000000, currency: 'KRW' });
  assert.deepEqual(amountOf({ value: 123.4, scale: 'billion', currency: 'KRW' }).amount, 123400000000);
  assert.deepEqual(amountOf({ value: 642.8, scale: 'crore', currency: 'INR' }).amount, 6428000000);
  assert.deepEqual(amountOf({ value: 1482.6, scale: 'Mio.', currency: 'EUR' }).amount, 1482600000);
  const pence = amountOf({ value: 18.6, currency: 'GBX' });
  assert.deepEqual([pence.currency, Math.abs(pence.amount - 0.186) < 1e-12], ['GBP', true]);
  assert.deepEqual(amountOf({ value: 41.5, scale: null, currency: null }), { amount: 41.5, currency: null });
  assert.equal(amountOf({ value: 'about six', scale: 1, currency: 'USD' }), null);
  assert.equal(amountOf({ value: 6, scale: 'dozen', currency: 'USD' }), null);
});

/** A trace in which every Source published one claim per given structure. */
function trace(structuresOf, { status = 'completed' } = {}) {
  return suite.scenarios.flatMap((scenario) => {
    const fixture = sourceOf(scenario);
    const common = { suite: suite.suite, scenario_id: scenario.id, trial: 0, attempt_id: scenario.id, status: 'completed' };
    const claims = structuresOf(scenario).map((figure, index) => ({ id: `${scenario.id}-${index}`, status: 'active', proposition: `figure ${index}`, evidence: [{ spanId: 'span' }],
      qualifiers: figure === null ? [] : [{ key: 'figure', value: typeof figure === 'string' ? figure : JSON.stringify(figure) }] }));
    return [{ ...common, step_id: '__setup' }, { ...common, step_id: '__seed' },
      { ...common, status, step_id: 's1', world_inference: [{ call_index: 0 }], before_chat: { records: [
        { record: { kind: 'source', title: fixture.title }, canonical_materials: [{ span_id: 'span' }] },
        { snapshot: { kind: 'world', payload: { nodes: [], claims } } },
      ] } }];
  });
}
const kept = (cell, change = {}) => ({ metric: cell.metric, period: cell.period, period_kind: 'unknown', period_end: cell.period_end, basis: cell.basis,
  scope: 'unknown', value: cell.value, unit: cell.unit, currency: cell.currency, scale: cell.scale, ...change });

test('every labelled figure kept as stated gives precision and recall of one', () => {
  const graded = gradeFigures(suite, trace((scenario) => scenario.cells.map((cell) => kept(cell))));
  assert.deepEqual([graded.labelled, graded.kept, graded.right, graded.precision, graded.recall, graded.pass], [58, 58, 58, 1, 1, true]);
  assert.deepEqual(graded.by_profile.japan, { labelled: 7, kept: 7, right: 7, recalled: 7 });
  assert.equal(graded.scenarios.every((item) => item.metric_as_stated === item.labelled), true);
  // A claim with no structure is neither kept nor wrong: only recall falls.
  const half = gradeFigures(suite, trace((scenario) => scenario.cells.map((cell, index) => (index === 0 ? kept(cell) : null))));
  assert.deepEqual([half.kept, half.right, half.precision, half.pass], [16, 16, 1, true]);
  assert.equal(half.recall, 16 / 58);
});

test('a kept structure with another value, period or basis is wrong, and precision under 95% fails', () => {
  const [us] = suite.scenarios;
  const only = (structures) => gradeFigures(suite, trace((scenario) => (scenario.id === us.id ? structures : []))).scenarios[0];
  const [revenue, income] = us.cells;
  assert.deepEqual(only([kept(revenue)]).wrong, { value: 0, period: 0, basis: 0, unreadable: 0 });
  // The same amount in another scale and spelling is the same figure.
  assert.equal(only([kept(revenue, { value: 1.4826, scale: 'billion' })]).right, 1);
  assert.equal(only([kept(revenue, { value: 148.26 })]).wrong.value, 1);
  assert.equal(only([kept(revenue, { currency: 'EUR' })]).wrong.value, 1);
  assert.equal(only([kept(revenue, { period_end: '2026-07-31' })]).wrong.period, 1);
  assert.equal(only([kept(revenue, { period_end: null })]).wrong.period, 1);
  assert.equal(only([kept(revenue, { basis: 'guidance' })]).wrong.basis, 1);
  assert.equal(only([kept(revenue, { basis: 'unknown' })]).wrong.basis, 1);
  assert.equal(only(['not json']).wrong.unreadable, 1);
  assert.equal(only([kept(revenue, { metric: 'Revenue, net' })]).metric_as_stated, 0);
  // The reader gets each structure that is not right, with the claim it sits on.
  assert.deepEqual(only([kept(income, { basis: 'estimate' })]).not_right.map(({ claim_id, outcome, cell }) => [claim_id, outcome, cell]), [[`${us.id}-0`, 'basis', 1]]);

  // A company's own outlook is one basis under either name; a third party's estimate is another.
  const outlook = suite.scenarios.find((item) => item.id === 'FIG-UK-2');
  const uk = (structures) => gradeFigures(suite, trace((scenario) => (scenario.id === outlook.id ? structures : []))).scenarios.find((item) => item.scenario_id === outlook.id);
  const [guided, , consensus] = outlook.cells;
  assert.deepEqual([guided.basis, consensus.basis], ['guidance', 'estimate']);
  assert.equal(uk([kept(guided, { basis: 'forecast' })]).right, 1);
  assert.equal(uk([kept(guided, { basis: 'estimate' })]).wrong.basis, 1);
  assert.equal(uk([kept(consensus, { basis: 'guidance' })]).wrong.basis, 1);

  // A period end finer than the Source states is counted, not failed; another month is wrong.
  const year = suite.scenarios.find((item) => item.id === 'FIG-JP-2');
  const japan = (structures) => gradeFigures(suite, trace((scenario) => (scenario.id === year.id ? structures : []))).scenarios.find((item) => item.scenario_id === year.id);
  assert.equal(year.cells[0].period_end, '2027-03');
  assert.deepEqual([japan([kept(year.cells[0], { period_end: '2027-03-31' })]).right, japan([kept(year.cells[0], { period_end: '2027-03-31' })]).finer_period_end], [1, 1]);
  assert.equal(japan([kept(year.cells[0], { period_end: '2027-12' })]).wrong.period, 1);

  // 58 kept with three wrong is 94.8%: under the gate. Two wrong is 96.6%: over it.
  const wrongFirst = (count) => { let left = count; return (scenario) => scenario.cells.map((cell) => (left-- > 0 ? kept(cell, { basis: 'unknown' }) : kept(cell))); };
  const [three, two] = [gradeFigures(suite, trace(wrongFirst(3))), gradeFigures(suite, trace(wrongFirst(2)))];
  assert.deepEqual([three.right, three.precision < MIN_PRECISION, three.pass, two.precision >= MIN_PRECISION, two.pass], [55, true, false, true, true]);
});

test('nothing kept or an incomplete run cannot pass, and the command exits 1 below the gate', async (t) => {
  const none = gradeFigures(suite, trace(() => []));
  assert.deepEqual([none.kept, none.precision, none.recall, none.pass], [0, null, 0, false]);
  const failed = gradeFigures(suite, trace((scenario) => scenario.cells.map((cell) => kept(cell)), { status: 'failed' }));
  assert.deepEqual([failed.complete, failed.kept, failed.precision, failed.pass], [false, null, null, false]);
  assert.throws(() => gradeFigures(suite, [{ suite: 'world-smoke-v018', scenario_id: 'FIG-US-1' }]), /must name the suite and its scenarios/u);

  const root = await mkdtemp(join(tmpdir(), 'figures-grade-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const write = async (name, rows) => { await writeFile(join(root, name), `${rows.map((row) => JSON.stringify(row)).join('\n')}\n`); return join(root, name); };
  const script = fileURLToPath(new URL('./figures-grade.mjs', import.meta.url));
  const suitePath = fileURLToPath(new URL('./figures-corpus-v1.json', import.meta.url));
  const good = await write('good.jsonl', trace((scenario) => scenario.cells.map((cell) => kept(cell))));
  const bad = await write('bad.jsonl', trace((scenario) => scenario.cells.map((cell) => kept(cell, { value: cell.value * 10 }))));
  const run = (traces, out) => spawnSync(process.execPath, [script, suitePath, traces, join(root, out)], { encoding: 'utf8' });
  assert.equal(run(good, 'good.json').status, 0);
  assert.deepEqual(JSON.parse(await readFile(join(root, 'good.json'), 'utf8')).precision, 1);
  const below = run(bad, 'bad.json');
  assert.equal(below.status, 1);
  assert.equal(JSON.parse(below.stdout).right, 0);
  assert.equal(run(good, 'good.json').status, 1, 'a report is never overwritten');
});
