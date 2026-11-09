#!/usr/bin/env node
/** Uncued Memory lifecycle checks over the production world_eval journal. No model calls. */
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { mergePending } from './grade.mjs';
import { analyzeToolCalls, toolValidity } from '../agent/tool-evidence.mjs';
import { canonicalJson } from '../agent/eval-contract.mjs';

export const sha256 = (text) => createHash('sha256').update(text).digest('hex');
const key = (scenario, trial, step) => canonicalJson([scenario, trial, step]);
const norm = (value) => String(value ?? '').normalize('NFC').toLowerCase().replace(/\s+/gu, ' ').trim();
const failed = new Set(['failed', 'skipped', 'cancelled', 'interrupted', 'aborted', 'timeout']);
const controls = ['chat_model', 'chat_thinking', 'world_model', 'world_thinking', 'turn_timeout_ms', 'world_timeout_ms', 'stage_timeout_ms'];
const assertions = new Set(['source_retained', 'world_edge', 'world_node', 'record_count', 'lens_created', 'lens_revised',
  'lens_pattern', 'lens_absent', 'records_unchanged', 'memory_read', 'original_not_read', 'no_commit', 'answer_pattern', 'answer_absent',
  'conversation_read', 'no_memory_access', 'tool_validity']);

export function validateMemorySuite(suite) {
  if (suite?.schema !== 'loupist-world-suite/1' || suite.version !== 1 || !suite.suite || !suite.scenarios?.length) throw new Error('Invalid Memory suite');
  const ids = new Set();
  for (const scenario of suite.scenarios) {
    if (!scenario.id || ids.has(scenario.id) || !scenario.steps?.length) throw new Error('Invalid or duplicate scenario');
    ids.add(scenario.id);
    if (scenario.seed_sources?.length) throw new Error('Implicit acquisition must start without seeded Sources');
    const steps = new Set();
    const fixtures = { ...scenario.workspace_files };
    for (const step of scenario.steps) {
      for (const [path, bytes] of Object.entries(step.workspace_files ?? {})) {
        if (path in fixtures || typeof bytes !== 'string') throw new Error('Fixture must be introduced once');
        fixtures[path] = bytes;
      }
      if (!step.id || steps.has(step.id) || !step.user?.trim() || !step.checks?.length) throw new Error('Invalid or duplicate step');
      if (step.sources?.length || step.query) throw new Error('Memory steps must be driven by Chat, not harness ingestion or retrieval');
      // This validates eval inputs, not product prompt wording. Natural teaching and one-off scope are intentional.
      if (/\b(?:world|lens|memory|remember|save|source_save|lens_file|world_read|note_save|result_ref|capability_load|json)\b|저장|기억|보관/iu.test(step.user)) {
        throw new Error(`Explicit Memory hint in ${scenario.id}/${step.id}`);
      }
      if (!['remember', 'read_only', 'off'].includes(step.memory) || !['train', 'probe'].includes(step.role)) throw new Error('Missing Memory mode/role');
      for (const check of step.checks) {
        if (!assertions.has(check.assert) || !check.metric) throw new Error(`Unsupported check: ${check.assert}`);
        if (check.reference_step && !steps.has(check.reference_step)) throw new Error('Check reference must name an earlier step');
        if (check.assert === 'conversation_read' && !check.reference_step) throw new Error('Conversation check requires an earlier user message');
        if (check.pattern != null) new RegExp(check.pattern, 'iu');
        if (check.assert === 'source_retained' && typeof fixtures[check.path] !== 'string') throw new Error('Source check has no fixture');
        if (check.assert === 'record_count' && (!Number.isInteger(check.equals) || check.equals < 0)) throw new Error('Invalid record count');
      }
      steps.add(step.id);
    }
  }
  return suite;
}

/** Null means absent/malformed evidence, and is never an observed empty collection. */
function records(snapshot, kind = 'all') {
  if (!Array.isArray(snapshot?.records)) return null;
  const ids = new Set();
  for (const entry of snapshot.records) {
    const id = entry.record?.recordId;
    if (!id || ids.has(id) || id !== entry.snapshot?.recordId || !entry.snapshot.revisionId
      || entry.record.revisionId !== entry.snapshot.revisionId || entry.record.kind !== entry.snapshot.kind
      || !entry.snapshot.payload || typeof entry.snapshot.payload !== 'object') return null;
    ids.add(id);
  }
  return snapshot.records.filter((entry) => kind === 'all' || entry.snapshot.kind === kind);
}
const identity = (entries) => entries && canonicalJson(entries.map((entry) => ({
  id: entry.record.recordId, revision: entry.snapshot.revisionId, payload: entry.snapshot.payload,
})).sort((a, b) => a.id.localeCompare(b.id)));
const answerText = (row) => typeof row?.chat_trace?.answer === 'string' ? row.chat_trace.answer : null;
// Preserve the original for literal checks, and also recognize equivalent
// Arabic-digit quantities written with Korean magnitude units (14만 5천).
function expandedKoreanQuantities(text) {
  const units = { 십: 10, 백: 100, 천: 1000, 만: 10000, 억: 100000000, 조: 1000000000000 };
  return text.replace(/\d[\d,.]*(?:\s*[조억만천백십](?:\s*\d[\d,.]*)?)+/gu, (expression) => {
    const tokens = expression.replaceAll(',', '').match(/\d+(?:\.\d+)?|[조억만천백십]/gu);
    let total = 0; let group = 0; let number = null;
    for (const token of tokens) {
      const unit = units[token];
      if (!unit) number = Number(token);
      else if (unit < 10000) { group += (number ?? 1) * unit; number = null; }
      else { total += (group + (number ?? 0)) * unit; group = 0; number = null; }
    }
    return String(total + group + (number ?? 0));
  });
}
function createdLenses(row) {
  const before = records(row?.before_chat, 'lens'); const after = records(row?.after_chat, 'lens');
  if (!before || !after) return null;
  return after.filter((entry) => !before.some((old) => old.record.recordId === entry.record.recordId));
}

/** Verify actual retained bytes against canonical span digests, not save receipts or answer prose. */
function retainedSpans(snapshot) {
  const sources = records(snapshot, 'source');
  if (!sources) return null;
  const spans = new Map();
  for (const entry of sources) {
    if (!Array.isArray(entry.canonical_materials) || !Array.isArray(entry.snapshot.payload.source?.captures)) return null;
    const materials = new Map(entry.canonical_materials.map((material) => [material.span_id, material]));
    for (const capture of entry.snapshot.payload.source.captures) for (const representation of capture.representations ?? []) {
      for (const span of representation.spans ?? []) {
        const material = materials.get(span.spanId);
        if (typeof material?.canonical_text !== 'string') continue;
        const hash = sha256(material.canonical_text);
        if (hash !== span.selectedDigest || hash !== material.bytes_sha256 || hash !== material.selected_digest) continue;
        spans.set(span.spanId, { hash, origin: entry.snapshot.payload.source.originLocator, recordId: entry.record.recordId });
      }
    }
  }
  return spans;
}
function graph(snapshot) {
  const worlds = records(snapshot, 'world'); const spans = retainedSpans(snapshot);
  if (!worlds || !spans) return null;
  if (worlds.length > 1) return null;
  if (!worlds.length) return { nodes: [], claims: [] };
  const body = worlds[0].snapshot.payload;
  if (!Array.isArray(body.nodes) || !Array.isArray(body.claims)) return null;
  const nodes = new Map(body.nodes.map((node) => [node.id, node]));
  const claims = body.claims.filter((claim) => claim.status === 'active' && Array.isArray(claim.endpoints)
    && claim.endpoints.every((end) => nodes.has(end.nodeId)) && claim.evidence?.length
    && claim.evidence.every((evidence) => spans.has(evidence.spanId)));
  return { nodes: body.nodes, claims };
}
function entityName(value) {
  return norm(value).replace(/\s*\((?:fictional(?: company)?|가상(?: 기업)?)\)$/u, '');
}
function named(node, name) { return [node.name, ...(node.aliases ?? [])].some((value) => entityName(value) === entityName(name)); }
const result = (pass, detail) => ({ pass, detail });
const unknown = (detail) => result(null, detail);

export function evaluateMemoryCheck(check, row, scenario, prior = new Map()) {
  if (!row) return unknown('stage missing');
  if (row.status !== 'completed') return result(failed.has(row.status) ? false : null, `stage ${row.status ?? 'unknown'}`);
  if (row.chat_trace?.status !== 'completed') return result(failed.has(row.chat_trace?.status) ? false : null, 'Chat did not complete');
  if (check.assert === 'tool_validity') {
    const analysis = analyzeToolCalls(row.chat_trace.tool_calls);
    if (analysis.unknown_call_lists || analysis.unknown_outcomes || analysis.repeated_failures_unknown) return unknown('tool outcomes unavailable');
    return result(analysis.invalid === 0 && analysis.repeated_indexes.length === 0,
      `${analysis.invalid} invalid calls; ${analysis.repeated_indexes.length} repeated failures`);
  }
  if (check.assert === 'no_memory_access') {
    const calls = row.chat_trace.tool_calls; const reads = row.chat_trace.memory_reads;
    if (!Array.isArray(calls) || !Array.isArray(reads) || calls.some((call) => typeof call.name !== 'string')) return unknown('Memory access evidence missing');
    const names = /^(?:world_read|world_search|lens_read|lens_search|source_read|source_search|conversation_memory)$/u;
    return result(!reads.length && !calls.some((call) => names.test(call.name)), 'no cross-chat or canonical Memory access');
  }
  if (check.assert === 'conversation_read') {
    const original = prior.get(check.reference_step)?.chat_trace?.user;
    const calls = row.chat_trace.tool_calls;
    if (typeof original !== 'string' || !Array.isArray(calls)) return unknown('prior user message or tool receipts unavailable');
    const chars = Array.from(original); const hash = sha256(original); const groups = new Map();
    for (const call of calls) {
      if (call.name !== 'conversation_memory' || call.outcome !== 'ok' || call.args?.action !== 'read') continue;
      let body = call.result?.details;
      if (!body) {
        try { body = JSON.parse(call.result?.content?.find((part) => part.type === 'text')?.text); } catch { continue; }
      }
      if (body.role !== 'user' || body.notEvidence !== true || body.textSha256 !== hash
        || body.chatId !== call.args.chatId || body.messageId !== call.args.messageId || !body.chatId || !body.messageId
        || body.characterUnits !== 'Unicode scalar values' || body.totalChars !== chars.length
        || !Number.isInteger(body.offset) || body.offset < 0 || typeof body.text !== 'string' || !body.text.length) continue;
      const length = Array.from(body.text).length; const end = body.offset + length;
      if (end > chars.length || body.text !== chars.slice(body.offset, end).join('')
        || body.nextOffset !== (end < chars.length ? end : null)) continue;
      const id = canonicalJson([body.chatId, body.messageId]);
      groups.set(id, [...(groups.get(id) ?? []), [body.offset, end, body.text]]);
    }
    const hit = [...groups.values()].some((pages) => {
      if (!check.complete) return pages.some((page) => !check.pattern || new RegExp(check.pattern, 'iu').test(page[2]));
      let covered = 0;
      for (const [start, end] of pages.sort((a, b) => a[0] - b[0])) {
        if (start > covered) return false;
        covered = Math.max(covered, end);
      }
      return covered === chars.length;
    });
    return result(hit, `verified original user text SHA-256, author, Unicode slices${check.complete ? ' and complete page coverage' : ''}`);
  }
  if (['answer_pattern', 'answer_absent'].includes(check.assert)) {
    const answer = answerText(row); if (answer === null) return unknown('answer missing');
    const plain = answer.replace(/[*_`]/gu, ''); const pattern = new RegExp(check.pattern, 'iu');
    const normalized = expandedKoreanQuantities(plain).replace(/(\d[\d,.]*)\s*%\s*포인트/gu, '$1%p');
    const hit = pattern.test(plain) || pattern.test(normalized);
    return result(check.assert === 'answer_pattern' ? hit : !hit, `${check.assert}: ${check.pattern}`);
  }
  if (check.assert === 'no_commit') {
    const commits = row.chat_trace.memory_commits;
    if (!Array.isArray(commits) || commits.some((item) => typeof item.kind !== 'string')) return unknown('commit evidence missing');
    const count = commits.filter((item) => check.kind === 'all' || item.kind === check.kind).length;
    return result(count === 0, `${count} ${check.kind} commits`);
  }
  if (check.assert === 'memory_read') {
    const reads = row.chat_trace.memory_reads;
    if (!Array.isArray(reads)) return unknown('read evidence missing');
    let ids = null;
    if (check.reference_step) {
      const learned = createdLenses(prior.get(check.reference_step));
      if (!learned) return unknown('referenced learning snapshot missing');
      ids = new Set(learned.map((entry) => entry.record.recordId));
    } else if (check.kind === 'lens') {
      // Reading the automatic standing perspective does not prove retrieval of
      // a method learned earlier in this scenario.
      const learned = [...prior.values()].flatMap((earlier) => createdLenses(earlier) ?? []);
      if (learned.length) ids = new Set(learned.map((entry) => entry.record.recordId));
    }
    return result(reads.some((read) => read.kind === check.kind && read.access === 'exact_read'
      && read.record_id && (!ids || ids.has(read.record_id))), `acknowledged exact ${check.kind} read`);
  }
  if (check.assert === 'original_not_read') {
    const calls = row.chat_trace.tool_calls;
    if (!Array.isArray(calls)) return unknown('tool evidence missing');
    const candidates = calls.filter((call) => ['read', 'workspace_capture', 'run_python'].includes(call.name));
    if (candidates.some((call) => !call.args || /\[redacted\]|\[truncated\]|"_truncated"/iu.test(JSON.stringify(call.args)))) return unknown('file read arguments unavailable');
    const hit = candidates.find((call) => check.paths.some((path) => JSON.stringify(call.args).includes(path)));
    return result(!hit, hit ? `original accessed by ${hit.name}` : 'no original-file access in observed tools');
  }
  if (check.assert === 'source_retained') {
    const spans = retainedSpans(row.after_chat); if (!spans) return unknown('source snapshot missing');
    const fixtures = Object.assign({}, scenario.workspace_files, ...(scenario.steps ?? []).map((step) => step.workspace_files));
    const hash = sha256(fixtures[check.path]);
    return result([...spans.values()].some((span) => span.hash === hash && String(span.origin).endsWith(check.path)), 'exact retained original bytes and origin');
  }
  if (['world_edge', 'world_node'].includes(check.assert)) {
    const world = graph(row.after_chat); if (!world) return unknown('canonical graph/source evidence missing');
    const ids = (name) => new Set(world.nodes.filter((node) => named(node, name)).map((node) => node.id));
    if (check.assert === 'world_node') {
      const matching = ids(check.name);
      return result(world.claims.some((claim) => claim.endpoints.some((end) => matching.has(end.nodeId))), `evidence-backed node ${check.name}`);
    }
    const from = ids(check.from_name); const to = ids(check.to_name);
    const match = world.claims.some((claim) => {
      if (check.directed === false) return claim.endpoints.some((end) => from.has(end.nodeId)) && claim.endpoints.some((end) => to.has(end.nodeId));
      return claim.endpoints.some((end) => from.has(end.nodeId) && ['source', 'subject'].includes(end.role))
        && claim.endpoints.some((end) => to.has(end.nodeId) && ['target', 'object'].includes(end.role));
    });
    return result(match, `evidence-backed relationship ${check.from_name} → ${check.to_name}`);
  }
  const before = records(row.before_chat, check.kind ?? 'lens'); const after = records(row.after_chat, check.kind ?? 'lens');
  if (!before || !after) return unknown('canonical before/after snapshots missing');
  if (check.assert === 'record_count') return result(after.length === check.equals, `${after.length} ${check.kind} records; expected ${check.equals}`);
  if (check.assert === 'records_unchanged') return result(identity(before) === identity(after), `${check.kind} record identities, revisions and contents unchanged`);
  if (['lens_pattern', 'lens_absent'].includes(check.assert)) {
    let selected = after;
    if (check.reference_step) {
      const learned = createdLenses(prior.get(check.reference_step));
      if (!learned?.length) return unknown('referenced learning snapshot missing');
      selected = after.filter((entry) => learned.some((old) => old.record.recordId === entry.record.recordId));
    }
    const hit = selected.some((entry) => new RegExp(check.pattern, 'iu').test(JSON.stringify(entry.snapshot.payload)));
    return result(check.assert === 'lens_pattern' ? hit : !hit, `${check.assert}: ${check.pattern}`);
  }
  if (check.assert === 'lens_created') {
    const added = createdLenses(row);
    const untouched = before.every((old) => after.some((entry) => identity([entry]) === identity([old])));
    return result(added.length === 1 && untouched, `${added.length} new Lens; existing perspective unchanged=${untouched}`);
  }
  if (check.assert === 'lens_revised') {
    const learned = createdLenses(prior.get(check.reference_step));
    if (!learned) return unknown('referenced learning snapshot missing');
    const id = learned.length === 1 && learned[0].record.recordId;
    const original = before.find((entry) => entry.record.recordId === id);
    const revised = after.find((entry) => entry.record.recordId === id);
    const othersUnchanged = identity(before.filter((entry) => entry.record.recordId !== id)) === identity(after.filter((entry) => entry.record.recordId !== id));
    return result(Boolean(original && revised && original.snapshot.revisionId !== revised.snapshot.revisionId
      && canonicalJson(original.snapshot.payload) !== canonicalJson(revised.snapshot.payload) && othersUnchanged), 'same learned Lens revised without duplicates or perspective changes');
  }
  throw new Error(`Unsupported assertion ${check.assert}`);
}

export function memoryReport(suite, rows, { suiteSha256, trials = 3 } = {}) {
  validateMemorySuite(suite);
  if (!suiteSha256 || !Number.isInteger(trials) || trials < 1) throw new Error('Suite digest and positive trials required');
  const grouped = new Map(); const unexpected = [];
  const expected = new Set();
  for (const scenario of suite.scenarios) for (let trial = 0; trial < trials; trial += 1) {
    for (const step of ['__setup', ...scenario.steps.map((item) => item.id)]) expected.add(key(scenario.id, trial, step));
  }
  for (const row of rows) {
    if (row.schema !== 'loupist-world-trace/1' || row.suite !== suite.suite || row.harness?.suite_sha256 !== suiteSha256) throw new Error('Trace schema, suite or input digest mismatch');
    const cell = key(row.scenario_id, row.trial, row.step_id);
    if (!expected.has(cell)) unexpected.push(cell);
    else grouped.set(cell, [...(grouped.get(cell) ?? []), row]);
  }
  const checks = []; const missing = []; const duplicate = []; const mismatched = []; const configurations = new Map();
  const attemptsSeen = new Map(); const harnessVersions = new Set();
  const observed = []; const trialResults = [];
  for (const scenario of suite.scenarios) for (let trial = 0; trial < trials; trial += 1) {
    const prior = new Map(); const attempts = new Set(); const selected = [];
    for (const id of ['__setup', ...scenario.steps.map((step) => step.id)]) {
      const cell = key(scenario.id, trial, id); const found = grouped.get(cell) ?? [];
      if (!found.length) missing.push(cell);
      else if (found.length > 1) duplicate.push(cell);
      else {
        const item = found[0];
        if (![item.run_id, item.attempt_id].every((value) => typeof value === 'string' && value)) {
          mismatched.push({ scenario: scenario.id, trial, step: id, reason: 'attempt identity unavailable' });
          continue;
        }
        selected.push(item); attempts.add(canonicalJson([item.run_id, item.attempt_id]));
      }
    }
    const coherent = attempts.size === 1;
    if (!coherent && selected.length) mismatched.push({ scenario: scenario.id, trial, reason: 'mixed attempts' });
    if (coherent) {
      const attempt = [...attempts][0];
      if (attemptsSeen.has(attempt)) mismatched.push({ scenario: scenario.id, trial, reason: 'attempt reused across independent trials' });
      else attemptsSeen.set(attempt, { scenario: scenario.id, trial });
    }
    const setup = selected.find((row) => row.step_id === '__setup');
    if (suite.fixture_delivery === 'per_step') {
      for (const stage of [{ id: '__setup', workspace_files: scenario.workspace_files }, ...scenario.steps]) {
        const trace = selected.find((row) => row.step_id === stage.id);
        const expectedFiles = Object.fromEntries(Object.entries(stage.workspace_files ?? {}).map(([path, bytes]) => [path, sha256(bytes)]));
        if (trace && canonicalJson(trace.introduced_workspace_files ?? null) !== canonicalJson(expectedFiles)) {
          mismatched.push({ scenario: scenario.id, trial, step: stage.id, reason: 'fixture introduction evidence mismatch' });
        }
      }
    }
    for (const step of scenario.steps) {
      let row = selected.find((item) => item.step_id === step.id);
      if (row && (controls.some((field) => field.endsWith('_ms')
        ? !Number.isFinite(row.harness[field]) || row.harness[field] <= 0
        : typeof row.harness[field] !== 'string' || !row.harness[field])
        || ['runner_sha256', 'system_prompt_sha256', 'tool_catalog_sha256'].some((field) => typeof row.harness[field] !== 'string' || !row.harness[field]))) {
        mismatched.push({ scenario: scenario.id, trial, step: step.id, reason: 'execution controls or harness identity unavailable' }); row = null;
      }
      const chat = row?.chat_trace;
      if (row && chat && (chat.user !== step.user || chat.memory_mode !== step.memory || chat.session_role !== step.role
        || chat.session_id !== (step.chat ?? step.id) || chat.observed_model !== row.harness.chat_model
        || `${chat.model?.provider}/${chat.model?.model}` !== row.harness.chat_model || chat.model?.thinking !== row.harness.chat_thinking)) {
        mismatched.push({ scenario: scenario.id, trial, step: step.id, reason: 'execution input/model mismatch' }); row = null;
      }
      if (row) {
        configurations.set(canonicalJson(Object.fromEntries(controls.map((field) => [field, row.harness[field] ?? null]))), true);
        harnessVersions.add(canonicalJson(['runner_sha256', 'system_prompt_sha256', 'tool_catalog_sha256'].map((field) => row.harness[field])));
        observed.push(row);
      }
      for (const [index, check] of step.checks.entries()) {
        const evaluated = !coherent ? unknown('incoherent trial') : setup?.status !== 'completed'
          ? result(setup && failed.has(setup.status) ? false : null, 'setup unavailable')
          : evaluateMemoryCheck(check, row, scenario, prior);
        checks.push({ scenario_id: scenario.id, trial, step_id: step.id, check_index: index, assert: check.assert, metric: check.metric, ...evaluated });
      }
      if (row) prior.set(step.id, row);
    }
    const cells = checks.filter((check) => check.scenario_id === scenario.id && check.trial === trial);
    trialResults.push({ scenario_id: scenario.id, trial, pass: cells.some((check) => check.pass === false) ? false : cells.every((check) => check.pass === true) ? true : null });
  }
  const metrics = {};
  for (const metric of new Set(checks.map((check) => check.metric))) {
    const group = checks.filter((check) => check.metric === metric);
    metrics[metric] = { passed: group.filter((check) => check.pass === true).length, failed: group.filter((check) => check.pass === false).length,
      unknown: group.filter((check) => check.pass == null).length, total: group.length };
    metrics[metric].rate_lower_bound = metrics[metric].passed / group.length;
  }
  if (harnessVersions.size > 1) mismatched.push({ reason: 'mixed harness versions in one evaluation' });
  const complete = !missing.length && !duplicate.length && !unexpected.length && !mismatched.length
    && configurations.size === 1 && checks.every((check) => check.pass !== null);
  const scenarioResults = suite.scenarios.map((scenario) => {
    const group = trialResults.filter((trial) => trial.scenario_id === scenario.id); const passed = group.filter((trial) => trial.pass === true).length;
    return { scenario_id: scenario.id, passed, trials, majority: complete ? passed > trials / 2 : null,
      pass_all: complete ? passed === trials : null };
  });
  return { schema: 'loupist-memory-report/1', suite: suite.suite, suite_sha256: suiteSha256, trials,
    scope: 'Deterministic lifecycle, provenance, and answer predicates; not a general semantic quality rating.',
    coverage: { complete, missing, duplicate, unexpected, mismatched }, configurations: [...configurations.keys()].map(JSON.parse),
    metrics, scenarios: scenarioResults, trial_results: trialResults, checks,
    acceptance: { pass: complete && trials === 3 ? scenarioResults.every((scenario) => scenario.pass_all) : null,
      reason: !complete ? 'incomplete or mismatched evidence' : trials !== 3 ? 'diagnostic run; three independent trials required' : 'all predicates in all three trials required' },
    tool_validity: toolValidity(observed.flatMap((row) => row.chat_trace ? [row.chat_trace] : [])),
    durations_ms: observed.map((row) => row.duration_ms ?? null), cost_usd: null,
    provenance: rows.map((row) => ({ run_id: row.run_id, attempt_id: row.attempt_id, scenario_id: row.scenario_id, trial: row.trial,
      step_id: row.step_id, status: row.status, harness: row.harness, scenario_input_sha256: row.scenario_input_sha256 })) };
}

export function compareMemoryReports(baseline, candidate) {
  if (baseline.suite_sha256 !== candidate.suite_sha256 || canonicalJson(baseline.configurations) !== canonicalJson(candidate.configurations)) throw new Error('Comparison inputs or execution controls differ');
  if ((baseline.grader_sha256 || candidate.grader_sha256) && baseline.grader_sha256 !== candidate.grader_sha256) throw new Error('Regrade both journals with the same evaluator before comparing');
  return { baseline_acceptance: baseline.acceptance, candidate_acceptance: candidate.acceptance,
    baseline_trials: baseline.trials, candidate_trials: candidate.trials,
    comparable: baseline.coverage.complete && candidate.coverage.complete,
    metrics: Object.fromEntries(Object.entries(candidate.metrics).map(([metric, value]) => [metric, {
      baseline: baseline.metrics[metric]?.rate_lower_bound ?? null, candidate: value.rate_lower_bound,
      delta: baseline.metrics[metric] ? value.rate_lower_bound - baseline.metrics[metric].rate_lower_bound : null,
    }])) };
}

async function readRows(path, optional = false) {
  try { return (await readFile(path, 'utf8')).split('\n').filter((line) => line.trim()).map((line) => JSON.parse(line)); }
  catch (error) { if (optional && error.code === 'ENOENT') return []; throw error; }
}
export async function main(args) {
  const options = {};
  for (let i = 0; i < args.length; i += 2) {
    const option = args[i];
    if (!['--suite', '--traces', '--out', '--trials', '--baseline'].includes(option) || options[option] || !args[i + 1]) throw new Error(`Unknown, duplicate or incomplete option ${option}`);
    options[option] = args[i + 1];
  }
  if (!options['--suite'] || !options['--traces'] || !options['--out']) throw new Error('--suite, --traces and new --out are required');
  const bytes = await readFile(options['--suite']); const suite = JSON.parse(bytes);
  const rows = mergePending(await readRows(options['--traces']), await readRows(`${options['--traces']}.pending.jsonl`, true));
  const report = memoryReport(suite, rows, { suiteSha256: sha256(bytes), trials: Number(options['--trials'] ?? 3) });
  report.grader_sha256 = sha256(await readFile(fileURLToPath(import.meta.url)));
  if (options['--baseline']) report.comparison = compareMemoryReports(JSON.parse(await readFile(options['--baseline'], 'utf8')), report);
  await writeFile(options['--out'], `${JSON.stringify(report, null, 2)}\n`, { flag: 'wx' });
  process.stdout.write(`${JSON.stringify({ coverage: report.coverage, acceptance: report.acceptance, metrics: report.metrics }, null, 2)}\n`);
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2)).catch((error) => { console.error(error.message); process.exitCode = 1; });
