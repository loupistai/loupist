import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { grade } from './grade.mjs';

const target = { title: 'Alder Rail', origin: 'https://fictional.example/alder', text: 'Freight rose 34%.' };
const check = { assert: 'cites_source_span', source_origin: target.origin, figure_pattern: '(?<![0-9.])34\\s*%', metric: 'retrieval' };
const turn = { user: 'How did freight change?', expect: { answer: [check] } };
const session = { id: 'research', role: 'probe', memory: 'read_only', turns: [turn] };
const suite = { schema: 'loupist-agent-suite/1', suite: 'sample', scenarios: [
  { id: 'test', seed: { sources: [target] }, sessions: [session] },
] };
const readUrl = '#evidence-ev_' + 'a'.repeat(32);
const searchUrl = '#evidence-ev_' + 'b'.repeat(32);

function trace() {
  const args = { scope: 'source', recordId: 'record', revisionId: 'revision', spanId: 'span' };
  const source = { spanId: args.spanId, selectedDigest: createHash('sha256').update(target.text).digest('hex'),
    attribution: `${target.title} — ${target.origin} (eval-fixture)` };
  return { schema: 'loupist-agent-trace/1', suite: suite.suite, scenario_id: 'test', trial: 0, turn_index: 0,
    run_id: 'run', session_id: session.id, session_role: session.role, memory_mode: session.memory,
    user: turn.user, status: 'completed', error: null, answer: `Freight rose [**34%**](${readUrl}).`,
    memory_reads: [{ access: 'exact_read', kind: 'source', record_id: args.recordId, title: target.title }],
    tool_calls: [
      { name: 'world_search', args: { query: '화물 운송' }, outcome: 'ok', result: { details: {
        hits: [{ ...args, readArgs: { ...args } }], citation_url: searchUrl,
      } } },
      { name: 'world_read', args: { ...args }, outcome: 'ok', result: {
        scope: args.scope, recordId: args.recordId, revisionId: args.revisionId, source, citation_url: readUrl,
      } },
    ],
  };
}
const score = (row) => grade(suite, [row]).results[0].pass;

test('a figure linked to the discovered and exact-read seeded span passes', () => {
  assert.equal(score(trace()), true);
  const row = trace();
  row.answer = `Freight rose [34%][memo].\n\n[memo]: ${readUrl}`;
  assert.equal(score(row), true);
});

test('the search, read, memory receipt and linked figure must agree in this turn', () => {
  for (const change of [
    row => row.tool_calls.pop(),
    row => row.tool_calls.reverse(),
    row => { row.tool_calls[0].outcome = 'error'; },
    row => { row.tool_calls[1].outcome = 'error'; },
    row => { row.tool_calls[0].result.details.hits[0].readArgs.recordId = 'similar-company'; },
    row => { row.tool_calls[0].result.details.hits[0].spanId = ''; },
    row => { row.tool_calls[1].args.revisionId = 'another-revision'; },
    row => { row.tool_calls[1].result.source.spanId = 'another-span'; },
    row => { row.tool_calls[1].result.source.selectedDigest = 'other-content'; },
    row => { row.tool_calls[1].result.source.attribution = 'Alder Rail — https://fictional.example/other (eval-fixture)'; },
    row => { row.memory_reads[0].record_id = 'similar-company'; },
    row => { row.memory_reads[0].kind = 'world'; },
    row => { row.answer = `[34%](${searchUrl}) ([memo](${readUrl})).`; },
    row => { row.answer = `34% ([134%](${readUrl})).`; },
    row => { row.answer = '`[34%](' + readUrl + ')`'; },
  ]) {
    const row = trace();
    change(row);
    assert.equal(score(row), false, change.toString());
  }
});

test('a heading can link a figure to a host result delivered in a Pi text block', () => {
  const row = trace();
  row.answer = `### Freight rose [34%](${readUrl})`;
  for (const call of row.tool_calls) {
    call.result = { content: [{ type: 'text', text: JSON.stringify(call.result.details ?? call.result) }] };
  }
  assert.equal(score(row), true);
});
test('redacted or missing deliveries leave the chain unmeasured', () => {
  for (const change of [
    row => { delete row.memory_reads; },
    row => { row.tool_calls[0].result_redacted = true; },
    row => { row.tool_calls[1].result = { truncated: true }; },
  ]) {
    const row = trace();
    change(row);
    assert.equal(score(row), null);
  }
});
