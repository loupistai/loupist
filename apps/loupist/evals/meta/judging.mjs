/** Retry/persistence orchestration around the unchanged public judge prompt/parser. */
import { appendFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { judgeItems, buildJudgeInput, renderJudgePrompt, judgeCacheKey, parseJudgeResult,
  itemId, grokBackend, PROMPT_VERSION } from '../agent/judge.mjs';
import { turnFields } from '../agent/eval-contract.mjs';
import { redact } from './core.mjs';

const MODEL = 'grok-4.7', EFFORT = 'medium';
const jsonl = values => values.map(v => JSON.stringify(v)).join('\n') + (values.length ? '\n' : '');
const readRows = async path => {
  try { return (await readFile(path, 'utf8')).split('\n').filter(Boolean).map(line => JSON.parse(line)); }
  catch (error) { if (error.code === 'ENOENT') return []; throw error; }
};
export function malformed(error) {
  return /(?:not JSON|JSON object|non-empty reason|boolean pass|unexpected field|Unexpected token|Unexpected end|JSON at position)/iu.test(error.message);
}
export async function retryMalformed(prompt, controls, { backend, priorAttempts = 0, record = async () => {} }) {
  for (let attempt = priorAttempts; attempt < 3; attempt++) {
    await record({ attempt, status: 'started' });
    try {
      const reminder = attempt ? '\n\nRespond with JSON only: {"pass": true or false, "reason": "observable evidence"}. No Markdown fences or other text.' : '';
      const result = parseJudgeResult(await backend.judge(prompt + reminder, controls));
      await record({ attempt, status: 'completed' });
      return { ...result, attempts: attempt + 1 };
    } catch (error) {
      if (!malformed(error)) { await record({ attempt, status: 'error', error: redact(error.message) }); throw error; }
      await record({ attempt, status: 'malformed', error: redact(error.message) });
    }
  }
  return { pass: null, judge_error: true, reason: 'Judge returned malformed JSON after three attempts', attempts: 3 };
}

export async function judgeWithRecovery({ suite, traces, cacheDir, directory, concurrency = 3,
  backend = grokBackend() }) {
  if (!Number.isInteger(concurrency) || concurrency < 1) throw new Error('Invalid judge concurrency');
  await mkdir(cacheDir, { recursive: true });
  const logPath = join(directory, 'judge-calls.jsonl');
  const history = await readRows(logPath);
  const previousErrors = new Map((await readRows(join(directory, 'judge-errors.jsonl'))).map(row => [row.item_id, row]));
  const items = judgeItems(suite, traces), results = new Array(items.length);
  let writeQueue = Promise.resolve(), started = 0;
  const log = entry => {
    if (entry.status === 'started') started++;
    writeQueue = writeQueue.then(() => appendFile(logPath, JSON.stringify({ ...entry, recorded_at: new Date().toISOString() }) + '\n', { mode: 0o600 }));
    return writeQueue;
  };
  const promises = new Map();
  async function evaluate(item) {
    const id = itemId(item.trace, item.criterion_id);
    const identity = { item_id: id, run_id: item.trace.run_id, ...turnFields(item.trace), criterion_id: item.criterion_id,
      criterion: item.criterion, metric: item.metric, kind: item.kind, memory_item: item.memory_item ?? null,
      model: MODEL, prompt_version: PROMPT_VERSION, reasoning_effort: EFFORT };
    if (previousErrors.has(id)) return { ...identity, ...previousErrors.get(id), cache: 'error-hit' };
    const input = buildJudgeInput(suite, item, traces);
    const key = judgeCacheKey({ input, criterion: item.criterion, model: MODEL, reasoningEffort: EFFORT });
    let cached;
    try {
      const value = JSON.parse(await readFile(join(cacheDir, `${key}.json`), 'utf8'));
      if (value.key === key) cached = parseJudgeResult({ pass: value.pass, reason: value.reason });
    } catch (error) { if (error.code && error.code !== 'ENOENT') throw error; }
    if (cached) return { ...identity, ...cached, cache: 'hit' };
    const shared = promises.has(key);
    if (!shared) promises.set(key, (async () => {
      const malformedAttempts = history.filter(entry => entry.item_id === id && entry.status === 'malformed').map(entry => entry.attempt + 1);
      const priorAttempts = Math.max(0, ...malformedAttempts);
      const result = await retryMalformed(renderJudgePrompt(input), { input, item, model: MODEL, reasoningEffort: EFFORT }, {
        backend, priorAttempts, record: entry => log({ item_id: id, scenario_id: item.trace.scenario_id, cache_key: key, ...entry }),
      });
      if (!result.judge_error) await writeFile(join(cacheDir, `${key}.json`), JSON.stringify({ key, model: MODEL,
        prompt_version: PROMPT_VERSION, reasoning_effort: EFFORT, pass: result.pass, reason: result.reason }) + '\n', { mode: 0o600 });
      return result;
    })());
    const result = await promises.get(key);
    return { ...identity, ...result, cache: shared ? 'shared' : 'miss' };
  }
  let next = 0, fatal;
  const worker = async () => {
    while (!fatal && next < items.length) {
      const index = next++;
      try { results[index] = await evaluate(items[index]); }
      catch (error) { fatal ??= error; }
    }
  };
  await Promise.all(Array.from({ length: Math.min(concurrency, items.length) }, worker));
  await writeQueue;
  const errors = results.filter(row => row?.judge_error);
  await writeFile(join(directory, 'judge-errors.jsonl'), jsonl([...previousErrors.values(), ...errors.filter(row => !previousErrors.has(row.item_id))]), { mode: 0o600 });
  if (fatal) throw fatal;
  return { judgments: results, usage: { calls: history.filter(row => row.status === 'started').length + started,
    new_calls: started, cached: results.filter(row => row.cache === 'hit' || row.cache === 'shared').length,
    judge_errors: errors.length, tokens: null, note: 'Logical calls include malformed retries; provider token usage unavailable' } };
}
