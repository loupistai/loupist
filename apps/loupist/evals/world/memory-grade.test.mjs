import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { evaluateMemoryCheck, memoryReport, validateMemorySuite, compareMemoryReports, sha256, main } from './memory-grade.mjs';

const record = (kind, id, revision, payload) => ({ record: { kind, recordId: id, revisionId: revision },
  snapshot: { kind, recordId: id, revisionId: revision, payload }, canonical_materials: [] });
const base = record('lens', 'base', 'base-v1', { markdown: '' });
const lens = record('lens', 'method', 'method-v1', { markdown: '85%를 초과할 때만 2점' });
const revised = record('lens', 'method', 'method-v2', { markdown: '90%를 초과할 때만 2점. 직원 출근율에는 적용하지 않음.' });
const state = (...records) => ({ records });
const row = (before = state(base), after = state(base, lens)) => ({ status: 'completed', before_chat: before, after_chat: after,
  chat_trace: { status: 'completed', answer: '2점입니다.', memory_commits: [], memory_reads: [], lens_receipts: [], tool_calls: [] } });
const scenario = { workspace_files: { 'report.txt': 'Fictional source. Buyer buys sensors from Supplier.\n' } };

function source() {
  const text = scenario.workspace_files['report.txt']; const hash = sha256(text);
  const source = record('source', 'source', 'source-v1', { source: { originLocator: 'workspace:report.txt', captures: [
    { representations: [{ spans: [{ spanId: 'span', selectedDigest: hash }] }] },
  ] } });
  source.canonical_materials = [{ span_id: 'span', canonical_text: text, bytes_sha256: hash, selected_digest: hash }];
  return source;
}
function world() {
  return record('world', 'world', 'world-v1', { nodes: [{ id: 'buyer', name: 'Buyer' }, { id: 'supplier', name: 'Supplier' }], claims: [
    { status: 'active', endpoints: [{ nodeId: 'buyer', role: 'source' }, { nodeId: 'supplier', role: 'target' }], evidence: [{ spanId: 'span' }] },
  ] });
}

test('corpus consists of uncued Chat steps and evaluator-only checks', async () => {
  const suite = JSON.parse(await readFile(new URL('./implicit-memory-corpus-v2.json', import.meta.url), 'utf8'));
  assert.equal(validateMemorySuite(suite), suite);
  assert.equal(suite.scenarios[0].workspace_files['correction-brief.txt'], undefined);
  const future = structuredClone(suite);
  future.scenarios[0].steps[0].checks.push({ assert: 'source_retained', metric: 'chronology', path: 'correction-brief.txt' });
  assert.throws(() => validateMemorySuite(future), /no fixture/);
  for (const injected of ['이 방법을 Lens에 저장해줘', 'Please remember this.', 'Use source_save', '이 자료를 보관해줘']) {
    const copy = structuredClone(suite); copy.scenarios[0].steps[0].user = injected;
    assert.throws(() => validateMemorySuite(copy), /Explicit Memory hint/);
  }
  const copy = structuredClone(suite); copy.scenarios[0].steps[0].sources = [{ text: 'pre-seeded answer' }];
  assert.throws(() => validateMemorySuite(copy), /driven by Chat/);
});

test('saving requires canonical content; an answer promise or tool receipt cannot replace it', () => {
  const trace = row(); trace.chat_trace.answer = 'The source is saved.'; trace.chat_trace.memory_commits = [{ kind: 'source' }];
  assert.equal(evaluateMemoryCheck({ assert: 'source_retained', path: 'report.txt' }, trace, scenario).pass, false);
  delete trace.after_chat;
  assert.equal(evaluateMemoryCheck({ assert: 'source_retained', path: 'report.txt' }, trace, scenario).pass, null);
  trace.after_chat = state(source());
  assert.equal(evaluateMemoryCheck({ assert: 'source_retained', path: 'report.txt' }, trace, scenario).pass, true);
  trace.after_chat.records[0].canonical_materials[0].canonical_text += 'Changed';
  assert.equal(evaluateMemoryCheck({ assert: 'source_retained', path: 'report.txt' }, trace, scenario).pass, false);
});

test('World checks require actual endpoints and retained evidence, not supplier names in prose', () => {
  const trace = row(state(base), state(base, source(), world()));
  const check = { assert: 'world_edge', from_name: 'Buyer', to_name: 'Supplier' };
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, true);
  trace.after_chat.records[2].snapshot.payload.nodes[0].name = 'Buyer (fictional)';
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, true);
  trace.after_chat.records[2].snapshot.payload.nodes[0].name = 'Buyer (Busan)';
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, false, 'place qualifiers can distinguish entities');
  trace.after_chat.records[2].snapshot.payload.nodes[0].name = 'BuyerX';
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, false, 'near-spelling remains distinct');
  trace.after_chat.records[2].snapshot.payload.nodes[0].name = 'Buyer';
  trace.after_chat.records[2].snapshot.payload.claims[0].endpoints.reverse();
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, true, 'endpoint array order does not change roles');
  trace.after_chat.records[2].snapshot.payload.claims[0].endpoints[0].role = 'source';
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, false, 'direction matters');
  trace.after_chat.records[2] = world();
  trace.after_chat.records[2].snapshot.payload.claims[0].evidence = [{ spanId: 'missing' }];
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, false);
  assert.equal(evaluateMemoryCheck({ assert: 'world_node', name: 'Supplier' }, trace, scenario).pass, false);
});

test('Lens refinement preserves identity and does not overwrite the standing perspective', () => {
  const learn = row();
  assert.equal(evaluateMemoryCheck({ assert: 'lens_created' }, learn, scenario).pass, true);
  const refine = row(state(base, lens), state(base, revised));
  const check = { assert: 'lens_revised', reference_step: 'learn' }; const prior = new Map([['learn', learn]]);
  assert.equal(evaluateMemoryCheck(check, refine, scenario, prior).pass, true);
  refine.after_chat = state(base, lens, { ...revised, record: { ...revised.record, recordId: 'duplicate' }, snapshot: { ...revised.snapshot, recordId: 'duplicate' } });
  assert.equal(evaluateMemoryCheck(check, refine, scenario, prior).pass, false);
  refine.after_chat = state(record('lens', 'base', 'base-v2', { markdown: '90% rule' }), revised);
  assert.equal(evaluateMemoryCheck(check, refine, scenario, prior).pass, false);
  assert.equal(evaluateMemoryCheck(check, refine, scenario, new Map()).pass, null);
});

test('absent, duplicate, and inconsistent snapshots cannot certify no changes', () => {
  const trace = row(state(base), state(base)); const check = { assert: 'records_unchanged', kind: 'all' };
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, true);
  trace.after_chat.records.push(base);
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, null);
  trace.after_chat = state({ ...base, record: { ...base.record, revisionId: 'different' } });
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, null);
  delete trace.after_chat;
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, null);
});

test('retrieval uses acknowledged exact reads, and can identify the taught method', () => {
  const trace = row(); const check = { assert: 'memory_read', kind: 'lens', reference_step: 'learn' };
  const prior = new Map([['learn', row()]]);
  trace.chat_trace.memory_reads = [{ kind: 'lens', record_id: 'method', access: 'search' }];
  assert.equal(evaluateMemoryCheck(check, trace, scenario, prior).pass, false);
  trace.chat_trace.memory_reads = [{ kind: 'lens', record_id: 'base', access: 'exact_read' }];
  assert.equal(evaluateMemoryCheck(check, trace, scenario, prior).pass, false);
  assert.equal(evaluateMemoryCheck({ assert: 'memory_read', kind: 'lens' }, trace, scenario, prior).pass, false);
  trace.chat_trace.memory_reads.push({ kind: 'lens', record_id: 'method', access: 'exact_read' });
  assert.equal(evaluateMemoryCheck(check, trace, scenario, prior).pass, true);
});

test('file reuse checks separate Skill reads from original reads and retain unknown arguments', () => {
  const trace = row(); const check = { assert: 'original_not_read', paths: ['report.txt'] };
  trace.chat_trace.tool_calls = [{ name: 'read', args: { path: '/agent/skills/world/SKILL.md' } }];
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, true);
  trace.chat_trace.tool_calls.push({ name: 'run_python', args: { code: 'print(open("report.txt").read())' } });
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, false);
  trace.chat_trace.tool_calls = [{ name: 'read' }];
  assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, null);
});

function completeFixture(trials = 3) {
  const suite = { schema: 'loupist-world-suite/1', suite: 'test', version: 1, scenarios: [
    { id: 'natural', steps: [{ id: 'answer', user: '가동률이 86%면 몇 점일까?', chat: 'chat', role: 'probe', memory: 'remember',
      checks: [{ assert: 'answer_pattern', pattern: '2\\s*점', metric: 'carry_forward' }] }] },
  ] };
  const harness = { suite_sha256: 'digest', chat_model: 'provider/model', chat_thinking: 'max', world_model: 'provider/model',
    world_thinking: 'high', turn_timeout_ms: 600000, world_timeout_ms: 60000, stage_timeout_ms: 300000,
    runner_sha256: 'runner', system_prompt_sha256: 'prompt', tool_catalog_sha256: 'catalog' };
  const traces = [];
  for (let trial = 0; trial < trials; trial += 1) {
    const common = { schema: 'loupist-world-trace/1', suite: 'test', run_id: 'run', attempt_id: `attempt-${trial}`,
      scenario_id: 'natural', trial, harness };
    traces.push({ ...common, step_id: '__setup', status: 'completed' });
    const current = row(); current.chat_trace = { ...current.chat_trace, user: suite.scenarios[0].steps[0].user,
      memory_mode: 'remember', session_role: 'probe', session_id: 'chat', observed_model: 'provider/model',
      model: { provider: 'provider', model: 'model', thinking: 'max' } };
    traces.push({ ...common, ...current, step_id: 'answer' });
  }
  return { suite, traces };
}

test('three coherent independent trials are required; missing and repeated attempts stay visible', () => {
  const { suite, traces } = completeFixture(); const opts = { suiteSha256: 'digest' };
  assert.equal(memoryReport(suite, traces, opts).acceptance.pass, true);
  assert.equal(memoryReport(suite, traces.slice(0, 2), { ...opts, trials: 1 }).acceptance.pass, null);
  assert.equal(memoryReport(suite, traces.slice(0, -1), opts).coverage.complete, false);
  assert.equal(memoryReport(suite, [...traces, traces[1]], opts).coverage.duplicate.length, 1);
  const mixed = structuredClone(traces); mixed[1].attempt_id = 'different';
  assert.equal(memoryReport(suite, mixed, opts).coverage.complete, false);
  const changed = structuredClone(traces); changed[1].chat_trace.user = 'different request';
  assert.equal(memoryReport(suite, changed, opts).coverage.mismatched.length, 1);
  const reused = structuredClone(traces); reused[2].attempt_id = reused[3].attempt_id = 'attempt-0';
  assert.equal(memoryReport(suite, reused, opts).coverage.complete, false);
  const unidentified = structuredClone(traces); delete unidentified[1].chat_trace.observed_model;
  assert.equal(memoryReport(suite, unidentified, opts).coverage.complete, false);
  const unknownControls = structuredClone(traces); unknownControls[1].harness = { ...unknownControls[1].harness, chat_thinking: null };
  assert.equal(memoryReport(suite, unknownControls, opts).coverage.complete, false);
  const mixedVersions = structuredClone(traces); mixedVersions[1].harness = { ...mixedVersions[1].harness, runner_sha256: 'other' };
  assert.equal(memoryReport(suite, mixedVersions, opts).coverage.complete, false);
  const failed = structuredClone(traces); failed[1].status = 'failed'; failed[1].chat_trace.status = 'failed';
  assert.equal(memoryReport(suite, failed, opts).acceptance.pass, false);
  assert.throws(() => memoryReport(suite, traces, { suiteSha256: 'other' }), /digest mismatch/);
});

test('comparisons refuse changed inputs or model controls, and disclose diagnostic trial counts', () => {
  const { suite, traces } = completeFixture();
  const candidate = memoryReport(suite, traces, { suiteSha256: 'digest' });
  const baseline = memoryReport(suite, traces.slice(0, 2), { suiteSha256: 'digest', trials: 1 });
  assert.equal(compareMemoryReports(baseline, candidate).baseline_trials, 1);
  assert.throws(() => compareMemoryReports({ ...baseline, suite_sha256: 'other' }, candidate), /inputs/);
  assert.throws(() => compareMemoryReports({ ...baseline, configurations: [] }, candidate), /controls/);
  assert.throws(() => compareMemoryReports({ ...baseline, grader_sha256: 'old' }, { ...candidate, grader_sha256: 'new' }), /Regrade/);
});

test('CLI preserves pending interruptions and refuses to overwrite reports', async () => {
  const root = await mkdtemp(join(tmpdir(), 'loupist-memory-grader-'));
  try {
    const { suite, traces } = completeFixture(1); const bytes = JSON.stringify(suite); const digest = sha256(bytes);
    for (const trace of traces) trace.harness.suite_sha256 = digest;
    const suitePath = join(root, 'suite.json'); const tracesPath = join(root, 'traces.jsonl'); const out = join(root, 'report.json');
    await writeFile(suitePath, bytes); await writeFile(tracesPath, `${JSON.stringify(traces[0])}\n`);
    await writeFile(`${tracesPath}.pending.jsonl`, `${JSON.stringify({ ...traces[1], status: 'interrupted' })}\n`);
    const args = ['--suite', suitePath, '--traces', tracesPath, '--out', out, '--trials', '1'];
    await main(args);
    const report = JSON.parse(await readFile(out, 'utf8'));
    assert.equal(report.metrics.carry_forward.failed, 1);
    await assert.rejects(main(args), { code: 'EEXIST' });
  } finally { await rm(root, { recursive: true, force: true }); }
});


test('per-step delivery requires observed bytes at the declared stage', () => {
  const { suite, traces } = completeFixture();
  suite.fixture_delivery = 'per_step';
  suite.scenarios[0].steps[0].workspace_files = { 'later.txt': 'new fact' };
  for (const trace of traces) trace.introduced_workspace_files = trace.step_id === '__setup' ? {} : { 'later.txt': sha256('new fact') };
  assert.equal(memoryReport(suite, traces, { suiteSha256: 'digest' }).acceptance.pass, true);
  traces[0].introduced_workspace_files = { 'later.txt': sha256('new fact') };
  assert.equal(memoryReport(suite, traces, { suiteSha256: 'digest' }).coverage.complete, false);
  traces[0].introduced_workspace_files = {};
  traces[1].introduced_workspace_files = { 'later.txt': sha256('different fact') };
  assert.equal(memoryReport(suite, traces, { suiteSha256: 'digest' }).acceptance.pass, null);
});

test('expanded suites remain uncued with valid chronology and reference targets', async () => {
  for (const name of ['regression-v3', 'validation-v3', 'promoted-v4', 'validation-v4', 'native-files-v5']) {
    const suite = JSON.parse(await readFile(new URL(`./implicit-memory-${name}.json`, import.meta.url), 'utf8'));
    assert.equal(validateMemorySuite(suite), suite);
  }
});

function conversationPage(original, offset, length, messageId = 'user-message') {
  const chars = Array.from(original); const end = Math.min(offset + length, chars.length);
  const details = { chatId: 'earlier-chat', messageId, role: 'user', notEvidence: true,
    characterUnits: 'Unicode scalar values', textSha256: sha256(original), totalChars: chars.length,
    offset, nextOffset: end < chars.length ? end : null, text: chars.slice(offset, end).join('') };
  return { name: 'conversation_memory', outcome: 'ok', args: { action: 'read', chatId: details.chatId, messageId, offset, limit: length }, result: { details } };
}

test('conversation provenance requires exact user text and complete Unicode page coverage', () => {
  const original = '원문🧭 café 東京 끝'; const earlier = row(); earlier.chat_trace.user = original;
  const prior = new Map([['earlier', earlier]]); const current = row();
  const check = { assert: 'conversation_read', reference_step: 'earlier', complete: true };
  const grade = () => evaluateMemoryCheck(check, current, scenario, prior).pass;
  current.chat_trace.tool_calls = [conversationPage(original, 0, 5)];
  assert.equal(grade(), false, 'a discovery snippet or incomplete first page does not prove full reading');
  current.chat_trace.tool_calls.push(conversationPage(original, 5, 100));
  assert.equal(grade(), true);
  current.chat_trace.tool_calls[1] = conversationPage(original, 6, 100);
  assert.equal(grade(), false, 'one missing Unicode scalar is a real gap');
  current.chat_trace.tool_calls[1] = conversationPage(original, 5, 100, 'other-message');
  assert.equal(grade(), false, 'pages from different messages cannot be assembled');
  current.chat_trace.tool_calls = [conversationPage(original, 0, 100)];
  for (const [field, value] of [['role', 'assistant'], ['notEvidence', false], ['textSha256', 'incorrect'], ['text', original.replace('café', 'cafe')], ['nextOffset', 0]]) {
    const call = conversationPage(original, 0, 100); call.result.details[field] = value;
    current.chat_trace.tool_calls = [call]; assert.equal(grade(), false, field);
  }
  const call = conversationPage(original, 0, 100);
  call.result = { content: [{ type: 'text', text: JSON.stringify(call.result.details) }] };
  current.chat_trace.tool_calls = [call]; assert.equal(grade(), true, 'JSON text receipts are also supported');
  assert.equal(evaluateMemoryCheck(check, current, scenario).pass, null);
});

test('tool validity and Memory-off access checks cannot silently ignore invalid or unknown calls', () => {
  const current = row(); const valid = { assert: 'tool_validity' }; const off = { assert: 'no_memory_access' };
  assert.equal(evaluateMemoryCheck(valid, current, scenario).pass, true);
  assert.equal(evaluateMemoryCheck(off, current, scenario).pass, true);
  current.chat_trace.tool_calls = [{ name: 'conversation_memory', outcome: 'ok', args: { action: 'search' } }];
  assert.equal(evaluateMemoryCheck(off, current, scenario).pass, false);
  current.chat_trace.tool_calls = [{ name: 'lens_read', outcome: 'error', args: {}, error_code: 'malformed_request' }];
  assert.equal(evaluateMemoryCheck(valid, current, scenario).pass, false);
  current.chat_trace.tool_calls = [{ name: 'read', outcome: 'unknown', args: {} }];
  assert.equal(evaluateMemoryCheck(valid, current, scenario).pass, null);
});

test('method pattern checks cannot borrow text from another learned method', () => {
  const learn = row(); const current = row(state(base, lens), state(base, lens, record('lens', 'other', 'other-v1', { markdown: '90% boundary' })));
  const check = { assert: 'lens_pattern', reference_step: 'learn', pattern: '90' };
  assert.equal(evaluateMemoryCheck(check, current, scenario, new Map([['learn', learn]])).pass, false);
});

test('answer predicates recognize equivalent Korean magnitude notation without rewriting quoted evidence', () => {
  const current = row(); const grade = (pattern) => evaluateMemoryCheck({ assert: 'answer_pattern', pattern }, current, scenario).pass;
  current.chat_trace.answer = 'West의 추정치는 **14만 5천** 컨테이너입니다.';
  assert.equal(grade('145000'), true);
  assert.equal(grade('150000'), false);
  assert.equal(grade('14만 5천'), true, 'literal form remains available');
  current.chat_trace.answer = '금액은 1억 2천만 원, 다른 금액은 13만 2,500원입니다.';
  assert.equal(grade('120000000'), true);
  assert.equal(grade('132500'), true);
  current.chat_trace.answer = '금액은 0만 원입니다.';
  assert.equal(grade('10000'), false);
  assert.equal(grade('0 원'), true);
  current.chat_trace.answer = '정확히 0점, 비율은 14.5%입니다.';
  assert.equal(grade('0점'), true);
  assert.equal(grade('145000'), false);
});


test('percentage-point notation stays distinct from a percentage', () => {
  const current = row();
  const grade = (pattern) => evaluateMemoryCheck({ assert: 'answer_pattern', pattern }, current, scenario).pass;
  current.chat_trace.answer = '영업이익률은 5%포인트 상승했습니다.';
  assert.equal(grade('5%p'), true);
  assert.equal(grade('50%p'), false);
  assert.equal(grade('5%포인트'), true, 'the literal notation remains available');
  current.chat_trace.answer = '매출은 5% 상승했습니다.';
  assert.equal(grade('5%p'), false, 'a relative percent is not a percentage-point change');
});

test('scoped Lens precision allows subject creates and rejects method or principle changes', () => {
  for (const kind of ['subject', 'method', 'principle']) for (const assertion of ['no_commit', 'records_unchanged']) {
    const trace = row(state(base), state(base, lens));
    trace.chat_trace.lens = { pages: [{ record_id: 'method', kind }] };
    trace.chat_trace.memory_commits = [{ kind: kind === 'principle' ? 'principle' : 'lens', record_id: 'method' }];
    const check = { assert: assertion, kind: 'lens', lens_kinds: ['method', 'principle', 'perspective'] };
    assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, kind === 'subject');
    delete trace.chat_trace.lens;
    assert.equal(evaluateMemoryCheck(check, trace, scenario).pass, null);
  }
});

test('context retrieval requires the current learned principle delivered and linked as a whole target', () => {
  const trace = row(state(base, lens), state(base, lens));
  const check = { assert: 'memory_read', kind: 'lens', reference_step: 'learn' };
  const prior = new Map([['learn', row()]]);
  trace.chat_trace.answer = '[My rule](lens:method) — meets.';
  trace.chat_trace.lens_receipts = [{ record_id: 'method', revision_id: 'method-v1', kind: 'principle', delivery: 'context' }];
  const evaluate = item => evaluateMemoryCheck(check, item, scenario, prior);
  assert.equal(evaluate(trace).pass, true);
  assert.equal(evaluate(trace).retrieval_evidence, 'context_link');
  for (const mutate of [
    t => { t.chat_trace.answer = 'method'; },
    t => { t.chat_trace.answer = '`[rule](lens:method)`'; },
    t => { t.chat_trace.answer = '[rule](lens:method-other)'; },
    t => { t.chat_trace.lens_receipts = []; },
    t => { t.chat_trace.lens_receipts[0].revision_id = 'stale'; },
    t => { t.chat_trace.lens_receipts[0].kind = 'method'; },
    t => { t.chat_trace.lens_receipts[0].kind = 'subject'; },
    t => { t.chat_trace.lens_receipts[0].record_id = 'base'; },
    t => { delete t.chat_trace.lens_receipts[0].revision_id; },
    t => { t.chat_trace.lens_receipts[0].delivery = 'exact_read'; },
  ]) { const copy = structuredClone(trace); mutate(copy); assert.equal(evaluate(copy).pass, false); }
  delete trace.chat_trace.lens_receipts;
  assert.equal(evaluate(trace).pass, null, 'old trace cannot establish context retrieval');
});

test('Lens kind scopes validate only real Lens kinds on Lens checks', () => {
  const suite = { schema: 'loupist-world-suite/1', suite:'scoped', version:1, scenarios:[{id:'case',steps:[{id:'step',user:'What is there?',role:'probe',memory:'remember',checks:[]}]}] };
  const checks = suite.scenarios[0].steps[0].checks;
  for (const kind of ['principle','perspective','method','subject','idea']) {
    checks[0] = {assert:'no_commit',kind:'lens',lens_kinds:[kind],metric:'save_precision'}; validateMemorySuite(suite);
  }
  for (const invalid of [{kind:'source',lens_kinds:['subject']},{kind:'lens',lens_kinds:['note']},{kind:'lens',lens_kinds:'subject'},{kind:'lens',lens_kinds:[]}]) {
    checks[0] = {assert:'no_commit',metric:'save_precision',...invalid}; assert.throws(()=>validateMemorySuite(suite),/lens_kinds/);
  }
});

test('a principle revision fails both scoped precision checks', () => {
  const trace=row(state(base,lens),state(base,revised));
  trace.chat_trace.lens={pages:[{record_id:'method',kind:'principle'}]};
  trace.chat_trace.memory_commits=[{kind:'principle',record_id:'method',operation:'revise'}];
  for(const assertion of ['no_commit','records_unchanged']) assert.equal(evaluateMemoryCheck({assert:assertion,kind:'lens',lens_kinds:['principle','method','perspective']},trace,scenario).pass,false);
});

test('reports separate exact-read and context-link retrieval counts', () => {
  const {suite,traces}=completeFixture();
  suite.scenarios[0].steps[0].checks=[{assert:'memory_read',kind:'lens',metric:'retrieval'}];
  for(let i=1;i<traces.length;i+=2){traces[i].before_chat=state(base,lens);traces[i].after_chat=state(base,lens);}
  traces[1].chat_trace.memory_reads=[{kind:'lens',record_id:'method',access:'exact_read'}];
  traces[3].chat_trace.lens_receipts=[{record_id:'method',revision_id:'method-v1',kind:'principle',delivery:'context'}];
  traces[3].chat_trace.answer='[Rule](lens:method) — meets.';
  const report=memoryReport(suite,traces,{suiteSha256:'digest'});
  assert.deepEqual(report.retrieval_evidence,{exact_read:1,context_link:1});
  assert.deepEqual({...report.metrics.retrieval,rate_lower_bound:undefined},{passed:2,failed:1,unknown:0,total:3,rate_lower_bound:undefined});
});
