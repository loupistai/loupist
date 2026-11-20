import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { grade } from './grade.mjs';
import { validateSuite } from './eval-contract.mjs';

const suite = JSON.parse(await readFile(new URL('./suites/tool-use-v1.json', import.meta.url)));
const traceFor = (id, extra = {}) => {
  const scenario = suite.scenarios.find(item => item.id === id);
  const session = scenario.sessions[0];
  return { schema: 'loupist-agent-trace/1', run_id: 'research-test', suite: suite.suite, scenario_id: id,
    session_id: session.id, session_role: session.role, memory_mode: session.memory, turn_index: 0, trial: 0,
    user: session.turns[0].user, status: 'completed', answer: 'Iseul uses separate statements.',
    tool_calls: [], memory_commits: [], ...extra };
};
const score = (trace, name) => grade(suite, [trace]).results.find(row => row.assert === name).pass;

const dataset = { dataset_id: 'dataset-fictional', revision: 1, digest: 'sha256-fictional' };
const evidenceId = 'ev_' + 'c'.repeat(32);
const sourceId = 'ev_' + 'd'.repeat(32);
const formula = 'Operating income / Revenue = 38,100,000,000 / 452,300,000,000 = 8.42%';
function calculationTrace() {
  const expected = { numerator: 38100000000, denominator: 452300000000 };
  const inputs = Object.entries(expected).map(([name, value], index) => ({ name, value, display: value.toLocaleString('en-US'),
    provenance: { ...dataset, kind: 'dataset', pointer: `/rows/${index}/value` } }));
  const calculation = { operation: 'ratio', formulas: [formula], inputs, citation_url: `#evidence-${evidenceId}` };
  return traceFor('T-055', {
    answer: `Operating margin is [8.42%](#evidence-${evidenceId}).`,
    tool_calls: [
      { name: 'dart_financials', outcome: 'ok', args: {}, result: { dataset, citation_url: `#evidence-${sourceId}` } },
      { name: 'calculate', outcome: 'ok', args: { operation: 'ratio', as_percent: true, inputs: Object.fromEntries(inputs.map(input => [input.name,
        { dataset_id: dataset.dataset_id, revision: dataset.revision, pointer: input.provenance.pointer }])) },
      result: { content: [{ type: 'text', text: JSON.stringify(calculation) }], details: calculation } },
    ],
    number_checks: [{ text: '8.42%', link_index: 0, evidence_id: evidenceId, kind: 'calculated', status: 'rounded',
      calc: { title: 'Ratio', formulas: [formula], inputs: inputs.map(input => ({ name: input.name, display: input.display, evidence_id: sourceId })) } }],
  });
}

test('dataset ratio checks the answer link, chip formula, resolved values and pinned lineage together', () => {
  assert.equal(score(calculationTrace(), 'calculation_chip'), true);
  for (const mutate of [
    trace => { trace.answer = 'Operating margin is 8.42%.'; },
    trace => { trace.answer = `8.42% [source](#evidence-${evidenceId})`; },
    trace => { trace.answer = trace.answer.replace(evidenceId, sourceId); },
    trace => { trace.number_checks[0].evidence_id = sourceId; },
    trace => { trace.number_checks[0].link_index = 1; },
    trace => { trace.number_checks[0].kind = 'data'; },
    trace => { trace.number_checks[0].status = 'differs'; },
    trace => { trace.number_checks[0].calc.formulas = ['381 / 4523 = 9.42%']; },
    trace => { trace.number_checks[0].calc.inputs.pop(); },
    trace => { trace.number_checks[0].calc.inputs[0].evidence_id = undefined; },
    trace => { trace.number_checks[0].calc.inputs[0].evidence_id = evidenceId; },
    trace => { trace.number_checks[0].calc.inputs[0].display = '999'; },
    trace => { trace.tool_calls[1].args.inputs.numerator = 38100000000; },
    trace => { trace.tool_calls[1].args.inputs.numerator.revision = 2; },
    trace => { trace.tool_calls[1].args.inputs.numerator.pointer = '/rows/5/value'; },
    trace => { trace.tool_calls[1].result = structuredClone(trace.tool_calls[1].result.details); trace.tool_calls[1].result.inputs[0].value = 1; },
    trace => { trace.tool_calls[1].result = structuredClone(trace.tool_calls[1].result.details); trace.tool_calls[1].result.inputs.pop(); },
    trace => { trace.tool_calls[1].result = structuredClone(trace.tool_calls[1].result.details); trace.tool_calls[1].result.inputs[0].provenance.kind = 'provided'; },
    trace => { trace.tool_calls[1].result = structuredClone(trace.tool_calls[1].result.details); trace.tool_calls[1].result.inputs[0].provenance.digest = 'wrong-digest'; },
    trace => { trace.tool_calls[0].result = { dataset: { ...dataset, revision: 2 } }; },
    trace => { trace.tool_calls.reverse(); },
    trace => { trace.tool_calls[1].outcome = 'error'; },
    trace => { trace.tool_calls = []; },
  ]) {
    const trace = calculationTrace(); mutate(trace);
    assert.equal(score(trace, 'calculation_chip'), false, mutate.toString());
  }
});

test('calculation evidence stays unknown when results, arguments or checks are unavailable', () => {
  for (const mutate of [
    trace => { delete trace.number_checks; },
    trace => { delete trace.tool_calls; },
    trace => { trace.tool_calls[1].result_redacted = true; },
    trace => { trace.tool_calls[0].result_redacted = true; },
    trace => { trace.tool_calls[0].result = { truncated: true, original_bytes: 500000 }; },
    trace => { trace.tool_calls[1].result = { truncated: true, original_bytes: 500000 }; },
    trace => { trace.tool_calls[1].args = { inputs: '[REDACTED]' }; },
    trace => { delete trace.tool_calls[1].outcome; },
  ]) {
    const trace = calculationTrace(); mutate(trace);
    assert.equal(score(trace, 'calculation_chip'), null);
  }
  assert.equal(score({ ...calculationTrace(), status: 'failed' }, 'calculation_chip'), false);
});

test('research assertion options validate before any paid run', () => {
  for (const invalid of [{ operation: '' }, { formula: '' }, { text: '' }, { inputs: {} }, { inputs: { numerator: '38100000000' } }]) {
    const copy = structuredClone(suite);
    Object.assign(copy.scenarios.find(item => item.id === 'T-055').sessions[0].turns[0].expect.answer[0], invalid);
    assert.throws(() => validateSuite(copy), /calculation_chip/);
  }
});
