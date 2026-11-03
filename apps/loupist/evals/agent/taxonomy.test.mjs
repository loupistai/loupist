import test from 'node:test';
import assert from 'node:assert/strict';
import { classifyFailure, taxonomy, taxonomyMarkdown } from './taxonomy.mjs';
import { suite, traces } from './fixtures/sample.mjs';

test('rules use host category, rubric, reasons, metrics, and tool evidence with stable priority', () => {
  assert.equal(classifyFailure({ metric: 'save_recall' }, { failureKind: 'provider_error' }), 'host');
  assert.equal(classifyFailure({ criterion_id: 'precedence' }), 'precedence');
  assert.equal(classifyFailure({ reason: 'It does not attach dates and gives no observable test.' }), 'date');
  assert.equal(classifyFailure({ reason: 'The only test is one bundled line.' }), 'test');
  assert.equal(classifyFailure({ reason: 'The revenue growth figure is undated.' }), 'date');
  assert.equal(classifyFailure({ reason: 'The only test is at an undated next report.' }), 'test');
  assert.equal(classifyFailure({ metric: 'save_precision', reason: 'A note mentions the latest figure.' }), 'save');
  assert.equal(classifyFailure({ reason: 'Claims access was available.' }, { tool_calls: [{ error_code: 'connection_unavailable' }] }), 'connection');
  assert.equal(classifyFailure({ assert: 'not_committed' }), 'save');
});
test('each failed check stays in the count and exclusions only affect prioritization', () => {
  const run = { name: 'control', suite, traces, tracePath: 'evidence.jsonl', judgments: [], exclusion: 'negative control' };
  const result = taxonomy([run]);
  assert.ok(result.failures.length > 0);
  assert.equal(result.clusters.reduce((n, c) => n + c.count, 0), result.failures.length);
  assert.equal(result.clusters.every(c => c.unresolved === 0), true);
  assert.deepEqual(taxonomy([run]), result);
  assert.match(taxonomyMarkdown(result), /negative control/u);
  assert.match(taxonomyMarkdown(result), /evidence.jsonl:/u);
});
