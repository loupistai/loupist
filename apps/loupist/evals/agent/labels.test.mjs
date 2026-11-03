import test from 'node:test';
import assert from 'node:assert/strict';
import { agreement, importLabels, labelInteractive } from './labels.mjs';

import { itemId } from './judge.mjs';
import { wilsonInterval } from './report.mjs';

const judgment = (id, criterion, pass) => {
  const trace = { run_id: id, suite: 'labels', scenario_id: 'S', session_id: 's', turn_index: 0, trial: 0 };
  return { ...trace, item_id: itemId(trace, criterion), criterion_id: criterion, pass, reason: 'judge' };
};

test('interactive labeling records pass or fail and a note', async () => {
  const lines = ['p shares were saved', 'bogus', 'f one-off detail', 's', 'q leftover'];
  const labels = await labelInteractive({
    items: [
      { item_id: 'a', criterion_id: 'states-diluted-shares', metric: 'save_recall', criterion: 'States 120 million.', user: 'remember', answer: '120 million' },
      { item_id: 'b', criterion_id: 'save_precision:lens-1', metric: 'save_precision', criterion: 'Reusable.', user: 'remember', answer: 'saved', memory_item: { kind: 'lens', title: 'Diluted', summary: '120 million' } },
      { item_id: 'c', criterion_id: 'states-diluted-shares', metric: 'save_recall', criterion: 'States 120 million.', user: 'q', answer: 'q' },
      { item_id: 'd', criterion_id: 'states-diluted-shares', metric: 'save_recall', criterion: 'States 120 million.', user: 'after quit', answer: 'no' },
    ],
    ask: async () => lines.shift(),
    now: () => '2026-10-18T00:00:00.000Z',
  });
  assert.deepEqual(labels, [
    { item_id: 'a', criterion_id: 'states-diluted-shares', human_pass: true, note: 'shares were saved', labeled_at: '2026-10-18T00:00:00.000Z' },
    { item_id: 'b', criterion_id: 'save_precision:lens-1', human_pass: false, note: 'one-off detail', labeled_at: '2026-10-18T00:00:00.000Z' },
  ]);
});

test('imported labels agree with the judge, and readiness needs balanced labels and class accuracy', () => {
  const imported = importLabels([
    '{"item_id":"a","criterion_id":"c1","human_pass":false,"note":"later"}',
    '{"item_id":"missing","criterion_id":"c1","human_pass":true}',
    '',
  ].join('\n'));
  assert.equal(imported.length, 2);
  assert.equal(imported.find((row) => row.item_id === 'a').human_pass, false);
  assert.throws(() => importLabels([imported[0], imported[0]].map((row) => JSON.stringify(row)).join('\n')), /duplicate item_id/u);
  const smallJudgments = [judgment('a', 'c1', true), judgment('b', 'c1', false), judgment('c', 'c2', true), judgment('d', 'c2', false)];
  const small = agreement(smallJudgments, smallJudgments.map((row, index) => ({ item_id: row.item_id, criterion_id: row.criterion_id, human_pass: index < 2 })));
  assert.equal(small.counts.tp, 1);
  assert.equal(small.counts.fn, 1);
  assert.equal(small.counts.fp, 1);
  assert.equal(small.counts.tn, 1);
  assert.equal(small.accuracy, 0.5);
  assert.equal(small.tpr, 0.5);
  assert.equal(small.tnr, 0.5);
  assert.equal(small.validated, false);
  assert.equal(small.per_criterion.c1.counts.tp, 1);
  assert.equal(small.per_criterion.c2.counts.fp, 1);
  const judgments = Array.from({ length: 30 }, (_, index) => judgment(`i${index}`, 'same', true));
  const correct = judgments.map((row) => ({ item_id: row.item_id, criterion_id: 'same', human_pass: true }));
  correct[0].human_pass = false;
  const almost = agreement(judgments.slice(0, 29), correct.slice(0, 29));
  const validated = agreement(judgments, correct.map((row, index) => ({ ...row, human_pass: index >= 6 })));
  const below = agreement(judgments, correct.map((row, index) => ({ ...row, human_pass: index >= 7 })));
  assert.equal(almost.validated, false);
  assert.equal(validated.counts.matched, 30);
  assert.equal(validated.accuracy, 0.8);
  assert.equal(validated.validated, false);
  assert.equal(below.accuracy, 23 / 30);
  assert.equal(below.validated, false);
  assert.equal(agreement([], []).tpr, null);
  assert.equal(agreement([], []).accuracy, null);
});

test('agreement rejects unknown verdicts, mismatched identities or criteria, and duplicate evidence', () => {
  const judgments = Array.from({ length: 30 }, (_, index) => judgment(`item-${index}`, 'criterion', index < 15));
  const labels = judgments.map((row) => ({ item_id: row.item_id, criterion_id: row.criterion_id, human_pass: row.pass }));
  assert.equal(agreement(judgments, labels).validated, true);
  for (const pass of [null, undefined, 'false', 0]) {
    const report = agreement(judgments.map((row) => ({ ...row, pass })), labels);
    assert.equal(report.validated, false);
    assert.equal(report.counts.matched, 0);
    assert.equal(report.counts.invalid_judgments, 30);
    assert.equal(report.accuracy, null);
  }
  const wrongIdentity = agreement(judgments.map((row) => ({ ...row, run_id: 'another-run' })), labels);
  assert.equal(wrongIdentity.counts.invalid_judgments, 30);
  assert.equal(wrongIdentity.validated, false);
  const wrongCriterion = agreement(judgments, labels.map((row) => ({ ...row, criterion_id: 'unrelated' })));
  assert.equal(wrongCriterion.counts.criterion_mismatches, 30);
  assert.equal(wrongCriterion.counts.matched, 0);
  assert.equal(wrongCriterion.validated, false);
  const duplicateJudgments = agreement([...judgments, judgments[0]], labels);
  assert.equal(duplicateJudgments.counts.duplicate_judgments, 1);
  assert.equal(duplicateJudgments.counts.matched, 29);
  assert.equal(duplicateJudgments.validated, false);
  const duplicateLabels = agreement(judgments, [...labels, labels[0]]);
  assert.equal(duplicateLabels.counts.duplicate_labels, 1);
  assert.equal(duplicateLabels.validated, false);
  const invalidLabels = agreement(judgments, labels.map((row) => ({ ...row, human_pass: null })));
  assert.equal(invalidLabels.counts.invalid_labels, 30);
  assert.equal(invalidLabels.validated, false);
});


test('balanced readiness rejects skewed accuracy and reports class support and failure reasons', () => {
  const rows = Array.from({ length: 100 }, (_, index) => judgment(`skew-${index}`, 'balanced', index < 95));
  const labels = rows.map((row, index) => ({ item_id: row.item_id, criterion_id: row.criterion_id, human_pass: index < 90 }));
  const skewed = agreement(rows, labels);
  assert.equal(skewed.accuracy, 0.95);
  assert.equal(skewed.tnr, 0.5);
  assert.equal(skewed.ready, false);
  assert.deepEqual(skewed.reasons, ['TNR < 0.8 or unavailable']);
  const balanced = Array.from({ length: 30 }, (_, index) => judgment(`balanced-${index}`, 'balanced', index < 15));
  const correct = balanced.map((row) => ({ item_id: row.item_id, criterion_id: row.criterion_id, human_pass: row.pass }));
  assert.equal(agreement(balanced, correct).ready, true);
  assert.deepEqual(agreement(balanced, correct).reasons, []);
  const boundary = balanced.map((row, index) => ({ ...row, pass: index < 3 || (index >= 15 && index < 18) ? !row.pass : row.pass }));
  assert.equal(agreement(boundary, correct).ready, true);
  assert.equal(agreement(balanced.slice(0, 29), correct.slice(0, 29)).ready, false);
  const oneClass = rows.map((row) => ({ ...row, pass: true }));
  const support = agreement(oneClass, labels);
  assert.equal(support.counts.positives, 90);
  assert.equal(support.counts.negatives, 10);
  const unbalancedLabels = labels.map((row, index) => ({ ...row, human_pass: index < 91 }));
  assert.ok(agreement(oneClass, unbalancedLabels).reasons.includes('negatives < 10'));
  assert.equal(agreement([], []).wilson_95.accuracy, null);
});

test('criterion and metric calibration include Wilson intervals and insufficient label groups', () => {
  const rows = Array.from({ length: 20 }, (_, index) => ({
    ...judgment(`group-${index}`, index < 12 ? 'large' : 'small', index % 2 === 0), metric: index < 12 ? 'task_success' : 'save_precision',
  }));
  rows.push({ ...judgment('unlabeled', 'unlabeled', true), metric: 'retrieval' });
  const labels = rows.slice(0, 20).map((row) => ({ item_id: row.item_id, criterion_id: row.criterion_id, human_pass: row.pass }));
  const result = agreement(rows, labels);
  assert.equal(result.per_criterion.large.status, 'pass');
  assert.equal(result.per_criterion.small.status, 'insufficient_labels');
  assert.equal(result.per_criterion.unlabeled.status, 'insufficient_labels');
  assert.equal(result.per_criterion.unlabeled.counts.matched, 0);
  assert.equal(result.per_metric.save_precision.status, 'insufficient_labels');
  assert.deepEqual(result.per_metric.task_success, result.per_criterion.large);
  assert.deepEqual(result.per_criterion.large.wilson_95, {
    accuracy: wilsonInterval(12, 12), tpr: wilsonInterval(6, 6), tnr: wilsonInterval(6, 6),
  });
  assert.deepEqual(result.wilson_95, { accuracy: wilsonInterval(20, 20), tpr: wilsonInterval(10, 10), tnr: wilsonInterval(10, 10) });
});
