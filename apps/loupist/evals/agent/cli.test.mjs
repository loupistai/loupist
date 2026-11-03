import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { suite, traces } from './fixtures/sample.mjs';
import { itemId, PROMPT_VERSION } from './judge.mjs';
import { DEFAULT_RUNNER, buildRunArgv, helpText, main } from './cli.mjs';

async function directory(t) {
  const path = await mkdtemp(join(tmpdir(), 'agent-eval-cli-'));
  t.after(() => rm(path, { recursive: true, force: true }));
  return path;
}

test('help lists every subcommand and the default runner stays configurable', () => {
  for (const name of ['run', 'grade', 'judge', 'label', 'calibrate', 'report', 'compare']) assert.match(helpText(), new RegExp(`\\b${name}\\b`, 'u'));
  assert.match(helpText(), /--reasoning-effort/u);
  assert.match(helpText(), /medium/u);
  const argv = buildRunArgv({ suite: 'suite.json', traces: 'traces.jsonl', extra: ['--trials', '3'] });
  assert.deepEqual(argv.slice(0, DEFAULT_RUNNER.length), DEFAULT_RUNNER);
  assert.deepEqual(argv.slice(DEFAULT_RUNNER.length), ['--suite', 'suite.json', '--out', 'traces.jsonl', '--trials', '3']);
  const custom = buildRunArgv({ runner: 'node mock-runner.mjs', suite: 'suite.json', traces: 'out.jsonl' });
  assert.deepEqual(custom, ['node', 'mock-runner.mjs', '--suite', 'suite.json', '--out', 'out.jsonl']);
});

test('run dispatches the native --out contract with explicit trial and runner options', async (t) => {
  const root = await directory(t);
  let argv;
  const suitePath = join(root, 'suite.json');
  await writeFile(suitePath, JSON.stringify(suite));
  const result = await main(['run', '--suite', suitePath, '--output', root, '--trials', '2', '--', '--thinking', 'medium'], {
    run: async (received) => { argv = received; return 0; },
  });
  assert.equal(result.output, join(root, 'traces.jsonl'));
  assert.deepEqual(argv, [...DEFAULT_RUNNER, '--suite', suitePath, '--out', result.output, '--thinking', 'medium', '--trials', '2']);
  assert.equal(argv.includes('--traces'), false);
});

test('grade, report, and compare write into the output directory without a runner', async (t) => {
  const root = await directory(t);
  const suitePath = join(root, 'suite.json');
  const tracePath = join(root, 'traces.jsonl');
  const output = join(root, 'out');
  await writeFile(suitePath, JSON.stringify(suite));
  await writeFile(tracePath, `${traces.map((row) => JSON.stringify(row)).join('\n')}\n`);
  const graded = await main(['grade', '--suite', suitePath, '--traces', tracePath, '--output', output]);
  assert.equal(graded.results.length > 0, true);
  const report = JSON.parse(await readFile(join(output, 'report.json'), 'utf8').catch(() => 'null'));
  assert.equal(report, null);
  const written = await main(['report', '--suite', suitePath, '--traces', tracePath, '--output', output]);
  assert.equal(JSON.parse(await readFile(join(output, 'report.json'), 'utf8')).tokens.total, null);
  assert.equal(written.coverage.trials, 3);
  const one = await main(['report', '--suite', suitePath, '--traces', tracePath, '--trials', '1', '--output', output]);
  assert.equal(one.coverage.trials, 1);
  assert.equal(Object.values(one.goals).some((goal) => goal.pass === true), false);
  await assert.rejects(main(['report', '--suite', suitePath, '--traces', tracePath, '--trials', '0', '--output', output]), /positive integer/u);
  const candidatePath = join(root, 'candidate.jsonl');
  const candidate = traces.map((row) => ({ ...row, run_id: `c-${row.run_id}`, model: { ...row.model, model: 'other-model' } }));
  await writeFile(candidatePath, `${candidate.map((row) => JSON.stringify(row)).join('\n')}\n`);
  const comparison = await main(['compare', '--suite', suitePath, '--baseline', tracePath, '--candidate', candidatePath, '--output', output]);
  assert.equal(comparison.paired, 0);
  assert.equal(comparison.unpaired.every((row) => row.reason === 'model_mismatch'), true);
  let ran = null;
  await main(['run', '--suite', suitePath, '--output', output, '--runner', 'node mock-runner.mjs', '--', '--trials', '1'], { run: async (argv) => { ran = argv; return 0; } });
  assert.equal(ran[0], 'node');
  assert.equal(ran.includes('--trials'), true);
  assert.equal(ran.includes('cargo'), false);
});

test('judge records the requested reasoning effort without calling a model', async (t) => {
  const root = await directory(t);
  const suitePath = join(root, 'suite.json');
  const tracePath = join(root, 'traces.jsonl');
  const output = join(root, 'out');
  const cache = join(root, 'cache');
  await writeFile(suitePath, JSON.stringify(suite));
  await writeFile(tracePath, `${JSON.stringify(traces[0])}\n`);
  let calls = 0;
  const backend = { model: 'fixture-judge', judge: async () => { calls += 1; return { pass: true, reason: 'observed in the answer' }; } };
  const high = await main(['judge', '--suite', suitePath, '--traces', tracePath, '--output', output, '--cache', cache, '--reasoning-effort', 'high'], { backend });
  const medium = await main(['judge', '--suite', suitePath, '--traces', tracePath, '--output', output, '--cache', cache], { backend });
  const again = await main(['judge', '--suite', suitePath, '--traces', tracePath, '--output', output, '--cache', cache, '--reasoning-effort', 'high'], { backend });
  assert.equal(calls, 4);
  assert.equal(high.every((row) => row.reasoning_effort === 'high' && row.cache === 'miss'), true);
  assert.equal(medium.every((row) => row.reasoning_effort === 'medium' && row.cache === 'miss'), true);
  assert.equal(again.every((row) => row.reasoning_effort === 'high' && row.cache === 'hit'), true);
});

test('invalid CLI options are rejected before invoking a runner or backend', async () => {
  let called = false;
  const io = { run: async () => { called = true; return 0; }, backend: { judge: async () => { called = true; } } };
  for (const [argv, error] of [
    [['run', '--suite'], /requires a value/u],
    [['run', '--suite', 'one', '--suite', 'two'], /only once/u],
    [['run', '--suite', 'one', '--trial', '3'], /Unknown option/u],
    [['run', '--suite', 'one', '--', '--trials', '0'], /positive integer/u],
    [['run', '--suite', 'one', '--', '--trials'], /requires a value/u],
    [['run', '--suite', 'one', '--', '--trials', '1', '--trials', '2'], /only once/u],
    [['grade', '--suite', 'one', '--'], /only by run/u],
    [['grade', '--suite', 'one'], /requires --traces/u],
  ]) await assert.rejects(main(argv, io), error);
  assert.equal(called, false);
});

const cliPath = fileURLToPath(new URL('./cli.mjs', import.meta.url));
function cli(args) {
  return spawnSync(process.execPath, [cliPath, ...args], { encoding: 'utf8' });
}

async function splitFixture(t) {
  const root = await directory(t);
  const fixture = { schema: suite.schema, suite: 'split-fixture', version: 1, scenarios: ['capability', 'regression'].map((split) => ({
    id: split, split, sessions: [{ id: 'probe', role: 'probe', memory: 'off', turns: [{
      user: 'Read the supplied fictional figure.', expect: {
        answer: [{ assert: 'contains_any', values: ['correct'], metric: 'task_success' }],
        rubric: [{ id: 'correct-answer', criterion: 'The answer correctly describes the supplied fictional figure.', metric: 'task_success' }],
      },
    }] }],
  })) };
  const rows = fixture.scenarios.map((scenario) => ({ ...traces[0], suite: fixture.suite, scenario_id: scenario.id,
    session_id: 'probe', session_role: 'probe', memory_mode: 'off', user: scenario.sessions[0].turns[0].user,
    answer: scenario.split === 'capability' ? 'wrong' : 'correct', tool_calls: [], memory_commits: [],
  }));
  const suitePath = join(root, 'suite.json');
  const tracePath = join(root, 'traces.jsonl');
  await writeFile(suitePath, JSON.stringify(fixture));
  await writeFile(tracePath, rows.map(JSON.stringify).join('\n'));
  return { root, fixture, rows, suitePath, tracePath, args: ['--suite', suitePath, '--traces', tracePath, '--output', join(root, 'out')] };
}

test('all suite commands refuse heldout access without explicit authorization, including symlinks', async (t) => {
  const f = await splitFixture(t);
  const heldoutPath = join(f.root, 'evals/agent/heldout/fixture-heldout-v1.json');
  await mkdir(join(f.root, 'evals/agent/heldout'), { recursive: true });
  const heldout = { ...f.fixture, scenarios: [{ ...f.fixture.scenarios[0], split: 'heldout' }] };
  await writeFile(heldoutPath, JSON.stringify(heldout));
  const aliasPath = join(f.root, 'alias.json');
  await symlink(heldoutPath, aliasPath);
  for (const command of ['run', 'grade', 'judge', 'report', 'calibrate', 'compare']) {
    for (const path of [heldoutPath, aliasPath]) {
      const args = [command, '--suite', path, '--output', join(f.root, 'out')];
      if (['grade', 'judge', 'report', 'calibrate'].includes(command)) args.push('--traces', f.tracePath);
      const result = cli(args);
      assert.equal(result.status, 2, result.stderr);
      assert.match(result.stderr, /--allow-heldout/u);
    }
  }
  for (const command of ['run', 'grade', 'judge', 'report', 'calibrate']) {
    const result = cli([command, '--split', 'heldout']);
    assert.equal(result.status, 2, result.stderr);
  }
  const allowed = cli(['grade', '--suite', heldoutPath, '--traces', f.tracePath, '--allow-heldout', '--split', 'heldout', '--output', join(f.root, 'allowed')]);
  assert.equal(allowed.status, 0, allowed.stderr);
  await writeFile(f.suitePath, JSON.stringify(heldout));
  assert.equal(cli(['grade', ...f.args]).status, 2);
  const misplaced = cli(['grade', ...f.args, '--allow-heldout']);
  assert.equal(misplaced.status, 1);
  assert.match(misplaced.stderr, /belong only under/u);
});

test('split filters grade, report and judge inputs and run scenarios without a live model', async (t) => {
  const f = await splitFixture(t);
  const graded = await main(['grade', ...f.args, '--split', 'regression']);
  assert.deepEqual(graded.results.map((row) => row.scenario_id), ['regression']);
  assert.equal(graded.unmatched.length, 0);
  const judgments = await main(['judge', ...f.args, '--split', 'regression'], {
    backend: { model: 'fixture', judge: async () => ({ pass: true, reason: 'Correct fixture answer.' }) },
  });
  assert.deepEqual(judgments.map((row) => row.scenario_id), ['regression']);
  const report = await main(['report', ...f.args, '--split', 'regression', '--trials', '1']);
  assert.equal(report.traces, 1);
  assert.equal(report.coverage.expected_turns, 1);
  await main(['run', '--suite', f.suitePath, '--output', join(f.root, 'run'), '--split', 'regression'], {
    run: async (argv) => {
      const selected = JSON.parse(await readFile(argv[argv.indexOf('--suite') + 1], 'utf8'));
      assert.deepEqual(selected.scenarios.map((scenario) => scenario.id), ['regression']);
      return 0;
    },
  });
  await assert.rejects(main(['run', '--suite', f.suitePath, '--split', 'typo']), /--split must be/u);
});

test('regression gate exits 1 only for failed regression checks and prints their identities', async (t) => {
  const f = await splitFixture(t);
  assert.equal(cli(['grade', ...f.args, '--fail-on-regression']).status, 0);
  f.rows[1].answer = 'wrong';
  await writeFile(f.tracePath, f.rows.map(JSON.stringify).join('\n'));
  const failed = cli(['grade', ...f.args, '--fail-on-regression']);
  assert.equal(failed.status, 1, failed.stderr);
  assert.match(failed.stderr, /regression probe#0 contains_any/u);
  assert.equal(cli(['grade', ...f.args]).status, 0);
  assert.equal(cli(['grade', ...f.args, '--split', 'capability', '--fail-on-regression']).status, 0);
});

test('selected runs retain replay paths for both ordinary and heldout files', async (t) => {
  const f = await splitFixture(t);
  const agentRoot = join(f.root, 'evals/agent');
  await mkdir(join(agentRoot, 'heldout'), { recursive: true });
  await mkdir(join(agentRoot, 'suites/replay'), { recursive: true });
  const replayPath = join(agentRoot, 'suites/replay/fixture.json');
  await writeFile(replayPath, '{"requests":[]}');
  for (const split of ['capability', 'heldout']) {
    const suitePath = join(agentRoot, split === 'heldout' ? 'heldout/fixture-heldout-v1.json' : 'suites/fixture-v1.json');
    await writeFile(suitePath, JSON.stringify({ ...f.fixture, scenarios: [{ ...f.fixture.scenarios[0], split, replay: 'replay/fixture.json' }] }));
    await main(['run', '--suite', suitePath, '--output', join(f.root, split), '--split', split, '--allow-heldout'], {
      run: async (argv) => {
        const selected = JSON.parse(await readFile(argv[argv.indexOf('--suite') + 1], 'utf8'));
        assert.equal(selected.scenarios[0].replay, 'replay/fixture.json');
        const stagedReplay = join(dirname(argv[argv.indexOf('--suite') + 1]), selected.scenarios[0].replay);
        assert.deepEqual(JSON.parse(await readFile(stagedReplay, 'utf8')), { requests: [] });
        return 0;
      },
    });
  }
});

test('selected runs reject replay traversal and runner suite overrides before dispatch', async (t) => {
  const f = await splitFixture(t);
  for (const replay of ['../outside.json', '/tmp/outside.json', '.loupist/private.json']) {
    f.fixture.scenarios[0].replay = replay;
    await writeFile(f.suitePath, JSON.stringify(f.fixture));
    await assert.rejects(main(['run', '--suite', f.suitePath, '--split', 'capability', '--output', f.root], {
      run: async () => { assert.fail('must not dispatch'); },
    }), /Replay paths must be relative/u);
  }
  await assert.rejects(main(['run', '--suite', f.suitePath, '--', '--suite', 'replacement.json']), /cannot override/u);
});


test('judge blocks same-family requested and observed models, with an explicit warning override', async (t) => {
  const f = await splitFixture(t);
  let calls = 0;
  const warnings = [];
  const backend = { model: 'grok-4.7', judge: async () => { calls += 1; return { pass: true, reason: 'fixture' }; } };
  for (const modelFields of [
    { model: { provider: 'xai', model: 'grok-4' }, observed_model: null },
    { model: { provider: 'custom', model: 'unknown' }, observed_model: 'xai/grok-4' },
  ]) {
    await writeFile(f.tracePath, JSON.stringify({ ...f.rows[0], ...modelFields }));
    const blocked = cli(['judge', ...f.args]);
    assert.equal(blocked.status, 2, blocked.stderr);
    assert.match(blocked.stderr, /shares model family xai.*--allow-same-family/u);
    await assert.rejects(main(['judge', ...f.args], { backend }), (error) => error.exitCode === 2);
  }
  assert.equal(calls, 0);
  const allowed = await main(['judge', ...f.args, '--allow-same-family'], { backend, warn: (message) => warnings.push(message) });
  assert.equal(calls, 1);
  assert.equal(allowed.length, 1);
  assert.match(warnings[0], /Warning:.*shares model family/u);
  await main(['judge', ...f.args], { backend: { ...backend, model: 'unknown' } });
  assert.equal(calls, 2);
  await main(['judge', ...f.args], { backend: { ...backend, model: 'claude-fixture' } });
  assert.equal(calls, 3);
});

async function calibrationFixture(t) {
  const f = await splitFixture(t);
  const rows = f.rows.flatMap((row) => Array.from({ length: 30 }, (_, trial) => ({ ...row, trial, run_id: `run-${trial}` })));
  const judgments = rows.map((row) => ({ ...row, item_id: itemId(row, 'correct-answer'), criterion_id: 'correct-answer',
    metric: 'task_success', pass: row.trial < 15, model: 'grok-fixture', prompt_version: PROMPT_VERSION, reasoning_effort: 'medium' }));
  const labels = judgments.map((row) => ({ item_id: row.item_id, criterion_id: row.criterion_id, human_pass: row.pass }));
  const judgmentsPath = join(f.root, 'judgments.jsonl');
  const labelsPath = join(f.root, 'labels.jsonl');
  await writeFile(f.tracePath, rows.map(JSON.stringify).join('\n'));
  await writeFile(judgmentsPath, judgments.map(JSON.stringify).join('\n'));
  await writeFile(labelsPath, labels.map(JSON.stringify).join('\n'));
  return { ...f, rows, judgments, labels, judgmentsPath, labelsPath,
    args: [...f.args, '--judgments', judgmentsPath, '--labels', labelsPath] };
}

test('calibrate writes versioned metadata, balanced breakdowns, and process exit codes 0, 3 and 2', async (t) => {
  const f = await calibrationFixture(t);
  const out = join(f.root, 'nested/judge-calibration.json');
  const args = ['calibrate', ...f.args, '--out', out];
  const ready = cli(args);
  assert.equal(ready.status, 0, ready.stderr);
  const saved = JSON.parse(await readFile(out, 'utf8'));
  assert.deepEqual(Object.keys(saved).sort(), ['schema', 'judge_model', 'prompt_version', 'reasoning_effort', 'created_at', 'overall', 'by_criterion', 'by_metric', 'ready', 'reasons'].sort());
  assert.equal(saved.schema, 'loupist-judge-calibration/1');
  assert.equal(saved.judge_model, 'grok-fixture');
  assert.equal(saved.prompt_version, PROMPT_VERSION);
  assert.equal(saved.reasoning_effort, 'medium');
  assert.ok(Number.isFinite(Date.parse(saved.created_at)));
  assert.equal(saved.ready, true);
  assert.equal(saved.overall.counts.matched, 60);
  assert.equal(saved.by_criterion['correct-answer'].status, 'pass');
  assert.equal(saved.by_metric.task_success.tnr, 1);
  assert.ok(saved.overall.wilson_95.tpr.low > 0);
  await writeFile(f.labelsPath, f.labels.map((row) => JSON.stringify({ ...row, human_pass: true })).join('\n'));
  const unready = cli(args);
  assert.equal(unready.status, 3, unready.stderr);
  const failed = JSON.parse(await readFile(out, 'utf8'));
  assert.equal(failed.ready, false);
  assert.ok(failed.reasons.includes('negatives < 10'));
  assert.equal(cli(['calibrate']).status, 2);
  assert.equal(cli([...args, '--typo']).status, 2);
  assert.equal(cli(['calibrate', ...f.args, '--out']).status, 2);
});

test('calibrate filters splits and supports metadata flags only when row metadata is absent', async (t) => {
  const f = await calibrationFixture(t);
  const selected = await main(['calibrate', ...f.args, '--split', 'regression']);
  assert.equal(selected.overall.counts.matched, 30);
  assert.equal(selected.overall.counts.labels, 30);
  assert.equal(selected.ready, true);
  assert.equal(JSON.parse(await readFile(join(f.root, 'out/judge-calibration.json'), 'utf8')).ready, true);
  const noMetadata = f.judgments.map(({ model: _model, prompt_version: _prompt, reasoning_effort: _effort, ...row }) => row);
  await writeFile(f.judgmentsPath, noMetadata.map(JSON.stringify).join('\n'));
  const missing = cli(['calibrate', ...f.args]);
  assert.equal(missing.status, 2);
  assert.match(missing.stderr, /Missing judge model/u);
  const flags = ['--model', 'claude-fixture', '--prompt-version', 'fixture/1', '--reasoning-effort', 'low'];
  const fallback = await main(['calibrate', ...f.args, ...flags]);
  assert.equal(fallback.judge_model, 'claude-fixture');
  assert.equal(fallback.prompt_version, 'fixture/1');
  assert.equal(fallback.reasoning_effort, 'low');
  for (const field of ['model', 'prompt_version', 'reasoning_effort']) {
    const mixed = f.judgments.map((row, index) => index === 0 ? { ...row, [field]: 'different' } : row);
    await writeFile(f.judgmentsPath, mixed.map(JSON.stringify).join('\n'));
    const rejected = cli(['calibrate', ...f.args, ...flags]);
    assert.equal(rejected.status, 2);
    assert.match(rejected.stderr, /requires a single judge/u);
  }
  await writeFile(f.judgmentsPath, f.judgments.map(JSON.stringify).join('\n'));
  await writeFile(f.tracePath, '');
  assert.equal(cli(['calibrate', ...f.args]).status, 2);
});

test('calibrate accepts explicitly authorized heldout fixtures', async (t) => {
  const f = await calibrationFixture(t);
  const root = join(f.root, 'evals/agent/heldout');
  await mkdir(root, { recursive: true });
  const path = join(root, 'fixture-heldout-v1.json');
  await writeFile(path, JSON.stringify({ ...f.fixture, scenarios: f.fixture.scenarios.map((scenario) => ({ ...scenario, split: 'heldout' })) }));
  const args = ['calibrate', ...f.args.map((arg) => arg === f.suitePath ? path : arg), '--split', 'heldout'];
  assert.equal(cli(args).status, 2);
  const allowed = cli([...args, '--allow-heldout']);
  assert.equal(allowed.status, 0, allowed.stderr);
});
