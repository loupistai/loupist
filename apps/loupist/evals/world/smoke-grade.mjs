#!/usr/bin/env node
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

const list = (value) => Array.isArray(value) ? value : [];
const kindPresent = (value) => typeof value === 'string' && value.trim().length > 0;
const countPresent = (value) => Number.isSafeInteger(value) && value >= 0;

function gradeSource(scenario, rows) {
  const [step] = scenario.steps;
  const [fixture] = step.sources;
  const stages = ['__setup', '__seed', step.id];
  const stage = rows.find((row) => row.step_id === step.id);
  const records = list(stage?.before_chat?.records);
  const spans = new Set(records.filter(({ record }) => record?.kind === 'source' && record.title === fixture.title)
    .flatMap(({ canonical_materials }) => list(canonical_materials))
    .filter((item) => item.span_id && item.canonical_text === fixture.text)
    .map((item) => item.span_id));
  const payload = records.find(({ snapshot }) => snapshot?.kind === 'world')?.snapshot.payload;
  const claims = list(payload?.claims).filter((claim) => claim.status === 'active'
    && list(claim.evidence).some((item) => spans.has(item.spanId)));
  const nodeIds = new Set(list(payload?.nodes).map(({ id }) => id));
  const nodes = new Set(claims.flatMap((claim) => list(claim.endpoints).map((item) => item.nodeId))
    .filter((id) => nodeIds.has(id)));
  const calls = list(stage?.world_inference);
  const usage = calls.length > 0 && calls.filter((call) => call.disposition === 'completed').length > 0
    && calls.every((call) => (call.disposition !== 'completed' && call.usage == null)
      || (countPresent(call.usage?.input) && countPresent(call.usage?.output)));
  const processingFailures = list(stage?.processing?.failures);
  const failedCalls = calls.filter((call) => call.disposition !== 'completed'
    || kindPresent(call.diagnostic?.helper?.failure_kind));
  const failures = [
    ...rows.filter(({ status }) => !['completed', 'skipped'].includes(status))
      .filter((row) => row !== stage || (processingFailures.length === 0 && failedCalls.length === 0))
      .map((row) => ({ step: row.step_id, kind: row.failure_kind ?? row.processing?.failure_kind })),
    ...processingFailures.map((failure) => {
      const job = list(stage.processing.jobs).find((item) => item.jobId === failure.job_id);
      return { job: failure.job_id, kind: failure.failure_kind ?? job?.failure?.kind };
    }),
    ...failedCalls
      .map((call) => ({ call: call.call_index, kind: call.failure_kind ?? call.diagnostic?.helper?.failure_kind })),
  ];
  const complete = rows.length === stages.length
    && rows.every((row, index) => row.step_id === stages[index] && row.status === 'completed')
    && new Set(rows.map(({ attempt_id }) => attempt_id)).size === 1;
  const typedFailures = failures.every((failure) => kindPresent(failure.kind));
  // Facts the figure check left out at publication, from the job receipts. A receipt omits
  // a zero count, so it is 0 once the World evidence carries `figureCheck`; a build with
  // neither leaves it unmeasured. Reported for the reader; it gates nothing.
  const jobs = list(stage?.processing?.jobs);
  const counts = jobs.map((job) => job.dropped?.figureMismatch).filter(countPresent);
  const checked = list(payload?.claims).some((claim) => list(claim.evidence).some((item) => item?.figureCheck != null));
  return {
    scenario_id: scenario.id,
    ...(scenario.profile ? { profile: scenario.profile, form: scenario.form ?? null } : {}),
    figures_left_out: counts.length || checked ? counts.reduce((total, count) => total + count, 0) : null,
    left_out_examples: jobs.flatMap((job) => list(job.droppedFigures)),
    complete,
    nodes: nodes.size,
    claims: claims.length,
    calls: calls.length,
    usage,
    failures,
    typed_failures: typedFailures,
    pass: complete && nodes.size > 0 && claims.length > 0 && usage && typedFailures,
  };
}

export function gradeSmoke(suite, rows) {
  const ids = new Set(suite.scenarios.map(({ id }) => id));
  if (rows.some((row) => row.suite !== suite.suite || row.trial !== 0 || !ids.has(row.scenario_id)
    || !kindPresent(row.attempt_id))) throw new Error('smoke traces must name the suite, scenario and single trial');
  const scenarios = suite.scenarios.map((item) => gradeSource(item, rows.filter((row) => row.scenario_id === item.id)));
  const calls = scenarios.reduce((total, item) => total + item.calls, 0);
  const leftOut = scenarios.map((item) => item.figures_left_out);
  return { suite: suite.suite, scenarios, calls,
    figures_left_out: leftOut.includes(null) ? null : leftOut.reduce((total, value) => total + value, 0),
    pass: calls <= 100 && scenarios.every((item) => item.pass) };
}

async function main([suitePath, tracePath, out]) {
  if (!suitePath || !tracePath || !out) throw new Error('usage: smoke-grade.mjs SUITE TRACE.jsonl NEW-REPORT.json');
  const suite = JSON.parse(await readFile(suitePath, 'utf8'));
  const rows = (await readFile(tracePath, 'utf8')).split('\n').filter((line) => line.trim()).map(JSON.parse);
  const report = gradeSmoke(suite, rows);
  await writeFile(out, `${JSON.stringify(report, null, 2)}\n`, { flag: 'wx' });
  if (!report.pass) process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
