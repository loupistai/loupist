import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { grade } from './grade.mjs';
import { buildJudgeInput, judgeItems, renderJudgePrompt } from './judge.mjs';
import { checkDryRunEvidence, progressLanguageEvidence, scheduledRunAssertion, scheduledRunEvidence } from './run-evidence.mjs';

const load = async (name) => JSON.parse(await readFile(new URL(`./suites/${name}`, import.meta.url), 'utf8'));
const release = await load('release-v018.json');
const global = await load('global-v1.json');
const learning = await load('learning-programs-v1.json');

/** One completed turn of a suite scenario as the runner would record it. */
function observed(suite, id, fields = {}, sessionIndex = 0) {
  const scenario = suite.scenarios.find((item) => item.id === id);
  const session = scenario.sessions[sessionIndex];
  return { schema: 'loupist-agent-trace/1', run_id: 'run', suite: suite.suite, scenario_id: id, session_id: session.id, turn_index: 0, trial: 0,
    status: 'completed', session_role: session.role, memory_mode: session.memory, user: session.turns[0].user, answer: 'Done.',
    tool_calls: [], memory_commits: [], memory_reads: [], lens: { pages: [] }, scheduled_tasks: [], ...fields };
}
const verdict = (suite, trace, name) => grade(suite, [trace]).results.find((row) => row.assert === name);
const budget = (outcome) => ({ requests: 13, soft: 4, hard: 5, left_at_start: 13, outcome });

test('scheduled_run reads the stored run record and leaves a missing field unmeasured', () => {
  const run = { status: 'succeeded', quiet: true, budget: budget('none'), usage: { input: 10 } };
  const expected = { assert: 'scheduled_run', status: 'succeeded', quiet: true };
  assert.equal(scheduledRunEvidence(expected, { scheduled_run: run }).pass, true);
  assert.deepEqual(scheduledRunEvidence(expected, { scheduled_run: { ...run, quiet: false } }), { pass: false, detail: 'quiet false, expected true' });
  assert.equal(scheduledRunEvidence(expected, { scheduled_run: { ...run, status: 'failed' } }).pass, false);
  assert.equal(scheduledRunEvidence(expected, {}).pass, null);
  assert.equal(scheduledRunEvidence(expected, { scheduled_run: { status: 'succeeded' } }).pass, null);
  // The budget outcome is the stored record's, whether the runner nests it or names it.
  assert.equal(scheduledRunEvidence({ assert: 'scheduled_run', budget: 'soft' }, { scheduled_run: { ...run, budget: budget('soft') } }).pass, true);
  assert.equal(scheduledRunEvidence({ assert: 'scheduled_run', budget: 'soft' }, { scheduled_run: { ...run, budget: 'refused' } }).pass, false);
  for (const invalid of [{}, { status: 'done' }, { quiet: 'yes' }, { budget: 'hard' }, { after_soft_note: false }, { after_soft_note: true, budget: 'soft' }]) {
    assert.throws(() => scheduledRunAssertion({ assert: 'scheduled_run', ...invalid }));
  }
});

test('a fixture built to reach the soft note is unmeasured when the run stays under it and fails past the hard level', () => {
  const trace = (outcome, extra = {}) => observed(learning, 'LP-005', { scheduled_run: { status: 'succeeded', quiet: false, budget: budget(outcome), ...extra } });
  assert.deepEqual([verdict(learning, trace('soft'), 'scheduled_run').pass, verdict(learning, trace('none'), 'scheduled_run').pass], [true, null]);
  assert.match(verdict(learning, trace('none'), 'scheduled_run').detail, /never sent/u);
  for (const outcome of ['refused', 'cancelled']) assert.equal(verdict(learning, trace(outcome), 'scheduled_run').pass, false, outcome);
  // A run that did not succeed fails whatever its budget says.
  assert.equal(verdict(learning, trace('none', { status: 'failed' }), 'scheduled_run').pass, false);
});

test('the quiet control passes on a quiet run that read the feed and wrote nothing, and fails each other way', () => {
  const feed = { name: 'world_changes', outcome: 'ok', args: { since: 'previous_run' } };
  const quiet = (fields = {}) => observed(release, 'R018-10', { answer: 'Nothing changed since the last run.',
    scheduled_run: { status: 'succeeded', quiet: true, budget: budget('none') }, tool_calls: [feed], lens: { pages: [] }, ...fields });
  const passes = (trace) => grade(release, [trace]).results.map((row) => [row.assert, row.pass]);
  assert.deepEqual(passes(quiet()), [['scheduled_run', true], ['tool_called', true], ['lens_unchanged', true], ['not_committed', true]]);
  assert.equal(verdict(release, quiet({ scheduled_run: { status: 'succeeded', quiet: false, budget: budget('none') } }), 'scheduled_run').pass, false);
  assert.equal(verdict(release, quiet({ tool_calls: [{ ...feed, args: { since: '2026-10-26' } }] }), 'tool_called').pass, false);
  assert.equal(verdict(release, quiet({ memory_commits: [{ kind: 'lens', record_id: 'rec', title: 'Alderquay Bearings research' }] }), 'not_committed').pass, false);
  // The same fixture under its learning-suite id grades the same way.
  const twin = { ...quiet(), suite: learning.suite, scenario_id: 'LP-003' };
  assert.deepEqual(grade(learning, [twin]).results.map((row) => row.pass), [true, true, true, true]);
});

test('check_dry_run reads a replay of a seeded check or the dry run a saved check returned', () => {
  const expected = { assert: 'check_dry_run', met_on: ['2026-11-24'], not_met_on: ['2026-11-23'], tools: ['world_changes'], no_tools: ['web_fetch'] };
  const replay = (days, tools = ['world_changes']) => ({ trigger_dry_run: { days, tools } });
  assert.equal(checkDryRunEvidence(expected, replay(['2026-11-24']), []).pass, true);
  assert.equal(checkDryRunEvidence(expected, replay([{ date: '2026-11-23', met: false }, { date: '2026-11-24', met: true }]), []).pass, true);
  assert.match(checkDryRunEvidence(expected, replay(['2026-11-23', '2026-11-24']), []).detail, /^met on 2026-11-23$/u);
  assert.match(checkDryRunEvidence(expected, replay([]), []).detail, /not met on 2026-11-24/u);
  assert.match(checkDryRunEvidence(expected, replay(['2026-11-24'], ['world_changes', 'web_fetch']), []).detail, /read web_fetch/u);
  assert.match(checkDryRunEvidence(expected, replay(['2026-11-24'], []), []).detail, /did not read world_changes/u);
  assert.equal(checkDryRunEvidence(expected, {}, []).pass, null);
  // A check the model saved: the host's dry run comes back in the tool result, without a tool list.
  const dry = { today_met: false, fired_on: ['2026-11-24'], replayed: 20, unreplayed: 0, days: [{ date: '2026-11-24', met: true }], reads: 22 };
  const saved = (result, outcome = 'ok') => [{ name: 'trigger_save', outcome, args: { template: 'world-change' }, result: { content: [{ type: 'text', text: JSON.stringify(result) }] } }];
  assert.equal(checkDryRunEvidence({ assert: 'check_dry_run', met_on: ['2026-11-24'] }, {}, saved({ action: 'created', dry_run: dry })).pass, true);
  assert.equal(checkDryRunEvidence({ assert: 'check_dry_run', met_on: ['2026-11-24'] }, {}, saved({ action: 'created', dry_run: { ...dry, fired_on: [] } })).pass, false);
  assert.equal(checkDryRunEvidence(expected, {}, saved({ action: 'created', dry_run: dry })).pass, null, 'the tool list is unknown');
  assert.equal(checkDryRunEvidence({ assert: 'check_dry_run', met_on: ['2026-11-24'] }, {}, saved({ dry_run: dry }, 'error')).pass, null, 'a refused save tested nothing');
});

test('the guidance check passes on a saved world-change check met on the day of the update', () => {
  const dry = { fired_on: ['2026-11-24'], days: [], reads: 3, replayed: 20, unreplayed: 0 };
  const save = (fired_on, template = 'world-change') => ({ name: 'trigger_save', outcome: 'ok', args: { template, when_met_summary: 'A change to Alderquay Bearings guidance starts a follow-up.' },
    result: { content: [{ type: 'text', text: JSON.stringify({ action: 'created', dry_run: { ...dry, fired_on } }) }] } });
  const run = (calls) => grade(release, [observed(release, 'R018-08', { answer: 'I proposed a check.', tool_calls: calls })]).results.map((row) => [row.assert, row.pass]);
  assert.deepEqual(run([save(['2026-11-24'])]), [['tool_called', true], ['check_dry_run', true], ['tool_not_called', true], ['tool_not_called', true]]);
  assert.deepEqual(run([save([])]).slice(0, 2), [['tool_called', true], ['check_dry_run', false]]);
  assert.deepEqual(run([save(['2026-11-24'], 'new-item')]).slice(0, 2), [['tool_called', false], ['check_dry_run', true]]);
  // The German guidance request is graded on the day its own fixture's update was published.
  const german = observed(global, 'G-004', { answer: 'Ich habe eine Prüfung vorgeschlagen, die bei einer geänderten Prognose einen Folgelauf startet.', tool_calls: [save(['2026-10-28'])] });
  assert.deepEqual(grade(global, [german]).results.map((row) => [row.assert, row.pass]), [['tool_called', true], ['check_dry_run', true], ['answer_language', true]]);
});

test('progress_language reads the notes the user saw, in the user\'s language whatever the interface', () => {
  const notes = (...texts) => ({ progress_notes: texts.map((text, index) => ({ at_ms: index * 100, text })) });
  const korean = { assert: 'progress_language', language: 'ko', min: 1 };
  assert.equal(progressLanguageEvidence(korean, notes('저장된 자료에서 미래타일 가마 운영 기록을 찾고 있습니다.', '폐열 비중을 확인했습니다.')).pass, true);
  assert.equal(progressLanguageEvidence(korean, notes('Looking through the saved Sources for the Mirae Tile kiln note.')).pass, false);
  assert.deepEqual(progressLanguageEvidence(korean, notes()), { pass: false, detail: '0 progress notes, need at least 1' });
  assert.equal(progressLanguageEvidence(korean, {}).pass, null);
  assert.equal(progressLanguageEvidence({ assert: 'progress_language', language: 'ko', min: 0 }, notes()).pass, null);
  // Through the suites: the Korean release unit, and a Japanese user under the English interface.
  const r04 = (fields) => verdict(release, observed(release, 'R018-04', fields), 'progress_language');
  assert.equal(r04(notes('미래타일 가마 운영 기록을 읽고 있습니다.')).pass, true);
  assert.equal(r04(notes('Reading the Mirae Tile kiln operations note.')).pass, false);
  assert.equal(r04({}).pass, null);
  assert.equal(r04(notes()).pass, null, 'a turn that shows no note is unmeasured, not failed');
  const g001 = (fields) => verdict(global, observed(global, 'G-001', fields), 'progress_language');
  assert.equal(g001(notes('灯台電子の決算短信を確認しています。営業利益の記載を読み取りました。')).pass, true);
  assert.equal(g001(notes('Reading the Todai Denshi earnings summary for the operating profit figure.')).pass, false);
  assert.equal(g001(notes()).pass, null, 'a short answer may show no note');
});

test('the judge sees progress notes, the run record and the dry run only when the trace recorded them', () => {
  const input = (suite, trace) => buildJudgeInput(suite, judgeItems(suite, [trace])[0], [trace]);
  const plain = input(release, observed(release, 'R018-04'));
  assert.deepEqual(['progress_notes', 'scheduled_run', 'trigger_dry_run'].filter((field) => Object.hasOwn(plain, field)), []);
  const noted = input(release, observed(release, 'R018-04', { progress_notes: [{ at_ms: 5, text: '자료를 읽는 중입니다.' }, { at_ms: 9, text: '근거를 확인했습니다.' }] }));
  assert.deepEqual(noted.progress_notes, ['자료를 읽는 중입니다.', '근거를 확인했습니다.']);
  assert.match(renderJudgePrompt(noted), /The progress_notes are the notes the user saw/u);
  assert.doesNotMatch(renderJudgePrompt(plain), /progress_notes|scheduled_run|trigger_dry_run/u);
  const run = { status: 'succeeded', quiet: false, budget: budget('none') };
  const ran = input(release, observed(release, 'R018-09', { scheduled_run: run, trigger_dry_run: { days: [], tools: [] } }));
  assert.deepEqual([ran.scheduled_run, ran.trigger_dry_run], [run, { days: [], tools: [] }]);
  // Nothing of the fixture's seeds, scripts or expectations reaches the judge.
  assert.doesNotMatch(JSON.stringify(ran), /q3-cfo|same_assertion|previous_run"|lens_pages"|section_matches/u);
});
