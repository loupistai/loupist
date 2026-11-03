#!/usr/bin/env node
/** Conservative World evaluation. Semantic labels are coordinator reviews, never human calibration. */
import { createHash } from 'node:crypto';
import { readFile, writeFile, access, mkdir } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { toolValidity } from '../agent/tool-evidence.mjs';
import { trailingJsonObject } from '../agent/grade.mjs';

const canonical = (v) => Array.isArray(v) ? v.map(canonical) : v && typeof v === 'object'
  ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, canonical(v[k])])) : v;
const json = (v) => JSON.stringify(canonical(v));
export const sha256 = (v) => createHash('sha256').update(v).digest('hex');
const digest = (v) => sha256(json(v));
const list = (v) => Array.isArray(v) ? v : [];
const finite = (v) => typeof v === 'number' && Number.isFinite(v);
const normalized = (v) => String(v ?? '').normalize('NFC').replace(/\s+/gu, ' ').trim();
const coord = (r) => ({ scenario_id: r.scenario_id, trial: r.trial, step_id: r.step_id });
const cellKey = (r) => json([r.scenario_id, r.trial, r.step_id]);
const trialKey = (r) => json([r.scenario_id, r.trial]);
const unitKey = (r) => json([r.run_id, r.attempt_id]);
const attemptKey = (r) => json([r.run_id, r.attempt_id, r.scenario_id, r.trial, r.step_id]);
const failed = (s) => ['failed', 'skipped', 'cancelled', 'interrupted', 'aborted', 'timeout'].includes(s);
const all = (v) => v.includes(false) ? false : v.some((x) => x == null) || !v.length ? null : true;
const ratio = (n, d) => d ? n / d : null;
const ANSWER_FACT_MAPPING_SHA256 = 'cd1653a5228174858cbe803593f9743c198ddeab2c4bf4eef7dcec655d1f4478';
const answerFactBytes = readFileSync(new URL('./answer-facts-v2.json', import.meta.url));
if (sha256(answerFactBytes) !== ANSWER_FACT_MAPPING_SHA256) throw new Error('Frozen answer-fact mapping digest mismatch');
const bundledAnswerFacts = JSON.parse(answerFactBytes);

function answerFactPlan(suite, oracle, inputSha256) {
  const acquisitionV2 = oracle.version === 2;
  const mapping = acquisitionV2 ? null : bundledAnswerFacts.suites.find((x) => x.input_sha256 === inputSha256);
  if (mapping && (mapping.suite !== suite.suite || mapping.oracle_content_sha256 !== digest(oracle))) throw new Error('Answer-fact mapping oracle mismatch');
  const plans = new Map(); const knownFacts = new Set(list(oracle.facts).map((x) => x.id));
  for (const scenario of suite.scenarios) for (const step of scenario.steps) {
    const checkpoint = list(oracle.checkpoints).find((x) => x.scenario_id === scenario.id && x.step_id === step.id);
    if (!checkpoint) continue;
    const entry = mapping?.checkpoints.find((x) => x.scenario_id === scenario.id && x.step_id === step.id);
    if (entry && (entry.question_sha256 !== sha256(step.user ?? '') || entry.answer_checks_content_sha256 !== digest(checkpoint.answer_checks ?? []))) throw new Error('Answer-fact mapping question/criteria mismatch');
    const ids = entry?.required_answer_fact_ids ?? checkpoint.required_answer_fact_ids;
    if (!Array.isArray(ids) || new Set(ids).size !== ids.length || ids.some((id) => !knownFacts.has(id))) throw new Error(`Explicit required answer facts are missing/invalid: ${scenario.id}/${step.id}`);
    if (acquisitionV2 && (!['initial_acquisition', 'retained_memory'].includes(checkpoint.evidence_phase) || !ids.length
      || checkpoint.evidence_phase === 'initial_acquisition' && (step.memory !== 'remember' || !scenario.world_enabled
        || checkpoint.storage_checkpoint !== 'after_post_chat_processing'))) throw new Error(`Invalid acquisition evidence phase: ${scenario.id}/${step.id}`);
    plans.set(json([scenario.id, step.id]), { ids, source: entry ? 'frozen_answer_fact_mapping' : 'oracle.required_answer_fact_ids', rationale: entry?.rationale ?? checkpoint.answer_fact_rationale ?? null,
      guard_only_fact_ids: entry?.guard_only_fact_ids ?? [], guard_interpretation: entry?.guard_interpretation ?? null });
  }
  return { plans, metadata: mapping ? { version: bundledAnswerFacts.version, sha256: ANSWER_FACT_MAPPING_SHA256, file: 'answer-facts-v2.json',
    input_sha256: inputSha256, derivation: bundledAnswerFacts.derivation, scope: bundledAnswerFacts.scope, supersedes: bundledAnswerFacts.supersedes }
    : { version: null, sha256: null, source: 'oracle.required_answer_fact_ids', oracle_content_sha256: digest(oracle) } };
}

/** Only the finalized row of the exact same attempt supersedes its journal row. */
export function mergePending(traces, pending) {
  const known = new Set(traces.map(attemptKey));
  return [...traces, ...pending.filter((row) => !known.has(attemptKey(row)))];
}

function validate(suite, oracle, inputSha256, trials, scenarios) {
  if (suite?.schema !== 'loupist-world-suite/1' || suite.version !== 1 || !Array.isArray(suite.scenarios)) throw new Error('Unsupported World suite');
  if (oracle?.schema !== 'loupist-world-oracle/1' || ![1, 2].includes(oracle.version)
    || oracle.version === 2 && oracle.metric_revision !== 'world-acquisition/2') throw new Error('Unsupported World oracle');
  if (!inputSha256 || oracle.input_sha256 !== inputSha256) throw new Error('Frozen corpus digest does not match oracle');
  if (!Number.isInteger(trials) || trials < 1) throw new Error('trials must be positive');
  const seen = new Set();
  for (const s of suite.scenarios) {
    if (!s.id || seen.has(s.id) || !Array.isArray(s.steps)) throw new Error('Invalid/duplicate scenario');
    seen.add(s.id); const steps = new Set();
    for (const step of s.steps) {
      if (!step.id || steps.has(step.id)) throw new Error('Invalid/duplicate step');
      steps.add(step.id);
    }
  }
  for (const id of scenarios) if (!seen.has(id)) throw new Error(`Unknown scenario ${id}`);
  const keys = list(oracle.checkpoints).map((x) => json([x.scenario_id, x.step_id]));
  if (new Set(keys).size !== keys.length) throw new Error('Duplicate oracle checkpoint');
}

function fixtures(suite, oracle) {
  const out = new Map();
  for (const s of suite.scenarios) for (const source of [...list(s.seed_sources), ...s.steps.flatMap((x) => list(x.sources))]) {
    if (out.has(source.id) && out.get(source.id).text !== source.text) throw new Error(`Conflicting source bytes: ${source.id}`);
    out.set(source.id, source);
  }
  for (const [id, text] of Object.entries(oracle.source_texts_for_review_only ?? {})) if (!out.has(id)) out.set(id, { id, text });
  for (const manifest of list(oracle.source_manifest)) {
    const source = out.get(manifest.source_id);
    if (!source || sha256(Buffer.from(source.text)) !== manifest.text_sha256) throw new Error(`Source manifest digest mismatch: ${manifest.source_id}`);
    out.set(source.id, { ...source, origin: source.origin ?? manifest.origin, manifest });
  }
  for (const fact of list(oracle.facts)) {
    const source = out.get(fact.source_id); const e = fact.evidence;
    if (!source || !e || Buffer.from(source.text).subarray(e.utf8_start, e.utf8_end_exclusive).toString('utf8') !== e.quote) throw new Error(`Oracle UTF-8 evidence mismatch: ${fact.id}`);
  }
  return out;
}

function sourceIndex(row, snapshot, sources, oracle) {
  const spans = new Map(); const records = new Map();
  const ingested = new Map([...list(row?.seed_ingestion), ...list(row?.source_ingestion)].filter((x) => x.saved?.spanId).map((x) => [x.saved.spanId, x]));
  for (const entry of list(snapshot?.records)) {
    const snap = entry.snapshot;
    if (snap?.kind === 'world') {
      records.set(entry.record?.recordId, entry); records.set(snap.revisionId, entry); continue;
    }
    const source = snap?.kind === 'source' && snap.payload?.source;
    if (!source) continue;
    for (const id of [entry.record?.recordId, snap.revisionId, source.sourceId]) if (id) records.set(id, entry);
    const materials = new Map(list(entry.canonical_materials).map((x) => [x.span_id, x]));
    for (const capture of list(source.captures)) for (const repr of list(capture.representations)) for (const span of list(repr.spans)) {
      const ingest = ingested.get(span.spanId);
      const material = materials.get(span.spanId) ?? (ingest && { canonical_text: ingest.canonical_text,
        bytes_sha256: ingest.saved.materialDigest, selected_digest: ingest.selected_digest });
      const text = typeof material?.canonical_text === 'string' ? material.canonical_text : null;
      const actual = text == null ? null : sha256(Buffer.from(text));
      const fixture = sources.get(repr.documentId) ?? [...sources.values()].find((x) => x.origin === source.originLocator || actual && x.manifest?.text_sha256 === actual);
      const expected = fixture?.manifest;
      const byteIntegrity = actual == null ? null : actual === material.bytes_sha256 && actual === span.selectedDigest
        && (material.selected_digest == null || actual === material.selected_digest);
      const exact = actual != null && actual === expected?.text_sha256;
      const mapped = Boolean(expected?.canonical_extracted_text && oracle.replay_sha256 && row?.replay_sha256 === oracle.replay_sha256
        && fixture?.origin === source.originLocator && text != null && normalized(text).includes(normalized(fixture.text)));
      const publication = capture.publicationTime?.value ?? capture.publicationTime;
      const metadataChecks = [];
      if (expected?.publication_time) metadataChecks.push({ field: 'publication_time', expected: expected.publication_time, actual: publication,
        pass: Number.isFinite(Date.parse(publication)) && Date.parse(publication) === Date.parse(expected.publication_time) });
      if (expected?.acquired_at) metadataChecks.push({ field: 'acquired_at', expected: expected.acquired_at, actual: capture.receivedAtMs,
        pass: capture.receivedAtMs === Date.parse(expected.acquired_at) });
      if (expected?.authorship) metadataChecks.push({ field: 'authorship', expected: expected.authorship, actual: capture.authorship,
        pass: capture.authorship === expected.authorship });
      if (span.selector?.kind === 'byte_range' && text != null) metadataChecks.push({ field: 'selected_byte_length',
        expected: Buffer.byteLength(text), actual: span.selector.end - span.selector.start,
        pass: Number.isInteger(span.selector.start) && span.selector.start >= 0 && Number.isInteger(span.selector.end)
          && span.selector.end - span.selector.start === Buffer.byteLength(text) });
      const verified = byteIntegrity === false || metadataChecks.some((x) => !x.pass) ? false
        : byteIntegrity == null || !expected ? null : exact || mapped;
      const item = { span_id: span.spanId, record_id: entry.record?.recordId, revision_id: snap.revisionId,
        source_id: source.sourceId, fixture_source_id: fixture?.id ?? null, source_title: source.title,
        origin: source.originLocator, authorship: capture.authorship, publication_time: capture.publicationTime,
        acquired_at_ms: capture.receivedAtMs, coverage: capture.coverage, selector: span.selector, canonical_text: text,
        actual_sha256: actual, expected_sha256: expected?.text_sha256 ?? null, byte_integrity: byteIntegrity,
        fixture_match: exact ? 'exact_bytes' : mapped ? 'normalized_article_body' : null, metadata_checks: metadataChecks, verified,
        original_provenance_group: expected?.original_provenance_group ?? null, original_support_status: expected?.support_status ?? null };
      spans.set(span.spanId, item); records.set(span.spanId, entry);
    }
  }
  return { spans, records };
}

function itemId(row, checkpoint, kind, key, content) {
  return `world:${digest({ run_id: row?.run_id ?? null, attempt_id: row?.attempt_id ?? null, ...coord(checkpoint), kind, key, content })}`;
}

function storageItems(row, checkpoint, snapshot, index) {
  const items = [];
  for (const entry of list(snapshot?.records)) {
    if (entry.snapshot?.kind !== 'world') continue;
    const body = entry.snapshot.payload; const nodes = new Map(list(body?.nodes).map((x) => [x.id, x]));
    const add = (kind, id, content, extra) => items.push({ kind: 'storage', storage_kind: kind,
      item_id: itemId(row, checkpoint, 'storage', `${kind}:${id}`, content), normalized_content_hash: digest(content),
      stored_id: id, content, world_record_id: entry.record.recordId, world_revision_id: entry.snapshot.revisionId, ...extra });
    for (const claim of list(body?.claims)) {
      const endpoints = list(claim.endpoints).map((x) => ({ ...x, node: nodes.get(x.nodeId) ?? null }));
      const evidence = list(claim.evidence).map((x) => ({ ...x, source: index.spans.get(x.spanId) ?? null }));
      add('claim', claim.id, claim, { endpoints, bound_evidence: evidence, allowed_span_ids: evidence.map((e) => e.spanId),
        integrity: all([endpoints.every((x) => x.node), evidence.length ? all(evidence.map((e) => e.source?.verified ?? null)) : null]) });
      items.at(-1).normalized_content_hash = digest({ ...claim, id: undefined, endpoints: endpoints.map(({ role, node }) => ({ role,
        name: node?.name, aliases: node?.aliases, properties: node?.properties })), evidence: evidence.map((e) => ({ role: e.role,
        attribution: e.attribution, origin: e.source?.origin, source_digest: e.source?.actual_sha256 })) });
    }
    for (const node of nodes.values()) {
      const claims = list(body?.claims).filter((x) => list(x.endpoints).some((e) => e.nodeId === node.id));
      const spanIds = [...new Set(claims.flatMap((x) => list(x.evidence).map((e) => e.spanId)))];
      add('node_description', node.id, node, { neighboring_claim_ids: claims.map((x) => x.id),
        bound_evidence: spanIds.map((id) => index.spans.get(id) ?? null), allowed_span_ids: spanIds });
      items.at(-1).normalized_content_hash = digest({ ...node, id: undefined });
    }
    for (const ann of list(body?.annotations)) add('annotation', ann.topicId, ann, {
      bound_evidence: list(ann.sourceSpanIds).map((id) => index.spans.get(id) ?? null), allowed_span_ids: list(ann.sourceSpanIds) });
  }
  return items;
}

function chatEvidence(row, prior, index) {
  const tools = []; const reads = [];
  for (const stage of [...prior, row].filter(Boolean)) {
    const chat = stage.chat_trace; if (!chat) continue;
    list(chat.tool_calls).forEach((call, i) => {
      const text = json({ args: call.args, result: call.result });
      tools.push({ ...call, ref: `tool:${stage.step_id}:${i}`, stage_id: stage.step_id,
        resolved_record_ids: [...index.records.keys()].filter((id) => id && text.includes(id)),
        result_available: call.result != null && !call.result_redacted && !call.result?.truncated && !call.result?._truncated });
    });
    list(chat.memory_reads).forEach((read, i) => reads.push({ ...read, ref: `read:${stage.step_id}:${i}`,
      stage_id: stage.step_id, resolved: index.records.has(read.record_id),
      acknowledged: read.access === 'exact_read' && ['source', 'world'].includes(read.kind) && index.records.has(read.record_id) }));
  }
  return { tools, reads, answer_ref: 'answer', answer: row?.chat_trace?.answer ?? null,
    current_chat_id: row?.chat_trace?.session_id ?? null, prior_same_chat_stages: prior.map((x) => x.step_id) };
}

export function numericalChecks(answer, expected = {}, tolerance = 1e-9) {
  const candidates = [...String(answer ?? '').matchAll(/```(?:json)?\s*([\s\S]*?)```/gu)].map((x) => x[1]);
  const trailing = trailingJsonObject(answer); if (trailing) candidates.push(trailing);
  let metrics = null;
  for (const candidate of candidates) try { const parsed = JSON.parse(candidate); if (parsed?.metrics && !Array.isArray(parsed.metrics)) metrics = parsed.metrics; } catch { /* prose is not numeric evidence */ }
  const checks = Object.entries(expected ?? {}).map(([key, value]) => ({ key, expected: value,
    actual: metrics && Object.hasOwn(metrics, key) ? metrics[key] : null, present: Boolean(metrics && Object.hasOwn(metrics, key)),
    pass: Boolean(metrics && Object.hasOwn(metrics, key) && (value === null ? metrics[key] === null
      : finite(value) && finite(metrics[key]) && Math.abs(metrics[key] - value) <= tolerance)) }));
  return { checks, accuracy: ratio(checks.filter((x) => x.pass).length, checks.length), pass: checks.length ? checks.every((x) => x.pass) : null, parsed_metrics: metrics };
}

function expectedCells(suite, oracle, traces, { trials, scenarios, selection, inputSha256 }) {
  const expected = new Map(); const choices = new Map();
  const checkpoints = new Map(list(oracle.checkpoints).map((x) => [json([x.scenario_id, x.step_id]), x]));
  const coverage = { expected_stages: 0, observed_attempt_rows: traces.length, completed_stages: 0, failed_stages: 0,
    missing: [], ambiguous: [], unexpected: [], incoherent_trials: [], reused_loupists: [], mismatched: [], setup_missing: [],
    selected_attempts: [], discarded_from_quality_only: [] };
  if (selection) {
    if (selection.schema !== 'loupist-world-selection/1' || !Array.isArray(selection.trials)) throw new Error('Invalid selection file');
    for (const choice of selection.trials) {
      if (choices.has(trialKey(choice)) || !choice.run_id || !choice.attempt_id) throw new Error('Invalid/duplicate selection');
      choices.set(trialKey(choice), unitKey(choice));
    }
  }
  for (const scenario of suite.scenarios.filter((s) => !scenarios.length || scenarios.includes(s.id))) for (let trial = 0; trial < trials; trial += 1) {
    for (const [step_index, step] of scenario.steps.entries()) {
      const cp = checkpoints.get(json([scenario.id, step.id]));
      const checkpoint = { ...(cp ?? {}), scenario_id: scenario.id, step_id: step.id, trial };
      expected.set(cellKey(checkpoint), { scenario, step, step_index, checkpoint, oracle_missing: !cp, rows: [] });
    }
  }
  coverage.expected_stages = expected.size;
  for (const row of traces) {
    if (row.schema !== 'loupist-world-trace/1' || !row.run_id || !row.attempt_id || !Number.isInteger(row.trial)) throw new Error('Invalid trace identity');
    if (scenarios.length && !scenarios.includes(row.scenario_id)) continue;
    if (row.suite !== suite.suite) { coverage.unexpected.push({ ...coord(row), reason: 'different_suite' }); continue; }
    if (String(row.step_id).startsWith('__')) continue;
    const cell = expected.get(cellKey(row));
    if (!cell) { coverage.unexpected.push(coord(row)); continue; }
    const choice = choices.get(trialKey(row));
    if (choice && choice !== unitKey(row)) coverage.discarded_from_quality_only.push(attemptKey(row));
    else cell.rows.push(row);
  }
  const units = new Map(); const loupists = new Map();
  const cells = [...expected.values()].map((cell) => {
    const where = coord(cell.checkpoint);
    if (!cell.rows.length) { coverage.missing.push(where); return { ...cell, row: null, status: 'missing' }; }
    if (cell.rows.length !== 1) { coverage.ambiguous.push({ ...where, attempts: cell.rows.map(attemptKey) }); return { ...cell, row: null, status: 'ambiguous' }; }
    const row = cell.rows[0]; const unit = units.get(trialKey(row)) ?? new Set(); unit.add(unitKey(row)); units.set(trialKey(row), unit);
    if (row.loupist_id) { const used = loupists.get(row.loupist_id) ?? new Set(); used.add(trialKey(row)); loupists.set(row.loupist_id, used); }
    const mismatch = [];
    if (row.harness?.suite_sha256 !== inputSha256) mismatch.push('suite_sha256');
    if (row.condition !== cell.scenario.condition) mismatch.push('condition');
    if (row.status === 'completed') {
      if (!row.loupist_id || row.processing?.status !== 'completed') mismatch.push('processing');
      if (!row.before_chat?.records || row.before_chat.query_status === 'failed') mismatch.push('before_snapshot');
      if (cell.step.user != null) {
        const chat = row.chat_trace;
        if (chat?.status !== 'completed') mismatch.push('chat_completion');
        if (chat?.user !== cell.step.user) mismatch.push('user');
        if (chat?.memory_mode !== (cell.step.memory ?? 'read_only')) mismatch.push('memory_mode');
        if (!row.harness?.chat_model || chat?.observed_model !== row.harness.chat_model) mismatch.push('observed_model');
        if (chat?.model?.thinking !== row.harness?.chat_thinking) mismatch.push('thinking');
        if (row.post_chat_processing?.status !== 'completed' || !row.after_chat?.records || row.after_chat.query_status === 'failed') mismatch.push('post_chat_processing');
      }
      coverage.completed_stages += 1;
    } else if (failed(row.status)) coverage.failed_stages += 1;
    else mismatch.push('unknown_status');
    if (mismatch.length) coverage.mismatched.push({ ...where, fields: mismatch });
    const setup = traces.filter((x) => x.step_id === '__setup' && trialKey(x) === trialKey(row) && unitKey(x) === unitKey(row));
    if (setup.length !== 1 || setup[0].status !== 'completed') coverage.setup_missing.push(where);
    coverage.selected_attempts.push(attemptKey(row));
    return { ...cell, row, status: row.status };
  });
  for (const [key, value] of units) if (value.size !== 1) coverage.incoherent_trials.push({ trial: JSON.parse(key), attempts: [...value].map(JSON.parse) });
  for (const [loupist_id, value] of loupists) if (value.size > 1) coverage.reused_loupists.push({ loupist_id, trials: [...value].map(JSON.parse) });
  for (const key of choices.keys()) if (![...expected.values()].some((x) => trialKey(x.checkpoint) === key)) throw new Error('Selection is outside requested matrix');
  coverage.complete = coverage.completed_stages === expected.size && expected.size > 0
    && ['missing', 'ambiguous', 'unexpected', 'incoherent_trials', 'reused_loupists', 'mismatched', 'setup_missing'].every((k) => coverage[k].length === 0)
    && cells.every((x) => !x.oracle_missing);
  return { cells, coverage };
}

function labelIndex(labels) {
  const result = new Map();
  for (const label of labels) { const bucket = result.get(label.item_id) ?? []; bucket.push(label); result.set(label.item_id, bucket); }
  return result;
}

function labelFor(item, labels, problems) {
  const found = labels.get(item.item_id) ?? [];
  if (!found.length) { problems.missing.push(item.item_id); return null; }
  if (found.length !== 1) { problems.duplicate.push(item.item_id); return null; }
  const label = found[0];
  if (label.kind !== item.kind || label.review_type !== 'coordinator_review' || !normalized(label.reviewer)
    || !normalized(label.rationale) || !Number.isFinite(Date.parse(label.reviewed_at))) {
    problems.invalid.push({ item_id: item.item_id, reason: 'invalid_review_provenance' }); return null;
  }
  return label;
}

function reviewStorage(items, labels, index, facts, required, problems, snapshotExists) {
  const atoms = new Map(); const matched = new Set(); let reviewed = 0;
  for (const item of items) {
    const label = labelFor(item, labels, problems); if (!label) continue;
    const invalid = (reason) => problems.invalid.push({ item_id: item.item_id, reason });
    if (label.reviewed_all_assertions !== true || !Array.isArray(label.atoms)
      || (!label.atoms.length && label.no_factual_assertions !== true)) { invalid('incomplete_atomic_review'); continue; }
    let valid = true;
    for (const atom of label.atoms) {
      if (!normalized(atom.text) || !normalized(atom.semantic_key) || typeof atom.supported !== 'boolean' || !Array.isArray(atom.fact_ids)) {
        invalid('malformed_atom'); valid = false; continue;
      }
      const evidence = list(atom.evidence);
      const support = evidence.length > 0 && evidence.every((e) => {
        const span = index.spans.get(e.span_id);
        return item.allowed_span_ids.includes(e.span_id) && span?.verified === true && typeof e.quote === 'string'
          && e.quote.length >= 8 && span.canonical_text.includes(e.quote);
      });
      if (atom.supported && (!support || item.integrity === false)) { invalid('unsupported_evidence_reference'); valid = false; continue; }
      if (!atom.supported && atom.fact_ids.length) { invalid('unsupported_atom_cannot_satisfy_recall'); valid = false; continue; }
      let factValid = true;
      for (const id of atom.fact_ids) {
        const fact = facts.get(id);
        const verified = fact && evidence.some((e) => {
          const span = index.spans.get(e.span_id);
          return span?.fixture_source_id === fact.source_id && span.verified === true
            && normalized(span.canonical_text).includes(normalized(fact.evidence.quote));
        });
        if (!verified) { invalid(`unverified_fact:${id}`); valid = false; factValid = false; }
      }
      const key = normalized(atom.semantic_key);
      const prior = atoms.get(key);
      if (prior && (prior.supported !== atom.supported || normalized(prior.text) !== normalized(atom.text))) {
        invalid('conflicting_duplicate_semantic_key'); valid = false; continue;
      }
      atoms.set(key, { ...atom, item_ids: [...(prior?.item_ids ?? []), item.item_id] });
      if (atom.supported && factValid) for (const id of atom.fact_ids) matched.add(id);
    }
    if (valid) reviewed += 1;
  }
  const complete = snapshotExists && reviewed === items.length;
  const supported = [...atoms.values()].filter((x) => x.supported).length;
  const requiredMatched = required.filter((id) => matched.has(id));
  return { stored_items: items.length, reviewed_items: reviewed, reviewed_all_items: complete,
    distinct_reviewed_atoms: atoms.size, supported_atoms: supported,
    precision: complete ? ratio(supported, atoms.size) : null, required_facts: required.length,
    matched_fact_ids: requiredMatched, missing_fact_ids: required.filter((id) => !matched.has(id)),
    recall: complete ? ratio(requiredMatched.length, required.length) : null };
}

function reviewBoolean(item, labels, evidence, problems) {
  const label = labelFor(item, labels, problems); if (!label) return null;
  const adverse = item.kind === 'adverse'; const value = adverse ? label.occurred : label.pass;
  const refs = list(label.evidence_refs);
  const allowed = new Set([...(evidence.answer != null ? ['answer'] : []), ...evidence.reads.map((x) => x.ref),
    ...evidence.tools.filter((x) => x.result_available || x.outcome === 'error').map((x) => x.ref)]);
  if (typeof value !== 'boolean' || !refs.length || refs.some((ref) => !allowed.has(ref))
    || adverse && typeof label.applicable !== 'boolean' || adverse && !label.applicable && value) {
    problems.invalid.push({ item_id: item.item_id, reason: 'invalid_boolean_or_evidence_reference' }); return null;
  }
  if (item.kind === 'utilization' && value && !evidence.reads.some((read) => read.acknowledged && refs.includes(read.ref))) {
    problems.invalid.push({ item_id: item.item_id, reason: 'no_acknowledged_original_memory_read' }); return null;
  }
  return adverse ? { occurred: value, applicable: label.applicable } : value;
}

function toolPayload(call) {
  if (call?.result?.details && typeof call.result.details === 'object') return call.result.details;
  for (const part of list(call?.result?.content)) if (typeof part.text === 'string') {
    try { return JSON.parse(part.text); } catch { /* Plain tool text is not a structured receipt. */ }
  }
  return call?.result ?? null;
}

function exactCurrentRead(evidence, read, entry, span, fact, quote, currentStage, reviewedWorldItems) {
  if (read.stage_id !== currentStage) return false;
  return evidence.tools.some((call) => {
    if (call.stage_id !== currentStage || !['world_read', 'source_read'].includes(call.name)
      || call.outcome !== 'ok' || !call.result_available) return false;
    const payload = toolPayload(call);
    if (payload?.recordId !== entry?.record?.recordId || payload.revisionId !== entry?.snapshot?.revisionId
      || payload.status !== 'delivered_for_this_turn' || !payload.readRef) return false;
    const delivered = normalized(payload.text ?? json(payload));
    if (entry.snapshot.kind === 'source') return payload.scope === 'source' && payload.source?.spanId === span.span_id
      && payload.source?.selectedDigest === span.actual_sha256 && delivered.includes(normalized(quote))
      && delivered.includes(normalized(fact.evidence.quote));
    // World prose may paraphrase its original. Require the actual reviewed
    // assertion to be delivered, not merely another claim bound to that span.
    return payload.scope === 'world' && reviewedWorldItems.some((item) => item.world_record_id === entry.record.recordId
      && item.world_revision_id === payload.revisionId && delivered.includes(item.stored_id)
      && delivered.includes(normalized(item.content.proposition ?? item.content.description)));
  });
}

function reviewUse(item, labels, evidence, index, fact, problems, currentStage = null, storedItems = []) {
  const pass = reviewBoolean(item, labels, evidence, problems);
  if (pass !== true) return pass;
  const label = labels.get(item.item_id)[0];
  const sourceEvidence = list(label.source_evidence);
  const valid = fact && sourceEvidence.length > 0 && sourceEvidence.every((e) => {
    const span = index.spans.get(e.span_id);
    if (span?.verified !== true || span.fixture_source_id !== fact.source_id || typeof e.quote !== 'string' || e.quote.length < 8
      || !span.canonical_text.includes(e.quote) || !normalized(span.canonical_text).includes(normalized(fact.evidence.quote))) return false;
    return evidence.reads.some((read) => {
      if (!read.acknowledged || !label.evidence_refs.includes(read.ref)) return false;
      const entry = index.records.get(read.record_id);
      const linked = entry?.record?.recordId === span.record_id || entry?.snapshot?.kind === 'world'
        && list(entry.snapshot.payload?.claims).some((claim) => list(claim.evidence).some((binding) => binding.spanId === e.span_id));
      const reviewedWorldItems = storedItems.filter((stored) => ['claim', 'node_description'].includes(stored.storage_kind)
        && normalized(stored.content.proposition ?? stored.content.description)
        && list(labels.get(stored.item_id)?.[0]?.atoms).some((atom) => atom.supported === true && list(atom.fact_ids).includes(fact.id)
          && list(atom.evidence).some((binding) => binding.span_id === span.span_id)));
      return linked && (currentStage == null || exactCurrentRead(evidence, read, entry, span, fact, e.quote, currentStage, reviewedWorldItems));
    });
  });
  if (!valid) { problems.invalid.push({ item_id: item.item_id, reason: 'no_verified_fact_source_linked_to_actual_read' }); return null; }
  return true;
}

function reviewAcquisition(item, labels, evidence, index, fact, row, problems, sources) {
  const pass = reviewBoolean(item, labels, evidence, problems);
  if (pass !== true) return pass;
  const label = labels.get(item.item_id)[0]; const refs = list(label.evidence_refs);
  const current = evidence.tools.filter((call) => call.stage_id === row?.step_id && call.outcome === 'ok' && call.result_available && refs.includes(call.ref));
  const valid = refs.includes('answer') && list(label.source_evidence).length > 0 && list(label.source_evidence).every((e) => {
    const span = index.spans.get(e.span_id);
    if (!fact || span?.verified !== true || span.fixture_source_id !== fact.source_id || typeof e.quote !== 'string' || e.quote.length < 8
      || !span.canonical_text.includes(e.quote) || !normalized(span.canonical_text).includes(normalized(fact.evidence.quote))) return false;
    let savedMaterial; try { savedMaterial = JSON.parse(span.canonical_text); } catch { return false; }
    const fetchedAndSaved = current.some((fetch) => {
      const payload = toolPayload(fetch);
      // web_fetch hashes the transport response before HTML extraction. The
      // manifest binds those bytes separately from the normalized article.
      const transportDigest = sources.get(fact.source_id)?.manifest?.transport_html_sha256;
      if (fetch.name !== 'web_fetch' || !payload?.result_ref || typeof payload.content !== 'string'
        || !(payload.status >= 200 && payload.status < 300) || payload.truncated !== false || payload.extraction_truncated !== false
        || (payload.content_offset ?? 0) !== 0 || payload.next_offset != null || payload.final_url !== span.origin
        || (transportDigest ?? sha256(payload.content)) !== payload.content_sha256
        || !normalized(payload.content).includes(normalized(fact.evidence.quote))) return false;
      const { result_ref, citation_url, ...material } = payload;
      if (json(savedMaterial) !== json(material)) return false;
      return current.some((save) => {
        const result = toolPayload(save);
        return save.name === 'source_save' && save.args?.result_ref === result_ref && result?.result_ref === result_ref
          && result.status === 'staged' && typeof result.saveRef === 'string' && result.saveRef.length > 0;
      });
    });
    const committed = list(row?.chat_trace?.memory_commits).some((commit) => commit.kind === 'source'
      && ['create', 'update'].includes(commit.operation) && [span.source_id, span.record_id].includes(commit.record_id));
    const completed = row?.post_chat_processing?.status === 'completed' && !list(row.post_chat_processing.failures).length
      && list(row.post_chat_processing.jobs).some((job) => {
        const binding = job.document?.sourceBinding; const world = index.records.get(job.worldRecordId);
        return job.status === 'completed' && job.loupistId === row.loupist_id && binding?.sourceId === span.source_id
          && binding.sourceRecordId === span.record_id && binding.sourceRevisionId === span.revision_id
          && binding.spanId === span.span_id && binding.payloadDigest === span.actual_sha256
          && world?.snapshot?.kind === 'world' && job.document.sealedRevisionId === world.snapshot.revisionId;
      });
    return fetchedAndSaved && committed && completed;
  });
  if (!valid) { problems.invalid.push({ item_id: item.item_id, reason: 'no_verified_current_acquisition_commit_and_world_chain' }); return null; }
  return true;
}

function authority(row, scenario, step, snapshot) {
  if (!row) return { pass: null, checks: [] };
  const checks = [];
  if (step.user != null && ['read_only', 'off'].includes(step.memory ?? 'read_only')) checks.push({ id: 'read_only_no_commits',
    pass: Array.isArray(row.chat_trace?.memory_commits) ? row.chat_trace.memory_commits.length === 0 : null });
  if (step.memory === 'off') checks.push({ id: 'memory_off_no_reads', pass: Array.isArray(row.chat_trace?.memory_reads) ? row.chat_trace.memory_reads.length === 0 : null });
  checks.push({ id: 'loupist_owned_snapshots', pass: snapshot?.records ? snapshot.records.every((x) => x.record?.ownerLoupistId === row.loupist_id) : null });
  if (!scenario.world_enabled) {
    checks.push({ id: 'disabled_world_no_inference', pass: Array.isArray(row.world_inference) ? row.world_inference.length === 0 : null });
    checks.push({ id: 'disabled_world_no_snapshot', pass: snapshot?.records ? !snapshot.records.some((x) => x.snapshot?.kind === 'world') : null });
  } else if (list(row.world_inference).length) {
    checks.push({ id: 'world_model_and_thinking_match_harness', pass: row.world_inference.every((call) =>
      `${call.provider}/${call.model}` === row.harness?.world_model && call.thinking_level === row.harness?.world_thinking) });
  }
  return { checks, pass: all(checks.map((x) => x.pass)) };
}

function adverseFor(oracle, scenario, checkpoint) {
  const families = new Set([...list(checkpoint.required_stored_fact_ids), ...list(checkpoint.source_only_fact_ids)].map((x) => x.split('-')[0]));
  return list(oracle.adverse_opportunities).filter((x) => (!x.family || families.has(x.family))
    && (!x.applicable_conditions || x.applicable_conditions.includes(scenario.condition)));
}

function repeatSummary(cells, trials) {
  const groups = new Map();
  for (const cell of cells) { const key = json([cell.scenario_id, cell.step_id]); const group = groups.get(key) ?? []; group.push(cell); groups.set(key, group); }
  return [...groups.values()].map((group) => {
    group.sort((a, b) => a.trial - b.trial);
    const out = { scenario_id: group[0].scenario_id, step_id: group[0].step_id, trials: group.length };
    for (const metric of ['answer_success', 'utilization_success', ...(Object.hasOwn(group[0], 'acquisition') ? ['acquisition_success'] : [])]) {
      const values = group.map((x) => x[metric]); const known = values.every((x) => typeof x === 'boolean') && group.length === trials;
      out[metric] = { values, mean: known ? ratio(values.filter(Boolean).length, values.length) : null,
        majority_of_3: known && trials === 3 ? values.filter(Boolean).length >= 2 : null,
        pass_all_3: known && trials === 3 ? values.every(Boolean) : null };
    }
    for (const metric of ['storage_precision', 'storage_recall', 'utilization', 'negative_transfer', ...(Object.hasOwn(group[0], 'acquisition') ? ['acquisition'] : [])]) {
      const values = group.map((x) => x[metric]);
      out[metric] = { values, mean: values.every(finite) && values.length === trials ? values.reduce((a, b) => a + b, 0) / values.length : null };
    }
    return out;
  });
}

function sumKnown(values) { return { total: values.length && values.every(finite) ? values.reduce((a, b) => a + b, 0) : null,
  known_total: values.filter(finite).reduce((a, b) => a + b, 0), known: values.filter(finite).length, expected: values.length }; }
function percentile(values, p) { const ordered = values.filter(finite).sort((a, b) => a - b); return ordered.length ? ordered[Math.ceil(ordered.length * p) - 1] : null; }
function workload(traces) {
  const stages = traces.filter((x) => !String(x.step_id).startsWith('__')); const chats = stages.map((x) => x.chat_trace).filter(Boolean);
  const inference = new Map();
  for (const row of traces) for (const call of list(row.world_inference)) inference.set(json([row.run_id, row.attempt_id, call.call_index]), call);
  const calls = [...inference.values()]; const tokens = {}; const world = {};
  for (const key of ['input', 'output', 'cache_read', 'cache_write']) tokens[key] = sumKnown(chats.map((x) => x.usage?.[key]));
  for (const [key, aliases] of Object.entries({ input: ['input', 'inputTokens', 'input_tokens'], output: ['output', 'outputTokens', 'output_tokens'],
    cache_read: ['cacheRead', 'cache_read'], cache_write: ['cacheWrite', 'cache_write'] })) world[key] = sumKnown(calls.map((x) => aliases.map((name) => x.usage?.[name]).find(finite)));
  return { stages_attempted: stages.length, chat_attempts: chats.length, world_model_calls: calls.length,
    duration_ms: sumKnown(stages.map((x) => x.duration_ms)), p50_ms: percentile(stages.map((x) => x.duration_ms), 0.5),
    p95_ms: percentile(stages.map((x) => x.duration_ms), 0.95), chat_tokens: tokens, world_tokens: world,
    world_model_duration_ms: sumKnown(calls.map((x) => x.duration_ms)), world_prompt_bytes: sumKnown(calls.map((x) => x.prompt_bytes)),
    chat_tool_result_bytes: sumKnown(chats.flatMap((x) => list(x.tool_calls).map((call) => call.result_bytes))),
    cost_usd: null, cost_note: 'Provider billing is unavailable; unknown is never zero.', usage_covers_unstarted_or_missing_attempts: false };
}

/** Completed jobs retain publication receipts, not historical extraction/World copies. */
function completedJobIntegrity(traces, sources, oracle) {
  const jobs = new Map();
  const hash = (value) => typeof value === 'string' && /^[a-f0-9]{64}$/u.test(value);
  const text = (value) => typeof value === 'string' && value.trim().length > 0;
  for (const row of traces) {
    const indexes = ['before_chat', 'after_chat'].map((phase) => sourceIndex(row, row[phase], sources, oracle));
    const spans = new Map(indexes.flatMap((index) => [...index.spans]));
    const records = new Map(indexes.flatMap((index) => [...index.records]));
    for (const phase of ['processing', 'post_chat_processing']) for (const job of list(row[phase]?.jobs)) {
      if (job.status !== 'completed') continue;
      // The output digest is part of the receipt, not identity: changing it must fail consistency.
      const key = json([row.run_id, row.attempt_id, row.scenario_id, row.trial, job.jobId]);
      const document = job.document ?? {}; const binding = document.sourceBinding;
      const receipt = { loupist_id: job.loupistId, world_record_id: job.worldRecordId,
        input_digest: job.payloadDigest, output_digest: document.logicalDigest,
        sealed_revision_id: document.sealedRevisionId, episode_kind: document.episodeKind,
        source_binding: binding ?? null, model_profile: document.extraction?.modelProfile };
      const errors = [];
      const world = records.get(job.worldRecordId);
      if (!text(job.jobId) || !text(job.loupistId) || job.loupistId !== row.loupist_id
        || world?.snapshot?.kind !== 'world' || world.record?.recordId !== job.worldRecordId
        || world.record?.ownerLoupistId !== job.loupistId) errors.push('invalid_job_identity');
      if (!hash(receipt.input_digest) || !hash(receipt.output_digest)) errors.push('invalid_job_digest');
      if (!text(receipt.sealed_revision_id)) errors.push('missing_sealed_revision');
      if (!['source_span', 'sealed_snapshot', 'world_recovery'].includes(receipt.episode_kind)) errors.push('invalid_episode_kind');
      if (document.usageUncertain !== false || document.leaseExpiresAtMs !== null || job.hostMayRedrive !== false) errors.push('unsettled_completed_job');
      if (document.episodeKind === 'source_span') {
        if (!binding || !['sourceId', 'sourceRecordId', 'sourceRevisionId', 'spanId'].every((field) => text(binding[field]))
          || !hash(binding.payloadDigest)) errors.push('invalid_source_binding');
        const span = spans.get(binding?.spanId);
        if (!span || span.record_id !== binding?.sourceRecordId || span.revision_id !== binding?.sourceRevisionId
          || span.source_id !== binding?.sourceId || span.actual_sha256 !== binding?.payloadDigest || span.byte_integrity !== true) errors.push('source_binding_mismatch');
        if (!text(receipt.model_profile) || document.inferenceAttempted !== true) errors.push('missing_inference_receipt');
      }
      if (document.artifactUtf8 != null && (typeof document.artifactUtf8 !== 'string'
        || sha256(document.artifactUtf8) !== receipt.output_digest)) errors.push('artifact_digest_mismatch');
      const prior = jobs.get(key);
      if (prior && json(prior.receipt) !== json(receipt)) errors.push('completed_job_receipt_changed');
      jobs.set(key, { check_id: `job:${sha256(key)}`, run_id: row.run_id, attempt_id: row.attempt_id,
        scenario_id: row.scenario_id, trial: row.trial, job_id: job.jobId, receipt: prior?.receipt ?? receipt,
        errors: [...new Set([...(prior?.errors ?? []), ...errors])],
        observed_at: [...(prior?.observed_at ?? []), { step_id: row.step_id, phase }] });
    }
  }
  const checks = [...jobs.values()].map((item) => ({ ...item, pass: item.errors.length === 0 }));
  return { required_completed_job_receipts: checks.length, checks, pass: checks.every((item) => item.pass) };
}

/** Raw attempts are immutable. Reports contain the planned matrix and complete review bundles. */
export function buildReport({ suite, oracle, traces, labels = [], trials = 3, scenarios = [], selection = null, inputSha256 }) {
  validate(suite, oracle, inputSha256, trials, scenarios);
  const acquisitionV2 = oracle.version === 2;
  const answerPlan = answerFactPlan(suite, oracle, inputSha256);
  const sources = fixtures(suite, oracle); const facts = new Map(list(oracle.facts).map((x) => [x.id, x]));
  const { cells, coverage } = expectedCells(suite, oracle, traces, { trials, scenarios, selection, inputSha256 });
  const indexedLabels = labelIndex(labels); const problems = { missing: [], duplicate: [], invalid: [], unmatched: [] };
  const audit = []; const measurements = [];
  for (const cell of cells) {
    const { row, scenario, step, checkpoint } = cell;
    const phase = checkpoint.storage_checkpoint === 'after_post_chat_processing' ? 'after_chat' : 'before_chat';
    const snapshot = row?.[phase]; const index = sourceIndex(row, snapshot, sources, oracle);
    const prior = cells.filter((x) => x.row && row && x.step_index < cell.step_index && x.scenario.id === scenario.id
      && x.checkpoint.trial === checkpoint.trial && unitKey(x.row) === unitKey(row)
      && x.row.chat_trace?.session_id === row.chat_trace?.session_id).map((x) => x.row);
    const evidence = chatEvidence(row, prior, index); const required = list(checkpoint.required_stored_fact_ids);
    const stored = storageItems(row, checkpoint, snapshot, index);
    const applicable = checkpoint.world_storage_applicable !== false && scenario.world_enabled;
    const storage = applicable ? reviewStorage(stored, indexedLabels, index, facts, required, problems, Boolean(snapshot?.records)) : null;
    const base = { checkpoint: coord(checkpoint), run_id: row?.run_id ?? null, attempt_id: row?.attempt_id ?? null };
    const answerItems = list(checkpoint.answer_checks).map((check) => ({ kind: 'answer_check', criterion_id: check.id,
      requirement: check.requirement, item_id: itemId(row, checkpoint, 'answer_check', check.id, { check, answer: evidence.answer }) }));
    // Acquisition/process requirements are separately reviewed; a correct answer does not prove them.
    const traceItems = list(checkpoint.trace_requirements).map((requirement, i) => ({ kind: 'trace_check', criterion_id: `trace-${i}`,
      requirement, item_id: itemId(row, checkpoint, 'trace_check', i, { requirement, evidence }) }));
    const answerReviews = [...answerItems, ...traceItems].map((item) => ({ item_id: item.item_id, criterion_id: item.criterion_id,
      pass: reviewBoolean(item, indexedLabels, evidence, problems) }));
    const factPlan = answerPlan.plans.get(json([scenario.id, step.id]));
    const initialAcquisition = acquisitionV2 && checkpoint.evidence_phase === 'initial_acquisition';
    const answerFactIds = initialAcquisition ? [] : factPlan?.ids ?? [];
    const acquisitionFactIds = initialAcquisition ? factPlan?.ids ?? [] : [];
    const useItems = answerFactIds.map((id) => ({ kind: 'utilization', fact_id: id, required_fact: facts.get(id), metric_scope: 'primary',
      requirement: oracle.grading_contract?.utilization ?? 'Correct answer use of this required fact through an actual acknowledged retained Source/World read.',
      item_id: itemId(row, checkpoint, 'utilization', id, { fact: facts.get(id), answer: evidence.answer, reads: evidence.reads }) }));
    const useReviews = useItems.map((item) => ({ item_id: item.item_id, fact_id: item.fact_id,
      pass: reviewUse(item, indexedLabels, evidence, index, facts.get(item.fact_id), problems, acquisitionV2 ? step.id : null, stored) }));
    const acquisitionItems = acquisitionFactIds.map((id) => ({ kind: 'acquisition', fact_id: id, required_fact: facts.get(id), metric_scope: 'initial_acquisition',
      requirement: oracle.grading_contract?.initial_acquisition,
      item_id: itemId(row, checkpoint, 'acquisition', id, { fact: facts.get(id), answer: evidence.answer,
        tools: evidence.tools.filter((call) => call.stage_id === step.id) }) }));
    const acquisitionReviews = acquisitionItems.map((item) => ({ item_id: item.item_id, fact_id: item.fact_id,
      pass: reviewAcquisition(item, indexedLabels, evidence, index, facts.get(item.fact_id), row, problems, sources) }));
    const adverseItems = adverseFor(oracle, scenario, checkpoint).map((opportunity) => ({ kind: 'adverse', opportunity_id: opportunity.id,
      description: opportunity.description, item_id: itemId(row, checkpoint, 'adverse', opportunity.id, { opportunity, answer: evidence.answer }) }));
    const adverse = adverseItems.map((item) => ({ item_id: item.item_id, opportunity_id: item.opportunity_id,
      review: reviewBoolean(item, indexedLabels, evidence, problems) }));
    const numeric = numericalChecks(evidence.answer, checkpoint.answer_metrics, oracle.grading_contract?.numeric_tolerance ?? 1e-9);
    const authorityChecks = authority(row, scenario, step, snapshot);
    const execution = cell.status === 'completed' ? true : failed(cell.status) ? false : null;
    const sourceIntegrity = { checks: [...index.spans.values()].map(({ canonical_text, ...x }) => x),
      pass: index.spans.size ? all([...index.spans.values()].map((x) => x.verified)) : null };
    const answerSuccess = all([execution, cell.oracle_missing ? null : true, authorityChecks.pass,
      numeric.checks.length ? numeric.pass : true, answerReviews.length ? all(answerReviews.map((x) => x.pass)) : null]);
    // Reviewers evaluate each fact's prose, qualifiers and evidence. Unrelated
    // whole-answer errors stay in answer_success; they do not erase a correct fact.
    const useComplete = Boolean(factPlan) && useReviews.every((x) => typeof x.pass === 'boolean') && execution === true;
    const usedFacts = useReviews.filter((x) => x.pass === true).length;
    const utilization = useComplete ? ratio(usedFacts, answerFactIds.length) : null;
    const acquisitionComplete = initialAcquisition && acquisitionReviews.every((x) => typeof x.pass === 'boolean') && execution === true;
    const acquiredFacts = acquisitionReviews.filter((x) => x.pass === true).length;
    const acquisition = acquisitionComplete ? ratio(acquiredFacts, acquisitionFactIds.length) : null;
    const validAdverse = adverse.every((x) => x.review != null) && execution === true;
    const applicableAdverse = adverse.filter((x) => x.review?.applicable);
    measurements.push({ ...coord(checkpoint), condition: scenario.condition, world_enabled: scenario.world_enabled, split: scenario.split ?? null, status: cell.status,
      oracle_missing: cell.oracle_missing, numeric, answer_reviews: answerReviews, authority: authorityChecks, source_integrity: sourceIntegrity,
      storage, storage_precision: storage?.precision ?? null, storage_recall: storage?.recall ?? null,
      ...(acquisitionV2 ? { evidence_phase: checkpoint.evidence_phase, acquisition,
        acquisition_success: acquisition == null ? null : acquisition >= (oracle.thresholds?.acquisition_min ?? 0.8),
        acquisition_evidence: { applicable: initialAcquisition, required_facts: acquisitionFactIds.length, correct_acquired_facts: acquiredFacts,
          complete: acquisitionComplete, required_fact_ids: acquisitionFactIds, fact_reviews: acquisitionReviews,
          denominator_source: factPlan?.source ?? null, mapping_rationale: factPlan?.rationale ?? null } } : {}),
      answer_success: answerSuccess, utilization, utilization_success: utilization == null ? null : utilization >= (oracle.thresholds?.utilization_min ?? 0.8),
      utilization_evidence: { required_facts: answerFactIds.length, correct_used_facts: usedFacts, complete: useComplete, fact_reviews: useReviews,
        required_fact_ids: answerFactIds, denominator_source: factPlan?.source ?? null, mapping_rationale: factPlan?.rationale ?? null,
        guard_only_fact_ids: factPlan?.guard_only_fact_ids ?? [], guard_interpretation: factPlan?.guard_interpretation ?? null },
      adverse,
      negative_transfer: validAdverse ? ratio(applicableAdverse.filter((x) => x.review.occurred).length, applicableAdverse.length) : null,
      duration_ms: row?.duration_ms ?? null, chat_usage: row?.chat_trace?.usage ?? null });
    audit.push({ schema: 'loupist-world-audit/1', ...base, scenario_title: scenario.title, condition: scenario.condition,
      split: scenario.split ?? null, execution_status: cell.status, question: step.user ?? null,
      information_cutoff: checkpoint.information_cutoff ?? null, canonical_snapshot_phase: phase,
      ...(acquisitionV2 ? { evidence_phase: checkpoint.evidence_phase } : {}),
      source_spans: [...index.spans.values()], required_facts: required.map((id) => facts.get(id)),
      actual_chat_evidence: evidence, runner_queries_diagnostic_only: snapshot?.queries ?? null,
      numeric, items: [...(applicable ? stored : []), ...answerItems, ...traceItems, ...useItems, ...acquisitionItems, ...adverseItems].map((item) => ({ ...base, ...item })) });
  }
  const ids = new Set(audit.flatMap((x) => x.items.map((item) => item.item_id)));
  problems.unmatched = labels.filter((x) => !ids.has(x.item_id)).map((x) => x.item_id ?? null);
  const repeated = repeatSummary(measurements, trials);
  const storage = measurements.filter((x) => x.storage).map((x) => x.storage);
  const storageComplete = storage.length > 0 && storage.every((x) => x.reviewed_all_items);
  const precisionN = storage.reduce((sum, x) => sum + x.supported_atoms, 0); const precisionD = storage.reduce((sum, x) => sum + x.distinct_reviewed_atoms, 0);
  const recallN = storage.reduce((sum, x) => sum + x.matched_fact_ids.length, 0); const recallD = storage.reduce((sum, x) => sum + x.required_facts, 0);
  const worldMeasurements = measurements.filter((x) => x.world_enabled);
  const utilizationTotal = (rows) => ({ required_facts: rows.reduce((n, x) => n + x.utilization_evidence.required_facts, 0),
    correct_used_facts: rows.reduce((n, x) => n + x.utilization_evidence.correct_used_facts, 0),
    complete: rows.length > 0 && rows.every((x) => x.utilization_evidence.complete) });
  const retainedMeasurements = worldMeasurements.filter((x) => x.evidence_phase !== 'initial_acquisition');
  const acquisitionMeasurements = worldMeasurements.filter((x) => x.evidence_phase === 'initial_acquisition');
  const acquisitionTotal = (rows) => ({ required_facts: rows.reduce((n, x) => n + x.acquisition_evidence.required_facts, 0),
    correct_acquired_facts: rows.reduce((n, x) => n + x.acquisition_evidence.correct_acquired_facts, 0),
    complete: rows.length > 0 && rows.every((x) => x.acquisition_evidence.complete) });
  const acquireTotal = acquisitionV2 ? acquisitionTotal(acquisitionMeasurements) : null;
  const useTotal = utilizationTotal(retainedMeasurements);
  const offUse = utilizationTotal(measurements.filter((x) => !x.world_enabled));
  offUse.rate = offUse.complete ? ratio(offUse.correct_used_facts, offUse.required_facts) : null;
  const adverse = worldMeasurements.flatMap((x) => x.adverse); const applicableAdverse = adverse.filter((x) => x.review?.applicable);
  const adverseComplete = worldMeasurements.length > 0 && worldMeasurements.every((x) => x.status === 'completed') && adverse.every((x) => x.review != null);
  const scopedTraces = traces.filter((x) => !scenarios.length || scenarios.includes(x.scenario_id));
  const completedJobs = completedJobIntegrity(scopedTraces, sources, oracle);
  const tools = toolValidity(scopedTraces.map((x) => x.chat_trace).filter(Boolean));
  const threshold = oracle.thresholds ?? {}; const full = coverage.complete && trials === 3;
  const goal = (value, target, direction, available = true) => ({ observed: value, target: `${direction}${target}`,
    pass: value == null || !available || !full ? null : direction === '>=' ? value >= target : value <= target });
  const goals = {
    storage_precision: goal(storageComplete ? ratio(precisionN, precisionD) : null, threshold.storage_precision_min ?? 0.8, '>='),
    storage_recall: goal(storageComplete ? ratio(recallN, recallD) : null, threshold.storage_recall_min ?? 0.8, '>='),
    utilization: goal(useTotal.complete ? ratio(useTotal.correct_used_facts, useTotal.required_facts) : null, threshold.utilization_min ?? 0.8, '>='),
    ...(acquisitionV2 ? { acquisition: goal(acquireTotal.complete ? ratio(acquireTotal.correct_acquired_facts, acquireTotal.required_facts) : null,
      threshold.acquisition_min ?? 0.8, '>=') } : {}),
    negative_transfer: goal(adverseComplete ? ratio(applicableAdverse.filter((x) => x.review.occurred).length, applicableAdverse.length) : null, threshold.negative_transfer_max ?? 0.1, '<='),
    invalid_tool_calls: goal(tools.rate, threshold.invalid_tool_call_rate_max ?? 0.05, '<=', !tools.unknown_outcomes && !tools.unknown_call_lists),
    repeated_failed_calls: goal(tools.calls ? tools.repeated_failures : null, threshold.repeated_failed_calls_max ?? 0, '<=',
      !tools.repeated_failures_unknown && !tools.unknown_outcomes && !tools.unknown_call_lists),
  };
  const byTrial = [];
  for (let trial = 0; trial < trials; trial += 1) {
    const rows = worldMeasurements.filter((x) => x.trial === trial);
    const ss = rows.map((x) => x.storage).filter(Boolean);
    const completeStorage = ss.length > 0 && ss.every((x) => x.reviewed_all_items);
    const use = utilizationTotal(rows.filter((x) => x.evidence_phase !== 'initial_acquisition'));
    const acquired = acquisitionV2 ? acquisitionTotal(rows.filter((x) => x.evidence_phase === 'initial_acquisition')) : null;
    const adv = rows.flatMap((x) => x.adverse); const eligible = adv.filter((x) => x.review?.applicable);
    const values = {
      storage_precision: completeStorage ? ratio(ss.reduce((n, x) => n + x.supported_atoms, 0), ss.reduce((n, x) => n + x.distinct_reviewed_atoms, 0)) : null,
      storage_recall: completeStorage ? ratio(ss.reduce((n, x) => n + x.matched_fact_ids.length, 0), ss.reduce((n, x) => n + x.required_facts, 0)) : null,
      utilization: use.complete ? ratio(use.correct_used_facts, use.required_facts) : null,
      ...(acquisitionV2 ? { acquisition: acquired.complete ? ratio(acquired.correct_acquired_facts, acquired.required_facts) : null } : {}),
      negative_transfer: rows.length && rows.every((x) => x.status === 'completed') && adv.every((x) => x.review != null)
        ? ratio(eligible.filter((x) => x.review.occurred).length, eligible.length) : null,
    };
    byTrial.push({ trial, values, thresholds_pass: Object.fromEntries(Object.entries(values).map(([key, value]) => {
      const goalValue = goals[key]; const target = Number(goalValue.target.slice(2));
      return [key, value == null ? null : goalValue.target.startsWith('>=') ? value >= target : value <= target];
    })) });
  }
  for (const key of ['storage_precision', 'storage_recall', 'utilization', 'negative_transfer', ...(acquisitionV2 ? ['acquisition'] : [])]) {
    const values = byTrial.map((x) => x.thresholds_pass[key]);
    goals[key].trial_passes = values; goals[key].majority_of_3 = trials === 3 && values.every((x) => x != null) ? values.filter(Boolean).length >= 2 : null;
    goals[key].pass_all_3 = trials === 3 && values.every((x) => x != null) ? values.every(Boolean) : null;
    if (goals[key].pass === true) goals[key].pass = goals[key].majority_of_3;
  }
  const paired = [];
  for (const clean of measurements.filter((x) => x.condition === 'clean')) {
    const conflict = measurements.find((x) => x.condition === 'conflict' && x.trial === clean.trial
      && x.scenario_id.replace(/-CONFLICT$/u, '') === clean.scenario_id.replace(/-CLEAN$/u, '') && x.step_id === clean.step_id);
    if (!conflict) continue;
    paired.push({ clean: coord(clean), conflict: coord(conflict), clean_correct: clean.answer_success,
      conflict_correct: conflict.answer_success, correct_clean_to_wrong_conflict: clean.answer_success == null || conflict.answer_success == null
        ? null : clean.answer_success && !conflict.answer_success });
  }
  for (const value of Object.values(goals)) if (value.pass == null) value.reason = !full ? 'requires_complete_three_trial_coverage' : 'unmeasured_or_incomplete_review';
  const reviewComplete = Object.values(problems).every((x) => !x.length);
  const integrityComplete = measurements.every((x) => x.source_integrity.pass === true && x.authority.pass === true);
  const answerChecksPass = measurements.length > 0 && measurements.every((x) => x.answer_success === true);
  const measurementsComplete = full && reviewComplete && completedJobs.pass != null
    && measurements.every((x) => x.source_integrity.pass != null && x.authority.pass != null)
    && Object.values(goals).every((x) => x.pass != null);
  const hardGatesPass = integrityComplete && completedJobs.pass === true;
  const primaryThresholdsMet = measurementsComplete && hardGatesPass && Object.values(goals).every((x) => x.pass === true);
  const report = { schema: 'loupist-world-report/1', generated_at: new Date().toISOString(),
    metric_revision: acquisitionV2 ? oracle.metric_revision : bundledAnswerFacts.metric_revision, answer_fact_mapping: answerPlan.metadata,
    integrity_contract: 'Exact Source bytes, authority and stable completed-job publication receipts; historical extraction copies are not required.',
    input_sha256: inputSha256, oracle_content_sha256: digest(oracle), trials, scope: scenarios.length ? scenarios : suite.scenarios.map((x) => x.id),
    label_provenance: { accepted: 'coordinator_review', human_label_count: 0, coordinator_review_is_not_human_calibration: true },
    utilization_scope: 'World-enabled arms may use acknowledged Source or World reads; this measures the full Memory path and does not isolate the graph-only benefit.',
    ...(acquisitionV2 ? { evidence_phase_contract: { initial_acquisition: 'Current-stage original web_fetch, source_save result reference, committed verified Source and completed post-chat World processing.',
      retained_memory: 'Current-stage acknowledged exact Source/World read of the specific retained revision and fact.',
      aggregation: 'Initial acquisition and retained Memory utilization have separate denominators, aggregate goals and trial-majority gates. Both must pass; no pooled phase score.' } } : {}),
    coverage, review_coverage: { complete: reviewComplete, expected_items: ids.size,
      supplied_labels: labels.length, ...problems },
    attempts: traces.map((row) => ({ ...coord(row), run_id: row.run_id, attempt_id: row.attempt_id, status: row.status,
      error: row.error ?? null, started_at: row.started_at ?? null, duration_ms: row.duration_ms ?? null,
      batch_stop_reason: row.batch_stop_reason ?? null, content_sha256: digest(row) })),
    measurements, repeated, by_trial: byTrial, paired_clean_conflict: paired, goals, tool_validity: tools, completed_job_integrity: completedJobs,
    totals: { storage_supported_atoms: precisionN, storage_reviewed_atoms: precisionD, required_fact_opportunities: recallD,
      reviewed_matched_fact_opportunities: recallN, utilization: useTotal, source_only_utilization: offUse, adverse_opportunities: adverse.length,
      ...(acquisitionV2 ? { acquisition: acquireTotal } : {}),
      reviewed_applicable_adverse: applicableAdverse.length, occurred_adverse: applicableAdverse.filter((x) => x.review.occurred).length },
    workload: workload(scopedTraces), completion: { scope: 'this_suite_quality_only_external_behavior_and_scale_gates_are_separate',
      measurements_complete: measurementsComplete, predeclared_hard_gates_pass: hardGatesPass,
      primary_thresholds_met: primaryThresholdsMet, answer_checks_pass: answerChecksPass,
      pass: primaryThresholdsMet && answerChecksPass,
      pass_interpretation: 'Strict diagnostic: primary_thresholds_met plus every whole-answer check passing. The frozen plan defines no 100% aggregate answer-correctness target; answer_checks_pass is reported separately, not added to its primary thresholds.',
      limitations: ['Numeric JSON correctness does not certify prose or evidence use.',
        'Storage precision counts reviewed semantic atoms, not graph edges.',
        'Missing, failed, ambiguous and unreviewed cells remain in the expected matrix.',
        'Semantic labels are coordinator reviews; no human calibration is claimed.'] } };
  return { report, audit };
}

async function exists(path) { try { await access(path); return true; } catch { return false; } }
const readJsonl = async (path) => (await readFile(path, 'utf8')).split('\n').filter((line) => line.trim()).map((line) => JSON.parse(line));
export async function main(argv = process.argv.slice(2)) {
  const options = {};
  for (let i = 0; i < argv.length; i += 1) {
    const key = argv[i];
    if (key === '--help') { console.log('world grade --suite corpus.json --oracle oracle.json --traces traces.jsonl [--labels reviews.jsonl] [--selection selected-trials.json] [--scenarios ID,ID] [--trials 3] --out NEW.json [--audit NEW.jsonl]'); return; }
    if (!['--suite', '--oracle', '--traces', '--labels', '--selection', '--scenarios', '--trials', '--out', '--audit'].includes(key)
      || options[key] != null || !argv[i + 1] || argv[i + 1].startsWith('--')) throw new Error(`Invalid option ${key}`);
    options[key] = argv[++i];
  }
  for (const key of ['--suite', '--oracle', '--traces', '--out']) if (!options[key]) throw new Error(`${key} is required`);
  const output = resolve(options['--out']); const auditPath = options['--audit'] && resolve(options['--audit']);
  if (await exists(output) || auditPath && await exists(auditPath) || auditPath === output) throw new Error('Refusing to overwrite evaluation output');
  const bytes = await readFile(resolve(options['--suite'])); const tracePath = resolve(options['--traces']);
  let traces = await readJsonl(tracePath); const pending = `${tracePath}.pending.jsonl`;
  if (await exists(pending)) traces = mergePending(traces, await readJsonl(pending));
  const { report, audit } = buildReport({ suite: JSON.parse(bytes), oracle: JSON.parse(await readFile(resolve(options['--oracle']), 'utf8')),
    traces, labels: options['--labels'] ? await readJsonl(resolve(options['--labels'])) : [],
    selection: options['--selection'] ? JSON.parse(await readFile(resolve(options['--selection']), 'utf8')) : null,
    trials: options['--trials'] ? Number(options['--trials']) : 3, scenarios: options['--scenarios']?.split(',') ?? [], inputSha256: sha256(bytes) });
  report.evidence_files = { traces: tracePath, pending: await exists(pending) ? pending : null,
    labels: options['--labels'] ? resolve(options['--labels']) : null, selection: options['--selection'] ? resolve(options['--selection']) : null };
  await mkdir(dirname(output), { recursive: true });
  if (auditPath) { await mkdir(dirname(auditPath), { recursive: true }); await writeFile(auditPath, audit.map((x) => JSON.stringify(x)).join('\n') + '\n', { flag: 'wx', mode: 0o600 }); }
  await writeFile(output, JSON.stringify(report, null, 2) + '\n', { flag: 'wx', mode: 0o600 });
  console.log(JSON.stringify({ output, audit: auditPath ?? null, coverage_complete: report.coverage.complete,
    review_complete: report.review_coverage.complete, measurements_complete: report.completion.measurements_complete,
    primary_thresholds_met: report.completion.primary_thresholds_met,
    completion: report.completion.pass, strict_completion_diagnostic: report.completion.pass }));
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main().catch((error) => { console.error(error.message); process.exitCode = 1; });
