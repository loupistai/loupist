import assert from 'node:assert/strict';
import test from 'node:test';
import { judgeAgreement, MIN_ITEMS } from './judge-agreement.mjs';

const row = (criterion, index, pass, model) => ({
  item_id: `${criterion}#${index}`, criterion_id: criterion, suite: 'global', scenario_id: `G-${index}`, session_id: 'research',
  turn_index: 0, trial: 0, pass, reason: pass ? 'met' : 'not met', model,
});
const judged = (criterion, passes, model) => passes.map((pass, index) => row(criterion, index, pass, model));

test('a criterion is reliable only with ten shared items and 85% agreement', () => {
  const twenty = Array.from({ length: 20 }, () => true);
  const a = [...judged('steady', twenty, 'opus'), ...judged('split', twenty, 'opus'), ...judged('few', twenty.slice(0, MIN_ITEMS - 1), 'opus')];
  const b = [
    ...judged('steady', twenty.map((pass, index) => index >= 17 ? false : pass), 'astra'), // 17 of 20: exactly 85%
    ...judged('split', twenty.map((pass, index) => index >= 16 ? false : pass), 'astra'), // 16 of 20: 80%
    ...judged('few', twenty.slice(0, MIN_ITEMS - 1), 'astra'),
  ];
  const report = judgeAgreement(a, b);
  const by = Object.fromEntries(report.criteria.map((criterion) => [criterion.criterion_id, criterion]));
  assert.deepEqual([by.steady.agreement, by.steady.reliable], [0.85, true]);
  assert.deepEqual([by.split.agreement, by.split.reliable, by.split.reasons], [0.8, false, ['agreement under 85%']]);
  assert.deepEqual([by.few.agreement, by.few.reliable, by.few.reasons], [1, false, ['fewer than 10 items']]);
  assert.deepEqual(report.unreliable, ['few', 'split']);
  assert.deepEqual(report.judges, { a: ['opus'], b: ['astra'] });
  assert.equal(report.disagreements.length, 7);
  assert.deepEqual(report.disagreements[0], {
    item_id: 'steady#17', criterion_id: 'steady', suite: 'global', scenario_id: 'G-17', session_id: 'research', turn_index: 0, trial: 0,
    a: { pass: true, reason: 'met' }, b: { pass: false, reason: 'not met' },
  });
});

test('only shared items are compared, and what one judge lacks is counted', () => {
  const a = judged('c', [true, true, false], 'opus');
  const b = judged('c', [true, false], 'astra');
  const report = judgeAgreement(a, b);
  assert.deepEqual(report.items, { a: 3, b: 2, shared: 2, only_a: 1, only_b: 0 });
  assert.equal(report.agreement, 0.5);
  assert.equal(judgeAgreement([], []).agreement, null);
});

test('rows that cannot be paired are refused instead of dropped', () => {
  const a = judged('c', [true], 'opus');
  assert.throws(() => judgeAgreement([...a, ...a], a), /duplicate item/u);
  assert.throws(() => judgeAgreement([{ ...a[0], pass: null }], a), /boolean pass/u);
  assert.throws(() => judgeAgreement(a, [{ ...a[0], criterion_id: 'other' }]), /different criteria/u);
});
