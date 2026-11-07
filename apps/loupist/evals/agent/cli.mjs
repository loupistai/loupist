#!/usr/bin/env node
/** Agent-eval commands. The headless runner is configurable and is not required for grading. */
import { spawn } from 'node:child_process';
import { mkdir, readFile, realpath, writeFile } from 'node:fs/promises';
import { dirname, isAbsolute, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { stdin as input, stdout as output } from 'node:process';
import { createInterface } from 'node:readline/promises';
import { assertScenarioSplit, isHeldoutSuitePath } from './suite-contract.mjs';
import { grade } from './grade.mjs';
import { validateSuite, validateTraceRecord, positiveTrialCount } from './eval-contract.mjs';
import { renderLensPages } from './lens-review.mjs';
import { judgeAll, judgeItems, itemId, modelFamily, grokBackend, codexBackend, openAIBackend, DEFAULT_JUDGE_MODEL, DEFAULT_REASONING_EFFORT } from './judge.mjs';
import { agreement, importLabels, labelInteractive, labelViews } from './labels.mjs';
import { compareReports, comparisonMarkdown } from './compare-reports.mjs';
import { buildReport, compareRuns } from './report.mjs';
import { assessAdoption } from './adopt.mjs';

export const SUBCOMMANDS = ['run', 'grade', 'judge', 'label', 'lens-pages', 'calibrate', 'report', 'compare', 'adopt'];
export const DEFAULT_RUNNER = [
  'cargo', 'run', '--manifest-path', 'apps/loupist/src-tauri/Cargo.toml',
  '--locked', '--offline', '--features', 'eval-harness', '--example', 'agent_eval', '--',
];

const outputRoot = fileURLToPath(new URL('../output/', import.meta.url));
const COMMAND_OPTIONS = {
  run: ['suite', 'runner', 'trials', 'split'],
  grade: ['suite', 'traces', 'split', 'fail-on-regression'],
  judge: ['split', 'suite', 'traces', 'model', 'reasoning-effort', 'endpoint', 'backend', 'api-key-env', 'cache', 'concurrency', 'allow-same-family'],
  calibrate: ['suite', 'traces', 'judgments', 'labels', 'out', 'split', 'model', 'prompt-version', 'reasoning-effort'],
  label: ['import', 'traces', 'judgments'],
  'lens-pages': ['traces', 'out'],
  report: ['split', 'suite', 'traces', 'judgments', 'trials'],
  adopt: ['baseline', 'candidate', 'security-traces', 'trials', 'reason'],
  compare: ['suite', 'baseline', 'candidate', 'baseline-judgments', 'candidate-judgments', 'trials'],
};

export function helpText() {
  return [
    'Loupist agent evals',
    '',
    'subcommands:',
    '  run       Run the configurable headless agent runner',
    '  grade     Grade trace, memory, and answer assertions',
    '  judge     Binary LLM judge for rubrics and save_precision',
    '  label     Label judged items on a TTY, or import labels',
    '  lens-pages Render each run\'s final Lens pages and per-turn ops as Markdown for review',
    '  calibrate Save judge calibration against human labels',
    '  report    Aggregate metrics, latency, and tokens',
    '  compare   Compare report JSON summaries, or paired traces with --suite',
    '  adopt     Check candidate reports against baseline, security traces, and input-token growth',
    '            --baseline report.json --candidate report.json --security-traces traces.jsonl [--trials 2] [--reason text]',
    '',
    'Outputs default to apps/loupist/evals/output/<timestamp>/.',
    'Judge readiness requires 30 matched labels, 10 positives, 10 negatives, TPR >= 0.8 and TNR >= 0.8.',
    'calibrate requires --suite, --traces, --judgments, --labels; --out selects judge-calibration.json.',
    'Same-family judging requires --allow-same-family; unknown model families never block.',
    'Judge --reasoning-effort defaults to medium and is stored on each judgment and in the cache key.',
    'run/report/compare --trials N defaults to 3; completion goals require all three trials.',
    'run passes --suite, --out, and --trials to the default or custom runner.',
    'run/grade/judge/report/calibrate --split capability|regression|heldout filters scenarios and traces.',
    'Held-out files and --split heldout require --allow-heldout (adoption checks only; otherwise exit 2).',
    'grade --fail-on-regression exits 1 and prints scenario/check IDs for failed regression checks.',
    'Without --split, reports cover every suite turn.',
  ].join('\n');
}

export function defaultOutputDir(now = new Date()) {
  return resolve(outputRoot, now.toISOString().replace(/[:.]/gu, '-'));
}

export function buildRunArgv({ runner, suite, traces, extra = [] } = {}) {
  if (!suite || !traces) throw new Error('run requires a suite and a traces path');
  const base = runner ? String(runner).split(/\s+/u).filter(Boolean) : [...DEFAULT_RUNNER];
  return [...base, '--suite', suite, '--out', traces, ...extra];
}

function parseArgv(argv) {
  const [command, ...rest] = argv;
  const options = { extra: [] };
  const allowed = new Set(['output', 'help', 'allow-heldout', ...(COMMAND_OPTIONS[command] ?? [])]);
  for (let index = 0; index < rest.length; index += 1) {
    const key = rest[index];
    if (key === '--') {
      if (command !== 'run') throw new Error('Runner arguments are supported only by run');
      options.extra = rest.slice(index + 1);
      break;
    }
    if (!key.startsWith('--')) throw new Error(`Unexpected argument: ${key}`);
    const name = key.slice(2);
    if (!allowed.has(name)) throw new Error(`Unknown option ${key} for ${command}`);
    if (Object.hasOwn(options, name)) throw new Error(`Specify ${key} only once`);
    if (['help', 'allow-heldout', 'fail-on-regression', 'allow-same-family'].includes(name)) { options[name] = true; continue; }
    const next = rest[index + 1];
    if (!next || next.startsWith('--')) throw new Error(`${key} requires a value`);
    options[name] = rest[++index];
  }
  return { command, options };
}

async function readJson(path) {
  return JSON.parse(await readFile(path, 'utf8'));
}

async function readJsonl(path) {
  const text = await readFile(path, 'utf8');
  return text.split('\n').filter((line) => line.trim()).map((line) => JSON.parse(line));
}

function jsonl(rows) {
  return rows.map((row) => JSON.stringify(row)).join('\n') + (rows.length ? '\n' : '');
}

function trialCount(value) {
  if (value === true) throw new Error('--trials must be a positive integer');
  return positiveTrialCount(value == null ? 3 : Number(value));
}

async function emit(directory, name, contents) {
  const path = resolve(directory, name);
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, contents, { mode: 0o600 });
  return path;
}

function runProcess(argv) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(argv[0], argv.slice(1), { stdio: 'inherit' });
    child.on('error', reject);
    child.on('close', (code) => resolvePromise(code ?? 1));
  });
}

function heldoutGuard(options, heldout) {
  if (heldout && !options['allow-heldout']) {
    const error = new Error('Held-out scenarios are for adoption checks only; pass --allow-heldout to load them.');
    error.exitCode = 2;
    throw error;
  }
}

async function loadSuite(options) {
  const requested = resolve(options.suite);
  heldoutGuard(options, isHeldoutSuitePath(requested));
  const path = await realpath(requested);
  heldoutGuard(options, isHeldoutSuitePath(path));
  const original = validateSuite(await readJson(path));
  heldoutGuard(options, original.scenarios.some((scenario) => scenario.split === 'heldout'));
  for (const scenario of original.scenarios) assertScenarioSplit(scenario, path);
  const suite = options.split
    ? { ...original, scenarios: original.scenarios.filter((scenario) => scenario.split === options.split) }
    : original;
  return { suite, path };
}

function selectRows(rows, suite, split) {
  if (!split) return rows;
  const ids = new Set(suite.scenarios.map((scenario) => scenario.id));
  return rows.filter((row) => row.suite === suite.suite && ids.has(row.scenario_id));
}

export async function main(argv = process.argv.slice(2), io = {}) {
  const { command, options } = parseArgv(argv);
  if (!command || command === '--help' || command === '-h' || options.help) {
    console.log(helpText());
    return { help: true };
  }
  if (!SUBCOMMANDS.includes(command)) throw new Error(`Unknown command ${command}`);
  if (options.split && !['capability', 'regression', 'heldout'].includes(options.split)) throw new Error('--split must be capability, regression, or heldout');
  heldoutGuard(options, options.split === 'heldout');
  if (options.suite) heldoutGuard(options, isHeldoutSuitePath(options.suite));
  const directory = resolve(options.output ?? defaultOutputDir());
  if (command === 'run') {
    const traces = resolve(directory, 'traces.jsonl');
    if (options.trials != null && options.extra.includes('--trials')) throw new Error('Specify --trials only once');
    const extra = options.extra.includes('--trials') ? options.extra : [...options.extra, '--trials', String(trialCount(options.trials))];
    const trialFlags = extra.flatMap((value, index) => value === '--trials' ? [index] : []);
    if (trialFlags.length !== 1) throw new Error('Specify --trials only once');
    if (extra[trialFlags[0] + 1] == null) throw new Error('--trials requires a value');
    trialCount(extra[trialFlags[0] + 1]);
    if (extra.some((arg) => ['--suite', '--out', '--split', '--allow-heldout'].some((flag) => arg === flag || arg.startsWith(`${flag}=`)))) {
      throw new Error('Runner arguments cannot override suite, output, or split selection');
    }
    if (!options.suite) throw new Error('--suite is required');
    const { suite, path } = await loadSuite(options);
    if (!suite.scenarios.length) throw new Error(`No scenarios match split ${options.split}`);
    let runnerSuite = options.suite;
    if (options.split || isHeldoutSuitePath(path)) {
      const replayRoot = isHeldoutSuitePath(path) ? resolve(dirname(path), '../suites') : dirname(path);
      for (const scenario of suite.scenarios) {
        if (scenario.replay === undefined) continue;
        const replay = scenario.replay;
        if (typeof replay !== 'string' || isAbsolute(replay) || replay.split(/[\\/]/u).some((part) => !part || part === '.' || part === '..') || replay.split(/[\\/]/u)[0] === '.loupist') {
          throw new Error('Replay paths must be relative without parent traversal');
        }
        await emit(directory, replay, await readFile(resolve(replayRoot, replay)));
      }
      runnerSuite = await emit(directory, 'selected-suite.json', `${JSON.stringify(suite, null, 2)}\n`);
    }
    const runnerArgv = buildRunArgv({ runner: options.runner, suite: runnerSuite, traces, extra });
    const code = await (io.run ?? runProcess)(runnerArgv);
    if (code !== 0) throw new Error(`Runner exited ${code}`);
    console.log(JSON.stringify({ output: traces, runner: runnerArgv[0] }));
    return { output: traces };
  }
  if (command === 'lens-pages') {
    if (!options.traces) throw new Error('lens-pages requires --traces');
    const path = await emit(options.out ? dirname(resolve(options.out)) : directory, options.out ? resolve(options.out) : 'lens-pages.md',
      renderLensPages(await readJsonl(resolve(options.traces))));
    console.log(JSON.stringify({ output: path }));
    return { output: path };
  }
  if (command === 'label') {
    let labels;
    if (options.import) labels = importLabels(await readFile(resolve(options.import), 'utf8'));
    else {
      if (!options.judgments) throw new Error('label requires --judgments or --import');
      if (!input.isTTY && !io.ask) throw new Error('Interactive labeling needs a TTY. Use --import for a labels JSONL file.');
      const traces = options.traces ? await readJsonl(resolve(options.traces)) : [];
      const judgments = await readJsonl(resolve(options.judgments));
      const ask = io.ask ?? (async (prompt) => {
        const rl = createInterface({ input, output });
        try { console.log(`\n${prompt}`); return await rl.question('> '); }
        finally { rl.close(); }
      });
      labels = await labelInteractive({ items: labelViews(judgments, traces), ask });
    }
    const labelsPath = await emit(directory, 'labels.jsonl', jsonl(labels));
    const report = options.judgments ? agreement(await readJsonl(resolve(options.judgments)), labels) : null;
    if (report) await emit(directory, 'agreement.json', `${JSON.stringify(report, null, 2)}\n`);
    console.log(JSON.stringify({ output: labelsPath, labels: labels.length, validated: report?.validated ?? null }));
    return { labels, agreement: report };
  }
  if (command === 'adopt') {
    if (!options.baseline || !options.candidate || !options['security-traces']) throw new Error('adopt requires --baseline, --candidate, and --security-traces');
    // Always use the repository's complete security suite; a caller cannot supply an easier subset.
    const securitySuite = validateSuite(await readJson(fileURLToPath(new URL('./suites/security-v1.json', import.meta.url))));
    const result = assessAdoption({ baseline: await readJson(resolve(options.baseline)),
      candidate: await readJson(resolve(options.candidate)), securitySuite,
      securityTraces: await readJsonl(resolve(options['security-traces'])),
      trials: trialCount(options.trials ?? 2), reason: options.reason });
    await emit(directory, 'adoption.json', `${JSON.stringify(result, null, 2)}\n`);
    console.log(JSON.stringify({ output: directory, adopted: result.adopted, reasons: result.reasons }));
    return { ...result, exitCode: result.adopted ? 0 : 1 };
  }
  if (command === 'compare' && !options.suite) {
    if (!options.baseline || !options.candidate) throw new Error('compare requires --baseline and --candidate');
    const result = compareReports(await readJson(resolve(options.baseline)), await readJson(resolve(options.candidate)));
    await emit(directory, 'compare.json', `${JSON.stringify(result, null, 2)}\n`);
    await emit(directory, 'compare.md', comparisonMarkdown(result));
    console.log(JSON.stringify({ output: directory, regression_drop: result.regression_drop }));
    return result;
  }
  if (!options.suite) throw new Error('--suite is required');
  if (['grade', 'judge', 'report', 'calibrate'].includes(command) && !options.traces) throw new Error(`${command} requires --traces`);
  const { suite } = await loadSuite(options);
  const readRows = async (path) => selectRows(await readJsonl(resolve(path)), suite, options.split);
  if (command === 'grade') {
    const graded = grade(suite, await readRows(options.traces));
    const path = await emit(directory, 'assertions.jsonl', jsonl(graded.results));
    const passed = graded.results.filter((row) => row.pass).length;
    console.log(JSON.stringify({ output: path, assertions: graded.results.length, pass: passed, unmatched: graded.unmatched.length, tool_validity: graded.tool_validity }));
    if (options['fail-on-regression']) {
      const regressionIds = new Set(suite.scenarios.filter((scenario) => scenario.split === 'regression').map((scenario) => scenario.id));
      const failed = graded.results.filter((row) => regressionIds.has(row.scenario_id) && row.pass === false);
      if (failed.length) {
        const ids = failed.map((row) => `${row.scenario_id} ${row.session_id}#${row.turn_index} ${row.assert}`);
        throw new Error(`Regression checks failed:\n${ids.join('\n')}`);
      }
    }
    return graded;
  }
  if (command === 'calibrate') {
    if (!options.judgments || !options.labels) throw new Error('calibrate requires --judgments and --labels');
    const traces = await readRows(options.traces);
    traces.forEach(validateTraceRecord);
    const judgments = await readRows(options.judgments);
    const expected = new Map(judgeItems(suite, traces).map((item) => [itemId(item.trace, item.criterion_id), item.metric]));
    if (judgments.some((row) => !expected.has(row.item_id) || expected.get(row.item_id) !== row.metric)) {
      throw new Error('Calibration judgments must match suite criteria and trace identities');
    }
    let labels = importLabels(await readFile(resolve(options.labels), 'utf8'));
    if (options.split) {
      const selected = new Set(judgments.map((row) => row.item_id));
      labels = labels.filter((row) => selected.has(row.item_id));
    }
    const metadata = (field, flag) => {
      const values = judgments.map((row) => row[field] ?? options[flag]);
      if (!values.length) values.push(options[flag]);
      if (values.some((value) => typeof value !== 'string' || !value.trim())) throw new Error(`Missing judge ${field}; supply --${flag}`);
      if (new Set(values).size !== 1) throw new Error(`Calibration requires a single judge ${field}`);
      return values[0];
    };
    const model = metadata('model', 'model');
    const promptVersion = metadata('prompt_version', 'prompt-version');
    const reasoningEffort = metadata('reasoning_effort', 'reasoning-effort');
    const { per_criterion, per_metric, ready, reasons, validated: _validated, ...overall } = agreement(judgments, labels);
    const calibration = { schema: 'loupist-judge-calibration/1', judge_model: model,
      prompt_version: promptVersion, reasoning_effort: reasoningEffort, created_at: new Date().toISOString(),
      overall, by_criterion: per_criterion, by_metric: per_metric, ready, reasons };
    const path = options.out ? resolve(options.out) : resolve(directory, 'judge-calibration.json');
    await emit(dirname(path), path, `${JSON.stringify(calibration, null, 2)}\n`);
    console.log(JSON.stringify({ output: path, ready, reasons }));
    return { ...calibration, exitCode: ready ? 0 : 3 };
  }
  if (command === 'judge') {
    const traces = await readRows(options.traces);
    const codex = options.backend === 'codex';
    const model = options.model ?? (codex ? 'gpt-6-astra' : DEFAULT_JUDGE_MODEL);
    const reasoningEffort = options['reasoning-effort'] ?? DEFAULT_REASONING_EFFORT;
    const backend = io.backend ?? (options.endpoint || options.backend === 'openai'
      ? openAIBackend({ endpoint: options.endpoint, apiKeyEnv: options['api-key-env'], model, reasoningEffort })
      : codex ? codexBackend({ model, reasoningEffort }) : grokBackend({ model, reasoningEffort }));
    const judgeModel = backend.model ?? model;
    const family = modelFamily(judgeModel);
    const conflicts = [...new Set(traces.flatMap((trace) => [trace.observed_model, trace.model?.model])
      .filter((id) => family !== 'unknown' && modelFamily(id) === family))];
    if (conflicts.length) {
      const message = `Judge ${judgeModel} shares model family ${family} with agent models: ${conflicts.join(', ')}. Use --allow-same-family to override.`;
      if (!options['allow-same-family']) throw Object.assign(new Error(message), { exitCode: 2 });
      (io.warn ?? console.warn)(`Warning: ${message}`);
    }
    const judgments = await judgeAll({
      suite, traces, model: judgeModel, reasoningEffort,
      cacheDir: resolve(options.cache ?? resolve(directory, 'cache')), backend,
      concurrency: Number(options.concurrency ?? 1),
    });
    const path = await emit(directory, 'judgments.jsonl', jsonl(judgments));
    console.log(JSON.stringify({ output: path, judgments: judgments.length, cache_hits: judgments.filter((row) => row.cache === 'hit').length }));
    return judgments;
  }
  if (command === 'report') {
    const traces = await readRows(options.traces);
    const judgments = options.judgments ? await readRows(options.judgments) : [];
    const report = buildReport({ suite, traces, judgments, trials: trialCount(options.trials) });
    const path = await emit(directory, 'report.json', `${JSON.stringify(report, null, 2)}\n`);
    console.log(JSON.stringify({ output: path, traces: report.traces, metrics: Object.keys(report.metrics).length }));
    return report;
  }
  if (!options.baseline || !options.candidate) throw new Error('compare requires --baseline and --candidate');
  const baseline = await readJsonl(resolve(options.baseline));
  const candidate = await readJsonl(resolve(options.candidate));
  const comparison = compareRuns({
    suite, baseline, candidate,
    trials: trialCount(options.trials),
    baselineJudgments: options['baseline-judgments'] ? await readJsonl(resolve(options['baseline-judgments'])) : [],
    candidateJudgments: options['candidate-judgments'] ? await readJsonl(resolve(options['candidate-judgments'])) : [],
  });
  const path = await emit(directory, 'compare.json', `${JSON.stringify(comparison, null, 2)}\n`);
  console.log(JSON.stringify({ output: path, paired: comparison.paired, unpaired: comparison.unpaired.length, flips: comparison.flips.length }));
  return comparison;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then((result) => { process.exitCode = result?.exitCode ?? 0; })
    .catch((error) => { console.error(error.message); process.exitCode = error.exitCode ?? (process.argv[2] === 'calibrate' ? 2 : 1); });
}
