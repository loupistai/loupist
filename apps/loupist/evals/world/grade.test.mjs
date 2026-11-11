import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { buildReport, mergePending, numericalChecks, sha256 } from './grade.mjs';

function fixture() {
  const text = 'Issuer revenue is USD 50 million. Operating profit is USD 4 million.';
  const source = { id: 's', text, origin: 'https://fixtures.invalid/s' };
  const steps = [{ id: 'probe', user: 'What are revenue and profit?', memory: 'read_only' }];
  const suite = { schema: 'loupist-world-suite/1', suite: 'test-world', version: 1,
    scenarios: [{ id: 'W', title: 'Fixture', condition: 'clean', world_enabled: true, seed_sources: [source], steps }] };
  const inputSha256 = sha256(JSON.stringify(suite)); const sourceHash = sha256(text);
  const facts = [['revenue', 'Issuer revenue is USD 50 million.'], ['profit', 'Operating profit is USD 4 million.']].map(([id, quote]) => ({
    id, source_id: 's', evidence: { quote, utf8_start: Buffer.byteLength(text.slice(0, text.indexOf(quote))),
      utf8_end_exclusive: Buffer.byteLength(text.slice(0, text.indexOf(quote))) + Buffer.byteLength(quote) } }));
  const oracle = { schema: 'loupist-world-oracle/1', version: 1, input_sha256: inputSha256,
    source_manifest: [{ source_id: 's', text_sha256: sourceHash }], facts,
    checkpoints: [{ scenario_id: 'W', step_id: 'probe', required_stored_fact_ids: ['revenue', 'profit'], required_answer_fact_ids: ['revenue', 'profit'],
      answer_metrics: { revenue: 50, profit: 4, forecast: null }, answer_checks: [{ id: 'prose', requirement: 'Prose matches source and JSON.' }] }],
    adverse_opportunities: [{ id: 'wrong-value', description: 'Gives an incorrect value.' }] };
  const snapshot = (trial) => ({ query_status: 'completed', records: [
    { record: { recordId: 'source-record', ownerLoupistId: `loupist-${trial}` }, snapshot: { kind: 'source', revisionId: 'source-rev', payload: { source: {
      sourceId: 'source', title: 'Original', originLocator: source.origin, captures: [{ authorship: 'publisher', publicationTime: '2026-01-01',
        representations: [{ documentId: 's', spans: [{ spanId: 'span', selectedDigest: sourceHash }] }] }] } } },
      canonical_materials: [{ span_id: 'span', canonical_text: text, bytes_sha256: sourceHash, selected_digest: sourceHash }] },
    { record: { recordId: 'world-record', ownerLoupistId: `loupist-${trial}` }, snapshot: { kind: 'world', revisionId: 'world-rev', payload: {
      nodes: [{ id: 'issuer', name: 'Issuer', description: '' }], claims: [{ id: 'claim', proposition: text,
        endpoints: [{ nodeId: 'issuer', role: 'subject' }], evidence: [{ spanId: 'span', role: 'support' }] }], annotations: [] } } },
  ] });
  const traces = [];
  for (let trial = 0; trial < 3; trial += 1) {
    const base = { schema: 'loupist-world-trace/1', suite: suite.suite, scenario_id: 'W', condition: 'clean',
      trial, run_id: 'run', attempt_id: `attempt-${trial}`, loupist_id: `loupist-${trial}`,
      harness: { suite_sha256: inputSha256, chat_model: 'provider/model', chat_thinking: 'medium' } };
    traces.push({ ...base, step_id: '__setup', status: 'completed' });
    traces.push({ ...base, step_id: 'probe', status: 'completed', processing: { status: 'completed' },
      before_chat: snapshot(trial), after_chat: snapshot(trial), post_chat_processing: { status: 'completed' }, world_inference: [],
      duration_ms: 100, chat_trace: { status: 'completed', user: steps[0].user, memory_mode: 'read_only', session_id: 'chat',
        model: { thinking: 'medium' }, observed_model: 'provider/model', memory_commits: [],
        answer: 'Revenue USD 50 million; operating profit USD 4 million.\n{"metrics":{"revenue":50,"profit":4,"forecast":null}}',
        memory_reads: [{ kind: 'source', access: 'exact_read', record_id: 'source-record' }],
        tool_calls: [{ name: 'world_read', args: { recordId: 'source-record', scope: 'source' }, outcome: 'ok', result: { text } }],
        usage: { input: 20, output: 10, cache_read: 0, cache_write: 0 } } });
  }
  return { suite, oracle, traces, inputSha256 };
}

function reviewed(input, alter = () => {}) {
  const { audit } = buildReport(input);
  const labels = audit.flatMap((bundle) => bundle.items.map((item) => {
    const common = { item_id: item.item_id, kind: item.kind, review_type: 'coordinator_review', reviewer: 'test coordinator',
      reviewed_at: '2026-10-20T00:00:00Z', rationale: 'Direct comparison against retained source and full answer.' };
    let label;
    if (item.kind === 'storage') label = { ...common, reviewed_all_assertions: true,
      ...(item.storage_kind === 'claim' ? { atoms: input.oracle.facts.map((fact) => ({
        text: fact.evidence.quote, semantic_key: fact.id, supported: true, fact_ids: [fact.id],
        evidence: [{ span_id: 'span', quote: fact.evidence.quote }] })) } : { atoms: [], no_factual_assertions: true }) };
    else if (item.kind === 'adverse') label = { ...common, occurred: false, applicable: true, evidence_refs: ['answer'] };
    else label = { ...common, pass: true,
      evidence_refs: item.kind === 'utilization' ? ['answer', `read:${bundle.checkpoint.step_id}:0`]
        : item.kind === 'acquisition' ? ['answer', `tool:${bundle.checkpoint.step_id}:0`, `tool:${bundle.checkpoint.step_id}:1`] : ['answer'],
      ...(['utilization', 'acquisition'].includes(item.kind) ? { source_evidence: [{ span_id: 'span', quote: item.required_fact.evidence.quote }] } : {}) };
    if (item.kind === 'utilization' && bundle.evidence_phase === 'initial_acquisition') { label.pass = false; label.evidence_refs = ['answer']; }
    alter(label, item, bundle); return label;
  }));
  return { ...input, labels };
}

async function acquisitionFixture() {
  const input = fixture(); const scenario = input.suite.scenarios[0]; const original = scenario.seed_sources[0];
  const transportHash = sha256(`<html><body>${original.text}</body></html>`);
  scenario.seed_sources = []; scenario.steps = [
    { id: 'acquire', user: 'Find and retain revenue and profit.', memory: 'remember' },
    { id: 'reuse', user: 'What is the retained profit?', memory: 'read_only' },
    { id: 'new-chat', user: 'What are the retained revenue and profit?', memory: 'read_only', new_chat: true },
  ];
  input.inputSha256 = sha256(JSON.stringify(input.suite));
  input.oracle = { ...input.oracle, version: 2, metric_revision: 'world-acquisition/2', input_sha256: input.inputSha256,
    replay_sha256: 'test-replay', source_texts_for_review_only: { s: original.text },
    source_manifest: [{ source_id: 's', origin: original.origin, text_sha256: sha256(original.text), transport_html_sha256: transportHash, canonical_extracted_text: true }],
    thresholds: { acquisition_min: 0.8, utilization_min: 0.8 },
    checkpoints: scenario.steps.map((step, i) => ({ ...input.oracle.checkpoints[0], step_id: step.id,
      required_answer_fact_ids: i === 1 ? ['profit'] : ['revenue', 'profit'],
      evidence_phase: i === 0 ? 'initial_acquisition' : 'retained_memory',
      storage_checkpoint: i === 0 ? 'after_post_chat_processing' : 'before_chat' })) };
  const payload = { status: 200, content: original.text, content_sha256: transportHash,
    final_url: original.origin, url: original.origin, truncated: false, extraction_truncated: false, content_offset: 0, next_offset: null };
  const material = JSON.stringify(payload); const materialHash = sha256(material);
  const old = input.traces; input.traces = [];
  for (let trial = 0; trial < 3; trial += 1) {
    const setup = structuredClone(old[trial * 2]); setup.harness.suite_sha256 = input.inputSha256; input.traces.push(setup);
    for (const [i, step] of scenario.steps.entries()) {
      const row = structuredClone(old[trial * 2 + 1]); row.step_id = step.id; row.harness.suite_sha256 = input.inputSha256;
      row.replay_sha256 = input.oracle.replay_sha256;
      for (const snapshot of [row.before_chat, row.after_chat]) {
        const source = snapshot.records[0]; source.snapshot.payload.source.captures[0].representations[0].spans[0].selectedDigest = materialHash;
        source.canonical_materials[0] = { span_id: 'span', canonical_text: material, bytes_sha256: materialHash, selected_digest: materialHash };
      }
      row.chat_trace.user = step.user; row.chat_trace.memory_mode = step.memory; row.chat_trace.session_id = i === 2 ? 'new-chat' : 'first-chat';
      if (i === 0) {
        row.before_chat.records = []; row.chat_trace.memory_reads = [];
        row.chat_trace.tool_calls = [
          { name: 'web_fetch', args: { url: original.origin }, outcome: 'ok', result: { details: { ...payload, result_ref: 'result:fetch', citation_url: '#evidence-fetch' } } },
          { name: 'source_save', args: { result_ref: 'result:fetch' }, outcome: 'ok', result: { details: { result_ref: 'result:fetch', status: 'staged', saveRef: 'save:1' } } },
        ];
        row.chat_trace.memory_commits = [{ kind: 'source', operation: 'create', record_id: 'source' }];
        row.post_chat_processing.jobs = [{ ...completedJob(row), jobId: `job-${trial}`,
          document: { ...completedJob(row).document, sealedRevisionId: 'world-rev', sourceBinding: {
            sourceId: 'source', sourceRecordId: 'source-record', sourceRevisionId: 'source-rev', spanId: 'span', payloadDigest: materialHash } } }];
      } else row.chat_trace.tool_calls = [{ name: 'world_read', args: { recordId: 'source-record', revisionId: 'source-rev', scope: 'source', spanId: 'span' },
        outcome: 'ok', result: { details: { recordId: 'source-record', revisionId: 'source-rev', readRef: 'read:delivered', status: 'delivered_for_this_turn', scope: 'source',
          source: { spanId: 'span', selectedDigest: materialHash }, text: material } } }];
      input.traces.push(row);
    }
  }
  return input;
}

test('acquisition v2 keeps original acquisition and later Memory goals independent', async (t) => {
  const input = reviewed(await acquisitionFixture()); const { report } = buildReport(input);
  assert.equal(report.completion.pass, true, JSON.stringify(report.review_coverage));
  assert.equal(report.metric_revision, 'world-acquisition/2');
  assert.deepEqual(report.totals.acquisition, { required_facts: 6, correct_acquired_facts: 6, complete: true });
  assert.equal(report.totals.utilization.required_facts, 9);
  assert.equal(report.measurements[0].utilization_evidence.required_facts, 0);
  assert.equal(report.measurements[0].utilization, null);
  assert.equal(report.measurements[1].utilization_evidence.required_facts, 1);
  assert.equal(report.totals.required_fact_opportunities, 18);
  for (const label of input.labels.filter((x) => x.kind === 'acquisition')) label.pass = false;
  const failed = buildReport(input).report;
  assert.equal(failed.goals.acquisition.observed, 0);
  assert.equal(failed.goals.acquisition.pass, false);
  assert.equal(failed.goals.utilization.observed, 1);
  assert.equal(failed.goals.utilization.pass, true);
  assert.equal(failed.completion.primary_thresholds_met, false);
  assert.deepEqual(failed.goals.acquisition.trial_passes, [false, false, false]);
});

test('acquisition rejects discovery, wrong origin, wrong save reference and incomplete persistence', async (t) => {
  const original = await acquisitionFixture();
  for (const mode of ['search', 'origin', 'wrong-transport-hash', 'save-reference', 'uncommitted', 'unfinished-world', 'wrong-binding', 'wrong-sealed-revision', 'altered-material']) {
    const input = structuredClone(original); const row = input.traces[1];
    if (mode === 'search') row.chat_trace.tool_calls[0].name = 'web_search';
    if (mode === 'origin') row.chat_trace.tool_calls[0].result.details.final_url = 'https://other.invalid/source';
    if (mode === 'wrong-transport-hash') row.chat_trace.tool_calls[0].result.details.content_sha256 = sha256(row.chat_trace.tool_calls[0].result.details.content);
    if (mode === 'save-reference') row.chat_trace.tool_calls[1].args.result_ref = 'result:unrelated';
    if (mode === 'uncommitted') row.chat_trace.memory_commits = [];
    if (mode === 'unfinished-world') row.post_chat_processing.status = 'failed';
    if (mode === 'wrong-binding') row.post_chat_processing.jobs[0].document.sourceBinding.sourceRevisionId = 'other-revision';
    if (mode === 'wrong-sealed-revision') row.post_chat_processing.jobs[0].document.sealedRevisionId = 'old-world-rev';
    if (mode === 'altered-material') row.chat_trace.tool_calls[0].result.details.title = 'Material not actually saved';
    const { report } = buildReport(reviewed(input));
    assert.ok(report.review_coverage.invalid.some((x) => x.reason === 'no_verified_current_acquisition_commit_and_world_chain'), mode);
    assert.equal(report.measurements[0].acquisition, null, mode);
    assert.equal(report.completion.primary_thresholds_met, false, mode);
  }
});

test('retained Memory requires this stage and the exact delivered revision, span and fact', async (t) => {
  const original = await acquisitionFixture();
  for (const mode of ['prior-only', 'old-revision', 'other-span', 'other-fact', 'no-acknowledgment']) {
    const input = structuredClone(original); const row = input.traces[2];
    if (mode === 'prior-only') {
      input.traces[1].chat_trace.memory_reads = structuredClone(row.chat_trace.memory_reads);
      input.traces[1].chat_trace.tool_calls.push(structuredClone(row.chat_trace.tool_calls[0]));
      row.chat_trace.memory_reads = []; row.chat_trace.tool_calls = [];
    }
    if (mode === 'old-revision') row.chat_trace.tool_calls[0].result.details.revisionId = 'old-source-rev';
    if (mode === 'other-span') row.chat_trace.tool_calls[0].result.details.source.spanId = 'other-span';
    if (mode === 'other-fact') row.chat_trace.tool_calls[0].result.details.text = 'Issuer revenue is USD 50 million.';
    if (mode === 'no-acknowledgment') row.chat_trace.memory_reads[0].access = 'search';
    const labeled = reviewed(input, (label, item, bundle) => {
      if (mode === 'prior-only' && item.kind === 'utilization' && bundle.checkpoint.trial === 0 && bundle.checkpoint.step_id === 'reuse') label.evidence_refs = ['answer', 'read:acquire:0'];
    });
    const { report } = buildReport(labeled);
    assert.equal(report.measurements[1].utilization, null, mode);
    assert.ok(report.review_coverage.invalid.length > 0, mode);
    assert.equal(report.completion.primary_thresholds_met, false, mode);
  }
});

test('unasked facts stay outside the denominator but actual wrong assertions still fail correctness and adverse checks', async (t) => {
  const input = await acquisitionFixture();
  input.traces[2].chat_trace.answer = 'Profit USD 4 million. Revenue USD 500 million.\n{"metrics":{"revenue":50,"profit":4,"forecast":null}}';
  const { report } = buildReport(reviewed(input, (label, item, bundle) => {
    if (bundle.checkpoint.trial !== 0 || bundle.checkpoint.step_id !== 'reuse') return;
    if (item.kind === 'answer_check') label.pass = false;
    if (item.kind === 'adverse') label.occurred = true;
    if (item.kind === 'utilization' && item.fact_id === 'revenue') label.pass = false;
  }));
  assert.equal(report.measurements[1].utilization_evidence.required_facts, 1);
  assert.equal(report.measurements[1].utilization, 1);
  assert.equal(report.measurements[1].answer_success, false);
  assert.equal(report.measurements[1].negative_transfer, 1);
  assert.equal(report.completion.pass, false);
});

test('a current World read must deliver the reviewed fact assertion, not just any claim sharing its source', async (t) => {
  const input = await acquisitionFixture(); const row = input.traces[2];
  const claim = row.before_chat.records[1].snapshot.payload.claims[0];
  claim.proposition = 'The issuer earned four million US dollars in operating profit on fifty million dollars of revenue.';
  row.chat_trace.memory_reads = [{ kind: 'world', access: 'exact_read', record_id: 'world-record' }];
  row.chat_trace.tool_calls[0].result.details = { recordId: 'world-record', revisionId: 'world-rev', readRef: 'read:world',
    status: 'delivered_for_this_turn', scope: 'world', text: JSON.stringify({ claims: [claim] }) };
  let report = buildReport(reviewed(input)).report;
  assert.equal(report.measurements[1].utilization, 1);
  row.chat_trace.tool_calls[0].result.details.revisionId = 'older-world-rev';
  report = buildReport(reviewed(input)).report;
  assert.equal(report.measurements[1].utilization, null);
  assert.ok(report.review_coverage.invalid.some((x) => x.reason === 'no_verified_fact_source_linked_to_actual_read'));
  row.chat_trace.tool_calls[0].result.details.revisionId = 'world-rev';
  row.chat_trace.tool_calls[0].result.details.text = JSON.stringify({ claims: [{ ...claim, id: 'other-claim', proposition: 'The issuer is listed.' }] });
  report = buildReport(reviewed(input)).report;
  assert.equal(report.measurements[1].utilization, null);
  assert.ok(report.review_coverage.invalid.some((x) => x.reason === 'no_verified_fact_source_linked_to_actual_read'));
});

test('missing and failed acquisition stages cannot be hidden by successful later reuse', async (t) => {
  const original = await acquisitionFixture();
  for (const mode of ['missing', 'failed']) {
    const input = structuredClone(original);
    if (mode === 'missing') input.traces.splice(1, 1); else input.traces[1].status = 'failed';
    const { report } = buildReport(reviewed(input));
    assert.equal(report.totals.acquisition.required_facts, 6);
    assert.equal(report.goals.acquisition.observed, null);
    assert.equal(report.goals.utilization.observed, 1);
    assert.equal(report.coverage.complete, false);
    assert.equal(report.completion.primary_thresholds_met, false);
  }
});

test('frozen acquisition oracle declares 9 acquisition and 12 retained fact opportunities', async () => {
  const bytes = await readFile(new URL('./acquisition-corpus-v1.json', import.meta.url));
  const suite = JSON.parse(bytes); const inputSha256 = sha256(bytes);
  const oracle = JSON.parse(await readFile(new URL('./acquisition-oracle-v2.json', import.meta.url)));
  const report = buildReport({ suite, oracle, inputSha256, traces: [] }).report;
  assert.equal(report.totals.acquisition.required_facts, 9);
  assert.equal(report.totals.utilization.required_facts, 12);
  assert.equal(report.completion.primary_thresholds_met, false);
});

test('complete three-trial direct-source reviews can pass; precision counts atoms not edges', () => {
  const { report, audit } = buildReport(reviewed(fixture()));
  assert.equal(report.completion.pass, true);
  assert.equal(report.coverage.expected_stages, 3);
  assert.equal(report.totals.storage_reviewed_atoms, 6);
  assert.equal(report.totals.required_fact_opportunities, 6);
  assert.equal(report.goals.storage_precision.observed, 1);
  assert.equal(report.repeated[0].answer_success.pass_all_3, true);
  assert.equal(audit[0].items[0].endpoints[0].node.name, 'Issuer');
  assert.equal(audit[0].source_spans[0].canonical_text, fixture().suite.scenarios[0].seed_sources[0].text);
  assert.equal(report.workload.cost_usd, null);
});

test('absent, failed and interrupted attempts retain every planned fact opportunity', () => {
  const missing = fixture(); missing.traces = [];
  const report = buildReport(missing).report;
  assert.equal(report.coverage.missing.length, 3);
  assert.equal(report.totals.required_fact_opportunities, 6);
  assert.equal(report.goals.storage_recall.observed, null);
  assert.equal(report.completion.pass, false);
  const partial = fixture(); partial.traces[1].status = 'failed'; delete partial.traces[1].before_chat;
  partial.traces[3].status = 'interrupted'; delete partial.traces[3].chat_trace;
  const result = buildReport(partial).report;
  assert.equal(result.coverage.failed_stages, 2);
  assert.equal(result.totals.required_fact_opportunities, 6);
  assert.equal(result.completion.pass, false);
});

test('correct JSON never repairs wrong prose or missing semantic labels', () => {
  const input = fixture();
  input.traces[1].chat_trace.answer = 'Operating profit is USD 400 million.\n{"metrics":{"revenue":50,"profit":4,"forecast":null}}';
  const labeled = reviewed(input, (label, item, bundle) => {
    if (bundle.checkpoint.trial === 0 && (item.kind === 'answer_check' || item.kind === 'utilization' && item.fact_id === 'profit')) label.pass = false;
  });
  const report = buildReport(labeled).report;
  assert.equal(report.measurements[0].numeric.pass, true);
  assert.equal(report.measurements[0].answer_success, false);
  assert.equal(report.measurements[0].utilization, 0.5);
  assert.equal(report.completion.pass, false);
  assert.equal(report.repeated[0].answer_success.pass_all_3, false);
  assert.equal(buildReport(input).report.review_coverage.complete, false);
});

test('a failed whole-answer criterion does not erase independently correct fact use', () => {
  const input = fixture();
  input.oracle.checkpoints[0].answer_checks.push({ id: 'uncertainty', requirement: 'State that the forecast is unknown.' });
  const { report } = buildReport(reviewed(input, (label, item, bundle) => {
    if (bundle.checkpoint.trial === 0 && item.criterion_id === 'uncertainty') label.pass = false;
  }));
  assert.equal(report.measurements[0].utilization, 1);
  assert.equal(report.measurements[0].answer_success, false);
  assert.equal(report.goals.utilization.observed, 1);
  assert.equal(report.completion.measurements_complete, true);
  assert.equal(report.completion.primary_thresholds_met, true);
  assert.equal(report.completion.answer_checks_pass, false);
  assert.equal(report.completion.pass, false);
});

test('only required answer facts enter utilization; cumulative storage remains fully required', () => {
  const input = fixture(); input.oracle.checkpoints[0].required_answer_fact_ids = ['profit'];
  const labels = reviewed(input);
  let report = buildReport(labels).report;
  assert.equal(report.totals.required_fact_opportunities, 6);
  assert.equal(report.totals.utilization.required_facts, 3);
  assert.equal(report.totals.utilization.correct_used_facts, 3);
  assert.equal(report.review_coverage.complete, true);
  assert.equal(report.completion.pass, true);
  labels.labels.find((label) => label.kind === 'utilization').pass = false;
  report = buildReport(labels).report;
  assert.equal(report.measurements[0].answer_success, true);
  assert.equal(report.measurements[0].utilization, 0);
  assert.equal(report.goals.utilization.observed, 2 / 3);
  assert.equal(report.completion.pass, false);
  assert.equal(report.completion.primary_thresholds_met, false);
});

test('an unmapped suite must declare its required answer facts explicitly', () => {
  const input = fixture(); delete input.oracle.checkpoints[0].required_answer_fact_ids;
  assert.throws(() => buildReport(input), /Explicit required answer facts are missing\/invalid/);
});

test('duplicate attempt requires explicit whole-trial selection and remains in workload', () => {
  const input = fixture();
  const retry = structuredClone(input.traces.slice(0, 2)).map((x) => ({ ...x, run_id: 'retry', attempt_id: 'retry-attempt', loupist_id: 'retry-loupist' }));
  for (const row of retry) for (const phase of ['before_chat', 'after_chat']) for (const entry of row[phase]?.records ?? []) entry.record.ownerLoupistId = 'retry-loupist';
  input.traces.push(...retry);
  assert.equal(buildReport(input).report.coverage.ambiguous.length, 1);
  input.selection = { schema: 'loupist-world-selection/1', trials: [{ scenario_id: 'W', trial: 0, run_id: 'run', attempt_id: 'attempt-0' }] };
  const report = buildReport(reviewed(input)).report;
  assert.equal(report.coverage.complete, true);
  assert.equal(report.coverage.discarded_from_quality_only.length, 1);
  assert.equal(report.attempts.length, 8);
  assert.equal(report.workload.chat_attempts, 4);
});

test('mixing stage successes across independent attempts cannot complete one trial', () => {
  const input = fixture();
  input.suite.scenarios[0].steps.push({ ...input.suite.scenarios[0].steps[0], id: 'probe2' });
  input.oracle.checkpoints.push({ ...input.oracle.checkpoints[0], step_id: 'probe2' });
  input.inputSha256 = sha256(JSON.stringify(input.suite)); input.oracle.input_sha256 = input.inputSha256;
  for (const row of input.traces) row.harness.suite_sha256 = input.inputSha256;
  const extra = structuredClone(input.traces).map((row) => ({ ...row, run_id: 'second', attempt_id: `${row.attempt_id}-second`,
    step_id: row.step_id === '__setup' ? '__setup' : 'probe2' }));
  input.traces.push(...extra);
  assert.equal(buildReport(input).report.coverage.incoherent_trials.length, 3);
  assert.equal(buildReport(input).report.coverage.complete, false);
});

test('nonexistent, corrupted or unbound evidence never certifies storage support', () => {
  for (const mode of ['missing', 'corrupt', 'unbound']) {
    const input = fixture();
    if (mode === 'corrupt') input.traces[1].before_chat.records[0].canonical_materials[0].canonical_text += 'tampered';
    if (mode === 'unbound') input.traces[1].before_chat.records[1].snapshot.payload.claims[0].evidence = [];
    const labels = reviewed(input, (label, item, bundle) => {
      if (mode === 'missing' && item.storage_kind === 'claim' && bundle.checkpoint.trial === 0) label.atoms[0].evidence[0].span_id = 'not-a-span';
    });
    const report = buildReport(labels).report;
    assert.equal(report.completion.pass, false, mode);
    assert.ok(report.review_coverage.invalid.length > 0, mode);
    assert.equal(report.goals.storage_precision.observed, null, mode);
  }
});

test('utilization needs an acknowledged actual Memory read, not runner queries', () => {
  const input = fixture(); input.traces[1].chat_trace.memory_reads[0].access = 'search';
  const report = buildReport(reviewed(input)).report;
  assert.ok(report.review_coverage.invalid.some((x) => x.reason === 'no_acknowledged_original_memory_read'));
  assert.equal(report.measurements[0].utilization, null);
  assert.equal(report.completion.pass, false);
});

test('unsupported extra atoms lower precision and cannot be dropped from review coverage', () => {
  const input = fixture();
  const labels = reviewed(input, (label, item) => {
    if (item.storage_kind === 'claim') label.atoms.push({ text: 'Unfounded forecast.', semantic_key: 'forecast', supported: false, fact_ids: [], evidence: [] });
  });
  const report = buildReport(labels).report;
  assert.equal(report.goals.storage_precision.observed, 2 / 3);
  assert.equal(report.goals.storage_precision.pass, false);
  assert.equal(report.totals.storage_reviewed_atoms, 9);
  labels.labels = labels.labels.filter((x) => x.kind !== 'storage');
  assert.equal(buildReport(labels).report.goals.storage_precision.observed, null);
});

test('unknown usage, unknown tool outcomes and read-only writes cannot silently pass', () => {
  const input = fixture(); delete input.traces[1].chat_trace.usage;
  input.traces[1].chat_trace.tool_calls[0].outcome = 'unknown';
  input.traces[1].chat_trace.memory_commits.push({ record_id: 'new-source' });
  const report = buildReport(reviewed(input)).report;
  assert.equal(report.workload.chat_tokens.input.total, null);
  assert.equal(report.workload.chat_tokens.input.known_total, 40);
  assert.equal(report.goals.invalid_tool_calls.pass, null);
  assert.equal(report.measurements[0].authority.pass, false);
  assert.equal(report.completion.pass, false);
});

test('semantic provenance, duplicate labels, missing null and fabricated facts are rejected', () => {
  const input = reviewed(fixture());
  input.labels[0].review_type = 'human';
  input.labels.push({ ...input.labels[1] });
  const report = buildReport(input).report;
  assert.ok(report.review_coverage.invalid.length);
  assert.ok(report.review_coverage.duplicate.length);
  assert.equal(report.completion.pass, false);
  assert.equal(numericalChecks('{"metrics":{"value":0}}', { value: 0, absent: null }).pass, false);
  const fake = reviewed(fixture(), (label, item) => { if (item.storage_kind === 'claim') label.atoms[0].fact_ids = ['fabricated']; });
  assert.ok(buildReport(fake).report.review_coverage.invalid.some((x) => x.reason === 'unverified_fact:fabricated'));
});

test('pending interruption remains unless its exact attempt finalized', () => {
  const input = fixture(); const final = input.traces[1];
  const pending = { ...final, status: 'interrupted' };
  const different = { ...pending, attempt_id: 'another' };
  assert.equal(mergePending([final], [pending, different]).length, 2);
  assert.equal(mergePending([], [pending])[0].status, 'interrupted');
});

test('frozen core oracle keeps full missing-stage denominators', async () => {
  const bytes = await readFile(new URL('corpus-v1.json', import.meta.url));
  const oracle = JSON.parse(await readFile(new URL('oracle-v1.json', import.meta.url)));
  const report = buildReport({ suite: JSON.parse(bytes), oracle, traces: [], inputSha256: sha256(bytes) }).report;
  assert.equal(report.coverage.missing.length, 33); assert.equal(report.totals.required_fact_opportunities, 267);
  assert.equal(report.totals.utilization.required_facts + report.totals.source_only_utilization.required_facts, 249);
  assert.equal(report.completion.pass, false);
});

test('frozen question-specific answer mapping preserves storage and rejects changed criteria', async () => {
  const bytes = await readFile(new URL('corpus-v1.json', import.meta.url));
  const input = { suite: JSON.parse(bytes), oracle: JSON.parse(await readFile(new URL('oracle-v1.json', import.meta.url))), traces: [], inputSha256: sha256(bytes) };
  const report = buildReport(input).report;
  const initial = report.measurements.find((row) => row.scenario_id === 'W-LIFECYCLE' && row.step_id === 'first-research');
  const followup = report.measurements.find((row) => row.scenario_id === 'W-LIFECYCLE' && row.step_id === 'follow-up');
  const material = report.measurements.find((row) => row.scenario_id === 'W-LIFECYCLE' && row.step_id === 'new-material');
  assert.deepEqual(followup.utilization_evidence.required_fact_ids, ['a-old-q2-profit', 'a-initial-h1', 'a-initial-parent', 'a-collision-profit']);
  assert.equal(followup.storage.required_facts, 7);
  assert.deepEqual(material.utilization_evidence.required_fact_ids, ['a-contract-value', 'a-contract-plan']);
  assert.equal(material.storage.required_facts, 9);
  assert.deepEqual(initial.utilization_evidence.required_fact_ids, ['a-initial-revenue', 'a-old-q2-profit', 'a-initial-h1', 'a-initial-parent', 'a-initial-cfo']);
  assert.equal(initial.storage.required_facts, 7);
  assert.deepEqual(initial.utilization_evidence.guard_only_fact_ids, ['a-collision-profit', 'a-collision-person']);
  for (const final of report.measurements.filter((row) => row.step_id === 'final-probe' || ['correction-and-conflict', 'new-chat'].includes(row.step_id))) {
    assert.equal(final.utilization_evidence.required_facts, 9);
    assert.equal(final.utilization_evidence.guard_only_fact_ids.length, 2);
    assert.ok(final.answer_reviews.some((review) => review.criterion_id === 'person-entity'));
    assert.ok(final.adverse.some((review) => review.opportunity_id.endsWith('-wrong-person')));
    if (final.world_enabled) assert.equal(final.storage.required_facts, 11);
  }
  assert.equal(report.answer_fact_mapping.sha256.length, 64);
  const changedQuestion = structuredClone(input); changedQuestion.suite.scenarios[0].steps[0].user += ' Changed.';
  assert.throws(() => buildReport(changedQuestion), /question\/criteria mismatch/);
  input.oracle.checkpoints[0].answer_checks[0].requirement += ' Changed.';
  assert.throws(() => buildReport(input), /oracle mismatch/);
});

function completedJob(row) {
  return { jobId: 'done', status: 'completed', loupistId: row.loupist_id, worldRecordId: 'world-record',
    payloadDigest: sha256('input'), hostMayRedrive: false,
    document: { episodeKind: 'source_span', logicalDigest: sha256('output'), artifactUtf8: null,
      sealedRevisionId: 'superseded-world-revision', usageUncertain: false, leaseExpiresAtMs: null,
      inferenceAttempted: true, extraction: { modelProfile: 'provider/model' },
      sourceBinding: { sourceId: 'source', sourceRecordId: 'source-record', sourceRevisionId: 'source-rev',
        spanId: 'span', payloadDigest: row.after_chat.records[0].canonical_materials[0].bytes_sha256 } } };
}

test('completed jobs need stable receipts across checkpoints, not historical extraction copies', () => {
  const input = reviewed(fixture()); const row = input.traces[1];
  row.processing.jobs = [completedJob(row)]; row.post_chat_processing.jobs = structuredClone(row.processing.jobs);
  // Neither the old World revision nor an extraction file exists in this fixture.
  row.trial_root = '/unavailable'; row.workspace = '/unavailable/workspace';
  const { report } = buildReport(input); const integrity = report.completed_job_integrity;
  assert.equal(integrity.required_completed_job_receipts, 1);
  assert.equal(integrity.checks[0].observed_at.length, 2);
  assert.equal(integrity.pass, true);
  assert.equal(report.completion.pass, true);
});

test('missing or unsettled completed-job receipts fail the hard integrity gate', () => {
  const changes = [
    (job) => { delete job.payloadDigest; },
    (job) => { job.document.logicalDigest = 'not-a-digest'; },
    (job) => { job.document.sealedRevisionId = null; },
    (job) => { job.document.sourceBinding = null; },
    (job) => { job.document.sourceBinding.payloadDigest = 'not-a-digest'; },
    (job) => { job.document.sourceBinding.payloadDigest = sha256('other-source'); },
    (job) => { job.document.sourceBinding.sourceRevisionId = 'other-revision'; },
    (job) => { job.document.extraction = {}; },
    (job) => { job.document.inferenceAttempted = false; },
    (job) => { job.document.usageUncertain = true; },
    (job) => { job.document.leaseExpiresAtMs = 123; },
    (job) => { job.hostMayRedrive = true; },
    (job) => { job.loupistId = 'another-loupist'; },
    (job) => { job.worldRecordId = 'another-world'; },
    (job) => { job.document.episodeKind = 'unknown'; },
  ];
  for (const change of changes) {
    const input = reviewed(fixture()); const row = input.traces[1]; const job = completedJob(row); change(job);
    row.processing.jobs = [job]; const { report } = buildReport(input);
    assert.equal(report.completed_job_integrity.pass, false);
    assert.equal(report.completion.predeclared_hard_gates_pass, false);
    assert.equal(report.completion.pass, false);
  }
});

test('a changed completion receipt cannot hide behind a new output digest', () => {
  for (const change of [
    (job) => { job.document.logicalDigest = sha256('different output'); },
    (job) => { job.document.sealedRevisionId = 'different-world'; },
    (job) => { job.document.sourceBinding.spanId = 'different-span'; },
    (job) => { job.document.extraction.modelProfile = 'different-model'; },
  ]) {
    const input = fixture(); const row = input.traces[1]; row.processing.jobs = [completedJob(row)];
    row.post_chat_processing.jobs = structuredClone(row.processing.jobs); change(row.post_chat_processing.jobs[0]);
    const integrity = buildReport(input).report.completed_job_integrity;
    assert.equal(integrity.required_completed_job_receipts, 1);
    assert.equal(integrity.pass, false);
    assert.ok(integrity.checks[0].errors.includes('completed_job_receipt_changed'));
  }
});

test('job receipts remain scoped to the scenario attempt and local loads need no inference', () => {
  const input = fixture();
  for (const row of input.traces.filter((item) => item.step_id === 'probe')) {
    const job = completedJob(row); job.document.episodeKind = 'world_recovery';
    job.document.inferenceAttempted = false; job.document.sourceBinding = null; job.document.extraction = {};
    row.processing.jobs = [job];
  }
  const integrity = buildReport(input).report.completed_job_integrity;
  assert.equal(integrity.required_completed_job_receipts, 3);
  assert.equal(integrity.pass, true);
});

test('optional inline extraction bytes must still match their digest', () => {
  const input = reviewed(fixture()); const row = input.traces[1]; const job = completedJob(row);
  row.processing.jobs = [job]; job.document.artifactUtf8 = 'output';
  assert.equal(buildReport(input).report.completed_job_integrity.pass, true);
  job.document.artifactUtf8 = 'corrupted';
  assert.equal(buildReport(input).report.completed_job_integrity.pass, false);
});

test('a fact utilization label cannot borrow an unrelated acknowledged Source read', () => {
  const input = fixture();
  input.traces[1].before_chat.records[0].snapshot.payload.source.sourceId = 'original-source';
  const labels = reviewed(input, (label, item, bundle) => {
    if (item.kind === 'utilization' && bundle.checkpoint.trial === 0) label.source_evidence[0].span_id = 'missing-span';
  });
  assert.ok(buildReport(labels).report.review_coverage.invalid.some((x) => x.reason === 'no_verified_fact_source_linked_to_actual_read'));
});

test('preserved bytes cannot conceal changed source publication metadata', () => {
  const input = fixture(); input.oracle.source_manifest[0].publication_time = '2026-01-02T00:00:00Z';
  const report = buildReport(reviewed(input)).report;
  assert.equal(report.measurements[0].source_integrity.pass, false);
  assert.equal(report.measurements[0].source_integrity.checks[0].byte_integrity, true);
  assert.equal(report.measurements[0].source_integrity.checks[0].metadata_checks[0].pass, false);
  assert.equal(report.completion.pass, false);
});
