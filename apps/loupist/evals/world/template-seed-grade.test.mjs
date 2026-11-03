import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { gradeTemplateSeed } from './template-seed-grade.mjs';

const root = new URL('.', import.meta.url);
const suiteBytes = await readFile(new URL('./template-seed-corpus-v2.json', root));
const suite = JSON.parse(suiteBytes);

function claim(id, relation, status = 'active') {
  return { id, relation, status, endpoints: [{ nodeId: 'a', role: 'source' }, { nodeId: 'b', role: 'target' }] };
}

/** Every step completed for one scenario, with the World after the last step. */
function rows(scenarioId, trial, claims, seedRelations = []) {
  const scenario = suite.scenarios.find((item) => item.id === scenarioId);
  const world = { records: [{ snapshot: { kind: 'world', payload: { nodes: [], claims } } }] };
  return [
    { scenario_id: scenarioId, trial, step_id: '__seed', status: 'completed', world_schema: { extension: { relations: seedRelations.map((name) => ({ name })) } } },
    ...scenario.steps.map((step) => ({ scenario_id: scenarioId, trial, step_id: step.id, status: 'completed', after_chat: world })),
  ];
}

test('the checked-in corpus has six Sources and two queries per class, with a template and its default focus', () => {
  assert.deepEqual(suite.scenarios.map((scenario) => [scenario.id, scenario.template, scenario.seed]), [['S-CRYPTO', 'crypto', 'none'], ['S-COMMODITIES', 'commodities', 'none'], ['S-MACRO', 'macro', 'none'], ['S-REAL-ASSETS', 'realAssets', 'none']]);
  for (const scenario of suite.scenarios) {
    assert.equal(scenario.steps.filter((step) => step.sources?.length).length, 6);
    assert.equal(scenario.steps.filter((step) => step.query).length, 2);
    assert.ok(scenario.steps.flatMap((step) => step.sources ?? []).every((source) => source.origin.startsWith('https://fixtures.invalid/')));
  }
});

test('a seed ships only when it keeps typed core relations and the typed share', () => {
  const none = [
    ...rows('S-CRYPTO', 0, [claim('1', 'SUPPLIES'), claim('2', 'PARTNERS_WITH'), claim('3', 'RELATES_TO')]),
    ...rows('S-COMMODITIES', 0, [claim('1', 'SUPPLIES'), claim('2', 'LOCATED_IN'), claim('3', 'USES')]),
  ];
  const template = [
    // Crypto keeps both core claims and types the third with a seed relation: ship.
    ...rows('S-CRYPTO', 0, [claim('1', 'SUPPLIES'), claim('2', 'PARTNERS_WITH'), claim('3', 'ISSUES'), claim('4', 'LISTED_ON', 'withdrawn')], ['ISSUES', 'LISTED_ON']),
    // Commodities moves a core USES claim into CONSUMES: fewer typed core relations, hold.
    ...rows('S-COMMODITIES', 0, [claim('1', 'SUPPLIES'), claim('2', 'LOCATED_IN'), claim('3', 'CONSUMES')], ['CONSUMES']),
  ];
  const report = gradeTemplateSeed({ suite, suiteBytes, arms: { none, template } });
  const crypto = report.scenarios.find((scenario) => scenario.id === 'S-CRYPTO');
  assert.equal(crypto.none.typed_core_relations, 2);
  assert.equal(crypto.template_arm.typed_core_relations, 2);
  assert.equal(crypto.template_arm.trials[0].seed_relation_claims, 1);
  assert.equal(crypto.template_arm.typed_share, 1);
  assert.equal(crypto.decision, 'ship');
  assert.equal(report.scenarios.find((scenario) => scenario.id === 'S-COMMODITIES').decision, 'hold');
  // Scenarios without traces are unmeasured, so they do not ship.
  assert.deepEqual(report.ship, { crypto: true, commodities: false, macro: false, realAssets: false });
  assert.match(report.input_sha256, /^[0-9a-f]{64}$/);
});

test('an incomplete trial is unmeasured, never a pass', () => {
  const none = rows('S-CRYPTO', 0, [claim('1', 'SUPPLIES')]).slice(0, 3);
  const template = rows('S-CRYPTO', 0, [claim('1', 'SUPPLIES')], ['ISSUES']);
  const report = gradeTemplateSeed({ suite, suiteBytes, arms: { none, template } });
  assert.equal(report.scenarios[0].decision, 'unmeasured');
  assert.equal(report.ship.crypto, false);
});
