#!/usr/bin/env node
/**
 * The paid-call ledger of one improvement loop. No model call.
 *
 * `ledger.jsonl` holds the loop's cap, one row per measured arm and one row per iteration. A call
 * is one model round of an agent turn, one World inference or one uncached judgment. They are
 * counted from the files a run wrote, never typed from memory; a count from elsewhere is an
 * `--calls` import with a note. Past the cap every command exits 1, and `plan` says before an
 * arm starts whether it fits.
 */
import { createHash } from 'node:crypto';
import { appendFile, mkdir, readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const COMMANDS = {
  cap: { required: ['ledger', 'loop', 'cap'], optional: ['reason'] },
  arm: { required: ['ledger', 'label', 'pin', 'suite', 'model', 'trials'], optional: ['traces', 'stage-logs', 'judgments', 'calls', 'note', 'charge-unknown', 'performance'] },
  iteration: { required: ['ledger', 'class', 'hypothesis', 'commit', 'result', 'decision'], optional: [] },
  plan: { required: ['ledger', 'calls'], optional: [] },
  sum: { required: ['ledger'], optional: [] },
};
const DECISIONS = new Set(['kept', 'reverted']);

export const USAGE = [
  'ledger.mjs cap --ledger <ledger.jsonl> --loop <name> --cap <calls> [--reason <text>]',
  'ledger.mjs arm --ledger <ledger.jsonl> --label <text> --pin <commit> --suite <name> --model <id> --trials <n>',
  '           [--traces a.jsonl,b.jsonl] [--stage-logs s.jsonl,...] [--judgments j.jsonl,...]',
  '           [--calls <n> --note <where the count comes from>] [--charge-unknown <rounds per turn>]',
  '           [--performance standard|fast]   the serving mode of the run, so later arms compare like with like',
  'ledger.mjs iteration --ledger <ledger.jsonl> --class <failure class> --hypothesis <text> --commit <sha>',
  '           --result <text> --decision kept|reverted',
  'ledger.mjs plan --ledger <ledger.jsonl> --calls <n>    exits 1 when the arm would pass the cap',
  'ledger.mjs sum --ledger <ledger.jsonl>                 exits 1 past the cap',
].join('\n');

const count = (value, name) => {
  const number = Number(value);
  if (typeof value === 'boolean' || !Number.isSafeInteger(number) || number < 0) throw new Error(`${name} must be a non-negative integer`);
  return number;
};
const jsonl = (text) => text.split('\n').filter((line) => line.trim()).map((line) => JSON.parse(line));

/** Model rounds of agent turns. A turn that never started made no call; any other unknown is refused or charged. */
export function agentRounds(rows, chargeUnknown = null) {
  let rounds = 0;
  let unknown = 0;
  for (const row of rows) {
    if (Number.isSafeInteger(row.model_rounds) && row.model_rounds >= 0) rounds += row.model_rounds;
    else if (row.startup_failed !== true) unknown += 1;
  }
  if (unknown && chargeUnknown === null) {
    throw new Error(`${unknown} turn(s) record no model_rounds; pass --charge-unknown <rounds per turn> to count them`);
  }
  return { calls: rounds + unknown * (chargeUnknown ?? 0), unknown_turns: unknown };
}

/** World inferences in a `LOUPIST_WORLD_STAGE_LOG` file: its `llm` records. Embeddings are local. */
export function worldInferences(rows) {
  return { calls: rows.filter((row) => row.kind === 'llm').length };
}

/** Judgments that spent a call: every row not marked as a cache hit. */
export function uncachedJudgments(rows) {
  return { calls: rows.filter((row) => row.cache !== 'hit').length };
}

const COUNTERS = { traces: ['agent_rounds', agentRounds], 'stage-logs': ['world_inferences', worldInferences], judgments: ['uncached_judgments', uncachedJudgments] };

export function readLedger(text) {
  const rows = jsonl(text);
  const caps = rows.filter((row) => row.kind === 'cap');
  const arms = rows.filter((row) => row.kind === 'arm');
  const cap = caps.at(-1) ?? null;
  const by = (key) => arms.reduce((sum, arm) => sum + (arm.calls?.[key] ?? 0), 0);
  const calls = { agent_rounds: by('agent_rounds'), world_inferences: by('world_inferences'), uncached_judgments: by('uncached_judgments'), imported: by('imported') };
  const used = Object.values(calls).reduce((sum, value) => sum + value, 0);
  return {
    loop: cap?.loop ?? null, cap: cap?.cap ?? null, used, left: cap ? cap.cap - used : null, over: cap ? used > cap.cap : null,
    calls, arms: arms.length, iterations: rows.filter((row) => row.kind === 'iteration').length,
    files: new Set(arms.flatMap((arm) => (arm.files ?? []).map((file) => file.sha256))),
  };
}

function parse(argv) {
  const [command, ...rest] = argv;
  const spec = COMMANDS[command];
  if (!spec) throw new Error(`Unknown command ${command ?? ''}\n${USAGE}`);
  const allowed = new Set([...spec.required, ...spec.optional]);
  const options = {};
  for (let index = 0; index < rest.length; index += 2) {
    const name = String(rest[index]).replace(/^--/u, '');
    if (!String(rest[index]).startsWith('--') || !allowed.has(name)) throw new Error(`Unknown option ${rest[index]} for ${command}`);
    if (Object.hasOwn(options, name)) throw new Error(`Specify --${name} only once`);
    if (rest[index + 1] === undefined || String(rest[index + 1]).startsWith('--')) throw new Error(`--${name} requires a value`);
    options[name] = rest[index + 1];
  }
  for (const name of spec.required) if (!options[name]) throw new Error(`${command} requires --${name}`);
  return { command, options };
}

/** Runs one command and returns `{ row, state, exitCode }`; `now` is injectable for tests. */
export async function main(argv, { now = () => new Date().toISOString(), log = console.log } = {}) {
  const { command, options } = parse(argv);
  const path = resolve(options.ledger);
  const text = await readFile(path, 'utf8').catch((error) => { if (error.code === 'ENOENT') return ''; throw error; });
  const before = readLedger(text);
  const append = async (row) => {
    await mkdir(dirname(path), { recursive: true });
    await appendFile(path, `${JSON.stringify(row)}\n`, { mode: 0o600 });
    return readLedger(`${text}${JSON.stringify(row)}\n`);
  };
  const finish = (state, row = null, planned = 0) => {
    const { files: _files, ...summary } = state;
    const over = state.used + planned > state.cap;
    log(JSON.stringify({ ...summary, ...(planned ? { planned, would_use: state.used + planned } : {}), over }));
    return { row, state: summary, exitCode: over ? 1 : 0 };
  };
  if (command === 'cap') {
    if (before.loop !== null && before.loop !== options.loop) throw new Error(`This ledger belongs to loop ${before.loop}`);
    const cap = count(options.cap, '--cap');
    if (before.cap !== null && !options.reason) throw new Error('Changing a cap requires --reason');
    const row = { kind: 'cap', at: now(), loop: options.loop, cap, ...(options.reason ? { reason: options.reason } : {}) };
    return finish(await append(row), row);
  }
  if (before.cap === null) throw new Error('This ledger has no cap; start it with the cap command');
  if (command === 'sum') return finish(before);
  if (command === 'plan') return finish(before, null, count(options.calls, '--calls'));
  if (command === 'iteration') {
    if (!DECISIONS.has(options.decision)) throw new Error('--decision must be kept or reverted');
    const row = { kind: 'iteration', at: now(), n: before.iterations + 1, class: options.class, hypothesis: options.hypothesis,
      commit: options.commit, result: options.result, decision: options.decision };
    return finish(await append(row), row);
  }
  const chargeUnknown = options['charge-unknown'] === undefined ? null : count(options['charge-unknown'], '--charge-unknown');
  const calls = { agent_rounds: 0, world_inferences: 0, uncached_judgments: 0, imported: 0 };
  const files = [];
  let unknownTurns = 0;
  for (const [option, [key, counter]] of Object.entries(COUNTERS)) {
    for (const file of (options[option] ?? '').split(',').filter(Boolean)) {
      const bytes = await readFile(resolve(file));
      const sha256 = createHash('sha256').update(bytes).digest('hex');
      if (before.files.has(sha256) || files.some((item) => item.sha256 === sha256)) throw new Error(`${file} is already counted in this ledger`);
      const counted = counter(jsonl(bytes.toString('utf8')), chargeUnknown);
      calls[key] += counted.calls;
      unknownTurns += counted.unknown_turns ?? 0;
      files.push({ path: resolve(file), kind: key, sha256, calls: counted.calls });
    }
  }
  if (options.calls !== undefined) {
    if (!options.note) throw new Error('--calls is an import and requires --note');
    calls.imported = count(options.calls, '--calls');
  } else if (options.note) throw new Error('--note belongs to a --calls import');
  if (!files.length && options.calls === undefined) throw new Error('An arm needs files to count or a --calls import');
  if (options.performance !== undefined && !['standard', 'fast'].includes(options.performance)) throw new Error('--performance must be standard or fast');
  const row = { kind: 'arm', at: now(), label: options.label, pin: options.pin, suite: options.suite, model: options.model,
    trials: count(options.trials, '--trials'), calls, total: Object.values(calls).reduce((sum, value) => sum + value, 0), files,
    ...(unknownTurns ? { unknown_turns: unknownTurns, charged_per_unknown_turn: chargeUnknown } : {}),
    ...(options.note ? { note: options.note } : {}),
    ...(options.performance ? { performance: options.performance } : {}) };
  return finish(await append(row), row);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then((result) => { process.exitCode = result.exitCode; })
    .catch((error) => { console.error(error.message); process.exitCode = 2; });
}
