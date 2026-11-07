/** Binary LLM judge. One criterion per call. Hidden scenario fields are never sent. */
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { trailingJsonObject } from './grade.mjs';
import { locateTurn, canonicalJson, turnIdentity, turnFields, validateSuite, validateTraceRecord,
  toolCalls, memoryCommits, answerEvidence, isRecord } from './eval-contract.mjs';

export const PROMPT_VERSION = 'agent-judge/3';
export const DEFAULT_JUDGE_MODEL = 'grok-4.7';
export const DEFAULT_REASONING_EFFORT = 'medium';
export const SAVE_PRECISION_CRITERION = 'The committed memory item belongs in this Loupist\'s Memory for later work and is filed in a fitting kind: a user-stated research rule in a principle; a way of checking the user taught or corrected in a method Lens page; dated company, asset or sector facts and what explains them, drawn from material read this turn, in a subject Lens page about that company or theme with a source in its basis (page_kind subject, page_basis including span or claim); an unverified thesis with what would break it in an idea page; facts the user stated or this Chat\'s progress in a note; an acquired original in a Source; a judgment the user explicitly stated for the record in a Journal entry, including an analytical research assessment without a trade action. Fail chatter, a one-off assumption the user said not to keep, a superseded value kept as current, a method page created from the assistant\'s own analysis, general knowledge or a single company\'s case, company facts written into a method or perspective page, a subject page whose facts cite no source, a Journal entry when the user asked only for research without stating their own judgment for the record, or a Journal entry holding a stance the assistant originated rather than the user.';
/** Provider routing prefixes do not establish the underlying model family. */
export function modelFamily(id) {
  const model = typeof id === 'string' ? id.trim().toLowerCase().split('/').at(-1) : '';
  if (/^(?:gpt-|o\d|codex-)/u.test(model)) return 'openai';
  for (const [prefix, family] of [['claude-', 'anthropic'], ['grok-', 'xai'], ['gemini-', 'google'],
    ['deepseek-', 'deepseek'], ['kimi-', 'moonshot'], ['minimax-', 'minimax']]) {
    if (model.startsWith(prefix)) return family;
  }
  return 'unknown';
}

export const JUDGE_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['pass', 'reason'],
  properties: { pass: { type: 'boolean' }, reason: { type: 'string', minLength: 1 } },
};

const canonical = (value) => JSON.parse(canonicalJson(value));
const observedFields = (value, fields) => Object.fromEntries(fields.map((field) => [field, value[field] ?? null]));
const commitEvidence = (commit) => observedFields(commit,
  ['kind', 'record_id', 'title', 'summary', 'change_summary', 'operation', 'prediction', 'outcome']);
/** A Lens commit names only "lens"; the stored page tells a subject page from a method. */
function savedItemEvidence(commit, trace) {
  const evidence = commitEvidence(commit);
  const page = commit.kind === 'lens' ? trace.lens?.pages?.find((item) => item.record_id === commit.record_id) : undefined;
  if (!page) return evidence;
  const basis = [...new Set(page.sections.flatMap((section) => section.basis ?? []))].sort();
  return { ...evidence, page_kind: page.kind, page_basis: basis };
}

export function itemId(trace, criterionId) {
  return canonicalJson([turnIdentity(trace), criterionId]);
}

export function parseJudgeResult(value) {
  const parsed = typeof value === 'string' ? parseJsonText(value) : value;
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) throw new Error('Judge result must be a JSON object');
  if (typeof parsed.pass !== 'boolean' || typeof parsed.reason !== 'string' || !parsed.reason.trim()) throw new Error('Judge result must include boolean pass and a non-empty reason');
  for (const key of Object.keys(parsed)) {
    if (key !== 'pass' && key !== 'reason') throw new Error(`Judge result has unexpected field ${key}`);
  }
  return { pass: parsed.pass, reason: parsed.reason };
}

function parseJsonText(text) {
  const trimmed = String(text ?? '').trim();
  try { return JSON.parse(trimmed); } catch { /* Fall through to the trailing object. */ }
  const trailing = trailingJsonObject(trimmed);
  if (!trailing) throw new Error('Judge result is not JSON');
  return JSON.parse(trailing);
}

export function parseGrokJudgeStdout(stdout) {
  const wrapper = parseJsonText(stdout);
  if (wrapper?.type === 'error') throw new Error(wrapper.message || 'grok judge failed');
  if (wrapper?.structured_output && typeof wrapper.structured_output === 'object') return parseJudgeResult(wrapper.structured_output);
  if (typeof wrapper?.text === 'string') return parseJudgeResult(wrapper.text);
  return parseJudgeResult(wrapper);
}

export function grokJudgeArgs({ prompt, model = DEFAULT_JUDGE_MODEL, cwd, reasoningEffort = DEFAULT_REASONING_EFFORT }) {
  const args = ['-p', prompt, '--json-schema', JSON.stringify(JUDGE_SCHEMA), '-m', model, '--disable-web-search', '--cwd', cwd];
  if (reasoningEffort) args.push('--reasoning-effort', reasoningEffort);
  args.push('--output-format', 'json', '--max-turns', '1', '--no-subagents', '--verbatim');
  return args;
}

function traceSummary(trace) {
  const calls = toolCalls(trace);
  const commits = memoryCommits(trace);
  const reads = Array.isArray(trace.memory_reads) && trace.memory_reads.every(isRecord) ? trace.memory_reads : null;
  return {
    status: trace.status ?? null,
    tools: calls?.map((call) => observedFields(call,
      ['name', 'args', 'outcome', 'error_code', 'error_message', 'result', 'result_redacted', 'result_bytes'])) ?? null,
    memory_reads: reads?.map((read) => observedFields(read, ['kind', 'record_id', 'title', 'access'])) ?? null,
    memory_commits: commits?.map(commitEvidence) ?? null,
  };
}

function priorSessions(suite, traces, trace) {
  const { scenario, session } = locateTurn(suite, trace);
  if (!scenario || !session) return [];
  const current = scenario.sessions.findIndex((item) => item.id === session.id);
  const earlier = new Set(scenario.sessions.slice(0, Math.max(current, 0)).map((item) => item.id));
  const grouped = new Map();
  for (const candidate of traces) {
    if (candidate.run_id !== trace.run_id || candidate.suite !== trace.suite || candidate.scenario_id !== trace.scenario_id || candidate.trial !== trace.trial) continue;
    if (!earlier.has(candidate.session_id) && !(candidate.session_id === trace.session_id && candidate.turn_index < trace.turn_index)) continue;
    const turns = grouped.get(candidate.session_id) ?? [];
    turns.push(candidate);
    grouped.set(candidate.session_id, turns);
  }
  return scenario.sessions.filter((item) => earlier.has(item.id) || grouped.has(item.id)).map((item) => ({
    session_id: item.id,
    role: item.role ?? null,
    turns: (grouped.get(item.id) ?? []).sort((left, right) => left.turn_index - right.turn_index).map((turn) => ({
      user: turn.user ?? null,
      answer: answerEvidence(turn),
      trace_summary: traceSummary(turn),
    })),
  }));
}

export function judgeItems(suite, traces) {
  const items = [];
  for (const trace of traces) {
    const { session, turn } = locateTurn(suite, trace);
    if (!turn) continue;
    for (const rubric of turn.expect?.rubric ?? []) {
      items.push({ kind: 'rubric', criterion_id: rubric.id, metric: rubric.metric ?? null, criterion: rubric.criterion, trace });
    }
    if (session.role !== 'train') continue;
    memoryCommits(trace)?.forEach((commit, index) => {
      const recordId = commit.record_id || String(index);
      items.push({
        kind: 'save_precision', criterion_id: `save_precision:${index}:${recordId}`, metric: 'save_precision', criterion: SAVE_PRECISION_CRITERION, trace,
        memory_item: savedItemEvidence(commit, trace),
      });
    });
  }
  return items;
}

/** Preserve observed evidence without a second truncation. Never include scenario hidden fields. */
export function buildJudgeInput(suite, item, traces) {
  const answer = answerEvidence(item.trace);
  if (answer === null) throw new Error('Judge requires observed answer text');
  const input = {
    user: item.trace.user ?? null,
    answer,
    trace_summary: traceSummary(item.trace),
    prior_sessions: priorSessions(suite, traces, item.trace),
    criterion: item.criterion,
  };
  if (item.memory_item) input.memory_item = item.memory_item;
  return input;
}

export function renderJudgePrompt(input) {
  return [
    'You are a binary judge for a fictional investment-research agent evaluation.',
    'Decide whether the criterion is satisfied using only the observed input.',
    'Return JSON matching the schema: {"pass": boolean, "reason": string}.',
    'pass is true only when the criterion is clearly met. The reason must cite observable evidence.',
    'Tool names or claimed saves alone do not prove successful results. Null, redacted, or truncated evidence cannot establish the missing facts.',
    'Do not use tools, web search, or outside knowledge. No hidden outcomes are included.',
    '',
    `Criterion: ${input.criterion}`,
    '',
    'Observed input:',
    JSON.stringify(input),
  ].join('\n');
}

export function judgeCacheKey({ input, criterion, model, promptVersion = PROMPT_VERSION, reasoningEffort = DEFAULT_REASONING_EFFORT }) {
  return createHash('sha256').update(JSON.stringify({ input: canonical(input), criterion, model, prompt_version: promptVersion, reasoning_effort: reasoningEffort })).digest('hex');
}

async function readCache(cacheDir, key) {
  try {
    const cached = JSON.parse(await readFile(join(cacheDir, `${key}.json`), 'utf8'));
    if (cached.key !== key) return null;
    return parseJudgeResult({ pass: cached.pass, reason: cached.reason });
  } catch (error) {
    if (error.code === 'ENOENT') return null;
    return null;
  }
}

async function writeCache(cacheDir, key, model, promptVersion, reasoningEffort, result) {
  await mkdir(cacheDir, { recursive: true });
  await writeFile(join(cacheDir, `${key}.json`), `${JSON.stringify({ key, model, prompt_version: promptVersion, reasoning_effort: reasoningEffort, pass: result.pass, reason: result.reason })}\n`, { mode: 0o600 });
}

function judgmentRow(item, extra) {
  return {
    item_id: itemId(item.trace, item.criterion_id),
    run_id: item.trace.run_id, ...turnFields(item.trace), criterion_id: item.criterion_id,
    criterion: item.criterion, metric: item.metric, kind: item.kind,
    memory_item: item.memory_item ?? null, ...extra,
  };
}

export async function judgeAll({ suite, traces, model = DEFAULT_JUDGE_MODEL, cacheDir, backend, promptVersion = PROMPT_VERSION, reasoningEffort = DEFAULT_REASONING_EFFORT, concurrency = 1 }) {
  if (!cacheDir) throw new Error('Judge cache directory is required');
  if (!backend?.judge) throw new Error('Judge backend is required');
  if (!Number.isInteger(concurrency) || concurrency < 1) throw new Error('Judge concurrency must be a positive integer');
  validateSuite(suite);
  traces.forEach(validateTraceRecord);
  const items = judgeItems(suite, traces);
  if (new Set(items.map((item) => itemId(item.trace, item.criterion_id))).size !== items.length) throw new Error('Ambiguous judge item identity');
  if (items.some((item) => answerEvidence(item.trace) === null)) throw new Error('Judge requires observed answer text');
  const results = new Array(items.length);
  const judgeAt = async (index) => {
    const item = items[index];
    const input = buildJudgeInput(suite, item, traces);
    const key = judgeCacheKey({ input, criterion: item.criterion, model, promptVersion, reasoningEffort });
    const cached = await readCache(cacheDir, key);
    if (cached) {
      results[index] = judgmentRow(item, { ...cached, model, prompt_version: promptVersion, reasoning_effort: reasoningEffort, cache: 'hit' });
      return;
    }
    const judged = parseJudgeResult(await backend.judge(renderJudgePrompt(input), { input, item, model, reasoningEffort }));
    await writeCache(cacheDir, key, model, promptVersion, reasoningEffort, judged);
    results[index] = judgmentRow(item, { ...judged, model, prompt_version: promptVersion, reasoning_effort: reasoningEffort, cache: 'miss' });
  };
  let next = 0;
  const worker = async () => {
    while (next < items.length) await judgeAt(next++);
  };
  await Promise.all(Array.from({ length: Math.min(concurrency, items.length) }, worker));
  return results;
}

function runProcess(command, args, { cwd }) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.on('error', reject);
    child.on('close', (code) => {
      if (code === 0) resolve(stdout);
      else reject(new Error(`${command} exited ${code}: ${stderr.trim().slice(0, 400)}`));
    });
  });
}

export function grokBackend({ model = DEFAULT_JUDGE_MODEL, reasoningEffort = DEFAULT_REASONING_EFFORT, command = 'grok', run = runProcess } = {}) {
  return {
    model,
    async judge(prompt, controls = {}) {
      const cwd = await mkdtemp(join(tmpdir(), 'agent-judge-'));
      try {
        const stdout = await run(command, grokJudgeArgs({ prompt, model, cwd, reasoningEffort: controls.reasoningEffort ?? reasoningEffort }), { cwd });
        return parseGrokJudgeStdout(stdout);
      } finally {
        await rm(cwd, { recursive: true, force: true });
      }
    },
  };
}

export function codexJudgeArgs({ prompt, model, cwd, schemaPath, outputPath, reasoningEffort = DEFAULT_REASONING_EFFORT }) {
  return ['exec', '--ignore-user-config', '--ignore-rules', '--skip-git-repo-check', '--ephemeral', '-m', model,
    '-c', `model_reasoning_effort="${reasoningEffort}"`, '-c', 'features.shell_tool=false', '-c', 'web_search="disabled"',
    '-c', 'features.multi_agent=false', '--sandbox', 'read-only', '--cd', cwd,
    '--output-schema', schemaPath, '--output-last-message', outputPath, prompt];
}

/** A tool-free `codex exec` judge, for agents whose own family is xAI. */
export function codexBackend({ model = 'gpt-6-astra', reasoningEffort = DEFAULT_REASONING_EFFORT, command = 'codex', run = runProcess } = {}) {
  return {
    model,
    async judge(prompt, controls = {}) {
      const cwd = await mkdtemp(join(tmpdir(), 'agent-judge-'));
      try {
        const schemaPath = join(cwd, 'schema.json');
        const outputPath = join(cwd, 'verdict.json');
        await writeFile(schemaPath, JSON.stringify(JUDGE_SCHEMA));
        await run(command, codexJudgeArgs({ prompt, model, cwd, schemaPath, outputPath, reasoningEffort: controls.reasoningEffort ?? reasoningEffort }), { cwd });
        return parseJudgeResult(await readFile(outputPath, 'utf8'));
      } finally {
        await rm(cwd, { recursive: true, force: true });
      }
    },
  };
}

export function openAIBackend({ endpoint, apiKeyEnv, model = DEFAULT_JUDGE_MODEL, reasoningEffort = DEFAULT_REASONING_EFFORT, fetchImpl = globalThis.fetch } = {}) {
  if (!endpoint || !/^https:\/\//u.test(endpoint)) throw new Error('OpenAI-compatible judge requires an https endpoint');
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/u.test(apiKeyEnv ?? '')) throw new Error('api_key_env must be an environment variable name');
  return {
    model,
    async judge(prompt, controls = {}) {
      const apiKey = process.env[apiKeyEnv];
      if (!apiKey) throw new Error(`Environment variable ${apiKeyEnv} is not set`);
      const response = await fetchImpl(endpoint, {
        method: 'POST',
        headers: { 'content-type': 'application/json', authorization: `Bearer ${apiKey}` },
        body: JSON.stringify({
          model,
          reasoning_effort: controls.reasoningEffort ?? reasoningEffort,
          messages: [{ role: 'user', content: prompt }],
          response_format: { type: 'json_schema', json_schema: { name: 'agent_judge', strict: true, schema: JUDGE_SCHEMA } },
        }),
      });
      if (!response.ok) throw new Error(`Judge endpoint failed with status ${response.status}`);
      const body = await response.json();
      return parseJudgeResult(body.choices?.[0]?.message?.content);
    },
  };
}
