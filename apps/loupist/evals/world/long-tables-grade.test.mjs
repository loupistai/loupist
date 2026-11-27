import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { amounts, gradeLongTables, periodOf } from './long-tables-grade.mjs';

const suite = JSON.parse(await readFile(new URL('./smoke-long-tables-v1.json', import.meta.url), 'utf8'));
const [india, uk] = suite.scenarios;
const sourceOf = (scenario) => scenario.steps[0].sources[0];

/** The printed cells of a row, read from the Source itself. */
function printedRow(scenario, row) {
  const { text } = sourceOf(scenario);
  if (scenario.form === 'markdown') {
    const line = text.split('\n').find((item) => item.startsWith(`| ${row} |`));
    return line.split('|').slice(2, -1).map((cell) => cell.trim());
  }
  const line = text.split('\n').find((item) => item.startsWith(`<tr><th scope="row">${row} (`));
  return [...line.matchAll(/<td>([^<]*)<\/td>/gu)].map((match) => match[1]);
}
const columnsOf = (scenario) => (scenario.form === 'markdown'
  ? sourceOf(scenario).text.split('\n').find((line) => line.startsWith('| Particulars |')).split('|').slice(2, -1).map((cell) => cell.trim())
  : [...sourceOf(scenario).text.split('\n').find((line) => line.startsWith('<thead>')).matchAll(/<th scope="col">([^<]*)<\/th>/gu)].map((match) => match[1]).slice(1));

test('the two long tables are over 7,000 characters, from two market profiles, in Markdown and in HTML', () => {
  assert.deepEqual(suite.scenarios.map(({ profile, form }) => [profile, form]), [['india', 'markdown'], ['united_kingdom', 'html']]);
  assert.match(sourceOf(india).text, /All amounts are in ₹ crore[\s\S]*\| --- \|/u);
  assert.match(sourceOf(uk).text, /<table>[\s\S]*Monetary amounts are in £m|Monetary amounts are in £m[\s\S]*<table>/u);
  for (const scenario of suite.scenarios) {
    assert.equal(scenario.steps.length, 1);
    assert.ok(sourceOf(scenario).text.length > 7000, scenario.id);
    assert.match(sourceOf(scenario).origin, /^https:\/\/fixtures\.invalid\//u);
    assert.match(sourceOf(scenario).text, /fictional/u);
  }
});

test('every labelled cell is the figure the Source prints at that row and column', () => {
  for (const scenario of suite.scenarios) {
    assert.ok(scenario.cells.length >= 30, scenario.id);
    const columns = columnsOf(scenario);
    const seen = new Set();
    let late = 0;
    for (const cell of scenario.cells) {
      assert.deepEqual(Object.keys(cell), ['row', 'column', 'value', 'unit', 'currency'], scenario.id);
      const printed = printedRow(scenario, cell.row)[columns.indexOf(cell.column)];
      assert.equal(Number(printed.replaceAll(',', '')), cell.value, `${scenario.id} ${cell.row} ${cell.column}`);
      assert.equal(seen.has(`${cell.row}|${cell.column}`), false);
      seen.add(`${cell.row}|${cell.column}`);
      assert.doesNotThrow(() => periodOf(cell.column));
      // Most labels lie beyond the first few thousand characters, where a chunk no longer holds the header row.
      if (sourceOf(scenario).text.indexOf(cell.row) > 3500) late += 1;
    }
    assert.ok(late >= 20, `${scenario.id} has ${late} labelled cells past the start of the table`);
    // A row is recognized by the first word of its name, so that word is no other row's and not the company's.
    const names = [...new Set(sourceOf(scenario).text.match(scenario.form === 'markdown' ? /^\| [^|:]+:/gmu : /<th scope="row">[^:<]+:/gu))]
      .map((label) => label.replace(/^\| |<th scope="row">/u, '').split(' ')[0].toLowerCase());
    assert.equal(new Set(names).size, names.length, scenario.id);
    for (const name of names) assert.equal(sourceOf(scenario).title.toLowerCase().includes(name), false, `${scenario.id} ${name}`);
    assert.equal(new Set(scenario.cells.map((cell) => cell.column)).size, columns.length, `${scenario.id} labels every period`);
  }
  assert.deepEqual([...new Set(india.cells.map((cell) => `${cell.unit}/${cell.currency}`))], ['crore/INR']);
  assert.deepEqual([...new Set(uk.cells.map((cell) => `${cell.unit}/${cell.currency}`))].sort(), ['count/null', 'million/GBP']);
});

test('a period label is stated by its usual spellings and a full year is not a quarter of that year', () => {
  const [quarter, half, year, ukHalf, ukYear] = ['Q2 FY2026-27', 'H1 FY2026-27', 'FY2025-26', 'H2 2025/26', 'FY 2025/26'].map(periodOf);
  for (const text of ['in Q2 FY2026-27', 'for the second quarter of FY2026-27', 'in FY 2026-27 Q2', 'in Q2 of 2026-2027']) assert.equal(quarter.states(text), true, text);
  for (const text of ['in Q1 FY2026-27', 'in Q2 FY2025-26', 'in H1 FY2026-27', 'in FY2026-27']) assert.equal(quarter.states(text), false, text);
  for (const text of ['in H1 FY2026-27', 'for the half year FY2026-27', 'in the first half of FY2026-27']) assert.equal(half.states(text), true, text);
  assert.equal(year.states('for FY2025-26'), true);
  assert.equal(year.states('in the year 2025-26'), true);
  assert.equal(year.states('in Q2 FY2025-26'), false);
  assert.equal(year.states('in H1 FY2025-26'), false);
  assert.equal(ukHalf.states('in H2 2025/26'), true);
  assert.equal(ukHalf.states('in the second half of 2025/26'), true);
  assert.equal(ukHalf.states('in H1 2025/26'), false);
  assert.equal(ukYear.states('for FY 2025/26'), true);
  assert.equal(ukYear.states('for H2 2025/26'), false);
  assert.throws(() => periodOf('September 2026'), /Unsupported period label/u);
  assert.deepEqual(amounts('Segment revenue was ₹1,284.50 crore in Q2 FY2026-27, against 588.15 in Q2 FY2025-26, announced 9 November 2026.'), [1284.5, 588.15]);
});

/** A trace of one arm: one claim per sentence, each citing the scenario's Source. */
function trace(sentences, { status = 'completed' } = {}) {
  return suite.scenarios.flatMap((scenario) => {
    const fixture = sourceOf(scenario);
    const common = { suite: suite.suite, scenario_id: scenario.id, trial: 0, attempt_id: scenario.id, status: 'completed' };
    const claims = (sentences[scenario.id] ?? []).map((proposition, index) => ({ id: `${scenario.id}-${index}`, status: 'active', proposition, evidence: [{ spanId: 'span' }] }));
    return [{ ...common, step_id: '__setup' }, { ...common, step_id: '__seed' },
      { ...common, status, step_id: 's1', world_inference: [{ call_index: 0 }, { call_index: 1 }], before_chat: { records: [
        { record: { kind: 'source', title: fixture.title }, canonical_materials: [{ span_id: 'span' }] },
        { snapshot: { kind: 'world', payload: { nodes: [], claims: [...claims, { id: 'elsewhere', status: 'active', proposition: 'Coimbatore segment revenue was 1.00 in Q2 FY2026-27.', evidence: [{ spanId: 'another-source' }] }] } } },
      ] } }];
  });
}
const say = (cell, value = cell.value) => {
  const [entity, measure] = cell.row.split(': ');
  const figure = cell.unit === 'count' ? `${value}` : cell.currency === 'INR' ? `₹${value} crore` : `£${value}m`;
  return `${entity} reported ${measure.toLowerCase()} of ${figure} in ${cell.column}.`;
};

test('a claim that names the row and column and states the figure is found; a different figure there is wrong', () => {
  const all = Object.fromEntries(suite.scenarios.map((scenario) => [scenario.id, scenario.cells.map((cell) => say(cell))]));
  const full = gradeLongTables(suite, trace(all));
  assert.deepEqual([full.labelled, full.found, full.wrong, full.missing, full.calls], [72, 72, 0, 0, 4]);
  assert.equal(gradeLongTables(suite, trace({})).missing, 72);

  const [first, second, third, fourth] = india.cells;
  const graded = gradeLongTables(suite, trace({ [india.id]: [
    say(first, first.value + 1),                                                    // another figure at that row and column
    say(second).replace(second.column, 'Q1 FY2026-27'),                             // the right figure under another period
    `${third.row.split(': ')[0]} reported ${third.row.split(': ')[1].toLowerCase()} of INR ${third.value * 10} million in ${third.column}.`, // restated in millions
    `The ${fourth.row.split(': ')[0].split(' ')[0]} unit grew in ${fourth.column}.`,  // names the row and period, states no figure
  ] })).scenarios[0];
  const outcome = (cell) => graded.cells.find((item) => item.row === cell.row && item.column === cell.column).outcome;
  assert.deepEqual([outcome(first), outcome(second), outcome(third), outcome(fourth)], ['wrong', second.column === 'Q1 FY2026-27' ? 'found' : 'missing', 'found', 'missing']);
  assert.deepEqual([graded.found, graded.wrong, graded.missing], [1, 1, 34]);
  assert.match(graded.cells.find((item) => item.outcome === 'wrong').proposition, /Coimbatore/u);
  // A claim from another Source, a withdrawn claim and an unknown scenario never count.
  const withdrawn = trace({ [uk.id]: [say(uk.cells[0])] });
  withdrawn.at(-1).before_chat.records.at(-1).snapshot.payload.claims[0].status = 'withdrawn';
  assert.equal(gradeLongTables(suite, withdrawn).found, 0);
  assert.throws(() => gradeLongTables(suite, [{ suite: suite.suite, scenario_id: 'OTHER' }]), /must name the suite and its scenarios/u);
});

test('a claim that leaves the division to its endpoint still names the row', () => {
  // The World loop's UK trial (v018x-b long.rows): the subject is only the node.
  const proposition = 'Unaudited Adjusted operating profit (£m): H1 2026/27, the six months ended 30 September 2026, £3.6 million; '
    + 'H1 2025/26, £4.2 million; H2 2025/26, the six months ended 31 March, £4.1 million; FY 2025/26, £8.3 million; FY 2024/25, £8.5 million.';
  const rows = trace({ [uk.id]: [proposition] });
  const payload = rows.at(-1).before_chat.records.at(-1).snapshot.payload;
  const outcome = () => gradeLongTables(suite, rows).scenarios.find((item) => item.scenario_id === uk.id).cells
    .find((cell) => cell.row === 'Kennet Filtration: Adjusted operating profit' && cell.column === 'H1 2026/27').outcome;
  assert.equal(outcome(), 'missing');
  payload.nodes.push({ id: 'kennet', name: 'Kennet Filtration', aliases: [] });
  payload.claims[0].endpoints = [{ nodeId: 'kennet', role: 'source' }, { nodeId: 'kennet', role: 'target' }];
  assert.equal(outcome(), 'found');
  // Another division's node does not name this row.
  payload.nodes[0].name = 'Medway Valves';
  assert.equal(outcome(), 'missing');
});

test('an incomplete trial has no counts, and the command grades two arms side by side', async (t) => {
  const all = Object.fromEntries(suite.scenarios.map((scenario) => [scenario.id, scenario.cells.map((cell) => say(cell))]));
  const failed = gradeLongTables(suite, trace(all, { status: 'failed' }));
  assert.deepEqual([failed.found, failed.wrong, failed.missing, failed.scenarios[0].complete], [null, null, null, false]);

  const root = await mkdtemp(join(tmpdir(), 'long-tables-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const write = async (name, rows) => { await writeFile(join(root, name), `${rows.map((row) => JSON.stringify(row)).join('\n')}\n`); return join(root, name); };
  const one = await write('one.jsonl', trace({ [india.id]: india.cells.slice(0, 9).map((cell) => say(cell)) }));
  const two = await write('two.jsonl', trace(all));
  const out = join(root, 'report.json');
  const script = fileURLToPath(new URL('./long-tables-grade.mjs', import.meta.url));
  const suitePath = fileURLToPath(new URL('./smoke-long-tables-v1.json', import.meta.url));
  const run = spawnSync(process.execPath, [script, '--suite', suitePath, '--arm', `chunker1=${one}`, '--arm', `chunker2=${two}`, '--out', out], { encoding: 'utf8' });
  assert.equal(run.status, 0, run.stderr);
  const report = JSON.parse(await readFile(out, 'utf8'));
  assert.deepEqual(report.arms.map((arm) => [arm.arm, arm.found, arm.missing]), [['chunker1', 9, 63], ['chunker2', 72, 0]]);
  // A report is never overwritten.
  assert.equal(spawnSync(process.execPath, [script, '--suite', suitePath, '--arm', `a=${one}`, '--out', out], { encoding: 'utf8' }).status, 1);
});
