import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { sha256 } from './ontology-grade.mjs';
import { gradeSeed, seedCriteria } from './seed-grade.mjs';

const root = new URL('.', import.meta.url);
const CORE_KINDS = ['Organization', 'Person', 'Asset', 'Product', 'Place', 'Policy', 'Event', 'Metric', 'Entity', 'Company', 'Stock', 'Executive', 'Drug', 'Country'];
const LITHIUM_STEPS = ['kestrel-resource', 'kestrel-offtake', 'tamsin-lease', 'brightwater-shipment', 'kestrel-cash', 'tamsin-study', 'q-permits', 'q-offtake'];

function node(id, name, kind, aliases = []) {
  return { id, name, aliases, description: '', kind, sourceRole: 'entity', properties: {} };
}

function claim(id, from, to, proposition) {
  return {
    id, proposition, relation: 'RELATES_TO', attribution: 'publisher', status: 'active', qualifiers: [], evidence: [],
    endpoints: [{ nodeId: from, role: 'source' }, { nodeId: to, role: 'target' }],
  };
}

function world(nodes, claims) {
  return { records: [{ snapshot: { kind: 'world', payload: { nodes, claims, annotations: [] } } }] };
}

function seedType(name) {
  return {
    name, category: 'event', description: 'A recurring kind the core schema does not name well.',
    labels: { en: name, ko: name }, blurbs: { en: 'A short line for investors.', ko: '짧은 설명' }, fields: [],
  };
}

function trialRows({ trial = 0, nodes, claims, seedNames = ['ClinicalTrial'], seedStatus = 'applied', calls }) {
  const types = seedNames.map(seedType);
  const base = { scenario_id: 'S-BIOTECH', trial, attempt_id: `t-${trial}`, status: 'completed' };
  const inference = calls ?? [{ call_index: 0, disposition: 'completed', usage: { input: 100, output: 20 } }];
  return [
    {
      ...base, step_id: '__seed', world_schema: {
        mode: 'model', extension: { version: 1, types }, proposal: { status: seedStatus, types }, seed_inference: inference,
      },
      world_inference: inference,
    },
    { ...base, step_id: 'vel-204-start', before_chat: world(nodes, claims), world_inference: inference },
    { ...base, step_id: 'vel-310-license', before_chat: world(nodes, claims), world_inference: inference },
    { ...base, step_id: 'vel-101-review', before_chat: world(nodes, claims), world_inference: inference },
    { ...base, step_id: 'hp-77-phase3', before_chat: world(nodes, claims), world_inference: inference },
    { ...base, step_id: 'veltrion-runway', before_chat: world(nodes, claims), world_inference: inference },
    { ...base, step_id: 'vel-204-enrollment', before_chat: world(nodes, claims), world_inference: inference },
    { ...base, step_id: 'q-vel204', before_chat: { ...world(nodes, claims), queries: [{ status: 'completed', result: 'VEL-204 readout in Q2 2027' }] } },
    { ...base, step_id: 'q-pso', before_chat: { ...world(nodes, claims), queries: [{ status: 'completed', result: 'HP-77 Phase 3 psoriasis PASI-75' }] } },
  ];
}

function scenarioRows({ scenarioId, stepIds, trial = 0, nodes, claims, queries = {} }) {
  const base = { scenario_id: scenarioId, trial, attempt_id: `t-${trial}`, status: 'completed' };
  const inference = [{ call_index: 0, disposition: 'completed', usage: { input: 10, output: 1 } }];
  return stepIds.map((stepId) => ({
    ...base,
    step_id: stepId,
    before_chat: {
      ...world(nodes, claims),
      queries: queries[stepId] ? [{ status: 'completed', result: queries[stepId] }] : [],
    },
    world_inference: inference,
  }));
}

const typedNodes = [
  node('co', 'Veltrion Therapeutics', 'Organization'),
  node('lic', 'Orinda Bio', 'Organization'),
  node('comp', 'Halden Pharma', 'Organization'),
  node('reg', 'Office of Drug Review', 'Organization'),
  node('cfo', 'Marisa Quell', 'Person'),
  node('d204', 'VEL-204', 'Product'),
  node('d310', 'VEL-310', 'Product'),
  node('d101', 'VEL-101', 'Product'),
  node('d77', 'HP-77', 'Product'),
  node('t204', 'VEL-204 Phase 2 trial', 'ClinicalTrial'),
  node('t77', 'HP-77 Phase 3 trial', 'Trial'),
  node('pso', 'psoriasis', 'Indication'),
  node('ad', 'atopic dermatitis', 'Disease'),
  node('app', 'VEL-101 application', 'Application'),
];

const typedClaims = [
  claim('c1', 't204', 'd204', 'The Phase 2 trial studies VEL-204.'),
  claim('c2', 't77', 'd77', 'The Phase 3 trial studies HP-77.'),
  claim('c3', 'co', 't204', 'Veltrion sponsors the VEL-204 trial.'),
  claim('c4', 'comp', 't77', 'Halden sponsors the HP-77 trial.'),
  claim('c5', 'd204', 'pso', 'VEL-204 is for psoriasis.'),
  claim('c6', 'd101', 'ad', 'VEL-101 is for atopic dermatitis.'),
  claim('c7', 'd77', 'pso', 'HP-77 is for psoriasis.'),
  claim('c8', 'lic', 'd310', 'Orinda licensed VEL-310 for USD 45 million upfront.'),
  claim('c9', 'reg', 'app', 'The Office accepted the VEL-101 application with a decision date of 2027-02-14.'),
  claim('c10', 'd204', 'co', 'VEL-204 enrollment is 240 patients.'),
  claim('c11', 'd77', 'comp', 'HP-77 met its primary endpoint with PASI-75 of 68% versus 12% on placebo.'),
  claim('c12', 'co', 'cfo', 'R&D spend was USD 32.1 million.'),
];

async function loadFrozen() {
  const suiteBytes = await readFile(new URL('./seed-corpus-v2.json', root));
  const oracleBytes = await readFile(new URL('./seed-oracle-v2.json', root));
  return { suite: JSON.parse(suiteBytes), suiteBytes, oracle: JSON.parse(oracleBytes), oracleBytes };
}

async function gradeBiotech(nodes, claims = typedClaims) {
  const frozen = await loadFrozen();
  const report = gradeSeed({ ...frozen, arms: [{ arm: 'B', traces: trialRows({ nodes, claims }) }] });
  return report.arms[0].scenarios[0].trials[0];
}

function concept(id) {
  return {
    id,
    name_patterns: ['^Exact$'],
    anchor_patterns: ['Anchor'],
    cue_patterns: ['Cue'],
    kind_patterns: ['^Kind$'],
  };
}

function fixtureBundle({ concepts, entities, relations, steps = [{ id: 'ingest' }, { id: 'q' }] }) {
  const suite = {
    schema: 'loupist-world-suite/1', suite: 'fixture', version: 1,
    scenarios: [{
      id: 'S-FIX', title: 'Fixture', split: 'seed', condition: 'seed', world_enabled: true,
      research_focus: 'Fixture focus', seed: 'none', seed_sources: [], steps,
    }],
  };
  const suiteBytes = Buffer.from(JSON.stringify(suite));
  const oracle = {
    schema: 'loupist-world-seed-oracle/2', version: 2, input_sha256: sha256(suiteBytes),
    scenarios: [{
      id: 'S-FIX', entities, domain_concepts: concepts, relations, numeric_facts: [],
      queries: [{ step_id: 'q', language: 'en', target_entity_ids: [entities[0].id] }],
    }],
  };
  return { suite, suiteBytes, oracle, oracleBytes: Buffer.from(JSON.stringify(oracle)) };
}

function gradeFixture(bundle, nodes, claims) {
  const traces = scenarioRows({
    scenarioId: 'S-FIX', stepIds: ['ingest', 'q'], nodes, claims,
    queries: { q: 'Left status' },
  });
  const report = gradeSeed({ ...bundle, arms: [{ arm: 'B', traces }] });
  return report.arms[0].scenarios[0].trials[0];
}

test('typed extension kinds score the domain concepts and the seed call is counted once', async () => {
  const frozen = await loadFrozen();
  const traces = [0, 1, 2].flatMap((trial) => trialRows({ trial, nodes: typedNodes, claims: typedClaims }));
  const report = gradeSeed({ ...frozen, arms: [{ arm: 'B', traces }] });
  const trial = report.arms[0].scenarios[0].trials[0];
  assert.equal(trial.metrics.domain_typed_rate, 1);
  assert.equal(trial.metrics.domain_present_rate, 1);
  assert.equal(trial.metrics.untyped_share, 0);
  assert.equal(trial.metrics.entity_recall, 1);
  assert.equal(trial.metrics.type_accuracy, 1);
  assert.equal(trial.metrics.relation_pair_recall, 1);
  assert.equal(trial.metrics.relation_pair_recall_direct, 1);
  assert.equal(trial.metrics.duplicate_entities, 0);
  assert.equal(trial.metrics.numeric_fidelity, 1);
  assert.equal(trial.seed.domain_kind, true);
  assert.equal(trial.seed.duplicates_core, false);
  assert.equal(trial.usage.world_model_calls, 1);
  assert.equal(trial.usage.world_input_tokens, 100);
  assert.equal(trial.queries.every((query) => query.hit), true);
  const velTrial = trial.domain_concepts.find((item) => item.id === 'vel204-trial');
  const hpTrial = trial.domain_concepts.find((item) => item.id === 'hp77-trial');
  assert.deepEqual(velTrial.names, ['VEL-204 Phase 2 trial']);
  assert.deepEqual(velTrial.kinds, ['ClinicalTrial']);
  assert.deepEqual(hpTrial.names, ['HP-77 Phase 3 trial']);
  assert.deepEqual(hpTrial.kinds, ['Trial']);
});

test('an Entity-typed concept is not domain-typed, and Drug duplicates a core type', async () => {
  const frozen = await loadFrozen();
  const nodes = typedNodes.map((item) => item.id === 't204' ? { ...item, kind: 'Entity' } : item);
  const traces = trialRows({ nodes, claims: typedClaims, seedNames: ['Drug'] });
  const report = gradeSeed({ ...frozen, arms: [{ arm: 'B', traces }] });
  const trial = report.arms[0].scenarios[0].trials[0];
  assert.equal(trial.metrics.counts.domain[0], 4);
  assert.equal(trial.metrics.domain_present_rate, 1);
  assert.equal(trial.metrics.untyped_share, 1 / nodes.length);
  assert.equal(trial.seed.duplicates_core, true);
  assert.equal(trial.seed.domain_kind, false);
  assert.equal(trial.metrics.relation_pair_recall, 1);
  const velTrial = trial.domain_concepts.find((item) => item.id === 'vel204-trial');
  assert.equal(velTrial.present, true);
  assert.equal(velTrial.extension_typed, false);
});

test('instance rules accept a short name, a typed anchor, and an anchor with a cue', async () => {
  const byName = async (nodes) => gradeBiotech(nodes, []);
  const anchored = await byName([
    node('drug', 'VEL-204', 'Product'),
    node('short', 'VEL-204 trial', 'Entity'),
  ]);
  const short = anchored.domain_concepts.find((item) => item.id === 'vel204-trial');
  assert.deepEqual(short.names, ['VEL-204 trial']);
  assert.equal(short.extension_typed, false);
  assert.equal(anchored.domain_concepts.find((item) => item.id === 'hp77-trial').present, false);

  const typedAnchor = await byName([
    node('drug', 'VEL-204', 'Product'),
    node('study', 'Phase 2 VEL-204 program in adults with moderate disease', 'ClinicalTrial'),
  ]);
  const study = typedAnchor.domain_concepts.find((item) => item.id === 'vel204-trial');
  assert.deepEqual(study.names, ['Phase 2 VEL-204 program in adults with moderate disease']);
  assert.equal(study.extension_typed, true);

  const cued = await byName([
    node('sentence', 'Phase 2 VEL-204 trial in adults with moderate psoriasis', 'Entity'),
  ]);
  const sentence = cued.domain_concepts.find((item) => item.id === 'vel204-trial');
  assert.equal(sentence.present, true);
  assert.equal(sentence.extension_typed, false);
  assert.equal(cued.domain_concepts.find((item) => item.id === 'psoriasis').present, false);
});

test('a Product named with the protocol code is not a trial instance', async () => {
  const trial = await gradeBiotech([
    node('d204', 'VEL-204', 'Product'),
    node('d77', 'HP-77', 'Product'),
  ], []);
  for (const id of ['vel204-trial', 'hp77-trial']) {
    const concept = trial.domain_concepts.find((item) => item.id === id);
    assert.equal(concept.present, false);
    assert.equal(concept.matched_nodes, 0);
  }
});

test('relations join directly, through a via instance, or not at all', async () => {
  const direct = await gradeBiotech(
    [node('d204', 'VEL-204', 'Product'), node('pso', 'psoriasis', 'Indication')],
    [claim('c', 'd204', 'pso', 'VEL-204 is for psoriasis.')],
  );
  const directRelation = direct.relations.find((item) => item.id === 'vel204-psoriasis');
  assert.equal(directRelation.path, 'direct');
  assert.equal(directRelation.via_node, null);

  const trialName = 'Phase 2 VEL-204 trial in adults with moderate psoriasis';
  const hopped = await gradeBiotech(
    [
      node('d204', 'VEL-204', 'Product'),
      node('pso', 'psoriasis', 'Indication'),
      node('study', trialName, 'ClinicalTrial'),
    ],
    [
      claim('a', 'd204', 'study', 'VEL-204 is studied in the trial.'),
      claim('b', 'pso', 'study', 'The trial enrolls psoriasis patients.'),
    ],
  );
  const hop = hopped.relations.find((item) => item.id === 'vel204-psoriasis');
  assert.equal(hop.path, 'two_hop');
  assert.equal(hop.via_node, trialName);

  const stranger = await gradeBiotech(
    [
      node('d204', 'VEL-204', 'Product'),
      node('pso', 'psoriasis', 'Indication'),
      node('note', 'Notebook', 'Entity'),
    ],
    [
      claim('a', 'd204', 'note', 'A note mentions VEL-204.'),
      claim('b', 'note', 'pso', 'A note mentions psoriasis.'),
    ],
  );
  assert.equal(stranger.relations.find((item) => item.id === 'vel204-psoriasis').path, null);
});

test('two-hop rejects a via instance that is an endpoint', () => {
  const bundle = fixtureBundle({
    entities: [
      { id: 'left', kind: 'Organization', name_patterns: ['^Left$'] },
      { id: 'right', kind: 'Organization', name_patterns: ['^Right$'] },
    ],
    concepts: [{
      id: 'mid', name_patterns: ['^Middle$'], anchor_patterns: ['Middle'], kind_patterns: ['^Connector$'],
    }],
    relations: [{
      id: 'pair', source: 'left', target: 'right', via: ['mid'], directions: [],
    }],
  });
  const trial = gradeFixture(
    bundle,
    [node('l', 'Left', 'Organization'), node('r', 'Right', 'Organization', ['Middle'])],
    [claim('c', 'l', 'r', 'Left meets Right.')],
  );
  const relation = trial.relations.find((item) => item.id === 'pair');
  assert.equal(relation.path, null);
  assert.equal(relation.via_node, null);
});

test('an anchor and a cue must occur on the same name', () => {
  const bundle = fixtureBundle({
    entities: [{ id: 'left', kind: 'Organization', name_patterns: ['^Left$'] }],
    concepts: [concept('item')],
    relations: [],
  });
  const split = gradeFixture(
    bundle,
    [node('n', 'Cue only', 'Entity', ['Anchor only'])],
    [],
  );
  assert.equal(split.domain_concepts[0].present, false);
  const together = gradeFixture(
    bundle,
    [node('n', 'Anchor Cue', 'Entity')],
    [],
  );
  assert.equal(together.domain_concepts[0].present, true);
  assert.equal(together.domain_concepts[0].extension_typed, false);
});

test('also_targets counts the drug as the filing relation', async () => {
  const trial = await gradeBiotech(
    [
      node('reg', 'Office of Drug Review', 'Organization'),
      node('d101', 'VEL-101', 'Product'),
    ],
    [claim('c', 'reg', 'd101', 'The Office is reviewing VEL-101.')],
  );
  const relation = trial.relations.find((item) => item.id === 'office-reviews-vel101');
  assert.equal(relation.path, 'direct');
  assert.equal(trial.domain_concepts.find((item) => item.id === 'vel101-decision').present, false);
});

test('domain_present_rate counts untyped instances and domain_typed_rate does not', async () => {
  const trial = await gradeBiotech([
    node('study', 'Phase 2 VEL-204 program in adults with moderate disease', 'ClinicalTrial'),
    node('pso', 'Psoriasis', 'Entity'),
    node('d77', 'HP-77', 'Product'),
  ], []);
  assert.equal(trial.metrics.domain_present_rate, 2 / 5);
  assert.equal(trial.metrics.domain_typed_rate, 1 / 5);
  assert.equal(trial.domain_concepts.find((item) => item.id === 'psoriasis').extension_typed, false);
  assert.equal(trial.domain_concepts.find((item) => item.id === 'hp77-trial').present, false);
});

test('S-LITHIUM scores direct core links and via agreement or permit links', async () => {
  const frozen = await loadFrozen();
  const nodes = [
    node('k', 'Kestrel Lithium', 'Organization'),
    node('a', 'Ardell Cells', 'Organization'),
    node('b', 'Brightwater Minerals', 'Organization'),
    node('d', 'Corvane Department of Mines', 'Organization'),
    node('p', 'Dana Oyelaran', 'Person'),
    node('c', 'Corvane Province', 'Place'),
    node('project', 'Tamsin Flats', 'MiningProject'),
    node('mine', 'Lake Ostra mine', 'Mine'),
    node('lease', 'Tamsin Flats mining lease', 'MiningLease'),
    node('deal', 'Kestrel-Ardell offtake agreement', 'OfftakeAgreement'),
    node('study', 'Tamsin Flats feasibility study', 'FeasibilityStudy'),
  ];
  const claims = [
    claim('r1', 'k', 'project', 'Kestrel owns Tamsin Flats, a resource of 48 million tonnes.'),
    claim('r2', 'b', 'mine', 'Brightwater shipped 8,500 tonnes from the mine.'),
    claim('r3', 'd', 'lease', 'The department granted the lease.'),
    claim('r4', 'k', 'study', 'Kestrel presented the study.'),
    claim('r5', 'p', 'k', 'Dana reported AUD 88 million in cash.'),
    claim('r6', 'k', 'deal', 'Kestrel signed the offtake.'),
    claim('r7', 'a', 'deal', 'Ardell signed the offtake for 60,000 tonnes of spodumene concentrate.'),
    claim('r8', 'deal', 'project', 'The offtake draws on Tamsin Flats.'),
    claim('r9', 'lease', 'project', 'The lease covers Tamsin Flats.'),
    claim('r10', 'project', 'c', 'Tamsin Flats is in Corvane Province.'),
    claim('r11', 'study', 'project', 'The study covers Tamsin Flats.'),
    claim('grade', 'k', 'project', 'The resource grades 1.3% Li2O.'),
  ];
  const traces = scenarioRows({
    scenarioId: 'S-LITHIUM', stepIds: LITHIUM_STEPS, nodes, claims,
    queries: { 'q-permits': 'Tamsin Flats mining lease', 'q-offtake': 'Kestrel-Ardell offtake agreement' },
  });
  const report = gradeSeed({ ...frozen, arms: [{ arm: 'B', traces }] });
  const trial = report.arms[0].scenarios[1].trials[0];
  assert.equal(trial.metrics.entity_recall, 1);
  assert.equal(trial.metrics.type_accuracy, 1);
  assert.equal(trial.metrics.domain_typed_rate, 1);
  assert.equal(trial.metrics.domain_present_rate, 1);
  assert.equal(trial.metrics.relation_pair_recall, 1);
  assert.equal(trial.metrics.duplicate_entities, 0);
  assert.equal(trial.metrics.numeric_fidelity, 1);
  const path = (id) => trial.relations.find((item) => item.id === id);
  for (const id of ['kestrel-tamsin', 'brightwater-ostra', 'department-lease', 'kestrel-study', 'dana-kestrel']) {
    assert.equal(path(id).path, 'direct', id);
  }
  assert.equal(path('kestrel-ardell').path, 'two_hop');
  assert.equal(path('kestrel-ardell').via_node, 'Kestrel-Ardell offtake agreement');
  assert.equal(path('ardell-tamsin').path, 'two_hop');
  assert.equal(path('department-tamsin').path, 'two_hop');
  assert.equal(path('department-tamsin').via_node, 'Tamsin Flats mining lease');
  assert.equal(path('tamsin-corvane').path, 'direct');
  assert.equal(path('lease-tamsin').path, 'direct');
  assert.equal(path('study-tamsin').path, 'direct');
  assert.deepEqual(trial.domain_concepts.find((item) => item.id === 'tamsin-project').names, ['Tamsin Flats']);
  assert.deepEqual(trial.domain_concepts.find((item) => item.id === 'tamsin-lease').names, ['Tamsin Flats mining lease']);

  const viaStudy = gradeSeed({
    ...frozen,
    arms: [{
      arm: 'B',
      traces: scenarioRows({
        scenarioId: 'S-LITHIUM', stepIds: LITHIUM_STEPS,
        nodes: [nodes[0], nodes[6], nodes[10]],
        claims: [
          claim('a', 'k', 'project', 'Kestrel owns Tamsin Flats.'),
          claim('b', 'study', 'project', 'The study covers Tamsin Flats.'),
        ],
      }),
    }],
  }).arms[0].scenarios[1].trials[0];
  const studyPath = viaStudy.relations.find((item) => item.id === 'kestrel-study');
  assert.equal(studyPath.path, 'two_hop');
  assert.equal(studyPath.via_node, 'Tamsin Flats');
});

function criteriaBundle() {
  const source = (id) => ({
    id, title: 'Source', text: 'Fixture text.', origin: 'https://fixtures.invalid/world/seed/fixture',
    publication_time: '2027-01-01T00:00:00Z', acquired_at: '2027-01-01T00:00:00Z', authorship: 'publisher',
  });
  const suite = {
    schema: 'loupist-world-suite/1', suite: 'criteria', version: 1,
    scenarios: ['S1', 'S2'].map((id) => ({
      id, title: id, split: 'seed', condition: 'seed', world_enabled: true,
      research_focus: 'Fixture focus for seed criteria', seed: 'none', seed_sources: [],
      steps: [{ id: 'ingest', sources: [source(`${id}-src`)] }, { id: 'q', query: 'status' }],
    })),
  };
  const suiteBytes = Buffer.from(JSON.stringify(suite));
  const scenarioOracle = (id) => ({
    id,
    entities: [
      { id: 'org-a', kind: 'Organization', name_patterns: ['^Acme$'] },
      { id: 'org-b', kind: 'Organization', name_patterns: ['^Bravo$'] },
    ],
    domain_concepts: [
      { id: 'c1', name_patterns: ['^Widget$'], anchor_patterns: ['Widget'], cue_patterns: ['gadget'], kind_patterns: ['^Gadget$'] },
      { id: 'c2', name_patterns: ['^Bolt$'], anchor_patterns: ['Bolt'], kind_patterns: ['^Part$'] },
    ],
    relations: [
      { id: 'r1', source: 'org-a', target: 'c1', directions: ['forward', 'reverse'] },
      { id: 'r2', source: 'org-b', target: 'c2', directions: ['forward', 'reverse'] },
    ],
    numeric_facts: [{ id: 'n', subject: 'org-a', status: 'reported', value_patterns: ['(?<!\\d)48(?!\\d)'], status_patterns: ['tonne'] }],
    queries: [{ step_id: 'q', language: 'en', target_entity_ids: ['org-a'] }],
  });
  const oracle = {
    schema: 'loupist-world-seed-oracle/2', version: 2, input_sha256: sha256(suiteBytes),
    scenarios: [scenarioOracle('S1'), scenarioOracle('S2')],
  };
  return { suite, suiteBytes, oracle, oracleBytes: Buffer.from(JSON.stringify(oracle)) };
}

function criteriaGraph({ widget = 'Gadget', bolt = 'Part', extra = 0, joins = ['r1', 'r2'] }) {
  const nodes = [
    node('a', 'Acme', 'Organization'),
    node('b', 'Bravo', 'Organization'),
    node('w', 'Widget', widget),
    node('p', 'Bolt', bolt),
  ];
  for (let index = 0; index < extra; index += 1) nodes.push(node(`x${index}`, `Note ${index}`, 'Entity'));
  const claims = [claim('n', 'a', 'b', 'Acme reported 48 tonnes')];
  if (joins.includes('r1')) claims.push(claim('r1', 'a', 'w', 'Acme uses the widget'));
  if (joins.includes('r2')) claims.push(claim('r2', 'b', 'p', 'Bravo uses the bolt'));
  return { nodes, claims };
}

function criteriaRows(scenarioId, trial, graph, mode) {
  const worldCalls = (mode === 'none' ? [0, 1, 2, 3, 4] : [1, 2, 3, 4, 5])
    .map((call_index) => ({ call_index, disposition: 'completed', usage: { input: 200, output: 10 } }));
  const seedCalls = mode === 'none' ? [] : [{ call_index: 0, disposition: 'completed', usage: { input: 40, output: 10 } }];
  const types = mode === 'none' ? [] : [seedType('Gadget')];
  const base = { scenario_id: scenarioId, trial, attempt_id: `${scenarioId}-${trial}`, status: 'completed' };
  const schema = mode === 'none'
    ? { mode: 'none', extension: { version: 0, types: [] }, proposal: null, seed_inference: [] }
    : { mode: 'model', extension: { version: 1, types }, proposal: { status: 'applied', types }, seed_inference: seedCalls };
  const snap = (step_id, queries) => ({
    ...base,
    step_id,
    before_chat: queries ? { ...world(graph.nodes, graph.claims), queries } : world(graph.nodes, graph.claims),
    world_inference: worldCalls,
  });
  return [
    { ...base, step_id: '__seed', world_schema: schema, world_inference: seedCalls },
    snap('ingest'),
    snap('q', [{ status: 'completed', result: 'Acme status' }]),
  ];
}

function criteriaTraces(mode, graphFor) {
  return ['S1', 'S2'].flatMap((scenarioId) => [0, 1, 2].flatMap((trial) => (
    criteriaRows(scenarioId, trial, graphFor(scenarioId), mode)
  )));
}

test('seedCriteria returns auto_apply, remove, and keep_suggestion', () => {
  const frozen = criteriaBundle();
  const bare = () => criteriaGraph({ widget: 'Entity', bolt: 'Entity', extra: 1 });
  const typed = () => criteriaGraph({ widget: 'Gadget', bolt: 'Part' });
  const auto = gradeSeed({
    ...frozen,
    arms: [
      { arm: 'A', traces: criteriaTraces('none', bare) },
      { arm: 'B', traces: criteriaTraces('model', typed) },
    ],
  });
  assert.equal(auto.schema, 'loupist-world-seed-report/2');
  assert.equal(auto.criteria.ready, true);
  assert.equal(auto.criteria.decision, 'auto_apply');
  for (const key of ['domain_typed_rate', 'untyped_share', 'relation_pair_recall', 'duplicate_entities', 'numeric_and_type', 'world_cost', 'seed_proposal']) {
    assert.equal(auto.criteria[key].pass, true, key);
  }
  assert.equal(auto.arms[0].usage_per_episode.world_model_calls, 5);
  assert.equal(auto.arms[0].usage_per_episode.world_input_tokens, 1000);
  assert.equal(auto.arms[1].usage_per_episode.world_model_calls, 6);
  assert.equal(auto.arms[1].usage_per_episode.world_input_tokens, 1040);

  const remove = gradeSeed({
    ...frozen,
    arms: [
      { arm: 'A', traces: criteriaTraces('none', (scenarioId) => criteriaGraph({
        widget: 'Entity', bolt: 'Entity', extra: 1, joins: scenarioId === 'S1' ? ['r1'] : ['r1', 'r2'],
      })) },
      { arm: 'B', traces: criteriaTraces('model', (scenarioId) => criteriaGraph({
        widget: 'Gadget', bolt: 'Part', joins: scenarioId === 'S1' ? ['r1', 'r2'] : ['r1'],
      })) },
    ],
  });
  assert.equal(remove.criteria.decision, 'remove');
  assert.equal(remove.criteria.relation_pair_recall.pass, false);
  assert.equal(remove.criteria.domain_typed_rate.pass, true);
  assert.equal(remove.criteria.duplicate_entities.pass, true);
  assert.equal(remove.criteria.numeric_and_type.pass, true);
  assert.equal(remove.criteria.relation_pair_recall.B, 0.75);
  assert.equal(remove.criteria.relation_pair_recall.per_scenario[1].B, 0.5);
  assert.equal(remove.criteria.relation_pair_recall.per_scenario[1].A, 1);

  const keep = gradeSeed({
    ...frozen,
    arms: [
      { arm: 'A', traces: criteriaTraces('none', bare) },
      { arm: 'B', traces: criteriaTraces('model', () => criteriaGraph({ widget: 'Entity', bolt: 'Entity' })) },
    ],
  });
  assert.equal(keep.criteria.decision, 'keep_suggestion');
  assert.equal(keep.criteria.domain_typed_rate.pass, false);
  assert.equal(keep.criteria.relation_pair_recall.pass, true);
  assert.equal(keep.criteria.duplicate_entities.pass, true);
  assert.equal(keep.criteria.numeric_and_type.pass, true);
  assert.equal(keep.criteria.untyped_share.pass, true);
  assert.equal(keep.criteria.seed_proposal.pass, true);
});

test('duplicate slack, numeric margin, and an incomplete run stay out of auto_apply', () => {
  const metrics = (overrides = {}) => ({
    domain_typed_rate: 1,
    untyped_share: 0.1,
    relation_pair_recall: 1,
    duplicate_entities: 0,
    numeric_fidelity: 1,
    type_accuracy: 1,
    ...overrides,
  });
  const usage = { world_model_calls: 5, world_input_tokens: 1000, world_output_tokens: 10 };
  const seed = { domain_kind: true, duplicates_core: false, names: ['Gadget'] };
  const trial = (metric, { complete = true, duplicate = 0 } = {}) => ({
    complete, snapshot_step_id: complete ? 'ingest' : null,
    metrics: { ...metric, duplicate_entities: duplicate },
    seed, usage,
  });
  const arm = (name, metric, duplicates, complete = true) => ({
    arm: name,
    scenarios: ['S1', 'S2'].map((scenario_id, scenarioIndex) => ({
      scenario_id,
      trials: [0, 1, 2].map((trialIndex) => trial(metric, {
        complete, duplicate: duplicates[scenarioIndex * 3 + trialIndex] ?? 0,
      })),
    })),
    means: metric,
    usage_per_episode: name === 'B' ? { ...usage, world_model_calls: 6, world_input_tokens: 1100 } : usage,
  });
  const judge = (bMetrics, bDuplicates, aDuplicates = [0, 0, 0, 0, 0, 0]) => seedCriteria({
    arms: [
      arm('A', metrics({ untyped_share: 0.3 }), aDuplicates),
      arm('B', bMetrics, bDuplicates),
    ],
  });
  const within = judge(metrics(), [1, 0, 0, 0, 0, 0]);
  assert.equal(within.duplicate_entities.pass, true);
  assert.equal(within.duplicate_entities.B_total, 1);
  assert.equal(within.decision, 'auto_apply');
  const over = judge(metrics(), [1, 1, 0, 0, 0, 0]);
  assert.equal(over.duplicate_entities.pass, false);
  assert.equal(over.duplicate_entities.B_total, 2);
  assert.equal(over.decision, 'remove');
  const numericEdge = judge(metrics({ numeric_fidelity: 0.95 }), [0, 0, 0]);
  assert.equal(numericEdge.numeric_and_type.pass, true);
  assert.equal(numericEdge.decision, 'auto_apply');
  const numericMiss = judge(metrics({ numeric_fidelity: 0.94 }), [0, 0, 0]);
  assert.equal(numericMiss.numeric_and_type.pass, false);
  assert.equal(numericMiss.decision, 'remove');
  const typeMiss = judge(metrics({ type_accuracy: 0.94 }), [0, 0, 0]);
  assert.equal(typeMiss.decision, 'remove');
  const incomplete = seedCriteria({
    arms: [
      arm('A', metrics({ untyped_share: 0.3 }), [0, 0, 0], false),
      arm('B', metrics(), [0, 0, 0]),
    ],
  });
  assert.equal(incomplete.ready, false);
  assert.equal(incomplete.decision, null);
  assert.equal(incomplete.domain_typed_rate.pass, false);
  assert.equal(incomplete.relation_pair_recall.pass, false);
});

test('a cue matches the noun-phrase head, and a sentence around the anchor does not', async () => {
  const absent = async (name, kind) => {
    const trial = await gradeBiotech([node('n', name, kind)], []);
    assert.equal(trial.domain_concepts.find((item) => item.id === 'vel204-trial').present, false, name);
  };
  await absent('Placebo comparator in the Phase 2 VEL-204 trial', 'Product');
  await absent('Phase 2 VEL-204 trial planned enrollment: 240 patients', 'Metric');

  const halden = await gradeBiotech([
    node('n', "Halden Pharma's Phase 3 trial of HP-77 in psoriasis", 'Entity'),
  ], []);
  const hp = halden.domain_concepts.find((item) => item.id === 'hp77-trial');
  assert.equal(hp.present, true);
  assert.equal(hp.extension_typed, false);
  assert.equal(halden.domain_concepts.find((item) => item.id === 'psoriasis').present, false);

  const passed = await gradeBiotech([
    node('n', 'Phase 2 VEL-204 trial in adults with moderate psoriasis', 'Entity'),
  ], []);
  assert.equal(passed.domain_concepts.find((item) => item.id === 'vel204-trial').present, true);

  const frozen = await loadFrozen();
  const leaseName = 'Tamsin Flats project mining lease';
  const leaseTrial = gradeSeed({
    ...frozen,
    arms: [{
      arm: 'B',
      traces: scenarioRows({
        scenarioId: 'S-LITHIUM', stepIds: LITHIUM_STEPS,
        nodes: [node('lease', leaseName, 'Entity')], claims: [],
      }),
    }],
  }).arms[0].scenarios[1].trials[0];
  assert.equal(leaseTrial.domain_concepts.find((item) => item.id === 'tamsin-lease').present, true);
  assert.equal(leaseTrial.domain_concepts.find((item) => item.id === 'tamsin-lease').extension_typed, false);
  assert.equal(leaseTrial.domain_concepts.find((item) => item.id === 'tamsin-project').present, false);

  const cues = gradeSeed({
    ...frozen,
    arms: [{
      arm: 'B',
      traces: scenarioRows({
        scenarioId: 'S-LITHIUM', stepIds: LITHIUM_STEPS,
        nodes: [
          node('deal', 'offtake deal', 'Entity'),
          node('name', 'Ardell offtake', 'Entity'),
          node('dfs', 'feasibility study', 'Entity'),
          node('timetable', 'Tamsin timetable', 'Entity'),
        ],
        claims: [],
      }),
    }],
  }).arms[0].scenarios[1].trials[0];
  const conceptNames = (id) => cues.domain_concepts.find((item) => item.id === id).names;
  assert.deepEqual(conceptNames('offtake'), ['offtake deal']);
  assert.deepEqual(conceptNames('tamsin-study'), ['feasibility study']);
});

test('kind matching uses the last PascalCase word for both lithium projects', async () => {
  const phase = await gradeBiotech([
    node('phase', 'Phase 2 VEL-204 trial in adults with moderate psoriasis', 'TrialPhase'),
  ], []);
  const untyped = phase.domain_concepts.find((item) => item.id === 'vel204-trial');
  assert.equal(untyped.present, true);
  assert.equal(untyped.extension_typed, false);

  const frozen = await loadFrozen();
  const phaseRows = trialRows({
    nodes: [node('phase', 'Phase 2 VEL-204 trial in adults with moderate psoriasis', 'TrialPhase')],
    claims: [],
    seedNames: ['TrialPhase'],
  });
  const phaseSeed = gradeSeed({ ...frozen, arms: [{ arm: 'B', traces: phaseRows }] });
  assert.equal(phaseSeed.arms[0].scenarios[0].trials[0].seed.domain_kind, false);
  assert.equal(phaseSeed.arms[0].scenarios[0].trials[0].seed.duplicates_core, false);

  const nodes = [
    node('project', 'Tamsin Flats', 'MiningProject'),
    node('mine', 'Lake Ostra mine', 'MiningProject'),
  ];
  const rows = scenarioRows({ scenarioId: 'S-LITHIUM', stepIds: LITHIUM_STEPS, nodes, claims: [] });
  const types = [seedType('MiningProject')];
  const inference = [{ call_index: 0, disposition: 'completed', usage: { input: 10, output: 1 } }];
  rows.unshift({
    scenario_id: 'S-LITHIUM', trial: 0, attempt_id: 't-0', status: 'completed', step_id: '__seed',
    world_schema: {
      mode: 'model', extension: { version: 1, types }, proposal: { status: 'applied', types }, seed_inference: inference,
    },
    world_inference: inference,
  });
  const report = gradeSeed({ ...frozen, arms: [{ arm: 'B', traces: rows }] });
  const trial = report.arms[0].scenarios[1].trials[0];
  const project = trial.domain_concepts.find((item) => item.id === 'tamsin-project');
  const mine = trial.domain_concepts.find((item) => item.id === 'lake-ostra');
  assert.equal(project.extension_typed, true);
  assert.deepEqual(project.names, ['Tamsin Flats']);
  assert.equal(mine.extension_typed, true);
  assert.deepEqual(mine.names, ['Lake Ostra mine']);
  assert.equal(trial.seed.domain_kind, true);
  assert.equal(trial.seed.duplicates_core, false);
});

test('frozen seed corpus and oracle agree', async () => {
  const corpusBytes = await readFile(new URL('./seed-corpus-v2.json', root));
  const oracleBytes = await readFile(new URL('./seed-oracle-v2.json', root));
  const corpus = JSON.parse(corpusBytes);
  const oracle = JSON.parse(oracleBytes);
  assert.equal(oracle.schema, 'loupist-world-seed-oracle/2');
  assert.equal(oracle.version, 2);
  assert.equal(oracle.input_sha256, sha256(corpusBytes));
  assert.deepEqual(corpus.scenarios.map((scenario) => scenario.id), ['S-BIOTECH', 'S-LITHIUM']);
  for (const scenario of corpus.scenarios) {
    assert.equal(scenario.seed, 'none');
    assert.equal(scenario.world_enabled, true);
    const sources = scenario.steps.flatMap((step) => step.sources ?? []);
    assert.equal(sources.length, 6);
    for (const source of sources) {
      const words = source.text.trim().split(/\s+/u).length;
      assert.ok(words >= 150 && words <= 400, `${source.id} has ${words} words`);
      assert.equal(source.origin.startsWith('https://fixtures.invalid/world/seed/'), true);
    }
  }
  assert.equal(corpus.scenarios[0].research_focus, 'Clinical-stage biotech pipelines: trial phases, readouts and approval timelines');
  assert.equal(corpus.scenarios[1].research_focus, 'Junior lithium miners: projects, permits and offtake deals');
  assert.deepEqual(corpus.scenarios[0].steps.slice(-2).map((step) => step.query), [
    'VEL-204 trial readout timing',
    'psoriasis trials Phase 3 results',
  ]);
  assert.deepEqual(corpus.scenarios[1].steps.slice(-2).map((step) => step.query), [
    'Tamsin Flats permits and timeline',
    'lithium offtake agreements',
  ]);
  const biotech = oracle.scenarios[0];
  const lithium = oracle.scenarios[1];
  assert.equal(biotech.domain_concepts.length, 5);
  assert.equal(biotech.numeric_facts.length, 5);
  assert.equal(lithium.entities.length, 6);
  assert.equal(lithium.domain_concepts.length, 5);
  assert.equal(lithium.numeric_facts.length, 5);
  assert.equal(lithium.relations.length, 11);
  const conceptIds = new Set(lithium.domain_concepts.map((item) => item.id));
  const entityKinds = new Map(lithium.entities.map((item) => [item.id, item.kind]));
  const directCore = lithium.relations.filter((item) => !item.via?.length && conceptIds.has(item.target) && entityKinds.has(item.source));
  assert.ok(directCore.length >= 3);
  assert.ok(lithium.relations.filter((item) => item.via?.length).length >= 3);
  assert.deepEqual(biotech.relations.find((item) => item.id === 'vel204-psoriasis').via, ['vel204-trial']);
  assert.deepEqual(biotech.relations.find((item) => item.id === 'hp77-psoriasis').via, ['hp77-trial']);
  assert.deepEqual(biotech.relations.find((item) => item.id === 'vel101-atopic').via, ['vel101-decision']);
  assert.deepEqual(biotech.relations.find((item) => item.id === 'office-reviews-vel101').also_targets, ['vel-101']);
  assert.equal(biotech.domain_concepts.find((item) => item.id === 'psoriasis').cue_patterns, undefined);
  assert.equal(biotech.domain_concepts.find((item) => item.id === 'atopic').cue_patterns, undefined);
  assert.deepEqual(biotech.domain_concepts.find((item) => item.id === 'vel204-trial').kind_patterns, ['^(?:Trial|Study)$']);
  assert.deepEqual(biotech.domain_concepts.find((item) => item.id === 'vel101-decision').kind_patterns, ['^(?:Application|Submission|Filing|Decision|Approval|Review)$']);
  assert.deepEqual(lithium.relations.find((item) => item.id === 'kestrel-study').via, ['tamsin-project']);
  assert.equal(lithium.relations.find((item) => item.id === 'tamsin-corvane').source, 'tamsin-project');
  assert.equal(lithium.relations.find((item) => item.id === 'tamsin-corvane').target, 'corvane');
  assert.equal(lithium.relations.find((item) => item.id === 'lease-tamsin').via, undefined);
  assert.equal(lithium.relations.find((item) => item.id === 'study-tamsin').via, undefined);
  assert.deepEqual(lithium.domain_concepts.find((item) => item.id === 'tamsin-project').kind_patterns, lithium.domain_concepts.find((item) => item.id === 'lake-ostra').kind_patterns);
  assert.deepEqual(lithium.domain_concepts.find((item) => item.id === 'offtake').cue_patterns, ['agreement|deal|contract']);
  assert.deepEqual(lithium.domain_concepts.find((item) => item.id === 'tamsin-study').cue_patterns, ['study|DFS']);
  for (const scenario of oracle.scenarios) {
    for (const item of scenario.domain_concepts) {
      assert.ok(item.anchor_patterns.length >= 1);
      for (const pattern of item.kind_patterns) {
        const compiled = new RegExp(pattern, 'iu');
        for (const kind of CORE_KINDS) assert.equal(compiled.test(kind), false, `${item.id} matches ${kind}`);
      }
    }
  }
});

test('oracle v1 is rejected', async () => {
  const frozen = await loadFrozen();
  assert.throws(() => gradeSeed({
    ...frozen,
    oracle: { ...frozen.oracle, schema: 'loupist-world-seed-oracle/1', version: 1 },
    arms: [{ arm: 'B', traces: [] }],
  }), /Unsupported seed oracle/);
});

test('the command writes a report for one arm', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'seed-grade-'));
  const trace = join(directory, 'arm.jsonl');
  const out = join(directory, 'report.json');
  const rows = trialRows({ nodes: typedNodes, claims: typedClaims });
  await writeFile(trace, `${rows.map((row) => JSON.stringify(row)).join('\n')}\n`);
  const result = spawnSync(process.execPath, [
    new URL('./seed-grade.mjs', root).pathname,
    '--suite', new URL('./seed-corpus-v2.json', root).pathname,
    '--oracle', new URL('./seed-oracle-v2.json', root).pathname,
    '--arm', `B=${trace}`,
    '--out', out,
  ], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  const report = JSON.parse(await readFile(out, 'utf8'));
  assert.equal(report.schema, 'loupist-world-seed-report/2');
  assert.equal(report.arms[0].scenarios[0].trials[0].metrics.domain_typed_rate, 1);
});
