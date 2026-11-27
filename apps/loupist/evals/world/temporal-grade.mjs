#!/usr/bin/env node
/**
 * Deterministic temporal-invalidation grader for World eval traces and profiles.
 *
 * For each scenario trial it reads the final World snapshot, finds the claims
 * joining the oracle's entity pair whose evidence cites the named Source, and
 * checks whether they were invalidated (invalidAt or expiredAt set) as the
 * oracle expects.
 */
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
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

const qualifier = (claim, key) => list(claim.qualifiers).find((item) => item.key === key)?.value;

function selectedClaims(payload, selector, spans) {
  const evidence = spans.get(selector.source_title) ?? new Set();
  const claims = selector.between
    ? pairClaims(payload, selector.between, evidence)
    : list(payload?.claims).filter((claim) => claim.status !== 'withdrawn'
      && list(claim.evidence).some((item) => evidence.has(spanOf(item))));
  if (!selector.proposition) return claims;
  const pattern = new RegExp(selector.proposition, 'iu');
  return claims.filter((claim) => pattern.test(claim.proposition));
}

function timeStamp(time) {
  if (!set(time)) return null;
  const value = time.precision === 'instant' && Number.isFinite(Date.parse(time.value))
    ? new Date(time.value).toISOString()
    : time.value;
  return { value, precision: time.precision, approximate: time.approximate ?? false };
}

const sameTime = (actual, expected) => JSON.stringify(timeStamp(actual)) === JSON.stringify(timeStamp(expected));

const endpointKey = (claim) => list(claim.endpoints).map(nodeId).sort().join('\u0000');

/**
 * The later Source's own statement of an ending ("stepped down effective June 15"). The host keeps
 * it as a claim of its own: no start, the stated end, and its own evidence as the witness.
 */
function statesOwnEnd(claim) {
  const witness = qualifier(claim, 'superseded_by_span');
  return qualifier(claim, 'end_basis') === 'stated' && !qualifier(claim, 'superseded_by_claim')
    && Boolean(witness) && list(claim.evidence).some((item) => spanOf(item) === witness);
}

/** A host qualifier whose value is JSON (`reopened {at, by_span}`, `lapsed [{from, to}]`), else the text as stored. */
function structuredQualifier(claim, key) {
  const value = qualifier(claim, key);
  if (value === undefined) return null;
  try { return JSON.parse(value); } catch { return value; }
}

function describeV3(claim) {
  const [reopened, lapsed] = [structuredQualifier(claim, 'reopened'), structuredQualifier(claim, 'lapsed')];
  return {
    id: claim.id,
    // The lineage of a reference ending that a later restatement withdrew.
    ...(reopened === null ? {} : { reopened }),
    ...(lapsed === null ? {} : { lapsed }),
    endpoints: endpointKey(claim),
    status: claim.status,
    proposition: claim.proposition,
    interval: {
      start: timeStamp(claim.validAt),
      end: timeStamp(claim.invalidAt),
    },
    expired: set(claim.expiredAt),
    end_basis: qualifier(claim, 'end_basis') ?? null,
    witness_span: qualifier(claim, 'superseded_by_span') ?? null,
    replaced_by: qualifier(claim, 'superseded_by_claim') ?? null,
    conflicts_with: qualifier(claim, 'conflicts_with_claim') ?? null,
  };
}

/** Read the head revision persisted by the product in this trial's own profile. */
function lensHead(row, expected, oldIds) {
  if (!row?.workspace || !row?.loupist_id) return { measured: false, reason: 'profile location absent' };
  let database;
  try {
    database = new DatabaseSync(join(row.workspace, '.loupist', 'loupist.sqlite3'), { readOnly: true });
    const pages = database.prepare(`
      SELECT m.record_id, v.payload_json
      FROM knowledge_lens_metadata m
      JOIN knowledge_record_heads h ON h.record_id = m.record_id
      JOIN knowledge_record_revisions v ON v.revision_id = h.revision_id AND v.record_id = m.record_id
      WHERE m.loupist_id = ? AND m.title = ? AND m.kind = 'subject' AND m.archived_at_ms IS NULL
    `).all(row.loupist_id, expected.title);
    if (pages.length !== 1) return { measured: true, page_count: pages.length, pass: false };
    const page = pages[0];
    const sections = list(JSON.parse(page.payload_json).sections).filter((item) => item.key === expected.section_key);
    const section = sections.length === 1 ? sections[0] : null;
    const citesOldClaim = list(section?.basis?.refs).some((ref) => ref.kind === 'claim' && oldIds.has(ref.claimId));
    return {
      measured: true,
      record_id: page.record_id,
      state: section?.state?.kind ?? null,
      cites_old_claim: citesOldClaim,
      pass: section?.state?.kind === 'superseded' && citesOldClaim,
    };
  } catch (error) {
    return { measured: false, reason: error.code ?? error.name };
  } finally {
    database?.close();
  }
}

const endKeys = new Set(['end_basis', 'superseded_by_span', 'superseded_by_claim']);
const open = (claim) => !invalidated(claim) && !list(claim.qualifiers).some(({ key }) => endKeys.has(key));

/** Publication time of the Source that holds each span, where the snapshot states one. */
function publicationBySpan(rows) {
  const published = new Map();
  for (const row of rows) {
    for (const entry of list(snapshotOf(row)?.records)) {
      if (entry?.record?.kind !== 'source') continue;
      const captures = list(entry.snapshot?.payload?.source?.captures);
      for (const capture of captures) {
        const at = publicationMs(capture.publicationTime);
        if (at === null) continue;
        for (const representation of list(capture.representations)) {
          for (const span of list(representation.spans)) if (span?.spanId) published.set(span.spanId, at);
        }
        for (const material of list(entry.canonical_materials)) {
          if (material?.span_id && (material.capture_id === capture.captureId || captures.length === 1)) published.set(material.span_id, at);
        }
      }
    }
  }
  return published;
}

function publicationMs(time) {
  if (!set(time)) return null;
  const at = Date.parse(time.precision === 'date' ? `${time.value}T00:00:00Z` : time.value);
  return Number.isFinite(at) ? at : null;
}

/**
 * How many ended claims gained support from a Source published after the Source that
 * witnessed their ending. Null when a witness has no publication time: unknown is not zero.
 */
export function restatedAfterEnd(payload, published) {
  let restated = 0;
  for (const claim of list(payload?.claims)) {
    const witness = qualifier(claim, 'superseded_by_span');
    if (claim.status === 'withdrawn' || !witness) continue;
    const endedAt = published.get(witness);
    if (endedAt === undefined) return null;
    if (list(claim.evidence).some((item) => published.get(spanOf(item)) > endedAt)) restated += 1;
  }
  return restated;
}

const PRECISIONS = ['year', 'month', 'date', 'instant'];

/** Claims whose validity dates are finer than the Source can support (`max_date_precision`). */
function inventedDates(expected, pairs) {
  if (!expected.max_date_precision) return null;
  const limit = PRECISIONS.indexOf(expected.max_date_precision);
  const finer = (time) => time !== null && PRECISIONS.indexOf(time.precision) > limit;
  const claims = pairs.flatMap((pair) => [...pair.old_claims, ...pair.new_claims])
    .filter((claim) => finer(claim.interval.start) || finer(claim.interval.end));
  return { max: expected.max_date_precision, pass: claims.length === 0, claim_ids: claims.map(({ id }) => id) };
}

/** What a record-only scenario observed after a restatement. It gates nothing. */
function recordRestatement(expected, payload, spans, pair) {
  const later = spans.get(expected.restatement.source_title) ?? new Set();
  const cites = (claim) => list(claim.evidence).some((item) => later.has(spanOf(item)));
  const first = selectedClaims(payload, expected.old, spans);
  const again = selectedClaims(payload, { ...expected.old, source_title: expected.restatement.source_title }, spans);
  const firstIds = new Set(first.map(({ id }) => id));
  const restatedAs = first.some((claim) => cites(claim) && !open(claim)) ? 'support_on_ended'
    : first.some((claim) => cites(claim) && open(claim)) ? 'support_on_open'
      : again.some((claim) => !firstIds.has(claim.id)) ? 'new_claim' : 'absent';
  return {
    first_ended: first.length > 0 && first.every((claim) => !open(claim)),
    restated_as: restatedAs,
    second_ended: pair.new_claims.length > 0 && pair.new_claims.every((claim) => claim.interval.end !== null || claim.expired),
  };
}

/**
 * An assertion replaced and then restated by a later Source (X, Y, X again), none with a date.
 * Accepted: the first assertion is current and the second ended at the restatement's day, or
 * the two stand as a conflict with no ending. The first ended while the second is current is
 * rejected: the newest Source says the first holds.
 */
function gradeRestated(expected, payload, spans) {
  const first = selectedClaims(payload, expected.old, spans);
  const second = selectedClaims(payload, expected.new, spans);
  const [firstIds, secondIds] = [first, second].map((claims) => new Set(claims.map(({ id }) => id)));
  const restating = spans.get(expected.restatement.source_title) ?? new Set();
  const day = { value: expected.restatement.published, precision: 'date', approximate: true };
  const retained = first.length > 0 && second.length > 0 && [...first, ...second].every((claim) => claim.status === 'active');
  const linksTo = (claim, ids) => list(claim.qualifiers).some(({ key, value }) => key === 'conflicts_with_claim' && ids.has(value));
  const reopened = retained && first.every(open)
    && second.every((claim) => sameTime(claim.invalidAt, day) && qualifier(claim, 'end_basis') === 'reference'
      && restating.has(qualifier(claim, 'superseded_by_span')));
  const conflict = retained && [...first, ...second].every(open)
    && first.every((claim) => linksTo(claim, secondIds)) && second.every((claim) => linksTo(claim, firstIds));
  const outcome = reopened ? 'reopened' : conflict ? 'conflict'
    : first.some((claim) => !open(claim)) && second.some(open) ? 'first_ended_second_current' : 'other';
  const cites = (claim) => list(claim.evidence).some((item) => restating.has(spanOf(item)));
  return {
    pass: reopened || conflict, outcome, ending: null, retained, conflict: retained ? conflict : null, support: null,
    restatement_on_first: first.some(cites),
    reopened_claims: first.filter((claim) => qualifier(claim, 'reopened') !== undefined).length,
    planned_endings: reopened ? second.map(({ id }) => id) : [],
    old_claims: first.map(describeV3),
    new_claims: second.map(describeV3),
  };
}

function gradePair(expected, payload, spans) {
  const old = selectedClaims(payload, expected.old, spans);
  const fresh = expected.new ? selectedClaims(payload, expected.new, spans) : [];
  const oldIds = new Set(old.map((claim) => claim.id));
  const freshIds = new Set(fresh.map((claim) => claim.id));
  const retained = old.length > 0 && (!expected.new || fresh.some((claim) => !oldIds.has(claim.id)));
  const linksTo = (claim, ids) => list(claim.qualifiers).some(({ key, value }) => key === 'conflicts_with_claim' && ids.has(value));
  const oldLinks = old.filter((claim) => linksTo(claim, freshIds));
  const newLinks = fresh.filter((claim) => linksTo(claim, oldIds));
  const conflict = expected.conflict
    ? retained && oldLinks.length === old.length && newLinks.length === fresh.length
      && [...old, ...fresh].every((claim) => claim.status === 'active')
    : null;
  const witnessSpans = spans.get(expected.witness_source) ?? new Set();
  // The replacement an ended claim names: the new assertion, or the witness Source's own
  // statement of the ending about the same two entities.
  const endStatements = new Set(list(payload?.claims).filter((claim) => statesOwnEnd(claim)
    && witnessSpans.has(qualifier(claim, 'superseded_by_span')) && old.some((item) => endpointKey(item) === endpointKey(claim)))
    .map((claim) => claim.id));
  const replaces = (id) => freshIds.has(id) || endStatements.has(id);
  const support = expected.support
    ? old.length === 1 && list(old[0].evidence).some((item) => spans.get(expected.support.source_title)?.has(spanOf(item)))
      && sameTime(old[0].publicationTime, { value: expected.support.earliest_publication, precision: 'date' })
    : null;
  const ending = expected.end
    ? retained && (support ?? true) && oldLinks.length === 0 && newLinks.length === 0
      && old.every((claim) => claim.status === 'active'
        && sameTime(claim.validAt, expected.old_start)
        && sameTime(claim.invalidAt, expected.end)
        && qualifier(claim, 'end_basis') === expected.end.basis
        && witnessSpans.has(qualifier(claim, 'superseded_by_span'))
        && (!expected.new || replaces(qualifier(claim, 'superseded_by_claim'))))
      && fresh.every((claim) => claim.status === 'active' && open(claim) && sameTime(claim.validAt, expected.new_start))
    : null;
  return {
    pass: retained && (ending ?? ([...old, ...fresh].every(open) && (conflict ?? true))),
    ending,
    retained,
    conflict,
    support,
    old_claims: old.map(describeV3),
    new_claims: fresh.map(describeV3),
  };
}

/** Score the claims returned by the product cutoff read. Recomputing the view
 * from the final snapshot would miss regressions in the read itself. */
function gradeCutoffs(probes, views, expected, pair, spans) {
  const ids = new Set(pair.old_claims.map(({ id }) => id));
  const witness = spans.get(expected.witness_source) ?? new Set();
  return list(probes).map(({ date, ended, witness_known }) => {
    const edges = views?.[date]?.edges;
    const matches = list(edges).filter((edge) => ids.has(edge?.claim?.id));
    const pass = ids.size > 0 && matches.length === ids.size && new Set(matches.map(({ claim }) => claim.id)).size === ids.size
      && matches.every((edge) => {
        const { claim } = edge;
        const leaksWitness = witness_known === false && list(claim.evidence).some((item) => witness.has(spanOf(item)));
        if (claim.status !== 'active' || leaksWitness) return false;
        if (!ended) return open(claim) && edge.ended !== true && claim.ended !== true;
        return sameTime(claim.invalidAt, expected.end) && qualifier(claim, 'end_basis') === expected.end.basis
          && witness.has(qualifier(claim, 'superseded_by_span'))
          && (!set(claim.expiredAt) || Date.parse(claim.expiredAt.value) < Date.parse(date) + 86400000);
      });
    return { date, ended, measured: Array.isArray(edges), pass };
  });
}

function gradeV3(expected, rows, trial) {
  const stepIds = ['__setup', '__seed', ...expected.steps];
  const terminal = rows.at(-1);
  const payload = finalWorld([terminal]);
  const spans = spansByTitle([terminal]);
  const pairs = expected.check === 'restated' ? [gradeRestated(expected, payload, spans)]
    : [expected, ...list(expected.additional_pairs)].map((item) => gradePair(item, payload, spans));
  const primary = pairs[0];
  const endedIds = new Set([...pairs.filter((item) => item.ending !== null).flatMap((item) => item.old_claims.map(({ id }) => id)),
    ...pairs.flatMap((item) => list(item.planned_endings))]);
  const lens = expected.lens ? lensHead(terminal, expected.lens, endedIds) : null;
  const cutoffs = gradeCutoffs(expected.as_of, terminal?.as_of_views, expected, primary, spans);
  const complete = rows.length === stepIds.length
    && rows.every((row, index) => row.step_id === stepIds[index] && row.status === 'completed')
    && Array.isArray(payload?.claims) && Array.isArray(payload?.nodes)
    && expected.required_sources.every((title) => spans.get(title)?.size > 0)
    && (lens?.measured ?? true) && cutoffs.every((item) => item.measured);
  // An end statement about the entities of an expected ending is that ending said by its witness, not a second one.
  const endedPairs = new Set(pairs.filter((item) => item.ending !== null).flatMap((item) => item.old_claims.map(({ endpoints }) => endpoints)));
  const unplanned = list(payload?.claims).filter((claim) => !open(claim) && !endedIds.has(claim.id)
    && !(statesOwnEnd(claim) && endedPairs.has(endpointKey(claim))));
  const recordOnly = expected.check === 'record';
  const dates = inventedDates(expected, pairs);
  return {
    ...primary,
    scenario_id: expected.id,
    check: expected.check,
    trial,
    complete,
    // A record-only scenario reports what happened; only an incomplete trial fails it.
    pass: complete && (recordOnly || (pairs.every((item) => item.pass) && unplanned.length === 0
      && (lens?.pass ?? true) && cutoffs.every((item) => item.pass) && (dates?.pass ?? true))),
    pairs,
    cutoffs,
    lens,
    date_precision: dates,
    restated_after_end: complete ? restatedAfterEnd(payload, publicationBySpan([terminal])) : null,
    ...(recordOnly ? { recorded: recordRestatement(expected, payload, spans, primary) } : {}),
    unexpected_ending_ids: unplanned.map(({ id }) => id),
    invalidated_claims: list(payload?.claims).filter(invalidated).length,
    claims: list(payload?.claims).length,
  };
}

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
export function gradeArm(oracle, rows, { trials: trialCount = oracle.trials } = {}) {
  const v3 = oracle.version >= 3;
  if (v3 && (!Number.isInteger(trialCount) || trialCount < 1)) throw new Error('trials must be a positive integer');
  if (v3 && rows.some((row) => row.suite !== oracle.suite
    || !oracle.scenarios.some((scenario) => scenario.id === row.scenario_id)
    || !Number.isInteger(row.trial) || row.trial < 0 || row.trial >= trialCount
    || typeof row.attempt_id !== 'string' || !row.attempt_id)) {
    throw new Error('v3 traces must match the suite, scenario and trial identities and name their attempts');
  }
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
    if (v3) {
      for (let trial = 0; trial < trialCount; trial++) {
        const entries = [...(groups.get(`${expected.id}\u0000${trial}`) ?? [])];
        const graded = gradeV3(expected, entries.at(-1)?.[1] ?? [], trial);
        graded.unscored_attempt_ids = entries.slice(0, -1).map(([id]) => id);
        trials.push(graded);
      }
      continue;
    }
    for (const [key, attempts] of groups) {
      if (key.split('\u0000')[0] !== expected.id) continue;
      const entries = [...attempts.entries()];
      const graded = gradeScenario(expected, entries.at(-1)[1]);
      graded.unscored_attempt_ids = entries.slice(0, -1).map(([id]) => id);
      trials.push(graded);
    }
  }
  if (v3) {
    const orderArms = compareOrderArms(oracle, trials, trialCount);
    // A record-only scenario states no expected outcome, so it adds to neither count below.
    const gated = trials.filter((trial) => trial.check !== 'record');
    const pairs = gated.flatMap((trial) => trial.pairs.map((pair) => ({ ...pair, complete: trial.complete })));
    const endings = pairs.filter((pair) => pair.ending !== null);
    const outcomes = gated.filter((trial) => trial.check === 'restated' && trial.complete).map((trial) => trial.outcome);
    const restated = Object.fromEntries(oracle.scenarios.map(({ id }) => {
      const counts = trials.filter((trial) => trial.scenario_id === id).map((trial) => trial.restated_after_end);
      return [id, counts.includes(null) ? null : counts.reduce((sum, value) => sum + value, 0)];
    }));
    return {
      trials,
      order_arms: orderArms,
      // Reported for the reopen rule a later release may add (a restatement after an ending is
      // support on the ended claim today). It never changes `failed` or the acceptance.
      restated_after_end: restated,
      pooled: {
        invalidation_passes: [endings.filter((pair) => pair.complete && pair.ending).length, endings.length],
        // An unexpected ending fails any scenario; only a control's counts as a false ending.
        control_false_invalidations: trials.filter((trial) => trial.check === 'control' && trial.complete && trial.unexpected_ending_ids.length > 0).length,
        invented_dates: gated.filter((trial) => trial.complete && trial.date_precision?.pass === false).length,
        // Complete trials of the restated scenarios by what the World showed, and the claims that carry a `reopened` qualifier.
        restated_outcomes: Object.fromEntries(['reopened', 'conflict', 'first_ended_second_current', 'other']
          .map((outcome) => [outcome, outcomes.filter((item) => item === outcome).length])),
        reopened_claims: trials.reduce((total, trial) => total + (trial.reopened_claims ?? 0), 0),
        incomplete: trials.filter((trial) => !trial.complete).length,
        failed: trials.filter((trial) => !trial.pass).length,
        order_failures: orderArms.filter(({ equal }) => equal !== true).length,
      },
    };
  }
  const of = (check) => trials.filter((trial) => trial.check === check);
  const expectedChanges = [...of('supersede'), ...of('end')];
  return {
    trials,
    pooled: {
      invalidation_passes: [expectedChanges.filter((trial) => trial.complete && trial.pass).length, expectedChanges.length],
      control_false_invalidations: of('control').filter((trial) => trial.complete && !trial.pass).length,
      control_trials: of('control').length,
      invalidated_claims: trials.reduce((total, trial) => total + trial.invalidated_claims, 0),
      incomplete: trials.filter((trial) => !trial.complete).length,
    },
  };
}

function compareOrderArms(oracle, trials, trialCount) {
  const intervals = (claims) => claims.map((claim) => JSON.stringify(claim.interval)).sort();
  return Array.from({ length: trialCount }, (_, trial) => {
    const outcomes = oracle.order_arms.map((id) => trials.find((item) => item.scenario_id === id && item.trial === trial));
    const equal = outcomes.every((item) => item.complete && item.retained)
      ? new Set(outcomes.map((item) => JSON.stringify([intervals(item.old_claims), intervals(item.new_claims)]))).size === 1
      : null;
    return { trial, equal };
  });
}

export function releaseAcceptance(graded) {
  return { holds: graded.pooled.failed === 0 && graded.pooled.order_failures === 0 };
}

export function acceptance(baseline, candidate) {
  const baselineComplete = baseline.pooled.incomplete === 0;
  const holds = candidate.pooled.invalidation_passes[0] >= baseline.pooled.invalidation_passes[0]
    && candidate.pooled.control_false_invalidations <= baseline.pooled.control_false_invalidations
    && candidate.pooled.incomplete === 0;
  // A comparison against an incomplete baseline is not evidence; rerun the baseline first.
  return { holds, baseline_complete: baselineComplete, baseline: baseline.pooled, candidate: candidate.pooled };
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
    else if (flag === '--trials') options.trials = Number(value);
    else if (flag === '--out') options.out = value;
    else if (flag === '--arm') {
      const [name, paths] = value.split('=');
      options.arms.push({ name, paths: paths.split(',') });
    } else throw new Error(`unknown flag ${flag}`);
  }
  const oracle = JSON.parse(await readFile(options.oracle, 'utf8'));
  const arms = [];
  const profiled = oracle.version >= 3;
  if (options.trials !== undefined && !profiled) throw new Error('--trials applies to oracle version 3 and later');
  const trials = options.trials ?? oracle.trials;
  for (const arm of options.arms) arms.push({ arm: arm.name, ...gradeArm(oracle, await readRows(arm.paths), { trials }) });
  const report = { suite: oracle.suite, acceptance_rule: oracle.acceptance, ...(profiled ? { trials } : {}), arms };
  if (profiled) {
    if (arms.length !== 1) throw new Error('release grading requires one arm with every ingestion order');
    report.acceptance = releaseAcceptance(arms[0]);
  } else if (arms.length === 2) report.acceptance = acceptance(arms[0], arms[1]);
  await writeFile(options.out, `${JSON.stringify(report, null, 1)}\n`, profiled ? { flag: 'wx' } : undefined);
  for (const arm of arms) console.log(arm.arm, JSON.stringify(arm.pooled));
  if (report.acceptance) console.log('acceptance', report.acceptance.holds ? 'holds' : 'fails', report.acceptance.baseline_complete !== false ? '' : '(baseline incomplete: not evidence)');
  for (const arm of profiled ? arms : []) console.log(arm.arm, 'restated_after_end', JSON.stringify(arm.restated_after_end));
  if (profiled && !report.acceptance.holds) process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
