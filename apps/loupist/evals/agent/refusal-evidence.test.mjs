import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { firstAttemptRefusals, judgeRefusalItems, refusalItemId, refusalJudgeItems, refusalPairs, refusalReport,
  renderRefusalPrompt, RULE_STATED_CRITERION } from './refusal-evidence.mjs';

const catalog = { tools: [
  { name: 'ls', description: 'List a folder of the Loupist folder.', parameters: { type: 'object', properties: { path: { type: 'string' } } } },
  { name: 'world_changes', description: 'What changed. An asset id needs an Asset page.', parameters: { type: 'object' } },
  { name: 'lens_file', description: 'File a page.', parameters: { type: 'object' } },
  { name: 'dart_filing_search', description: 'Search filings.', parameters: { type: 'object' } },
] };

const refused = (name, error_code, extra = {}) => ({ name, outcome: 'error', error_code, error_message: `${name} refused: ${error_code}`, args: {}, ...extra });
const ok = (name) => ({ name, outcome: 'ok', args: {} });
const turn = (scenario_id, tool_calls, trial = 0) => ({ run_id: 'run', suite: 'refusals', scenario_id, session_id: 'bot', turn_index: 0, trial, tool_calls });

test('a refusal counts once per turn for its tool and reason, and a typed reason names the pair', () => {
  const traces = [
    turn('P-1', [ok('ls'), refused('ls', 'outside_workspace'), refused('ls', 'outside_workspace'), refused('ls', 'protected_path')]),
    turn('P-2', [refused('world_changes', 'execution_failed', { error: { reason: 'asset_page_missing' } })]),
    turn('P-2', [refused('world_changes', 'execution_failed', { error_reason: 'Asset Page Missing' })], 1),
  ];
  const { refusals, repeats, unmeasured } = firstAttemptRefusals(traces, catalog);
  assert.deepEqual(refusals.map(({ scenario_id, trial, call_index, tool, reason, typed }) => [scenario_id, trial, call_index, tool, reason, typed]), [
    ['P-1', 0, 1, 'ls', 'outside_workspace', false],
    ['P-1', 0, 3, 'ls', 'protected_path', false],
    ['P-2', 0, 0, 'world_changes', 'asset_page_missing', true],
    ['P-2', 1, 0, 'world_changes', 'asset_page_missing', true],
  ]);
  assert.equal(repeats, 1);
  assert.deepEqual(unmeasured, []);
  assert.deepEqual(refusalPairs(refusals).map(({ tool, reason, count, scenarios }) => [tool, reason, count, scenarios]), [
    ['world_changes', 'asset_page_missing', 2, ['P-2']],
    ['ls', 'outside_workspace', 1, ['P-1']],
    ['ls', 'protected_path', 1, ['P-1']],
  ]);
});

test('data and transport outcomes, invalid arguments and unknown tools are not refusals', () => {
  const traces = [turn('P-1', [
    refused('dart_filing_search', 'empty_result'),
    refused('dart_filing_search', 'provider_error'),
    refused('lens_file', 'tool_validation_failed'),
    refused('python.read', 'not_exposed'),
    { name: 'lens_file', outcome: 'error' },
    ok('lens_file'),
  ])];
  assert.deepEqual(firstAttemptRefusals(traces, catalog).refusals, []);
  // A typed reason makes even a generic code a refusal: the host said which rule it was.
  const typed = [turn('P-1', [refused('dart_filing_search', 'empty_result', { error: { reason: 'window_too_wide' } })])];
  assert.deepEqual(firstAttemptRefusals(typed, catalog).refusals.map((row) => row.reason), ['window_too_wide']);
});

test('a turn without a call list is unmeasured and leaves the metric null', () => {
  const traces = [turn('P-1', [refused('ls', 'outside_workspace')]), { ...turn('P-2', null), tool_calls: undefined }];
  const report = refusalReport(traces, catalog, [{ item_id: refusalItemId('ls', 'outside_workspace'), pass: false, reason: 'not stated' }]);
  assert.equal(report.unmeasured_turns.length, 1);
  assert.equal(report.hidden_rule_refusal, null);
});

test('one judge item per pair holds the description and the refusal, and nothing of the run', () => {
  const traces = [
    turn('P-1', [refused('ls', 'outside_workspace', { args: { path: '/attachments/secret-plan.txt' } })]),
    turn('P-3', [refused('ls', 'outside_workspace')], 2),
  ];
  const items = refusalJudgeItems(refusalPairs(firstAttemptRefusals(traces, catalog).refusals), catalog);
  assert.equal(items.length, 1);
  assert.deepEqual(items[0].input, {
    tool: 'ls', description: 'List a folder of the Loupist folder.', parameters: catalog.tools[0].parameters,
    refusal: { reason: 'outside_workspace', message: 'ls refused: outside_workspace' }, criterion: RULE_STATED_CRITERION,
  });
  const prompt = renderRefusalPrompt(items[0].input);
  assert.match(prompt, /List a folder of the Loupist folder\./u);
  assert.doesNotMatch(prompt, /secret-plan|P-1|P-3/u);
});

test('each pair is judged once, cached across runs, and the count is the pairs judged not stated', async (t) => {
  const cacheDir = await mkdtemp(join(tmpdir(), 'refusal-judge-'));
  t.after(() => rm(cacheDir, { recursive: true, force: true }));
  const traces = [
    turn('P-1', [refused('ls', 'outside_workspace')]),
    turn('P-1', [refused('ls', 'outside_workspace')], 1),
    turn('P-2', [refused('world_changes', 'execution_failed', { error: { reason: 'asset_page_missing' } })]),
    turn('P-4', [refused('unlisted_tool', 'outside_workspace')]),
  ];
  const prompts = [];
  const backend = { judge: async (prompt, { input }) => {
    prompts.push(prompt);
    return { pass: input.tool === 'world_changes', reason: input.tool === 'world_changes' ? 'states "An asset id needs an Asset page"' : 'the description names no path limit' };
  } };
  const items = refusalJudgeItems(refusalPairs(firstAttemptRefusals(traces, catalog).refusals), catalog);
  const first = await judgeRefusalItems({ items, model: 'judge-model', cacheDir, backend });
  assert.equal(prompts.length, 2);
  assert.deepEqual(first.map(({ tool, refusal_reason, pass, cache, criterion_id }) => [tool, refusal_reason, pass, cache, criterion_id]), [
    ['ls', 'outside_workspace', false, 'miss', 'rule-stated'],
    ['world_changes', 'asset_page_missing', true, 'miss', 'rule-stated'],
  ]);
  const again = await judgeRefusalItems({ items, model: 'judge-model', cacheDir, backend });
  assert.equal(prompts.length, 2);
  assert.deepEqual(again.map((row) => row.cache), ['hit', 'hit']);

  const report = refusalReport(traces, catalog, first);
  assert.equal(report.first_attempt_refusals, 3);
  assert.equal(report.hidden_rule_refusal, 1);
  assert.deepEqual(report.pairs.map(({ tool, reason, count, stated }) => [tool, reason, count, stated]), [
    ['ls', 'outside_workspace', 2, false], ['world_changes', 'asset_page_missing', 1, true],
  ]);
  // A changed description is a new question; an unjudged pair leaves the metric unmeasured.
  const revised = { tools: catalog.tools.map((tool) => tool.name === 'ls' ? { ...tool, description: `${tool.description} Paths outside it are refused.` } : tool) };
  const revisedItems = refusalJudgeItems(refusalPairs(firstAttemptRefusals(traces, revised).refusals), revised);
  await judgeRefusalItems({ items: revisedItems, model: 'judge-model', cacheDir, backend });
  assert.equal(prompts.length, 3);
  assert.equal(refusalReport(traces, catalog).hidden_rule_refusal, null);
  assert.notEqual(refusalReport(traces, revised).catalog_sha256, report.catalog_sha256);
});

test('a refused tool the catalog does not describe cannot be judged as stated', async () => {
  const sparse = { tools: [{ name: 'ls' }] };
  const traces = [turn('P-1', [refused('ls', 'outside_workspace')])];
  const items = refusalJudgeItems(refusalPairs(firstAttemptRefusals(traces, sparse).refusals), sparse);
  assert.deepEqual(await judgeRefusalItems({ items, model: 'm', cacheDir: tmpdir(), backend: { judge: async () => assert.fail('no call') } }), []);
  const report = refusalReport(traces, sparse);
  assert.deepEqual([report.pairs[0].described, report.pairs[0].stated, report.hidden_rule_refusal], [false, null, null]);
});
