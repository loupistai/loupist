#!/usr/bin/env node
/**
 * Deterministic temporal-invalidation grader for temporal-corpus-v1. No model calls.
 *
 * For each scenario trial it reads the final World snapshot, finds the claims
 * joining the oracle's entity pair whose evidence cites the named Source, and
 * checks whether they were invalidated (invalidAt or expiredAt set) as the
 * oracle expects.
 */
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

const list = (value) => (Array.isArray(value) ? value : []);

export function normalizeName(value) {
  const compact = String(value ?? '').normalize('NFKC').trim().replace(/\s+/gu, ' ');
  return compact.replace(/\s*(?:\([^)]*\)|\[[^\]]*\])$/u, '').trim();
}

const namesOf = (node) => [node?.name, ...list(node?.aliases)].filter((value) => typeof value === 'string');
const matches = (pattern, node) => namesOf(node).some((name) => new RegExp(pattern, 'iu').test(normalizeName(name)));
const nodeId = (endpoint) => endpoint?.nodeId ?? endpoint?.node_id ?? null;
const spanOf = (evidence) => evidence?.spanId ?? evidence?.span_id ?? null;
const set = (time) => Boolean(time && (time.value ?? null) !== null && time.precision !== 'unknown');

export function invalidated(claim) {
  return set(claim?.invalidAt ?? claim?.invalid_at) || set(claim?.expiredAt ?? claim?.expired_at);
}

function snapshotOf(row) {
  return row?.after_chat?.records ? row.after_chat : row?.before_chat;
}

/** Span ids by the title of the Source record that holds them. */
function spansByTitle(rows) {
  const spans = new Map();
  for (const row of rows) {
    for (const entry of list(snapshotOf(row)?.records)) {
      if (entry?.record?.kind !== 'source') continue;
      const title = entry.record.title;
      const known = spans.get(title) ?? new Set();
      for (const material of list(entry.canonical_materials)) if (material?.span_id) known.add(material.span_id);
      spans.set(title, known);
    }
  }
  return spans;
}

function finalWorld(rows) {
  let payload = null;
  for (const row of rows) {
    const world = list(snapshotOf(row)?.records).filter((entry) => entry?.snapshot?.kind === 'world').at(-1);
    if (world?.snapshot?.payload) payload = world.snapshot.payload;
  }
  return payload;
}

/** Claims joining the two patterns (either direction) that cite one of `spans`. */
export function pairClaims(payload, between, spans) {
  const nodes = list(payload?.nodes);
  const [left, right] = between.map((pattern) => new Set(nodes.filter((node) => matches(pattern, node)).map((node) => node.id)));
  return list(payload?.claims).filter((claim) => {
    if (claim?.status === 'withdrawn') return false;
    const ends = list(claim?.endpoints).map(nodeId);
    const joins = (ends.some((id) => left.has(id)) && ends.some((id) => right.has(id)));
    return joins && list(claim?.evidence).some((item) => spans.has(spanOf(item)));
  });
}

const describe = (claim) => ({
  id: claim.id,
  relation: claim.relation,
  proposition: claim.proposition,
  valid_at: claim.validAt?.value ?? null,
  invalid_at: claim.invalidAt?.value ?? null,
  expired: set(claim.expiredAt),
  invalidated: invalidated(claim),
});

export function gradeScenario(expected, rows) {
  const payload = finalWorld(rows);
  const spans = spansByTitle(rows);
  const complete = rows.filter((row) => !String(row.step_id).startsWith('__')).every((row) => row.status === 'completed');
  const old = pairClaims(payload, expected.old.between, spans.get(expected.old.source_title) ?? new Set());
  const fresh = expected.new ? pairClaims(payload, expected.new.between, spans.get(expected.new.source_title) ?? new Set()) : [];
  const oldInvalidated = old.filter(invalidated).length;
  let pass;
  if (expected.check === 'supersede') pass = old.length > 0 && oldInvalidated === old.length && fresh.some((claim) => !invalidated(claim));
  else if (expected.check === 'end') pass = old.length > 0 && oldInvalidated === old.length;
  else pass = old.length > 0 && oldInvalidated === 0;
  return {
    scenario_id: expected.id,
    check: expected.check,
    trial: rows[0]?.trial ?? null,
    complete,
    pass: complete && payload !== null && pass,
    old_claims: old.map(describe),
    new_claims: fresh.map(describe),
    invalidated_claims: list(payload?.claims).filter(invalidated).length,
    claims: list(payload?.claims).length,
  };
}

/** Groups rows by scenario-trial, then by attempt; a retry's later attempt is the one scored. */
export function gradeArm(oracle, rows) {
  const groups = new Map();
  for (const row of rows) {
    const key = `${row.scenario_id}\u0000${row.trial}`;
    if (!groups.has(key)) groups.set(key, new Map());
    const attempts = groups.get(key);
    const attempt = row.attempt_id ?? '';
    if (!attempts.has(attempt)) attempts.set(attempt, []);
    attempts.get(attempt).push(row);
  }
  const trials = [];
  for (const expected of oracle.scenarios) {
    for (const [key, attempts] of groups) {
      if (key.split('\u0000')[0] !== expected.id) continue;
      const entries = [...attempts.entries()];
      const graded = gradeScenario(expected, entries.at(-1)[1]);
      graded.unscored_attempt_ids = entries.slice(0, -1).map(([id]) => id);
      trials.push(graded);
    }
  }
  const of = (check) => trials.filter((trial) => trial.check === check);
  const expectedChanges = [...of('supersede'), ...of('end')];
  return {
    trials,
    pooled: {
      invalidation_passes: [expectedChanges.filter((trial) => trial.pass).length, expectedChanges.length],
      // An incomplete control proves nothing either way; `incomplete` counts it.
      control_false_invalidations: of('control').filter((trial) => trial.complete && !trial.pass).length,
      control_trials: of('control').length,
      invalidated_claims: trials.reduce((total, trial) => total + trial.invalidated_claims, 0),
      incomplete: trials.filter((trial) => !trial.complete).length,
    },
  };
}

export function acceptance(baseline, candidate) {
  const holds = candidate.pooled.invalidation_passes[0] >= baseline.pooled.invalidation_passes[0]
    && candidate.pooled.control_false_invalidations <= baseline.pooled.control_false_invalidations
    && candidate.pooled.incomplete === 0;
  // A comparison against an incomplete baseline is not evidence; rerun the baseline first.
  return { holds, baseline_complete: baseline.pooled.incomplete === 0, baseline: baseline.pooled, candidate: candidate.pooled };
}

async function readRows(paths) {
  const rows = [];
  for (const path of paths) {
    for (const line of (await readFile(path, 'utf8')).split('\n')) if (line.trim()) rows.push(JSON.parse(line));
  }
  return rows;
}

async function main(argv) {
  const options = { arms: [] };
  for (let index = 0; index < argv.length; index += 2) {
    const [flag, value] = [argv[index], argv[index + 1]];
    if (flag === '--oracle') options.oracle = value;
    else if (flag === '--out') options.out = value;
    else if (flag === '--arm') {
      const [name, paths] = value.split('=');
      options.arms.push({ name, paths: paths.split(',') });
    } else throw new Error(`unknown flag ${flag}`);
  }
  const oracle = JSON.parse(await readFile(options.oracle, 'utf8'));
  const arms = [];
  for (const arm of options.arms) arms.push({ arm: arm.name, ...gradeArm(oracle, await readRows(arm.paths)) });
  const report = { suite: oracle.suite, acceptance_rule: oracle.acceptance, arms };
  if (arms.length === 2) report.acceptance = acceptance(arms[0], arms[1]);
  await writeFile(options.out, `${JSON.stringify(report, null, 1)}\n`);
  for (const arm of arms) console.log(arm.arm, JSON.stringify(arm.pooled));
  if (report.acceptance) console.log('acceptance', report.acceptance.holds ? 'holds' : 'fails', report.acceptance.baseline_complete ? '' : '(baseline incomplete: not evidence)');
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
