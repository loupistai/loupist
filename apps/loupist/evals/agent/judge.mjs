/** Binary LLM judge. One criterion per call. Hidden scenario fields are never sent. */
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { scheduledDeliveryEvidence } from './scheduled-evidence.mjs';
import { visibleLabels } from './label-evidence.mjs';
import { trailingJsonObject } from './grade.mjs';
import { locateTurn, canonicalJson, turnIdentity, turnFields, validateSuite, validateTraceRecord,
  toolCalls, memoryCommits, answerEvidence, isRecord, visibleLabelsAssertion } from './eval-contract.mjs';

export const PROMPT_VERSION = 'agent-judge/3';
export const DEFAULT_JUDGE_MODEL = 'grok-4.7';
export const DEFAULT_CLAUDE_JUDGE_MODEL = 'claude-opus-5-5';
export const DEFAULT_REASONING_EFFORT = 'medium';
export const SAVE_PRECISION_CRITERION = 'The committed memory item belongs in this Loupist\'s Memory for later work and is filed in a fitting kind: a user-stated research rule in a principle; a way of checking the user taught, corrected or asked to keep in a method Lens page; dated company, asset or sector facts and what explains them, drawn from material read this turn, in a subject Lens page about that company or theme with a source in its basis (page_kind subject, page_basis including span or claim); an unverified thesis with what would break it in an idea page; facts the user stated or this Chat\'s progress in a note; an acquired original in a Source, including original material the user pasted into the Chat (a filing excerpt, article, notice or table) kept as a user_provided Source; a judgment the user explicitly stated for the record in a Journal entry, including an analytical research assessment without a trade action. Fail chatter, a one-off assumption the user said not to keep, a superseded value kept as current, a method page the user did not ask for, created from the assistant\'s own analysis, general knowledge or a single company\'s case, company facts written into a method or perspective page, a subject page whose facts cite no source, a Journal entry when the user asked only for research without stating their own judgment for the record, or a Journal entry holding a stance the assistant originated rather than the user.';
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
const commitEvidence = (commit) => Object.fromEntries(
  ['kind', 'record_id', 'title', 'summary', 'change_summary', 'operation', 'judgment', 'reasoning', 'check', 'prediction', 'outcome']
    .filter(field => Object.hasOwn(commit, field)).map(field => [field, commit[field]]));
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
  const { scenario, turn } = locateTurn(suite, item.trace);
  const labelKeys = (turn?.expect?.answer ?? []).filter(check => check.assert === 'visible_labels')
    .flatMap(check => visibleLabelsAssertion(check, scenario));
  const labels = visibleLabels(item.trace.resolved_appearance);
  if (labelKeys.length && labelKeys.every(key => Object.hasOwn(labels, key))) {
    input.screen_labels = Object.fromEntries(labelKeys.map(key => [key, labels[key]]));
  }
  if (turn?.scheduled && scheduledDeliveryEvidence(item.trace, scenario, turn, traces).pass === true) {
    input.answer = item.trace.session_answer.content;
    input.bot_preview = answer;
  }
  if (turn?.expect?.trace?.some(check => check.assert === 'scheduled_tasks') && Array.isArray(item.trace.scheduled_tasks)) {
    input.saved_tasks = item.trace.scheduled_tasks.map(({ title, prompt, status }) => ({ title, prompt, status }));
  }
  if (item.memory_item) input.memory_item = item.memory_item;
  const pages = storedLensPages(item.trace);
  if (pages) input.lens_pages_after_turn = pages;
  return input;
}

/** What the store holds after the turn, so a judge reads what a page says is current. */
function storedLensPages(trace) {
  const pages = trace.lens?.pages;
  if (!Array.isArray(pages) || pages.length === 0) return null;
  return pages.filter((page) => !page.archived).map((page) => ({
    record_id: page.record_id, kind: page.kind, title: page.title, summary: page.summary ?? '',
    sections: (page.sections ?? []).map((section) => ({ key: section.key, heading: section.heading, markdown: section.markdown,
      basis: section.basis ?? [], contested: section.contested === true })),
    ops_this_turn: page.ops_this_turn ?? [],
  }));
}

export function renderJudgePrompt(input) {
  return [
    'You are a binary judge for a fictional investment-research agent evaluation.',
    'Decide whether the criterion is satisfied using only the observed input.',
    'Return JSON matching the schema: {"pass": boolean, "reason": string}.',
    'pass is true only when the criterion is clearly met. The reason must cite observable evidence.',
    'Tool names or claimed saves alone do not prove successful results. Null, redacted, or truncated evidence cannot establish the missing facts.',
    'Do not use tools, web search, or outside knowledge. No hidden outcomes are included.',
    ...(Object.hasOwn(input, 'bot_preview') ? [
      'The answer is the full scheduled session reply. The bot_preview is the delivered excerpt.',
    ] : []),
    ...(Object.hasOwn(input, 'screen_labels') ? [
      'The screen_labels are the actual labels in this turn’s resolved appearance. Compare the answer with these exact labels. Correct directions can pass when no tool call was made.',
    ] : []),
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

/** `env` defaults to this process's environment; `input`, when given, is written to stdin. */
function runProcess(command, args, { cwd, env, input }) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, env, stdio: [input === undefined ? 'ignore' : 'pipe', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.on('error', reject);
    // A child that exits before reading stdin is reported by its exit code below.
    if (input !== undefined) child.stdin.on('error', () => {}).end(input);
    child.on('close', (code) => {
      if (code === 0) resolve(stdout);
      else reject(Object.assign(new Error(`${command} exited ${code}: ${stderr.trim().slice(0, 400)}`), { stdout }));
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

/**
 * Arguments for a headless `claude -p` judge call on the Claude subscription login. The
 * prompt carries untrusted trace text, so the call must have no tools and load no context:
 * - `--tools ""` removes every built-in tool: Bash and other code runners, file read, edit
 *   and search, web fetch and search, subagents, Skill and EndConversation.
 * - `--strict-mcp-config`, with no `--mcp-config`, connects no MCP server from `.mcp.json`,
 *   `~/.claude.json`, plugins or claude.ai connectors; `--disallowedTools mcp__*` denies any
 *   MCP tool that a managed policy still adds.
 * - `--safe-mode` loads no CLAUDE.md, auto memory, skills, plugins, hooks, MCP servers,
 *   custom agents or output styles, and keeps the subscription login (`--bare` would
 *   require an API key).
 * - `--setting-sources ""` reads no user, project or local settings file, so none of their
 *   permission rules, hooks, env, model or `apiKeyHelper` apply.
 * - `--disable-slash-commands` keeps skills and commands out of the context and stops a
 *   leading `/name` from expanding.
 * - `--no-session-persistence` writes no transcript to resume.
 * - `--max-turns 1` allows one model turn. `--json-schema` adds the CLI's own
 *   StructuredOutput tool, the only tool left: a valid call ends that turn, and an invalid
 *   call or a text answer has no second turn to retry in, so the item fails.
 * The prompt goes on stdin, so a variadic flag such as `--tools` cannot take it as a value
 * and it stays out of the process list.
 */
export function claudeJudgeArgs({ model = DEFAULT_CLAUDE_JUDGE_MODEL, reasoningEffort = DEFAULT_REASONING_EFFORT } = {}) {
  const args = ['-p', '--model', model];
  if (reasoningEffort) args.push('--effort', reasoningEffort);
  args.push('--tools', '', '--strict-mcp-config', '--disallowedTools', 'mcp__*', '--safe-mode',
    '--setting-sources', '', '--disable-slash-commands', '--no-session-persistence', '--max-turns', '1',
    '--output-format', 'json', '--json-schema', JSON.stringify(JUDGE_SCHEMA));
  return args;
}

/**
 * The child keeps the subscription login (`CLAUDE_CONFIG_DIR`, `CLAUDE_CODE_OAUTH_TOKEN`) and
 * drops every `ANTHROPIC_*` variable (API keys, profiles, federation, custom headers, base URLs
 * and other providers' keys, any of which can outrank or redirect the subscription login) and
 * what a parent Claude Code session exports: its session ID and messaging socket,
 * `CLAUDE_CODE_SIMPLE` (bare mode, API key only) or a provider switch.
 */
export function claudeJudgeEnv(env = process.env) {
  return Object.fromEntries(Object.entries(env).filter(([name]) =>
    !/^(?:ANTHROPIC_.+|CLAUDECODE|CLAUDE_(?!CODE_OAUTH_TOKEN$|CONFIG_DIR$).+)$/u.test(name)));
}

/** Only a successful, unrefused result with a schema-valid structured verdict counts. */
export function parseClaudeJudgeStdout(stdout) {
  const result = parseJsonText(stdout);
  if (!isRecord(result) || result.type !== 'result') throw new Error('claude judge output is not a result message');
  if (result.stop_reason === 'refusal') throw new Error('claude judge refused the request');
  if (result.subtype !== 'success' || result.is_error !== false) {
    const detail = Array.isArray(result.errors) && result.errors.length ? result.errors.join('; ') : String(result.result ?? '');
    throw new Error(`claude judge failed (${result.subtype ?? 'unknown'}): ${detail.slice(0, 400)}`);
  }
  if (!isRecord(result.structured_output)) throw new Error('claude judge returned no structured verdict');
  return parseJudgeResult(result.structured_output);
}

/** A tool-free `claude -p` judge, for agents outside the Anthropic family. Every call runs in its own new directory. */
export function claudeBackend({ model = DEFAULT_CLAUDE_JUDGE_MODEL, reasoningEffort = DEFAULT_REASONING_EFFORT, command = 'claude', run = runProcess, env = process.env } = {}) {
  return {
    model,
    async judge(prompt, controls = {}) {
      const cwd = await mkdtemp(join(tmpdir(), 'agent-judge-claude-'));
      try {
        const args = claudeJudgeArgs({ model, reasoningEffort: controls.reasoningEffort ?? reasoningEffort });
        let stdout;
        try {
          stdout = await run(command, args, { cwd, env: claudeJudgeEnv(env), input: prompt });
        } catch (error) {
          // A failed run prints its result on stdout; name that failure. A nonzero exit is never a verdict.
          let detail = '';
          if (typeof error?.stdout === 'string' && error.stdout.trim()) {
            try { parseClaudeJudgeStdout(error.stdout); } catch (parseError) { detail = ` (${parseError.message})`; }
          }
          throw detail ? new Error(`${error.message}${detail}`, { cause: error }) : error;
        }
        return parseClaudeJudgeStdout(stdout);
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
