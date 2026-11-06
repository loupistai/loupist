#!/usr/bin/env node
/** Deterministic failure triage. Classification never changes a check's score. */
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { grade } from './grade.mjs';
import { turnIdentity } from './eval-contract.mjs';

export const CLUSTERS = {
  date: { name: 'cites a figure without its date', severity: 3, change: 'Journal evidence step: repeat each figure’s period/as-of date in follow-ups.' },
  test: { name: 'no observable test named', severity: 3, change: 'Journal cases step: give a measurable confirm/refute test for each case, including follow-ups.' },
  precedence: { name: 'method-principle conflict not reconciled', severity: 4, change: 'SYSTEM Memory: retain the replaced rule as history and explain principle precedence.' },
  save: { name: 'save restraint', severity: 1, change: 'Memory Skill: distinguish requested filing from inferred saving and confirm actual commits.' },
  connection: { name: 'connection honesty', severity: 4, change: 'Connection tool description: state unavailable access and avoid claims of unseen evidence.' },
  retrieval: { name: 'retrieval or application', severity: 2, change: 'Method Skill: exact-read applicable evidence and preserve its scope.' },
  revision: { name: 'revision disclosure', severity: 2, change: 'Memory Skill: distinguish historical values from current facts.' },
  format: { name: 'output contract', severity: 1, change: 'Method Skill: state required output units and encodings.' },
  host: { name: 'host execution failure', severity: 4, change: 'Context policy: retain completed work and surface the host failure; investigate runtime separately.' },
  other: { name: 'other task completion', severity: 1, change: 'Relevant Skill: supply the missing requested deliverable.' },
};

// First matching rule owns a failed check. Dates precede tests when one rubric names both.
export function classifyFailure(check, trace = {}) {
  const text = `${check.criterion_id ?? ''} ${check.reason ?? ''} ${check.detail ?? ''}`.toLowerCase();
  const tools = trace.tool_calls ?? [];
  if (trace.failureKind || (trace.status && trace.status !== 'completed')) return 'host';
  if (check.metric === 'connection_honesty' || /connection|pretend.*access|fabricat.*(?:fetch|source)/u.test(text)
    || tools.some(t => /permission_denied|connection_unavailable/u.test(t.error_code ?? '')) && /access|fetch|available/u.test(text)) return 'connection';
  if (check.criterion_id === 'precedence' || /conflicting.*method|principle.*over.*method/u.test(text)) return 'precedence';
  if (/\bundated\b(?! (?:next|future) (?:report|update))|does not attach|omits both dates|without (?:their |its |the )?dates?|missing.*as.of/u.test(text)) return 'date';
  if (/observable.*test|confirm.or.refute test|only (?:forward check|test)|(?:no|without) (?:observable )?(?:tests?\b|falsifier\b)/u.test(text)) return 'test';
  if (/^save_/u.test(check.metric ?? '') || /committed|not_committed/u.test(check.assert ?? '')
    || /records-user-judgment|saves-titled|keeps-source/u.test(text)
    || tools.some(t => /^(?:note_save|principle_save|source_save|principle_save|journal_save)$/u.test(t.name)) && /save|fil(?:e|ing)/u.test(text)) return 'save';
  if (check.metric === 'revision_disclosure' || /records-termination/u.test(text)) return 'revision';
  if (check.assert === 'metrics') return 'format';
  if (['retrieval', 'carry_forward'].includes(check.metric)) return 'retrieval';
  return 'other';
}

export function taxonomy(runs) {
  const failures = [];
  for (const run of runs) {
    const traces = new Map(run.traces.map((trace, index) => [turnIdentity(trace), { trace, line: index + 1 }]));
    for (const [index, check] of [...(run.assertions ?? grade(run.suite, run.traces).results), ...(run.judgments ?? [])].entries()) {
      if (check.pass !== false) continue;
      const found = traces.get(turnIdentity(check));
      if (!found) throw new Error(`Failed check has no trace: ${turnIdentity(check)}`);
      const cluster = classifyFailure(check, found.trace);
      const exclusion = run.exclusion ?? run.exclusions?.find(rule =>
        (!rule.scenarios || rule.scenarios.includes(check.scenario_id)) &&
        (!rule.checks || rule.checks.includes(check.criterion_id ?? check.assert)) &&
        (!rule.detail || new RegExp(rule.detail, 'u').test(check.reason ?? check.detail ?? ''))) ?.reason ?? null;
      failures.push({ arm: run.name, cluster, exclusion, check_index: index,
        scenario: check.scenario_id, suite: check.suite, trial: check.trial,
        check: check.criterion_id ?? check.assert, detail: check.reason ?? check.detail,
        trace: `${run.tracePath}:${found.line}`, failureKind: found.trace.failureKind ?? null });
    }
  }
  const clusters = Object.entries(CLUSTERS).map(([id, info]) => {
    const rows = failures.filter(row => row.cluster === id);
    const active = rows.filter(row => !row.exclusion);
    return { id, ...info, count: rows.length, unresolved: active.length, priority: active.length * info.severity,
      scenarios: [...new Set(rows.map(row => `${row.suite}/${row.scenario}`))].sort(),
      example: (active[0] ?? rows[0])?.trace ?? '—' };
  }).sort((a, b) => b.priority - a.priority || a.id.localeCompare(b.id));
  return { schema: 'loupist-failure-taxonomy/1', failures, clusters };
}

const cell = value => String(value).replaceAll('|', '\\|').replaceAll('\n', ' ');
export function taxonomyMarkdown(result) {
  return ['# Failure taxonomy', '', 'Each failed check has one primary cluster. All failures remain counted; only unresolved checks determine priority (count × severity). Severity: authority/access 4, missing research evidence 3, retrieval/revision 2, filing/format/completion 1.', '',
    '| Cluster | Count | Unresolved | Severity | Priority | Suites / scenarios | Example trace | Proposed harness change |',
    '| --- | ---: | ---: | ---: | ---: | --- | --- | --- |',
    ...result.clusters.map(c => `| ${[c.name, c.count, c.unresolved, c.severity, c.priority, c.scenarios.join(', '), c.example, c.change].map(cell).join(' | ')} |`), '',
    '## Excluded from prioritization', '', '| Arm | Scenario | Check | Reason | Trace |', '| --- | --- | --- | --- | --- |',
    ...result.failures.filter(f => f.exclusion).map(f => `| ${[f.arm, f.scenario, f.check, f.exclusion, f.trace].map(cell).join(' | ')} |`), ''].join('\n');
}

export async function main(argv = process.argv.slice(2)) {
  const [manifest, output] = argv;
  if (!manifest || !output || argv.length !== 2) throw new Error('Usage: taxonomy.mjs manifest.json output.md (paths relative to current directory)');
  const json = async path => JSON.parse(await readFile(path, 'utf8'));
  const jsonl = async path => (await readFile(path, 'utf8')).split('\n').filter(Boolean).map(line => JSON.parse(line));
  const runs = [];
  for (const run of await json(manifest)) runs.push({ ...run, suite: await json(run.suite),
    traces: await jsonl(run.traces), tracePath: run.traces, judgments: run.judgments ? await jsonl(run.judgments) : [],
    assertions: run.assertions ? await jsonl(run.assertions) : undefined });
  const result = taxonomy(runs);
  await writeFile(output, taxonomyMarkdown(result));
  await writeFile(`${output}.json`, JSON.stringify(result, null, 2) + '\n');
  return result;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
