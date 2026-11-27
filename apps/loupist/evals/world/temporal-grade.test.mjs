import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import test from 'node:test';

import { acceptance, gradeArm, releaseAcceptance, restatedAfterEnd } from './temporal-grade.mjs';
import { gradeSmoke } from './smoke-grade.mjs';

const oracle = JSON.parse(await readFile(new URL('./temporal-oracle-v2.json', import.meta.url), 'utf8'));
const time = (value) => (value ? { precision: 'instant', value, approximate: false } : null);

function rows(scenario, claims, nodes, titles) {
  const sources = Object.entries(titles).map(([title, span]) => ({
    record: { kind: 'source', title },
    snapshot: { kind: 'source' },
    canonical_materials: [{ span_id: span }],
  }));
  return [
    { scenario_id: scenario, trial: 0, step_id: '__seed', status: 'completed' },
    {
      scenario_id: scenario,
      trial: 0,
      step_id: 's2',
      status: 'completed',
      before_chat: { records: [...sources, { record: { kind: 'world' }, snapshot: { kind: 'world', payload: { nodes, claims } } }] },
    },
  ];
}

const claim = (id, from, to, span, { invalidAt = null, expiredAt = null } = {}) => ({
  id,
  relation: 'LEADS',
  proposition: id,
  status: 'active',
  endpoints: [{ nodeId: from, role: 'source' }, { nodeId: to, role: 'target' }],
  evidence: [{ spanId: span }],
  invalidAt: time(invalidAt),
  expiredAt: time(expiredAt),
});

const ceoNodes = [
  { id: 'n1', name: 'Lena Ortiz', aliases: [] },
  { id: 'n2', name: 'Corvane Logistics', aliases: [] },
  { id: 'n3', name: 'Marcus Hale', aliases: [] },
];
const ceoTitles = {
  'Corvane Logistics names Lena Ortiz chief executive': 'spn_old',
  'Corvane Logistics changes chief executive': 'spn_new',
};

test('a superseded leader passes only when the old claim ends and the new one is active', () => {
  const superseded = rows('T-CEO', [
    claim('old', 'n1', 'n2', 'spn_old', { invalidAt: '2026-06-15T00:00:00+00:00', expiredAt: '2026-11-01T00:00:00+00:00' }),
    claim('new', 'n3', 'n2', 'spn_new'),
  ], ceoNodes, ceoTitles);
  const graded = gradeArm(oracle, superseded);
  assert.equal(graded.trials.find((trial) => trial.scenario_id === 'T-CEO').pass, true);
  const stale = rows('T-CEO', [claim('old', 'n1', 'n2', 'spn_old'), claim('new', 'n3', 'n2', 'spn_new')], ceoNodes, ceoTitles);
  assert.equal(gradeArm(oracle, stale).trials[0].pass, false);
});

test('a control fails when the disputed contract is invalidated, and acceptance compares arms', () => {
  const nodes = [
    { id: 'o', name: 'Orion Ferry Works', aliases: [] },
    { id: 'p', name: 'Pell Shipyards', aliases: [] },
  ];
  const titles = { 'Orion Ferry Works signs Pell Shipyards contract': 'spn_contract' };
  const kept = gradeArm(oracle, rows('T-DISPUTE', [claim('c', 'o', 'p', 'spn_contract')], nodes, titles));
  const broken = gradeArm(oracle, rows('T-DISPUTE', [claim('c', 'o', 'p', 'spn_contract', { expiredAt: '2026-11-01T00:00:00+00:00' })], nodes, titles));
  assert.equal(kept.pooled.control_false_invalidations, 0);
  assert.equal(broken.pooled.control_false_invalidations, 1);
  assert.equal(acceptance(kept, broken).holds, false);
  assert.equal(acceptance(broken, kept).holds, true);
});

test('a retried trial scores only its last attempt', () => {
  const failed = [
    { scenario_id: 'T-CEO', trial: 0, attempt_id: 'first', step_id: 's1', status: 'failed' },
    { scenario_id: 'T-CEO', trial: 0, attempt_id: 'first', step_id: 's2', status: 'skipped' },
  ];
  const retried = rows('T-CEO', [
    claim('old', 'n1', 'n2', 'spn_old', { invalidAt: '2026-06-15T00:00:00+00:00' }),
    claim('new', 'n3', 'n2', 'spn_new'),
  ], ceoNodes, ceoTitles).map((row) => ({ ...row, attempt_id: 'retry' }));
  const graded = gradeArm(oracle, [...failed, ...retried]);
  assert.equal(graded.trials.length, 1);
  assert.equal(graded.trials[0].pass, true);
  assert.deepEqual(graded.trials[0].unscored_attempt_ids, ['first']);
  assert.equal(graded.pooled.incomplete, 0);
});


const oracleV3 = JSON.parse(await readFile(new URL('./temporal-oracle-v3.json', import.meta.url), 'utf8'));
const corpusV3 = JSON.parse(await readFile(new URL('./temporal-corpus-v3.json', import.meta.url), 'utf8'));
const timeDate = (value, approximate = false) => ({ precision: 'date', value, approximate });

const facts = {
  'T-CEO-DATE': ['Lena Ortiz is CEO.', 'Marcus Hale is CEO.'],
  'T-GUIDE': ['Forecast production is 14,200 units.', 'Forecast production is 16,800 units.'],
  'T-RESTATE': ['Q2 output was 2,640 pumps.', 'Q2 output was 2,460 pumps.'],
  'T-DUP': ['Priya Desai is CFO.', 'Owen Bell is CFO.'],
  'T-PERIOD': ['Second-quarter revenue is 12 million crowns.', 'Third-quarter revenue is 15 million crowns.',
    'Forecast FY2026 output is 39,600 reels.', 'Actual FY2026 output is 41,300 reels.'],
  'T-DISPUTE': ['Orion supplies Pell under its contract.'],
  'T-SAMEDAY': ['The centre is in Kestle Bay.', 'The centre is in Amber Quay.', 'Tessa Bell is chair.', 'Ian Crowe is chair.'],
  'T-RECEIVED': ['Iris Chen is operations director.', 'Noah Aziz is operations director.'],
  'T-FUTURE-END': ['Mae Salter is CFO.'],
  'T-LATE-NOTICE': ['Kai Venn is CFO.'],
  'T-JP-FY': ['2027年3月期の連結売上高予想は4,200億円。', '2027年3月期の連結売上高予想は3,900億円。'],
  'T-DE-CFO': ['Jonas Brandt ist Finanzvorstand.', 'Miriam Stolz ist Finanzvorstand.'],
  'T-IN-RESTATE': ['Revenue from operations was ₹1,284.50 crore.', 'Restated revenue from operations was ₹1,248.50 crore.'],
  'T-ZH-PERIOD': ['2026年第一季度营业收入18.6亿元。', '2026年第二季度营业收入21.3亿元。'],
  'T-AMBIGUOUS-DATE': ['Dara Whitlock is finance director.', 'Owen Tarrant is finance director.'],
  'T-RESTATED': ['Ines Calloway is head of finance.', 'Rufus Dane is head of finance.'],
};

async function trialProfile(t) {
  const workspace = await mkdtemp(join(tmpdir(), 'temporal-grade-'));
  await mkdir(join(workspace, '.loupist'));
  const database = new DatabaseSync(join(workspace, '.loupist', 'loupist.sqlite3'));
  t.after(async () => {
    database.close();
    await rm(workspace, { recursive: true, force: true });
  });
  database.exec(`
    CREATE TABLE knowledge_lens_metadata (record_id TEXT, loupist_id TEXT, title TEXT, kind TEXT, archived_at_ms INTEGER);
    CREATE TABLE knowledge_record_heads (record_id TEXT, revision_id TEXT);
    CREATE TABLE knowledge_record_revisions (revision_id TEXT, record_id TEXT, payload_json TEXT);
  `);
  return { workspace, database };
}

function persistLens(profile, title, owner, oldId) {
  const recordId = `page-${owner}`;
  profile.database.prepare('INSERT INTO knowledge_lens_metadata VALUES (?, ?, ?, ?, NULL)').run(recordId, owner, title, 'subject');
  const save = profile.database.prepare('INSERT INTO knowledge_record_revisions VALUES (?, ?, ?)');
  for (const kind of ['superseded', 'current']) {
    save.run(`${recordId}-${kind}`, recordId, JSON.stringify({
      sections: [{ key: 'current-view', state: { kind }, basis: { refs: [{ kind: 'claim', claimId: oldId }] } }],
    }));
  }
  profile.database.prepare('INSERT INTO knowledge_record_heads VALUES (?, ?)').run(recordId, `${recordId}-superseded`);
  return (kind) => profile.database.prepare('UPDATE knowledge_record_heads SET revision_id = ? WHERE record_id = ?')
    .run(`${recordId}-${kind}`, recordId);
}

function attempt(expected, profile, corpus = corpusV3) {
  const id = expected.id;
  const sources = Object.fromEntries(expected.required_sources.map((title, index) => [title, `span-${index}`]));
  const propositions = facts[id] ?? facts['T-DUP'];
  const nodes = new Map();
  const claims = [];
  const build = (selector) => {
    const endpoints = (selector.between ?? []).map((pattern) => {
      // The anchored pattern without its optional groups is a name the pattern accepts.
      const name = pattern.slice(1, -1).replace(/\(\?:[^)]*\)\?/gu, '');
      if (!nodes.has(name)) nodes.set(name, { id: `n${nodes.size}`, name });
      return { nodeId: nodes.get(name).id };
    });
    const claim = { id: `${id}-${claims.length}`, status: 'active', proposition: propositions[claims.length],
      endpoints, evidence: [{ spanId: sources[selector.source_title] }], qualifiers: [] };
    claims.push(claim);
    return claim;
  };
  const pairs = [expected, ...(expected.additional_pairs ?? [])].map((item) => {
    const old = build(item.old);
    const fresh = item.new ? build(item.new) : null;
    if (item.end) {
      old.validAt = item.old_start;
      if (fresh) fresh.validAt = item.new_start;
      old.invalidAt = timeDate(item.end.value, item.end.basis === 'reference');
      old.expiredAt = time(`2026-11-${String(expected.required_sources.indexOf(item.old.source_title) + 1).padStart(2, '0')}T12:00:00Z`);
      old.qualifiers.push({ key: 'end_basis', value: item.end.basis },
        { key: 'superseded_by_span', value: sources[item.witness_source] });
      if (fresh) old.qualifiers.push({ key: 'superseded_by_claim', value: fresh.id });
    }
    if (item.support) {
      old.evidence.push({ spanId: sources[item.support.source_title] });
      old.publicationTime = timeDate('2026-01-16');
    }
    if (item.conflict) {
      old.qualifiers.push({ key: 'conflicts_with_claim', value: fresh.id });
      fresh.qualifiers.push({ key: 'conflicts_with_claim', value: old.id });
    }
    return { old, fresh };
  });
  const observed = rows(id, claims, [...nodes.values()], sources).at(-1).before_chat;
  const old = pairs[0].old;
  const setState = expected.lens
    ? persistLens(profile, expected.lens.title, id, pairs[expected.end ? 0 : 1].old.id) : null;
  const as_of_views = Object.fromEntries((expected.as_of ?? []).map(({ date, ended }) => [date, { edges: [{
    claim: { ...structuredClone(old), expiredAt: null, ...(ended ? {} : { invalidAt: null, qualifiers: [] }) },
  }] }]));
  const records = ['__setup', '__seed', ...expected.steps].map((step_id) => ({
    suite: corpus.suite, scenario_id: id, trial: 0, attempt_id: id,
    status: 'completed', step_id, loupist_id: id, workspace: profile.workspace,
    ...(step_id === expected.steps.at(-1) ? { before_chat: observed, as_of_views } : {}),
  }));
  return { records, pairs, setState, views: as_of_views };
}

const result = (graded, id) => graded.trials.find((item) => item.scenario_id === id);

test('v3 fits one trial with twelve scenarios and the duplicate arms change only the ingestion order', () => {
  assert.equal(oracleV3.trials, 1);
  assert.equal(corpusV3.scenarios.length, 12);
  assert.deepEqual(corpusV3.scenarios.map(({ id }) => id), oracleV3.scenarios.map(({ id }) => id));
  const arms = oracleV3.order_arms.map((id) => corpusV3.scenarios.find((item) => item.id === id));
  const sources = (arm) => arm.steps.flatMap((step) => step.sources);
  const sorted = (arm) => sources(arm).toSorted((left, right) => left.id.localeCompare(right.id));
  for (const arm of arms) {
    assert.deepEqual(sorted(arm), sorted(arms[0]));
    assert.equal(arm.steps.find(({ id }) => id === arm.pre_file.after_step).sources[0].id, 'dup-jan');
  }
  assert.deepEqual(arms.map((arm) => sources(arm).map(({ id }) => id)), [
    ['dup-jan', 'dup-mar', 'dup-feb'], ['dup-mar', 'dup-jan', 'dup-feb'], ['dup-mar', 'dup-feb', 'dup-jan'],
  ]);
});

for (const expected of oracleV3.scenarios) {
  test(`${expected.id} accepts the intended observations and rejects wrong outcomes`, async (t) => {
    const profile = await trialProfile(t);
    const { records, pairs, setState } = attempt(expected, profile);
    const measure = () => result(gradeArm(oracleV3, records), expected.id);
    assert.equal(measure().pass, true);
    for (const { old, fresh } of pairs) {
      const prior = structuredClone(old);
      if (old.invalidAt) {
        for (const violate of [
          () => { old.invalidAt = null; },
          () => { old.invalidAt.value = '2026-01-01'; },
          () => { old.invalidAt.precision = 'instant'; },
          () => { old.invalidAt.approximate = !prior.invalidAt.approximate; },
          () => { old.qualifiers[0].value = 'wrong'; },
          () => { old.qualifiers[1].value = 'unrelated'; },
          () => { old.status = 'withdrawn'; },
        ]) {
          violate();
          assert.equal(measure().pass, false, JSON.stringify(old));
          Object.assign(old, structuredClone(prior));
        }
      } else {
        old.expiredAt = time('2026-11-01T00:00:00Z');
        assert.equal(measure().pass, false);
        delete old.expiredAt;
      }
      if (fresh) {
        fresh.invalidAt = timeDate('2026-10-17', true);
        assert.equal(measure().pass, false);
        delete fresh.invalidAt;
      }
      old.evidence = [];
      assert.equal(measure().pass, false);
      Object.assign(old, prior);
    }
    if (setState) {
      setState('current');
      assert.equal(measure().pass, false, 'the persisted head matters even if an old revision was Superseded');
    }
  });
}

test('both directions of the conflict must be present, including when a different pair legitimately ends', async (t) => {
  const profile = await trialProfile(t);
  const expected = oracleV3.scenarios.find(({ id }) => id === 'T-SAMEDAY');
  const { records, pairs } = attempt(expected, profile);
  for (const claim of [pairs[0].old, pairs[0].fresh]) {
    const link = claim.qualifiers.pop();
    const measured = result(gradeArm(oracleV3, records), expected.id);
    assert.equal(measured.pairs[0].conflict, false);
    assert.equal(measured.pairs[1].ending, true);
    assert.equal(measured.pass, false);
    claim.qualifiers.push(link);
  }
});

test('release gate requires all three orders and the earliest qualifying publication', async (t) => {
  const profile = await trialProfile(t);
  const attempts = oracleV3.scenarios.map((expected) => attempt(expected, profile));
  const records = attempts.flatMap((item) => item.records);
  const grade = () => gradeArm(oracleV3, records);
  const arm = attempts.find(({ records }) => records[0].scenario_id === 'T-DUP-MARCH-FIRST');
  const { old } = arm.pairs[0];
  assert.deepEqual(grade().order_arms, [{ trial: 0, equal: true }]);
  assert.equal(releaseAcceptance(grade()).holds, true);
  assert.deepEqual(old.invalidAt, timeDate('2026-02-13', true));
  old.publicationTime = timeDate('2026-03-18');
  assert.equal(result(grade(), 'T-DUP-MARCH-FIRST').pairs[0].support, false);
  assert.equal(releaseAcceptance(grade()).holds, false);
  old.publicationTime = timeDate('2026-01-16');
  old.invalidAt = timeDate('2026-03-18', true);
  assert.deepEqual(grade().order_arms, [{ trial: 0, equal: false }]);
  old.invalidAt = timeDate('2026-02-13', true);

  const partial = arm.records.slice(0,-1).map((row) => ({ ...row, attempt_id: 'retry' }));
  const retried = gradeArm(oracleV3, [...records, ...partial]);
  assert.equal(retried.pooled.incomplete, 1);
  assert.equal(releaseAcceptance(retried).holds, false);
});

for (const id of ['T-FUTURE-END', 'T-LATE-NOTICE']) {
  test(`${id} observes an open April read and a later ended read without leaking end metadata`, async (t) => {
    const profile = await trialProfile(t);
    const expected = oracleV3.scenarios.find((item) => item.id === id);
    const { records, pairs, views } = attempt(expected, profile);
    const april = views['2026-04-01'].edges[0];
    const measure = () => result(gradeArm(oracleV3, records), id);
    assert.equal(measure().pass, true);
    april.ended = true;
    assert.equal(measure().pass, false);
    delete april.ended;
    if (id === 'T-LATE-NOTICE') {
      april.claim.evidence.push({ spanId: 'span-1' });
      assert.equal(measure().pass, false, 'the May witness itself cannot leak into April');
      april.claim.evidence.pop();
    }
    april.claim.qualifiers = [{ key: 'superseded_by_span', value: 'span-1' }];
    assert.equal(measure().pass, false, 'a hidden end must also hide its witness');
    april.claim = pairs[0].old;
    assert.equal(measure().pass, false, 'the final snapshot cannot stand in for the April read');
    delete views['2026-04-01'];
    assert.equal(measure().complete, false);
  });
}

test('the smoke gate requires attributable claims and usage for each Source and typed failures', async () => {
  const suite = JSON.parse(await readFile(new URL('./smoke-v018.json', import.meta.url), 'utf8'));
  assert.equal(suite.scenarios.length, 3);
  const trace = suite.scenarios.flatMap((scenario) => {
    const [fixture] = scenario.steps[0].sources;
    const common = { suite: suite.suite, scenario_id: scenario.id, trial: 0, attempt_id: scenario.id, status: 'completed' };
    return [
      { ...common, step_id: '__setup' },
      { ...common, step_id: '__seed' },
      { ...common, step_id: 's1', before_chat: { records: [
        { record: { kind: 'source', title: fixture.title }, canonical_materials: [{ span_id: 'source-span', canonical_text: fixture.text }] },
        { snapshot: { kind: 'world', payload: { nodes: [{ id: 'node' }],
          claims: [{ status: 'active', evidence: [{ spanId: 'source-span' }], endpoints: [{ nodeId: 'node' }] }] } } },
      ] }, world_inference: [{ call_index: 0, disposition: 'completed', usage: { input: 50, output: 10 } }] },
    ];
  });
  const grade = () => gradeSmoke(suite, trace);
  assert.equal(grade().pass, true);
  const stage = trace.at(-1);
  const material = stage.before_chat.records[0].canonical_materials[0];
  material.span_id = 'unrelated';
  assert.equal(grade().pass, false);
  assert.equal(grade().scenarios.at(-1).claims, 0);
  material.span_id = 'source-span';
  stage.world_inference[0].usage = null;
  assert.equal(grade().pass, false);
  stage.world_inference[0].usage = { input: 50, output: 10 };
  stage.world_inference.push({ disposition: 'sent_failed', call_index: 1 });
  assert.equal(grade().pass, false);
  stage.world_inference[1].diagnostic = { helper: { failure_kind: 'invalid_json' } };
  assert.equal(grade().pass, true);
  stage.status = 'failed';
  stage.processing = { failures: [{ job_id: 'job' }], jobs: [{ jobId: 'job', failure: { kind: 'invalid_json' } }] };
  assert.equal(grade().scenarios.at(-1).typed_failures, true);
  assert.equal(grade().pass, false);
});

const oracleV4 = JSON.parse(await readFile(new URL('./temporal-oracle-v4.json', import.meta.url), 'utf8'));
const corpusV4 = JSON.parse(await readFile(new URL('./temporal-corpus-v4.json', import.meta.url), 'utf8'));
const ADDED_V4 = ['T-JP-FY', 'T-DE-CFO', 'T-IN-RESTATE', 'T-ZH-PERIOD', 'T-AMBIGUOUS-DATE', 'T-RESTATED'];

/** Gives every Source entry of an attempt the publication time its corpus states. */
function withPublications(records, scenarioId, corpus = corpusV4) {
  const published = new Map(corpus.scenarios.find(({ id }) => id === scenarioId).steps
    .flatMap((step) => step.sources).map((source) => [source.title, source.publication_time]));
  for (const entry of records.at(-1).before_chat.records) {
    if (entry.record.kind !== 'source') continue;
    entry.snapshot.payload = { source: { captures: [{ captureId: 'cap',
      publicationTime: { precision: 'date', value: published.get(entry.record.title), approximate: false },
      representations: [{ spans: entry.canonical_materials.map(({ span_id }) => ({ spanId: span_id })) }] }] } };
  }
  return records;
}

test('v4 keeps the twelve scenarios of v3 unchanged and adds six from other languages and number formats', () => {
  assert.deepEqual(corpusV4.scenarios.slice(0, 12), corpusV3.scenarios);
  // Apart from T-SAMEDAY, whose conflicting pair v4 selects by proposition.
  const kept = structuredClone(oracleV3.scenarios);
  const sameDay = kept.find(({ id }) => id === 'T-SAMEDAY');
  sameDay.old = { source_title: sameDay.old.source_title, proposition: 'kestle bay' };
  sameDay.new = { source_title: sameDay.new.source_title, proposition: 'amber quay' };
  assert.deepEqual(oracleV4.scenarios.slice(0, 12), kept);
  assert.deepEqual(corpusV4.scenarios.slice(12).map(({ id }) => id), ADDED_V4);
  assert.deepEqual(oracleV4.scenarios.map(({ id }) => id), corpusV4.scenarios.map(({ id }) => id));
  assert.deepEqual([corpusV4.version, corpusV4.suite, oracleV4.suite, oracleV4.version], [1, 'world-temporal-v4', 'world-temporal-v4', 4]);
  assert.deepEqual(oracleV4.order_arms, oracleV3.order_arms);
  const text = (id) => corpusV4.scenarios.find((item) => item.id === id).steps.flatMap((step) => step.sources).map((source) => source.text).join('');
  assert.match(text('T-JP-FY'), /2027年3月期[\s\S]*億円/u);
  assert.match(text('T-DE-CFO'), /Finanzvorstand/u);
  assert.match(text('T-IN-RESTATE'), /crore[\s\S]*lakh[\s\S]*FY2026-27/u);
  assert.match(text('T-ZH-PERIOD'), /亿元/u);
  // The control states no date that reads one way only.
  assert.deepEqual(text('T-AMBIGUOUS-DATE').match(/\d{2}\/\d{2}\/\d{4}/gu), ['03/04/2026', '04/03/2026']);
  assert.doesNotMatch(text('T-AMBIGUOUS-DATE'), /January|February|March|April|May|June|July|August|September|October|November|December/u);
  for (const scenario of corpusV4.scenarios.slice(12)) {
    assert.deepEqual(oracleV4.scenarios.find(({ id }) => id === scenario.id).steps, scenario.steps.map(({ id }) => id));
    for (const source of scenario.steps.flatMap((step) => step.sources)) assert.match(source.origin, /^https:\/\/fixtures\.invalid\//u);
  }
});

for (const expected of oracleV4.scenarios.filter(({ id, check }) => ADDED_V4.includes(id) && check !== 'record')) {
  test(`${expected.id} accepts the intended observations and rejects wrong outcomes`, async (t) => {
    const profile = await trialProfile(t);
    const { records, pairs } = attempt(expected, profile, corpusV4);
    const measure = () => result(gradeArm(oracleV4, records), expected.id);
    assert.equal(measure().pass, true);
    const [{ old, fresh }] = pairs;
    const prior = structuredClone(old);
    if (expected.end) {
      for (const violate of [
        () => { old.invalidAt = null; },
        () => { old.invalidAt.approximate = false; },
        () => { old.qualifiers[0].value = 'stated'; },
        () => { old.validAt = timeDate('2026-04-01'); },
      ]) {
        violate();
        assert.equal(measure().pass, false, JSON.stringify(old));
        Object.assign(old, structuredClone(prior));
      }
    } else {
      old.invalidAt = timeDate('2026-06-10', true);
      const ended = measure();
      assert.deepEqual([ended.pass, ended.unexpected_ending_ids], [false, [old.id]]);
      assert.equal(gradeArm(oracleV4, records).pooled.control_false_invalidations, 1);
      old.invalidAt = null;
    }
    fresh.invalidAt = timeDate('2026-12-01', true);
    assert.equal(measure().pass, false);
    delete fresh.invalidAt;
    assert.equal(measure().pass, true);
  });
}

test('T-AMBIGUOUS-DATE fails on a start date finer than a month, whichever way it was read', async (t) => {
  const profile = await trialProfile(t);
  const expected = oracleV4.scenarios.find(({ id }) => id === 'T-AMBIGUOUS-DATE');
  const { records, pairs } = attempt(expected, profile, corpusV4);
  const measure = () => gradeArm(oracleV4, records);
  const [{ old, fresh }] = pairs;
  old.validAt = { precision: 'month', value: '2026-03', approximate: false };
  assert.deepEqual([result(measure(), expected.id).pass, measure().pooled.invented_dates], [true, 0]);
  for (const guess of ['2026-04-03', '2026-03-04']) {
    fresh.validAt = timeDate(guess);
    const graded = measure();
    assert.deepEqual(result(graded, expected.id).date_precision, { max: 'month', pass: false, claim_ids: [fresh.id] });
    assert.deepEqual([result(graded, expected.id).pass, graded.pooled.invented_dates, graded.pooled.control_false_invalidations], [false, 1, 0]);
  }
  fresh.validAt = time('2026-04-03T00:00:00Z');
  assert.equal(result(measure(), expected.id).pass, false);
});

test('restated_after_end counts support published after the ending and gates nothing', async (t) => {
  const profile = await trialProfile(t);
  const expected = oracleV4.scenarios.find(({ id }) => id === 'T-DUP');
  const { records, pairs } = attempt(expected, profile, corpusV4);
  // Without publication times the count is unknown, and the scenario still passes.
  assert.deepEqual([result(gradeArm(oracleV4, records), 'T-DUP').restated_after_end, result(gradeArm(oracleV4, records), 'T-DUP').pass], [null, true]);
  withPublications(records, 'T-DUP');
  const graded = gradeArm(oracleV4, records);
  // January ended in February; the March report restates it.
  assert.deepEqual([result(graded, 'T-DUP').restated_after_end, graded.restated_after_end['T-DUP'], result(graded, 'T-DUP').pass], [1, 1, true]);
  assert.equal(graded.restated_after_end['T-GUIDE'], null, 'a scenario without a trial is unknown, not zero');
  // The March support removed: nothing was restated. Its gate (`support`) is a separate check.
  pairs[0].old.evidence.pop();
  assert.equal(result(gradeArm(oracleV4, records), 'T-DUP').restated_after_end, 0);
  // Support published before the witness is not a restatement after the ending.
  const payload = { claims: [{ id: 'c', status: 'active', evidence: [{ spanId: 'early' }, { spanId: 'witness' }], qualifiers: [{ key: 'superseded_by_span', value: 'witness' }] },
    { id: 'open', status: 'active', evidence: [{ spanId: 'late' }], qualifiers: [] }] };
  assert.equal(restatedAfterEnd(payload, new Map([['early', 1], ['witness', 2], ['late', 3]])), 0);
  assert.equal(restatedAfterEnd(payload, new Map([['early', 1], ['late', 3]])), null);
});

test('T-RESTATED records what followed the restatement and passes whatever it was', async (t) => {
  const profile = await trialProfile(t);
  const expected = oracleV4.scenarios.find(({ id }) => id === 'T-RESTATED');
  const { records, pairs } = attempt(expected, profile, corpusV4);
  withPublications(records, 'T-RESTATED');
  const [{ old, fresh }] = pairs;
  const [, maySpan, julySpan] = ['span-0', 'span-1', 'span-2'];
  const measure = () => result(gradeArm(oracleV4, records), 'T-RESTATED');
  assert.deepEqual([measure().pass, measure().recorded], [true, { first_ended: false, restated_as: 'absent', second_ended: false }]);
  const expectedEndings = gradeArm(oracleV4, records).pooled.invalidation_passes[1];

  // Today's rule: the first assertion ended in May and the July report supports the ended claim.
  old.invalidAt = timeDate('2026-05-07', true);
  old.qualifiers.push({ key: 'end_basis', value: 'reference' }, { key: 'superseded_by_span', value: maySpan }, { key: 'superseded_by_claim', value: fresh.id });
  old.evidence.push({ spanId: julySpan });
  let graded = gradeArm(oracleV4, records);
  assert.deepEqual(result(graded, 'T-RESTATED').recorded, { first_ended: true, restated_as: 'support_on_ended', second_ended: false });
  assert.deepEqual([result(graded, 'T-RESTATED').pass, result(graded, 'T-RESTATED').restated_after_end, graded.restated_after_end['T-RESTATED']], [true, 1, 1]);
  // A recorded scenario adds to no gated count, though its ending is not one the oracle expects.
  assert.deepEqual([graded.pooled.control_false_invalidations, graded.pooled.invalidation_passes[1]], [0, expectedEndings]);

  // A reopen rule would instead file the July report as a new claim and end the May one.
  old.evidence.pop();
  const reopened = { ...structuredClone(old), id: 'T-RESTATED-again', evidence: [{ spanId: julySpan }], invalidAt: null, qualifiers: [] };
  records.at(-1).before_chat.records.at(-1).snapshot.payload.claims.push(reopened);
  fresh.invalidAt = timeDate('2026-07-09', true);
  graded = gradeArm(oracleV4, records);
  assert.deepEqual(result(graded, 'T-RESTATED').recorded, { first_ended: true, restated_as: 'new_claim', second_ended: true });
  assert.deepEqual([result(graded, 'T-RESTATED').pass, result(graded, 'T-RESTATED').restated_after_end], [true, 0]);

  // Only an incomplete trial fails it.
  records.at(-1).status = 'failed';
  assert.deepEqual([measure().pass, measure().complete], [false, false]);
});

test('the trial count is the oracle\'s unless the caller states another', async (t) => {
  const profile = await trialProfile(t);
  const expected = oracleV4.scenarios.find(({ id }) => id === 'T-ZH-PERIOD');
  const third = attempt(expected, profile, corpusV4).records.map((row) => ({ ...row, trial: 2, attempt_id: 'third' }));
  assert.throws(() => gradeArm(oracleV4, third), /must match the suite, scenario and trial identities/u);
  const graded = gradeArm(oracleV4, third, { trials: 3 });
  assert.equal(graded.trials.filter((item) => item.scenario_id === 'T-ZH-PERIOD').length, 3);
  assert.equal(graded.trials.find((item) => item.scenario_id === 'T-ZH-PERIOD' && item.trial === 2).pass, true);
  assert.deepEqual(graded.order_arms.map(({ trial }) => trial), [0, 1, 2]);
  assert.throws(() => gradeArm(oracleV4, third, { trials: 0 }), /positive integer/u);
});

test('the global smoke has four tables from four market profiles and two prose Sources, and its grade reports what the figure check left out', async () => {
  const suite = JSON.parse(await readFile(new URL('./smoke-global-v1.json', import.meta.url), 'utf8'));
  assert.deepEqual(suite.scenarios.map(({ profile, form }) => [profile, form]), [
    ['korea', 'table'], ['germany', 'table'], ['india', 'table'], ['united_kingdom', 'table'], ['japan', 'prose'], ['brazil', 'prose'],
  ]);
  const text = Object.fromEntries(suite.scenarios.map((scenario) => [scenario.profile, scenario.steps[0].sources[0].text]));
  assert.match(text.korea, /1,234억 원[\s\S]*1조 2,400억 원/u);
  assert.match(text.germany, /Mio\. EUR[\s\S]*1\.482,6[\s\S]*Mrd\. EUR/u);
  assert.match(text.india, /₹ crore[\s\S]*₹ lakh[\s\S]*9,875\.40/u);
  assert.match(text.united_kingdom, /30 September 2026[\s\S]*£m[\s\S]*pence[\s\S]*GBX/u);
  assert.match(text.japan, /2027年3月期[\s\S]*86億5,000万円/u);
  assert.match(text.brazil, /R\$ 1,84 bilhão/u);
  for (const scenario of suite.scenarios) {
    assert.equal(scenario.steps.length, 1);
    assert.equal(scenario.steps[0].sources.length, 1);
    assert.match(scenario.steps[0].sources[0].origin, /^https:\/\/fixtures\.invalid\//u);
    assert.equal((scenario.form === 'table'), scenario.steps[0].sources[0].text.includes(' | '));
  }
  const job = (extra = {}) => ({ jobId: 'job', status: 'completed', ...extra });
  const trace = (jobOf, evidence = {}) => suite.scenarios.flatMap((scenario) => {
    const [fixture] = scenario.steps[0].sources;
    const common = { suite: suite.suite, scenario_id: scenario.id, trial: 0, attempt_id: scenario.id, status: 'completed' };
    return [
      { ...common, step_id: '__setup' },
      { ...common, step_id: '__seed' },
      { ...common, step_id: 's1', processing: { failures: [], jobs: [jobOf(scenario)] }, before_chat: { records: [
        { record: { kind: 'source', title: fixture.title }, canonical_materials: [{ span_id: 'source-span', canonical_text: fixture.text }] },
        { snapshot: { kind: 'world', payload: { nodes: [{ id: 'node' }],
          claims: [{ status: 'active', evidence: [{ spanId: 'source-span', ...evidence }], endpoints: [{ nodeId: 'node' }] }] } } },
      ] }, world_inference: [{ call_index: 0, disposition: 'completed', usage: { input: 50, output: 10 } }] },
    ];
  });
  // Receipts of a build without the figure check: the count is unknown, and the grade still passes.
  const older = gradeSmoke(suite, trace(() => job()));
  assert.deepEqual([older.pass, older.figures_left_out, older.scenarios[0].figures_left_out], [true, null, null]);
  const example = { proposition: 'Revenue was KRW 12.34 billion.', stated: 'KRW 12.34 billion', passage: '1,234억 원' };
  // The job view omits a zero count; evidence the figure check marked shows the build has it.
  const checked = gradeSmoke(suite, trace((scenario) => scenario.profile === 'korea'
    ? job({ dropped: { figureMismatch: 1 }, droppedFigures: [example] }) : job(), { figureCheck: 'matched' }));
  assert.deepEqual([checked.pass, checked.figures_left_out], [true, 1]);
  const none = gradeSmoke(suite, trace(() => job(), { figureCheck: 'rounded' }));
  assert.deepEqual([none.figures_left_out, none.scenarios.map((item) => item.figures_left_out)], [0, [0, 0, 0, 0, 0, 0]]);
  assert.deepEqual(checked.scenarios.map((item) => [item.profile, item.form, item.figures_left_out]), [
    ['korea', 'table', 1], ['germany', 'table', 0], ['india', 'table', 0], ['united_kingdom', 'table', 0], ['japan', 'prose', 0], ['brazil', 'prose', 0],
  ]);
  assert.deepEqual(checked.scenarios[0].left_out_examples, [example]);
});

for (const id of ['T-CEO-DATE', 'T-FUTURE-END', 'T-LATE-NOTICE']) {
  test(`${id}: the witness Source's own statement of the ending is not a second, unplanned ending`, async (t) => {
    const profile = await trialProfile(t);
    const expected = oracleV4.scenarios.find((item) => item.id === id);
    const { records, pairs } = attempt(expected, profile, corpusV4);
    const [{ old }] = pairs;
    const claims = records.at(-1).before_chat.records.at(-1).snapshot.payload.claims;
    const witness = old.qualifiers.find(({ key }) => key === 'superseded_by_span').value;
    // What the host keeps from "stepped down effective June 15": no start, the stated end, its own evidence as witness.
    const statement = { id: `${id}-end-statement`, status: 'active', proposition: 'The tenure ended on the stated date.',
      endpoints: structuredClone(old.endpoints), evidence: [{ spanId: witness }], validAt: null,
      invalidAt: structuredClone(old.invalidAt), expiredAt: time('2026-11-20T00:00:00Z'),
      qualifiers: [{ key: 'end_basis', value: 'stated' }, { key: 'superseded_by_span', value: witness }] };
    claims.push(statement);
    const measure = () => gradeArm(oracleV4, records);
    assert.deepEqual([result(measure(), id).pass, result(measure(), id).unexpected_ending_ids, measure().pooled.control_false_invalidations], [true, [], 0]);
    // The ended claim may name that statement, instead of the new assertion, as what replaced it.
    const replacement = old.qualifiers.find(({ key }) => key === 'superseded_by_claim');
    if (replacement) {
      const named = replacement.value;
      replacement.value = statement.id;
      assert.equal(result(measure(), id).pass, true);
      replacement.value = 'unrelated-claim';
      assert.equal(result(measure(), id).pass, false);
      replacement.value = named;
    }
    const prior = structuredClone(statement);
    for (const [why, violate] of [
      ['other entities', () => { statement.endpoints = [{ nodeId: 'elsewhere' }, { nodeId: 'someone' }]; }],
      ['a basis other than a stated end', () => { statement.qualifiers[0].value = 'reference'; }],
      ['ended by another claim', () => { statement.qualifiers.push({ key: 'superseded_by_claim', value: old.id }); }],
      ['a witness that is not its own evidence', () => { statement.evidence = [{ spanId: 'another-source-span' }]; }],
    ]) {
      violate();
      const graded = measure();
      assert.deepEqual([result(graded, id).pass, result(graded, id).unexpected_ending_ids], [false, [statement.id]], why);
      // An unexpected ending fails the scenario; only a control's is counted as a false ending.
      assert.equal(graded.pooled.control_false_invalidations, 0, why);
      Object.assign(statement, structuredClone(prior));
    }
  });
}

test('T-SAMEDAY accepts the bulletin\'s own end statement as the replacement of the earlier chair', async (t) => {
  const profile = await trialProfile(t);
  const expected = oracleV4.scenarios.find(({ id }) => id === 'T-SAMEDAY');
  const { records, pairs } = attempt(expected, profile, corpusV4);
  const { old } = pairs[1];
  const claims = records.at(-1).before_chat.records.at(-1).snapshot.payload.claims;
  const witness = old.qualifiers.find(({ key }) => key === 'superseded_by_span').value;
  const statement = { id: 'chair-end-statement', status: 'active', proposition: 'Tessa Bell was chair before Ian Crowe succeeded her.',
    endpoints: structuredClone(old.endpoints), evidence: [{ spanId: witness }], validAt: null, invalidAt: structuredClone(old.invalidAt),
    qualifiers: [{ key: 'end_basis', value: 'stated' }, { key: 'superseded_by_span', value: witness }] };
  claims.push(statement);
  old.qualifiers.find(({ key }) => key === 'superseded_by_claim').value = statement.id;
  const graded = gradeArm(oracleV4, records);
  assert.deepEqual([result(graded, 'T-SAMEDAY').pass, result(graded, 'T-SAMEDAY').pairs[1].ending, graded.pooled.control_false_invalidations], [true, true, 0]);
  // The same claim about the conflicting pair's entities would be a false ending in this control.
  records.at(-1).before_chat.records.at(-1).snapshot.payload.nodes.push(
    { id: 'eversilt', name: 'Eversilt Ceramics' }, { id: 'kestle', name: 'Kestle Bay' });
  statement.endpoints = [{ nodeId: 'eversilt' }, { nodeId: 'kestle' }];
  const wrong = gradeArm(oracleV4, records);
  assert.deepEqual([result(wrong, 'T-SAMEDAY').pass, wrong.pooled.control_false_invalidations], [false, 1]);
});

const oracleV5 = JSON.parse(await readFile(new URL('./temporal-oracle-v5.json', import.meta.url), 'utf8'));
const RESTATED_V5 = ['T-DUP', 'T-DUP-MARCH-FIRST', 'T-DUP-BACKFILL-JANUARY', 'T-RESTATED'];

test('oracle v5 changes only the four restated scenarios of v4 and grades the same corpus', () => {
  assert.deepEqual([oracleV5.version, oracleV5.suite, oracleV5.trials], [5, corpusV4.suite, 1]);
  assert.deepEqual(oracleV5.scenarios.filter(({ check }) => check === 'restated').map(({ id }) => id), RESTATED_V5);
  assert.deepEqual(oracleV5.scenarios.filter(({ id }) => !RESTATED_V5.includes(id)), oracleV4.scenarios.filter(({ id }) => !RESTATED_V5.includes(id)));
  for (const expected of oracleV5.scenarios.filter(({ check }) => check === 'restated')) {
    const sources = corpusV4.scenarios.find(({ id }) => id === expected.id).steps.flatMap((step) => step.sources);
    const restating = sources.find(({ title }) => title === expected.restatement.source_title);
    // The restating Source is the newest, and it repeats the first assertion word for word.
    assert.equal(restating.publication_time, expected.restatement.published);
    assert.equal(sources.every((source) => source.publication_time <= restating.publication_time), true);
    assert.equal(restating.text, sources.find(({ title }) => title === expected.old.source_title).text);
    for (const key of ['end', 'support', 'lens', 'witness_source']) assert.equal(Object.hasOwn(expected, key), false, `${expected.id} ${key}`);
  }
});

// A World loop trial (v018x-b): the model gave the distribution centre its own node, so
// one Source's claim joins the centre to Kestle Bay and the other's joins the company to
// Amber Quay, each linked to the other as a conflict.
const loopSameDay = {
  nodes: [
    { id: 'nod_85db', name: 'Eversilt Ceramics' },
    { id: 'nod_c37f', name: 'Eversilt Ceramics distribution centre' },
    { id: 'nod_f00b', name: 'Kestle Bay' },
    { id: 'nod_7580', name: 'Amber Quay' },
  ],
  claims: [
    ['clm_37bb', 'Channel Desk profile of Eversilt Ceramics', 'nod_c37f', 'nod_f00b',
      'Channel Desk says the Eversilt Ceramics distribution centre is in Kestle Bay.', ['clm_5c13', 'clm_ac69']],
    ['clm_5c13', 'Trade Bulletin profile of Eversilt Ceramics', 'nod_c37f', 'nod_7580',
      'Trade Bulletin says Eversilt Ceramics’ only distribution centre is in Amber Quay.', ['clm_37bb']],
    ['clm_ac69', 'Trade Bulletin profile of Eversilt Ceramics', 'nod_85db', 'nod_7580',
      'Trade Bulletin says Eversilt Ceramics operates its only distribution centre in Amber Quay.', ['clm_37bb']],
  ],
};

test('T-SAMEDAY reads the conflict by proposition when the centre is its own node', async (t) => {
  const profile = await trialProfile(t);
  const expected = oracleV4.scenarios.find(({ id }) => id === 'T-SAMEDAY');
  const { records, pairs } = attempt(expected, profile, corpusV4);
  const payload = records.at(-1).before_chat.records.at(-1).snapshot.payload;
  const span = (title) => `span-${expected.required_sources.indexOf(title)}`;
  const replaced = new Set([pairs[0].old.id, pairs[0].fresh.id]);
  payload.claims = [...payload.claims.filter(({ id }) => !replaced.has(id)),
    ...loopSameDay.claims.map(([id, source, from, to, proposition, links]) => ({
      id, relation: 'LOCATED_IN', proposition, status: 'active',
      endpoints: [{ nodeId: from, role: 'source' }, { nodeId: to, role: 'target' }],
      evidence: [{ spanId: span(source) }], validAt: null, invalidAt: null, expiredAt: null,
      qualifiers: links.map((value) => ({ key: 'conflicts_with_claim', value })),
    }))];
  payload.nodes.push(...loopSameDay.nodes);
  const byEndpoints = structuredClone(oracleV4);
  Object.assign(byEndpoints.scenarios.find(({ id }) => id === 'T-SAMEDAY'),
    { old: oracleV3.scenarios.find(({ id }) => id === 'T-SAMEDAY').old, new: oracleV3.scenarios.find(({ id }) => id === 'T-SAMEDAY').new });
  const conflict = (oracle) => {
    const { pairs: [pair], pass } = result(gradeArm(oracle, records), 'T-SAMEDAY');
    return [pair.retained, pair.conflict, pass, pair.old_claims.map(({ id }) => id), pair.new_claims.map(({ id }) => id)];
  };
  // By endpoints the Channel Desk claim is not found, and the trial read as not retained.
  assert.deepEqual(conflict(byEndpoints).slice(0, 3), [false, false, false]);
  for (const oracle of [oracleV4, oracleV5]) {
    assert.deepEqual(conflict(oracle), [true, true, true, ['clm_37bb'], ['clm_5c13', 'clm_ac69']]);
  }
  // Every selected claim must carry the link: one Amber Quay claim without it is no conflict.
  payload.claims.find(({ id }) => id === 'clm_ac69').qualifiers = [];
  assert.deepEqual(conflict(oracleV4).slice(0, 3), [true, false, false]);
});

for (const id of RESTATED_V5) {
  test(`${id} under v5 accepts a reopened first assertion or a conflict, and rejects the first ended while the second is current`, async (t) => {
    const profile = await trialProfile(t);
    const expected = oracleV5.scenarios.find((item) => item.id === id);
    const { records, pairs } = attempt(expected, profile, corpusV4);
    const [{ old: first, fresh: second }] = pairs;
    const restating = `span-${expected.required_sources.indexOf(expected.restatement.source_title)}`;
    const witness = `span-${expected.required_sources.indexOf(expected.new.source_title)}`;
    const measure = () => gradeArm(oracleV5, records);
    const outcome = () => [result(measure(), id).outcome, result(measure(), id).pass];

    // Neither ended and no link: an unreconciled pair is not one of the accepted outcomes.
    assert.deepEqual(outcome(), ['other', false]);

    // A conflict with no ending needs the link on both claims.
    first.qualifiers.push({ key: 'conflicts_with_claim', value: second.id });
    assert.deepEqual(outcome(), ['other', false]);
    second.qualifiers.push({ key: 'conflicts_with_claim', value: first.id });
    assert.deepEqual(outcome(), ['conflict', true]);
    first.qualifiers = [];
    second.qualifiers = [];

    // Today's rule before the reopen: the first assertion ended at the second's day while the second is current.
    first.invalidAt = timeDate(corpusV4.scenarios.find((item) => item.id === id).steps.flatMap((step) => step.sources)
      .find(({ title }) => title === expected.new.source_title).publication_time, true);
    first.qualifiers = [{ key: 'end_basis', value: 'reference' }, { key: 'superseded_by_span', value: witness }, { key: 'superseded_by_claim', value: second.id }];
    first.evidence.push({ spanId: restating });
    const stale = measure();
    assert.deepEqual([result(stale, id).outcome, result(stale, id).pass, result(stale, id).restatement_on_first], ['first_ended_second_current', false, true]);
    assert.equal(stale.pooled.restated_outcomes.first_ended_second_current, 1);
    assert.equal(stale.pooled.control_false_invalidations, 0);

    // The reopen rule: the first is current again with its lineage, and the second ended at the restatement's day.
    first.invalidAt = null;
    first.qualifiers = [{ key: 'reopened', value: JSON.stringify({ at: expected.restatement.published, by_span: restating }) },
      { key: 'lapsed', value: JSON.stringify([{ from: '2026-02-13', to: expected.restatement.published }]) }];
    second.invalidAt = timeDate(expected.restatement.published, true);
    second.qualifiers = [{ key: 'end_basis', value: 'reference' }, { key: 'superseded_by_span', value: restating }, { key: 'superseded_by_claim', value: first.id }];
    const reopened = measure();
    assert.deepEqual([result(reopened, id).outcome, result(reopened, id).pass, result(reopened, id).unexpected_ending_ids], ['reopened', true, []]);
    assert.deepEqual(result(reopened, id).old_claims[0].reopened, { at: expected.restatement.published, by_span: restating });
    assert.deepEqual(result(reopened, id).old_claims[0].lapsed, [{ from: '2026-02-13', to: expected.restatement.published }]);
    assert.deepEqual([reopened.pooled.restated_outcomes.reopened, reopened.pooled.reopened_claims], [1, 1]);
    // An order in which the first assertion was never ended carries no lineage and is accepted the same.
    first.qualifiers = [];
    assert.deepEqual(outcome(), ['reopened', true]);

    // The second must end at the restatement's day, approximately, on the reference basis, witnessed by the restating Source.
    const prior = structuredClone(second);
    for (const violate of [
      () => { second.invalidAt = timeDate('2026-02-13', true); },
      () => { second.invalidAt.approximate = false; },
      () => { second.qualifiers[0].value = 'stated'; },
      () => { second.qualifiers[1].value = witness; },
    ]) {
      violate();
      assert.equal(result(measure(), id).pass, false, JSON.stringify(second));
      Object.assign(second, structuredClone(prior));
    }
    // Both ended is no accepted outcome either.
    first.invalidAt = timeDate(expected.restatement.published, true);
    assert.deepEqual(outcome(), ['other', false]);
  });
}

test('under v5 the three duplicate-publication orders must still agree, and v4 still expects the earlier rule', async (t) => {
  const profile = await trialProfile(t);
  const arms = oracleV5.order_arms.map((id) => {
    const expected = oracleV5.scenarios.find((item) => item.id === id);
    const built = attempt(expected, profile, corpusV4);
    const [{ old: first, fresh: second }] = built.pairs;
    second.invalidAt = timeDate('2026-03-18', true);
    second.qualifiers = [{ key: 'end_basis', value: 'reference' }, { key: 'superseded_by_span', value: `span-${expected.required_sources.indexOf(expected.restatement.source_title)}` }];
    return { ...built, first, second };
  });
  const records = arms.flatMap((arm) => arm.records);
  assert.deepEqual(gradeArm(oracleV5, records).order_arms, [{ trial: 0, equal: true }]);
  assert.equal(gradeArm(oracleV5, records).pooled.restated_outcomes.reopened, 3);
  // One order that ends the second assertion a day later disagrees with the others.
  arms[1].second.invalidAt = timeDate('2026-03-19', true);
  assert.deepEqual(gradeArm(oracleV5, records).order_arms, [{ trial: 0, equal: false }]);
  assert.equal(releaseAcceptance(gradeArm(oracleV5, records)).holds, false);
  // The same reopened World fails oracle v4, which still expects the January claim ended in February.
  arms[1].second.invalidAt = timeDate('2026-03-18', true);
  assert.equal(result(gradeArm(oracleV4, records), 'T-DUP').pass, false);
});
