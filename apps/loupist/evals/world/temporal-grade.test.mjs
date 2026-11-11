import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

import { acceptance, gradeArm } from './temporal-grade.mjs';

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
