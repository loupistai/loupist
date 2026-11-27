import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { agentRounds, main, readLedger, uncachedJudgments, worldInferences } from './ledger.mjs';

const script = fileURLToPath(new URL('./ledger.mjs', import.meta.url));

async function loop(t, cap = 100) {
  const root = await mkdtemp(join(tmpdir(), 'loop-ledger-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const ledger = join(root, 'v018x-a', 'ledger.jsonl');
  const quiet = { log: () => {}, now: () => '2026-11-27T00:00:00.000Z' };
  const run = (...argv) => main(argv, quiet);
  const file = async (name, rows) => {
    const path = join(root, name);
    await writeFile(path, `${rows.map((row) => JSON.stringify(row)).join('\n')}\n`);
    return path;
  };
  await run('cap', '--ledger', ledger, '--loop', 'A', '--cap', String(cap));
  const arm = (...extra) => run('arm', '--ledger', ledger, '--label', 'baseline', '--pin', '2ac07596', '--suite', 'release-v018',
    '--model', 'openai-codex/gpt-6.2-sol', '--trials', '1', ...extra);
  return { ledger, run, file, arm };
}

test('each kind of file counts what a call is', () => {
  assert.deepEqual(agentRounds([{ model_rounds: 4 }, { model_rounds: 7 }, { startup_failed: true, model_rounds: null }]), { calls: 11, unknown_turns: 0 });
  assert.throws(() => agentRounds([{ model_rounds: 4 }, { status: 'failed' }]), /record no model_rounds/u);
  assert.deepEqual(agentRounds([{ model_rounds: 4 }, { status: 'failed' }], 9), { calls: 13, unknown_turns: 1 });
  assert.deepEqual(worldInferences([{ kind: 'llm', stage: 'extract_nodes' }, { kind: 'embedding' }, { kind: 'llm', stage: 'resolve_edges' }, { kind: 'episode' }]), { calls: 2 });
  assert.deepEqual(uncachedJudgments([{ cache: 'miss' }, { cache: 'hit' }, {}]), { calls: 2 });
});

test('an arm sums traces, stage logs and uncached judgments, and the ledger keeps the files it counted', async (t) => {
  const { ledger, file, arm, run } = await loop(t);
  const traces = await file('traces.jsonl', [{ model_rounds: 4 }, { model_rounds: 9 }]);
  const stages = await file('stages.jsonl', [{ kind: 'llm' }, { kind: 'embedding' }, { kind: 'llm' }, { kind: 'llm' }]);
  const judgments = await file('judgments.jsonl', [{ cache: 'miss' }, { cache: 'hit' }]);
  const { row, state, exitCode } = await arm('--traces', traces, '--stage-logs', stages, '--judgments', judgments);
  assert.deepEqual(row.calls, { agent_rounds: 13, world_inferences: 3, uncached_judgments: 1, imported: 0 });
  assert.deepEqual([row.total, state.used, state.left, state.over, exitCode], [17, 17, 83, false, 0]);
  assert.deepEqual(row.files.map((item) => [item.kind, item.calls]), [['agent_rounds', 13], ['world_inferences', 3], ['uncached_judgments', 1]]);
  // The same bytes are never counted twice, under any name.
  await assert.rejects(arm('--traces', traces), /already counted/u);
  await assert.rejects(arm('--traces', await file('copy.jsonl', [{ model_rounds: 4 }, { model_rounds: 9 }])), /already counted/u);
  assert.equal((await run('sum', '--ledger', ledger)).state.used, 17);
});

test('past the cap every command exits 1, and plan refuses an arm that would pass it', async (t) => {
  const { ledger, file, arm, run } = await loop(t, 20);
  assert.equal((await run('plan', '--ledger', ledger, '--calls', '20')).exitCode, 0);
  assert.equal((await run('plan', '--ledger', ledger, '--calls', '21')).exitCode, 1);
  assert.equal((await arm('--traces', await file('one.jsonl', [{ model_rounds: 15 }]))).exitCode, 0);
  assert.equal((await run('plan', '--ledger', ledger, '--calls', '6')).exitCode, 1);
  // The calls were spent, so the row is recorded even though it passes the cap.
  const over = await arm('--traces', await file('two.jsonl', [{ model_rounds: 6 }]));
  assert.deepEqual([over.state.used, over.state.over, over.exitCode], [21, true, 1]);
  assert.equal((await run('sum', '--ledger', ledger)).exitCode, 1);
  const cli = spawnSync(process.execPath, [script, 'sum', '--ledger', ledger], { encoding: 'utf8' });
  assert.equal(cli.status, 1);
  assert.equal(JSON.parse(cli.stdout).over, true);
  // A changed cap is a new row with its reason; the latest one decides.
  await assert.rejects(run('cap', '--ledger', ledger, '--loop', 'A', '--cap', '40'), /requires --reason/u);
  await assert.rejects(run('cap', '--ledger', ledger, '--loop', 'B', '--cap', '40', '--reason', 'x'), /belongs to loop A/u);
  assert.equal((await run('cap', '--ledger', ledger, '--loop', 'A', '--cap', '40', '--reason', 'owner decision')).exitCode, 0);
  assert.equal((await run('sum', '--ledger', ledger)).state.left, 19);
});

test('an import needs a note, an unknown turn needs a charge, and an iteration records its decision', async (t) => {
  const { ledger, file, arm, run } = await loop(t);
  await assert.rejects(arm('--calls', '54'), /requires --note/u);
  await assert.rejects(arm(), /needs files to count or a --calls import/u);
  const imported = await arm('--calls', '54', '--note', 'release suite, two trials, counted by the baseline worker');
  assert.deepEqual([imported.row.calls.imported, imported.row.note, imported.state.used], [54, 'release suite, two trials, counted by the baseline worker', 54]);
  const broken = await file('broken.jsonl', [{ model_rounds: 3 }, { status: 'failed', error: 'timeout' }]);
  await assert.rejects(arm('--traces', broken), /record no model_rounds/u);
  const charged = await arm('--traces', broken, '--charge-unknown', '12');
  assert.deepEqual([charged.row.calls.agent_rounds, charged.row.unknown_turns, charged.row.charged_per_unknown_turn], [15, 1, 12]);

  await assert.rejects(run('iteration', '--ledger', ledger, '--class', 'hidden_rule_refusal', '--hypothesis', 'h', '--commit', 'abc1234',
    '--result', 'r', '--decision', 'maybe'), /kept or reverted/u);
  const first = await run('iteration', '--ledger', ledger, '--class', 'hidden_rule_refusal', '--hypothesis', 'ls does not say the folder limit',
    '--commit', 'abc1234', '--result', 'P-1 1/1 to 1/1, no sampled unit fell', '--decision', 'kept');
  const second = await run('iteration', '--ledger', ledger, '--class', 'check_overpromise', '--hypothesis', 'h', '--commit', 'def5678',
    '--result', 'R018-08 unchanged', '--decision', 'reverted');
  assert.deepEqual([first.row.n, second.row.n, second.state.iterations, second.state.used], [1, 2, 2, 69]);
  const rows = (await readFile(ledger, 'utf8')).trim().split('\n').map((line) => JSON.parse(line));
  assert.deepEqual(rows.map((row) => row.kind), ['cap', 'arm', 'arm', 'iteration', 'iteration']);
  assert.equal(readLedger(await readFile(ledger, 'utf8')).used, 69);
  // The serving mode is recorded with the arm, and only a known one.
  const fast = await arm('--calls', '1', '--note', 'one probe', '--performance', 'fast');
  assert.equal(fast.row.performance, 'fast');
  await assert.rejects(arm('--calls', '1', '--note', 'one probe', '--performance', 'turbo'), /--performance must be standard or fast/u);
});

test('a ledger without a cap, an unknown option and a missing value are refused', async (t) => {
  const { ledger, run } = await loop(t);
  const empty = join(ledger, '..', 'empty.jsonl');
  await assert.rejects(run('sum', '--ledger', empty), /has no cap/u);
  await assert.rejects(run('sum', '--ledger', ledger, '--cap', '5'), /Unknown option --cap for sum/u);
  await assert.rejects(run('plan', '--ledger', ledger, '--calls'), /requires a value/u);
  await assert.rejects(run('plan', '--ledger', ledger, '--calls', '-3'), /requires a value|non-negative/u);
  await assert.rejects(run('spend', '--ledger', ledger), /Unknown command spend/u);
  const cli = spawnSync(process.execPath, [script, 'sum', '--ledger', empty], { encoding: 'utf8' });
  assert.equal(cli.status, 2);
});
