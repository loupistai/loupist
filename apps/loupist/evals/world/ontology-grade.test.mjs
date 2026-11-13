import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { entityMatchesNode, gradeOntology, isMeasurementNode, normalizeOriginal, patternHits, sha256 } from './ontology-grade.mjs';

const root = new URL('.', import.meta.url);

test('measurement node detector keeps numbered entities in English and Korean', async () => {
  const corpus = JSON.parse(await readFile(new URL('./entity-name-corpus-v1.json', root), 'utf8'));
  for (const row of corpus) {
    assert.equal(isMeasurementNode(row), row.measurement, `${row.kind}: ${row.name}`);
  }
});

test('measurement share counts all node kinds, pools by node count, and keeps missing data unknown', () => {
  const trace = rows({
    nodes: [node('a', 'Acme'), node('m', 'Acme revenue USD 42 million', { kind: 'CustomObservation' })],
    claims: [], enResult: miss, koResult: miss,
  });
  const first = grade(trace);
  assert.equal(trialOf(first).metrics.measurement_node_share, 0.5);
  assert.deepEqual(first.arms[0].pooled.counts.measurement_nodes, [1, 2]);
  const second = rows({ trial: 1, nodes: [node('b', 'HBM4', { kind: 'Product' })], claims: [], enResult: miss, koResult: miss });
  assert.equal(grade([...trace, ...second]).arms[0].pooled.measurement_node_share, 1 / 3);
  assert.equal(grade(rows({ nodes: [], claims: [], enResult: miss, koResult: miss })).arms[0].pooled.measurement_node_share, 0);
  assert.equal(grade(rows({ nodes: [], claims: [], enResult: miss, koResult: miss, omitSnapshot: true })).arms[0].pooled.measurement_node_share, null);
});

test('fact language distinguishes English bilingual labels and aliases from non-English prose', () => {
  const corpus = [
    [false, 'Acme reported operating profit of KRW 8.7 billion.'],
    [false, 'Acme reported operating profit (영업이익) of KRW 8.7 billion.'],
    [false, 'Acme reported operating profit ( 영업이익 ) of KRW 8.7 billion.'],
    [false, 'Acme reported 영업이익 (operating profit) of KRW 8.7 billion.'],
    [false, 'Acme reported adjusted operating profit （조정 영업이익） this quarter.'],
    [false, 'Acme reported "영업이익" (operating profit) this quarter.'],
    [false, 'Acme reported operating profit “영업이익” this quarter.'],
    [false, "Acme reported '조정 영업이익' alongside its operating profit."],
    [false, 'Acme reported 영업이익 / operating profit this quarter.'],
    [false, 'Acme reported operating profit / 영업이익 this quarter.'],
    [false, 'Acme (아주 긴 한국어 회사 이름 주식회사) grew.'],
    [false, '아주긴한국어회사이름주식회사 (Acme) reported revenue.'],
    [false, 'Acme “아주긴한국어회사이름주식회사” reported revenue.'],
    [false, 'Acme / 아주긴한국어회사이름주식회사 reported revenue.'],
    [false, '아주긴한국어회사이름주식회사 / Acme reported revenue.'],
    [false, 'Acme reported 营业利润 (operating profit) this quarter.'],
    [false, 'Acme (Акме) reported profit this quarter.'],
    [false, 'Acme (다다) reported profit this quarter.'],
    [false, 'Acme reported annual production capacity (연산 능력) of 40,000 tons.'],
    [false, 'Acme reported 영업이익 (operating profit), not adjusted operating profit.'],
    [true, '아크메는 이번 분기에 영업이익 87억 원을 보고했다.'],
    [true, 'Acme는 Beta에 전기차용 배터리를 공급한다고 발표했다.'],
    [true, 'Acme reported revenue, 하지만 영업이익은 전년 대비 크게 늘었다.'],
    [true, 'Acme (아크메) reported revenue, 하지만 영업이익은 전년 대비 크게 늘었다.'],
    [true, '아크메는 영업이익 (operating profit)이 전년보다 늘었다고 보고했다.'],
    [true, 'Acme 영업이익은 늘었다 (operating profit rose).'],
    [true, '"아크메는 영업이익이 크게 늘었다고 보고했다."'],
    [true, 'Acme said "아크메는 영업이익이 크게 늘었다고 보고했다."'],
    [true, 'Acme said "아크메는 영업이익이 크게 늘었다고 보고했다"'],
    [true, 'Acme said "영업이익이 증가했다"'],
    [true, 'Acme said " 영업이익이 증가했다 "'],
    [true, 'Acme (이번 분기에는 영업이익이 크게 늘었다고 보고했다) reported results.'],
    [true, 'Acme reported revenue (하지만 영업이익은 전년 대비 크게 늘었다.), increasing profit.'],
    [true, 'Acme는 매출 / 영업이익을 구분해서 공시했다.'],
    [true, 'Компания Акме сообщила о росте операционной прибыли.'],
    [true, '该公司报告本季度营业利润增长。'],
  ];
  for (const [nonEnglish, proposition] of corpus) {
    const report = grade(rows({
      nodes: [node('a', 'Acme')],
      claims: [claim('c', 'REPORTS', 'a', 'a', proposition)],
      enResult: miss, koResult: miss,
    }));
    assert.equal(trialOf(report).metrics.non_english_fact_share, Number(nonEnglish), proposition);
  }
});

test('fact language pools claim counts after excluding bilingual annotations', () => {
  const trace = rows({
    nodes: [node('a', 'Acme')],
    claims: [
      claim('en', 'REPORTS', 'a', 'a', 'Acme reported 영업이익 (operating profit) this quarter.'),
      claim('ko', 'REPORTS', 'a', 'a', '아크메는 이번 분기에 영업이익을 보고했다.'),
    ],
    enResult: miss, koResult: miss,
  });
  assert.deepEqual(trialOf(grade(trace)).metrics.counts.non_english_facts, [1, 2]);
  const second = rows({
    trial: 1, nodes: [node('a', 'Acme')],
    claims: [claim('alias', 'REPORTS', 'a', 'a', 'Acme (아크메) reported revenue.')],
    enResult: miss, koResult: miss,
  });
  assert.equal(grade([...trace, ...second]).arms[0].pooled.non_english_fact_share, 1 / 3);
});

function suiteFor(id = 'CELL') {
  return {
    schema: 'loupist-world-suite/1', suite: 'ontology-test', version: 1,
    scenarios: [{
      id, title: 'Fixture', condition: 'ontology', world_enabled: true, seed_sources: [],
      steps: [
        { id: 'ingest', sources: [{ id: 's', text: 'Acme reported 10 widgets.', origin: 'https://fixtures.invalid/world/ontology/s' }] },
        { id: 'query-en', query: 'Acme' },
        { id: 'query-ko', query: '아크메' },
      ],
    }],
  };
}

function oracleFor(suiteBytes, id = 'CELL') {
  return {
    schema: 'loupist-world-ontology-oracle/1', version: 2, input_sha256: sha256(suiteBytes),
    scenarios: [{
      id,
      entities: [
        { id: 'acme', kind: 'Organization', name_patterns: ['^Acme$', '^아크메$'], original_name: '아크메', identifier: 'ACME' },
        { id: 'beta', kind: 'Organization', name_patterns: ['^Beta$'] },
      ],
      relations: [
        { id: 'supplies', source: 'acme', target: 'beta', relation: 'SUPPLIES', directions: ['forward', 'reverse'] },
        { id: 'open-link', source: 'acme', target: 'beta', relation: 'open', directions: ['forward', 'reverse'] },
      ],
      numeric_facts: [
        { id: 'widgets', subject: 'acme', status: 'reported', value_patterns: ['(?<!\\d)10(?!\\d)'], status_patterns: ['widgets'] },
        { id: 'outlook', subject: 'acme', status: 'forecast', value_patterns: ['(?<!\\d)99(?!\\d)'], status_patterns: ['forecast', '추정'] },
      ],
      queries: [
        { step_id: 'query-en', language: 'en', target_entity_ids: ['acme'] },
        { step_id: 'query-ko', language: 'ko', target_entity_ids: ['acme'] },
      ],
    }],
  };
}

function node(id, name, extra = {}) {
  return { id, name, aliases: extra.aliases ?? [], description: '', kind: extra.kind ?? 'Organization', sourceRole: 'entity', properties: extra.properties ?? {} };
}

function claim(id, relation, from, to, proposition) {
  return {
    id, proposition, relation, attribution: 'publisher', status: 'active', qualifiers: [], evidence: [],
    endpoints: [{ nodeId: from, role: 'source' }, { nodeId: to, role: 'target' }],
  };
}

function rows({ id = 'CELL', trial = 0, nodes, claims, usage, enResult, koResult, omitSnapshot = false }) {
  const payload = { nodes, claims, annotations: [] };
  const snapshot = omitSnapshot ? { records: [] } : { records: [{ snapshot: { kind: 'world', payload } }] };
  const base = { scenario_id: id, trial, attempt_id: `${id}-${trial}`, status: 'completed' };
  const inference = usage === undefined ? [] : usage;
  return [
    { ...base, step_id: 'ingest', before_chat: snapshot, world_inference: inference },
    { ...base, step_id: 'query-en', before_chat: { ...snapshot, queries: [{ query: 'Acme', status: 'completed', result: enResult }] } },
    { ...base, step_id: 'query-ko', before_chat: { ...snapshot, queries: [{ query: '아크메', status: 'completed', result: koResult }] } },
  ];
}

function grade(traces, { id = 'CELL', arm = 'candidate' } = {}) {
  const suite = suiteFor(id);
  const suiteBytes = Buffer.from(JSON.stringify(suite));
  return gradeOntology({
    suite, suiteBytes, oracle: oracleFor(suiteBytes, id),
    arms: [{ arm, traces }],
  });
}

function trialOf(report) {
  return report.arms[0].scenarios[0].trials[0];
}

const acmeHit = { view: { nodes: [{ name: 'Acme supplies Beta', aliases: [] }] } };
const miss = { view: { nodes: [{ name: 'Other', aliases: [] }] } };

test('typed graph scores recall, type, alias, identifier, canonical relation, numbers, and query hits', () => {
  const report = grade(rows({
    nodes: [
      node('a', 'Acme', { aliases: [], properties: { original_name: '아크메', ticker: 'ACME' } }),
      node('b', 'Beta'),
      node('m', 'Acme forecast 99', { kind: 'Metric' }),
    ],
    claims: [claim('c', 'SUPPLIES', 'a', 'b', 'Acme reported 10 widgets')],
    enResult: acmeHit,
    koResult: { view: { nodes: [{ name: '아크메', aliases: [] }] } },
    usage: [{ call_index: 0, usage: { input: 12, output: 3 }, duration_ms: 40 }],
  }));
  const metrics = trialOf(report).metrics;
  assert.equal(metrics.entity_recall, 1);
  assert.equal(metrics.type_accuracy, 1);
  assert.equal(metrics.duplicate_entities, 0);
  assert.equal(metrics.original_alias_rate, 1);
  assert.equal(metrics.identifier_fill, 1);
  assert.equal(metrics.relation_pair_recall, 1);
  assert.equal(metrics.canonical_relation_rate, 1);
  assert.equal(metrics.relates_to_share, 0);
  assert.equal(metrics.non_english_fact_share, 0);
  assert.equal(metrics.numeric_fidelity, 1);
  assert.equal(metrics.query_hit_rate.en, 1);
  assert.equal(metrics.query_hit_rate.ko, 1);
  assert.equal(metrics.relation_names_per_100_claims, 100);
  assert.deepEqual(trialOf(report).usage, {
    world_model_calls: 1, world_input_tokens: 12, world_output_tokens: 3, world_model_duration_ms: 40,
  });
});

test('a trailing code or title is removed before the anchored name match', () => {
  const entity = { name_patterns: ['^Hanbit Battery Co\\., Ltd\\.$', '^Northgate Motors, Inc\\.$', '^Dana Whitfield$'] };
  assert.equal(entityMatchesNode(entity, { name: 'Hanbit Battery Co., Ltd. (412340)' }), true);
  assert.equal(entityMatchesNode(entity, { name: 'Hanbit Battery Co., Ltd. [412340]' }), true);
  assert.equal(entityMatchesNode(entity, { name: 'Hanbit Battery Co., Ltd. （412340）' }), true);
  assert.equal(entityMatchesNode(entity, { name: '  NORTHGATE   MOTORS,  INC.  (NASDAQ: NGTM) ' }), true);
  assert.equal(entityMatchesNode(entity, { name: 'Dana Whitfield (CEO)' }), true);
  assert.equal(entityMatchesNode(entity, { name: 'Unrelated', aliases: ['Hanbit Battery Co., Ltd. (412340)'] }), true);
  assert.equal(entityMatchesNode(entity, { name: 'Hanbit Battery Co., Ltd. (412340) (KOSDAQ)' }), false);
});

test('a trailing corporate suffix is removed on both sides of an original-name comparison', () => {
  assert.equal(normalizeOriginal('한빛배터리 주식회사'), '한빛배터리');
  assert.equal(normalizeOriginal('한빛배터리  주식회사'), '한빛배터리');
  assert.equal(normalizeOriginal('한빛배터리 (주)'), '한빛배터리');
  assert.equal(normalizeOriginal('한빛배터리(주)'), '한빛배터리');
  assert.equal(normalizeOriginal('Hanbit Battery Co., Ltd.'), 'Hanbit Battery');
  assert.equal(normalizeOriginal('Northgate Motors, Inc.'), 'Northgate Motors');
  assert.equal(normalizeOriginal('Northgate Motors Corp.'), 'Northgate Motors');
  assert.equal(normalizeOriginal('Northgate Energy LLC'), 'Northgate Energy');
  assert.equal(normalizeOriginal('한빛배터리\u3000주식회사'), '한빛배터리');

  const suite = suiteFor();
  const suiteBytes = Buffer.from(JSON.stringify(suite));
  const oracle = oracleFor(suiteBytes);
  oracle.scenarios[0].entities[0] = {
    id: 'acme', kind: 'Organization',
    name_patterns: ['^Hanbit Battery Co\\., Ltd\\.$', '^한빛배터리 주식회사$'],
    original_name: '한빛배터리 주식회사',
  };
  const report = gradeOntology({
    suite, suiteBytes, oracle,
    arms: [{
      arm: 'candidate',
      traces: rows({
        nodes: [
          node('a', 'Hanbit Battery Co., Ltd.', { properties: { original_name: '한빛배터리 주식회사' } }),
          node('b', 'Beta'),
        ],
        claims: [claim('c', 'SUPPLIES', 'a', 'b', 'Acme reported 10 widgets and a forecast of 99')],
        enResult: acmeHit,
        koResult: miss,
      }),
    }],
  });
  assert.equal(trialOf(report).metrics.original_alias_rate, 1);
  assert.equal(trialOf(report).entities.find((item) => item.id === 'acme').original_alias, true);

  const bare = gradeOntology({
    suite, suiteBytes, oracle,
    arms: [{
      arm: 'candidate',
      traces: rows({
        nodes: [
          node('a', '한빛배터리 주식회사', { properties: {} }),
          node('b', 'Beta'),
        ],
        claims: [claim('c', 'SUPPLIES', 'a', 'b', 'Acme reported 10 widgets and a forecast of 99')],
        enResult: miss,
        koResult: miss,
      }),
    }],
  });
  assert.equal(trialOf(bare).entities.find((item) => item.id === 'acme').original_alias, true);

  const other = gradeOntology({
    suite, suiteBytes, oracle,
    arms: [{
      arm: 'candidate',
      traces: rows({
        nodes: [
          node('a', 'Hanbit Battery Co., Ltd.', { properties: { original_name: '대성케미칼 주식회사' } }),
          node('b', 'Beta'),
        ],
        claims: [claim('c', 'SUPPLIES', 'a', 'b', 'Acme reported 10 widgets and a forecast of 99')],
        enResult: miss,
        koResult: miss,
      }),
    }],
  });
  assert.equal(trialOf(other).entities.find((item) => item.id === 'acme').original_alias, false);
});

test('substring Event and Metric matches do not count as duplicate entities', () => {
  const suite = suiteFor();
  const suiteBytes = Buffer.from(JSON.stringify(suite));
  const oracle = oracleFor(suiteBytes);
  oracle.scenarios[0].entities.push({ id: 'deal', kind: 'Event', name_patterns: ['supply agreement'] });
  const report = gradeOntology({
    suite, suiteBytes, oracle,
    arms: [{
      arm: 'candidate',
      traces: rows({
        nodes: [
          node('a1', 'Acme'),
          node('a2', 'Acme'),
          node('b', 'Beta'),
          node('e1', 'Northgate supply agreement', { kind: 'Event' }),
          node('e2', 'Hanbit supply agreement announced', { kind: 'Event' }),
          node('m', 'supply agreement value KRW 480 billion', { kind: 'Metric' }),
        ],
        claims: [claim('c', 'SUPPLIES', 'a1', 'b', 'Acme reported 10 widgets and a forecast of 99')],
        enResult: acmeHit,
        koResult: miss,
      }),
    }],
  });
  const metrics = trialOf(report).metrics;
  assert.equal(metrics.duplicate_entities, 1);
  assert.equal(metrics.entity_recall, 1);
  assert.equal(metrics.type_accuracy, 1);
});

test('duplicate nodes and a reversed edge are counted without losing pair recall', () => {
  const report = grade(rows({
    nodes: [
      node('a1', 'Acme', { aliases: ['아크메'], properties: { ticker: 'ACME' } }),
      node('a2', 'Acme', { properties: { ticker: 'ACME' } }),
      node('b', 'Beta'),
    ],
    claims: [claim('c', 'SUPPLIES', 'b', 'a1', 'Acme reported 10 widgets and a forecast of 99')],
    enResult: acmeHit,
    koResult: miss,
  }));
  const metrics = trialOf(report).metrics;
  assert.equal(metrics.duplicate_entities, 1);
  assert.equal(metrics.entity_recall, 1);
  assert.equal(metrics.relation_pair_recall, 1);
  assert.equal(metrics.query_hit_rate.en, 1);
  assert.equal(metrics.query_hit_rate.ko, 0);
  assert.equal(trialOf(report).relations.find((item) => item.id === 'open-link').canonical_claims, null);
});

test('untyped baseline keeps Korean names and RELATES_TO claims', () => {
  const report = grade(rows({
    nodes: [node('a', '아크메', { kind: 'Observation', properties: {} }), node('b', 'Beta', { kind: 'Entity' })],
    claims: [claim('c', 'RELATES_TO', 'a', 'b', '아크메는 10 widgets를 보고했다')],
    enResult: miss,
    koResult: { view: { nodes: [{ name: '아크메' }] } },
  }));
  const metrics = trialOf(report).metrics;
  assert.equal(metrics.entity_recall, 1);
  assert.equal(metrics.type_accuracy, 0);
  assert.equal(metrics.original_alias_rate, 1);
  assert.equal(metrics.identifier_fill, 0);
  assert.equal(metrics.relation_pair_recall, 1);
  assert.equal(metrics.canonical_relation_rate, 0);
  assert.equal(metrics.relates_to_share, 1);
  assert.equal(metrics.non_english_fact_share, 1);
  assert.equal(metrics.numeric_fidelity, 0.5);
  assert.equal(metrics.relation_names_per_100_claims, 100);
});

test('missing usage stays null and an explicit zero stays zero', () => {
  const missing = grade(rows({
    nodes: [node('a', 'Acme', { properties: { original_name: '아크메', ticker: 'ACME' } }), node('b', 'Beta')],
    claims: [claim('c', 'SUPPLIES', 'a', 'b', 'Acme reported 10 widgets and a forecast of 99')],
    enResult: acmeHit, koResult: acmeHit,
    usage: [{ call_index: 1, usage: null, duration_ms: null }],
  }));
  assert.equal(trialOf(missing).usage.world_model_calls, 1);
  assert.equal(trialOf(missing).usage.world_input_tokens, null);
  assert.equal(trialOf(missing).usage.world_output_tokens, null);
  assert.equal(trialOf(missing).usage.world_model_duration_ms, null);

  const zero = grade(rows({
    nodes: [node('a', 'Acme'), node('b', 'Beta')],
    claims: [],
    enResult: miss, koResult: miss,
    usage: [],
  }));
  assert.deepEqual(trialOf(zero).usage, {
    world_model_calls: 0, world_input_tokens: 0, world_output_tokens: 0, world_model_duration_ms: 0,
  });

  const absent = grade([
    { scenario_id: 'CELL', trial: 0, attempt_id: 'x', step_id: 'ingest', status: 'completed', before_chat: { records: [{ snapshot: { kind: 'world', payload: { nodes: [], claims: [] } } }] } },
    { scenario_id: 'CELL', trial: 0, attempt_id: 'x', step_id: 'query-en', status: 'completed', before_chat: { records: [], queries: [] } },
    { scenario_id: 'CELL', trial: 0, attempt_id: 'x', step_id: 'query-ko', status: 'completed', before_chat: { records: [], queries: [] } },
  ]);
  assert.equal(trialOf(absent).usage.world_model_calls, null);
  assert.equal(trialOf(absent).usage.world_input_tokens, null);
});

test('a missing snapshot nulls pooled quality instead of reporting zeros', () => {
  const report = grade(rows({
    nodes: [], claims: [], enResult: miss, koResult: miss, omitSnapshot: true, usage: null,
  }));
  assert.equal(trialOf(report).metrics.entity_recall, null);
  assert.equal(trialOf(report).metrics.duplicate_entities, null);
  assert.equal(report.arms[0].pooled.entity_recall, null);
  assert.equal(report.arms[0].pooled.numeric_fidelity, null);
});

test('cumulative world_inference ledgers are counted once', () => {
  const call = { call_index: 0, usage: { input_tokens: 5, output_tokens: 2 }, duration_ms: 7 };
  const later = { call_index: 1, usage: { input: 4, output: 1 }, duration_ms: 3 };
  const report = grade([
    { scenario_id: 'CELL', trial: 0, attempt_id: 'x', step_id: 'ingest', status: 'completed', world_inference: [call], before_chat: { records: [{ snapshot: { kind: 'world', payload: { nodes: [node('a', 'Acme', { properties: { original_name: '아크메', ticker: 'ACME' } }), node('b', 'Beta')], claims: [claim('c', 'SUPPLIES', 'a', 'b', '10 widgets forecast 99')] } } }] } },
    { scenario_id: 'CELL', trial: 0, attempt_id: 'x', step_id: 'query-en', status: 'completed', world_inference: [call, later], before_chat: { queries: [{ status: 'completed', result: acmeHit }], records: [] } },
    { scenario_id: 'CELL', trial: 0, attempt_id: 'x', step_id: 'query-ko', status: 'completed', world_inference: [call, later], before_chat: { queries: [{ status: 'completed', result: miss }] } },
  ]);
  assert.equal(trialOf(report).usage.world_model_calls, 2);
  assert.equal(trialOf(report).usage.world_input_tokens, 9);
  assert.equal(trialOf(report).usage.world_output_tokens, 3);
  assert.equal(trialOf(report).usage.world_model_duration_ms, 10);
});

test('adoption criteria use the pre-registered thresholds', () => {
  const pass = (arm, kind, relation, calls) => rows({
    id: 'O-MIXED',
    nodes: [node('a', 'Acme', { kind, properties: { original_name: '아크메', ticker: 'ACME' } }), node('b', 'Beta')],
    claims: [claim('c', relation, 'a', 'b', 'Acme reported 10 widgets and a forecast of 99')],
    enResult: acmeHit, koResult: acmeHit,
    usage: [{ call_index: 0, usage: { input: calls, output: 1 }, duration_ms: 1 }],
  }).map((row) => ({ ...row, scenario_id: 'O-MIXED' }));
  const suite = suiteFor('O-MIXED');
  const suiteBytes = Buffer.from(JSON.stringify(suite));
  const oracle = oracleFor(suiteBytes, 'O-MIXED');
  const accepted = gradeOntology({
    suite, suiteBytes, oracle,
    arms: [
      { arm: 'baseline', traces: pass('baseline', 'Organization', 'SUPPLIES', 100) },
      { arm: 'candidate', traces: pass('candidate', 'Organization', 'SUPPLIES', 120) },
    ],
  });
  assert.equal(accepted.adoption.type_accuracy.pass, true);
  assert.equal(accepted.adoption.duplicate_entities.pass, true);
  assert.equal(accepted.adoption.original_alias_rate.pass, true);
  assert.equal(accepted.adoption.identifier_fill.pass, true);
  assert.equal(accepted.adoption.relation_pair_recall.pass, true);
  assert.equal(accepted.adoption.relates_to_and_english.pass, true);
  assert.equal(accepted.adoption.numeric_fidelity.pass, true);
  assert.equal(accepted.adoption.world_cost.pass, true);

  const rejected = gradeOntology({
    suite, suiteBytes, oracle,
    arms: [
      { arm: 'baseline', traces: pass('baseline', 'Organization', 'SUPPLIES', 100) },
      { arm: 'candidate', traces: pass('candidate', 'Observation', 'RELATES_TO', 200) },
    ],
  });
  assert.equal(rejected.adoption.type_accuracy.pass, false);
  assert.equal(rejected.adoption.relates_to_and_english.pass, false);
  assert.equal(rejected.adoption.world_cost.pass, false);
  assert.equal(rejected.adoption.canonical_relation_rate, undefined);
});

test('frozen ontology corpus, oracle, and manifest agree', async () => {
  const corpusBytes = await readFile(new URL('./ontology-corpus-v1.json', root));
  const oracleBytes = await readFile(new URL('./ontology-oracle-v2.json', root));
  const manifest = JSON.parse(await readFile(new URL('./ontology-input-manifest-v2.json', root), 'utf8'));
  const corpus = JSON.parse(corpusBytes);
  const oracle = JSON.parse(oracleBytes);
  assert.equal(oracle.version, 2);
  assert.equal(manifest.files['ontology-corpus-v1.json'], sha256(corpusBytes));
  assert.equal(manifest.files['ontology-oracle-v2.json'], sha256(oracleBytes));
  assert.equal(oracle.input_sha256, sha256(corpusBytes));
  assert.equal(corpus.schema, 'loupist-world-suite/1');
  assert.deepEqual(corpus.scenarios.map((scenario) => scenario.id), ['O-KO-SUPPLY', 'O-EN-MARKET', 'O-MIXED']);
  for (const scenario of corpus.scenarios) {
    assert.equal(scenario.world_enabled, true);
    assert.equal(scenario.steps.some((step) => step.user != null), false);
    assert.deepEqual(scenario.steps.slice(-2).map((step) => step.query), [
      'Hanbit Battery supply agreement Northgate',
      '한빛배터리 노스게이트 공급 계약',
    ]);
    for (const step of scenario.steps) for (const source of step.sources ?? []) {
      const words = source.text.split(/\s+/u).filter(Boolean).length;
      assert.equal(words >= 150 && words <= 400, true, `${source.id} has ${words} words`);
      assert.equal(source.origin.startsWith('https://fixtures.invalid/world/ontology/'), true);
    }
  }
  const names = {
    hanbit: ['Hanbit Battery Co., Ltd.', '한빛배터리 주식회사', 'Hanbit Battery Corporation', 'Han-bit Battery', '주식회사 한빛배터리'],
    northgate: ['Northgate Motors, Inc.', '노스게이트모터스', 'Northgate Motors Corp.'],
    'northgate-energy': ['Northgate Energy LLC'],
    kim: ['Kim Seo-yeon', '김서연', 'Kim Seo Yeon'],
    park: ['Park Jun-ho', '박준호', 'Park Joon-ho'],
    dana: ['Dana Whitfield'],
    lfp: ['LFP cathode material', 'LFP 양극재', 'LFP cathode active material'],
    chungju: ['Chungju plant', '충주 공장', 'Chungju factory'],
    austin: ['Austin, Texas', 'Austin, TX'],
    policy: ['Clean Vehicle Content Rule', '청정차량 부품 규정', 'the Clean Vehicle Content Rule'],
    daeseong: ['Daeseong Chemical', '대성케미칼', 'Daesung Chemical'],
    nuri: ['Nuri Investment Securities', '누리투자증권', 'Nuri Securities'],
    lithium: ['lithium carbonate', '탄산리튬'],
    tessera: ['Tessera 200', 'the Tessera 200 index'],
    ovs: ['Office of Vehicle Standards', 'the Office of Vehicle Standards'],
    halvorsen: ['Halvorsen Research', 'Halvorsen Research Inc.'],
    usdkrw: ['USD/KRW', 'USD-KRW'],
  };
  const mixed = oracle.scenarios.find((scenario) => scenario.id === 'O-MIXED');
  for (const [id, samples] of Object.entries(names)) {
    const entity = mixed.entities.find((item) => item.id === id);
    assert.ok(entity, id);
    for (const sample of samples) assert.equal(entity.name_patterns.some((pattern) => patternHits(pattern, sample)), true, `${id} should match ${sample}`);
  }
  const hanbit = mixed.entities.find((item) => item.id === 'hanbit');
  const northgate = mixed.entities.find((item) => item.id === 'northgate');
  assert.equal(hanbit.name_patterns.some((pattern) => patternHits(pattern, 'Hanbit Battery supply agreement')), false);
  assert.equal(northgate.name_patterns.some((pattern) => patternHits(pattern, 'Northgate Energy LLC')), false);
  const lfp = mixed.entities.find((item) => item.id === 'lfp');
  assert.equal(lfp.name_patterns.some((pattern) => patternHits(pattern, 'LFP cathode active material supplier')), false);
  const required = [
    ['ko-filing', '412340'], ['ko-filing', '김서연'], ['ko-filing', 'LFP 양극재'], ['ko-filing', '4만 톤'],
    ['ko-filing', '1,234억'], ['ko-filing', '87억'], ['ko-filing', '41%'],
    ['ko-contract', '4,800억'], ['ko-contract', '청정차량 부품 규정'], ['ko-contract', '6,000억'], ['ko-contract', '박준호'],
    ['ko-competitor', '대성케미칼'], ['ko-competitor', '9,800'], ['ko-competitor', '코스닥 지수'],
    ['en-press', 'NASDAQ: NGTM'], ['en-press', 'KRW 480 billion'], ['en-press', 'Dana Whitfield'], ['en-press', 'Northgate Energy LLC'], ['en-press', 'Austin, Texas'],
    ['en-analyst', '310,000'], ['en-analyst', 'Tessera 200'],
    ['en-rule', 'Office of Vehicle Standards'], ['en-rule', '2027-01-01'], ['en-rule', 'USD 9,800'], ['en-rule', 'USD/KRW 1,382'],
  ];
  const sources = new Map(corpus.scenarios.flatMap((scenario) => scenario.steps.flatMap((step) => step.sources ?? [])).map((source) => [source.id, source.text]));
  for (const [id, phrase] of required) assert.equal(sources.get(id).includes(phrase), true, `${id} missing ${phrase}`);
});

test('the oracle separates the KOSDAQ market from the KOSDAQ index', async () => {
  const corpusBytes = await readFile(new URL('./ontology-corpus-v1.json', root));
  const oracleBytes = await readFile(new URL('./ontology-oracle-v2.json', root));
  const oracle = JSON.parse(oracleBytes);
  const marketHits = ['KOSDAQ', 'KOSDAQ market', '코스닥', '코스닥 시장'];
  const indexHits = ['KOSDAQ index', 'KOSDAQ composite', 'KOSDAQ composite index', '코스닥 지수'];
  const marketMisses = ['KOSDAQ index', 'KOSDAQ composite', '코스닥 지수', 'KOSDAQ listing'];
  const indexMisses = ['KOSDAQ', 'KOSDAQ market', '코스닥', '코스닥 시장', 'KOSDAQ listing'];
  for (const scenario of oracle.scenarios.filter((item) => item.id !== 'O-EN-MARKET')) {
    const market = scenario.entities.find((item) => item.id === 'kosdaq-market');
    const index = scenario.entities.find((item) => item.id === 'kosdaq-index');
    assert.equal(scenario.entities.some((item) => item.id === 'kosdaq'), false);
    assert.equal(market.kind, 'Organization');
    assert.equal(market.original_name, '코스닥');
    assert.equal(index.kind, 'Asset');
    assert.equal(index.original_name, '코스닥 지수');
    for (const sample of marketHits) assert.equal(market.name_patterns.some((pattern) => patternHits(pattern, sample)), true, sample);
    for (const sample of indexHits) assert.equal(index.name_patterns.some((pattern) => patternHits(pattern, sample)), true, sample);
    for (const sample of marketMisses) assert.equal(market.name_patterns.some((pattern) => patternHits(pattern, sample)), false, sample);
    for (const sample of indexMisses) assert.equal(index.name_patterns.some((pattern) => patternHits(pattern, sample)), false, sample);
    const link = scenario.relations.find((item) => item.id === 'lithium-open-kosdaq-index');
    assert.equal(link.source, 'lithium');
    assert.equal(link.target, 'kosdaq-index');
    assert.equal(link.relation, 'open');
    assert.equal(scenario.relations.some((item) => item.target === 'kosdaq' || item.source === 'kosdaq'), false);
    assert.equal(scenario.numeric_facts.some((item) => item.subject === 'kosdaq'), false);
  }
  const english = oracle.scenarios.find((item) => item.id === 'O-EN-MARKET');
  assert.equal(english.entities.some((item) => item.id.startsWith('kosdaq')), false);
  const report = gradeOntology({
    suite: JSON.parse(corpusBytes),
    suiteBytes: corpusBytes,
    oracle,
    oracleBytes,
    arms: [{ arm: 'candidate', traces: [] }],
  });
  assert.equal(report.oracle_sha256, sha256(oracleBytes));
  assert.equal(report.arms[0].scenarios.length, oracle.scenarios.length);
});

test('CLI writes a report for a synthetic trace', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'ontology-grade-'));
  const suite = suiteFor();
  const suiteBytes = Buffer.from(JSON.stringify(suite));
  const suitePath = join(directory, 'suite.json');
  const oraclePath = join(directory, 'oracle.json');
  const tracePath = join(directory, 'trace.jsonl');
  const outPath = join(directory, 'report.json');
  await writeFile(suitePath, suiteBytes);
  await writeFile(oraclePath, JSON.stringify(oracleFor(suiteBytes)));
  const traces = rows({
    nodes: [node('a', 'Acme', { aliases: ['아크메'], properties: { ticker: 'ACME' } }), node('b', 'Beta')],
    claims: [claim('c', 'SUPPLIES', 'a', 'b', 'Acme reported 10 widgets and a forecast of 99')],
    enResult: acmeHit, koResult: acmeHit, usage: [{ call_index: 0, usage: { input: 1, output: 1 }, duration_ms: 1 }],
  });
  await writeFile(tracePath, `${traces.map((row) => JSON.stringify(row)).join('\n')}\n`);
  const ran = spawnSync(process.execPath, [
    new URL('./ontology-grade.mjs', root).pathname,
    '--suite', suitePath, '--oracle', oraclePath, '--out', outPath, '--arm', `candidate=${tracePath}`,
  ], { encoding: 'utf8' });
  assert.equal(ran.status, 0, ran.stderr);
  const report = JSON.parse(await readFile(outPath, 'utf8'));
  assert.equal(report.arms[0].pooled.entity_recall, 1);
  assert.equal(report.adoption, null);
});
