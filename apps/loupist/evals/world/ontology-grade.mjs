#!/usr/bin/env node
/** Deterministic World ontology grader. No model calls. Unknown usage stays null. */
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

const HANGUL = /[\u1100-\u11FF\u3130-\u318F\uAC00-\uD7A3]/u;
const SOURCE_ROLES = new Set(['source', 'from', 'subject']);
const TARGET_ROLES = new Set(['target', 'to', 'object']);
const NAMED_KINDS = new Set(['Organization', 'Person', 'Asset', 'Product', 'Place', 'Policy']);
const TRAILING_LABEL = /\s*(?:\([^)]*\)|\[[^\]]*\])$/u;
const CORPORATE_SUFFIX = /(?:\s+주식회사|\s*\(주\)|\s*,?\s+Co\.,\s*Ltd\.|\s*,?\s+Inc\.|\s*,?\s+Corp\.|\s*,?\s+LLC)$/iu;

// Independent of the extractor: detect value nodes even if they were mislabeled
// as a Product, Event or a user-defined kind. A digit/period alone is not enough.
const MEASUREMENT_NAME = /(?:\b(?:USD|KRW|EUR|GBP|JPY|CNY|RMB|CAD|AUD|CHF)\s*[+−-]?\d|[$€£₩¥]\s*[+−-]?\d|\d[\d,.]*\s*(?:%|퍼센트|bps\b|basis points\b|million\b|billion\b|trillion\b|thousand\b|percent\b|tonnes\b|tons\b|barrels\b|widgets\b|units\b|GWh\b|MWh\b|kWh\b|GW\b|MW\b|kg\b|km\b|억원|조원|만원|천원|원|달러|백만|천만|십억|백억|억|조|만|톤|명|대|개|주|건|배)|\d[\d,.]*\s+(?:shares|employees|patients|vehicles|dollars|euros|people)\b|\b(?:revenue|net income|operating (?:profit|income|margin)|earnings per share|market share|net (?:profit|loss)|sales volume|production capacity|yield)\b\s*[:=]?\s*[+−-]?\d|(?:매출액|순이익|영업이익|점유율|생산량|생산능력|주당순이익)\s*[:=]?\s*[+−-]?\d)/iu;
const COUPON_SECURITY = /^\d+(?:\.\d+)?%\s+(?:(?:notes|bonds)\s+(?:due\s+)?20\d{2}|20\d{2}년\s+만기\s+채권)$/iu;

export function isMeasurementNode(node) {
  const name = String(node?.name ?? '').normalize('NFKC').trim();
  if (node?.kind === 'Metric') return true;
  if (node?.kind === 'Asset' && COUPON_SECURITY.test(name)) return false;
  if (/^\d+$/u.test(name)) return !['Organization', 'Asset'].includes(node?.kind) || name.length > 12;
  if (/^[+−-]?\d[\d,.]*$/u.test(name)) return true;
  return MEASUREMENT_NAME.test(name) || /^\d{4}[-/.]\d{1,2}[-/.]\d{1,2}(?:[T ].*)?$/u.test(name);
}

export const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const finite = (value) => typeof value === 'number' && Number.isFinite(value);
const list = (value) => Array.isArray(value) ? value : [];
const ratio = (numerator, denominator) => denominator ? numerator / denominator : null;
const compile = (pattern) => new RegExp(pattern, 'iu');

function namesOf(node) {
  return [node?.name, ...list(node?.aliases)].filter((value) => typeof value === 'string');
}

/** NFKC, trimmed, with whitespace collapsed and one trailing code or title removed. Case folding is the pattern flag. */
export function normalizeName(value) {
  const compact = String(value ?? '').normalize('NFKC').trim().replace(/\s+/gu, ' ');
  return compact.replace(TRAILING_LABEL, '').trim();
}

/** Name normalization plus one trailing corporate suffix, applied to both sides of an original-name comparison. */
export function normalizeOriginal(value) {
  return normalizeName(value).replace(CORPORATE_SUFFIX, '').trim();
}

/** Anchored patterns match a whole name or alias. Query text also accepts the same pattern inside a longer string. */
export function patternHits(pattern, text, { contained = false } = {}) {
  const source = String(text ?? '');
  if (compile(pattern).test(source)) return true;
  if (!contained) return false;
  const body = pattern.replace(/^\^/, '').replace(/\$$/, '');
  return body !== pattern && compile(body).test(source);
}

export function entityMatchesNode(entity, node) {
  return namesOf(node).some((name) => entity.name_patterns.some((pattern) => patternHits(pattern, normalizeName(name))));
}

function matchingNodes(entity, nodes) {
  return nodes.filter((node) => entityMatchesNode(entity, node));
}

function carriesOriginal(node, original) {
  const needle = normalizeOriginal(original);
  if (!needle) return false;
  const candidates = [...namesOf(node)];
  const properties = node?.properties ?? {};
  const stored = properties.original_name ?? properties.originalName;
  if (typeof stored === 'string') candidates.push(stored);
  return candidates.some((value) => {
    const key = normalizeOriginal(value);
    return key.length > 0 && (key === needle || key.includes(needle));
  });
}

function propertyTokens(value, found = []) {
  if (typeof value === 'string' || typeof value === 'number') found.push(String(value).normalize('NFC').trim());
  else if (Array.isArray(value)) value.forEach((item) => propertyTokens(item, found));
  else if (value && typeof value === 'object') Object.values(value).forEach((item) => propertyTokens(item, found));
  return found;
}

function holdsIdentifier(node, identifier) {
  const want = String(identifier).normalize('NFC').trim();
  return propertyTokens(node?.properties ?? {}).some((value) => value === want || value.split(/[\s,;|/]+/u).includes(want));
}

function activeClaims(payload) {
  return list(payload?.claims).filter((claim) => claim?.status !== 'withdrawn');
}

function endpointNodeId(endpoint) {
  return endpoint?.nodeId ?? endpoint?.node_id ?? null;
}

function pairDirection(claim, sourceIds, targetIds) {
  const endpoints = list(claim?.endpoints);
  const sourceEnds = endpoints.filter((endpoint) => SOURCE_ROLES.has(endpoint?.role));
  const targetEnds = endpoints.filter((endpoint) => TARGET_ROLES.has(endpoint?.role));
  const forwardIds = sourceEnds.length || targetEnds.length
    ? [sourceEnds.map(endpointNodeId), targetEnds.map(endpointNodeId)]
    : [endpoints.slice(0, 1).map(endpointNodeId), endpoints.slice(1, 2).map(endpointNodeId)];
  const [from, to] = forwardIds;
  const forward = from.some((id) => sourceIds.has(id)) && to.some((id) => targetIds.has(id));
  const reverse = from.some((id) => targetIds.has(id)) && to.some((id) => sourceIds.has(id));
  if (forward) return 'forward';
  if (reverse) return 'reverse';
  return null;
}

function stringsIn(value, found = []) {
  if (typeof value === 'string') found.push(value);
  else if (Array.isArray(value)) value.forEach((item) => stringsIn(item, found));
  else if (value && typeof value === 'object') Object.values(value).forEach((item) => stringsIn(item, found));
  return found;
}

function textHits(patterns, text) {
  return patterns.some((pattern) => patternHits(pattern, text, { contained: true }));
}

function numericHit(fact, payload) {
  const metricNames = list(payload?.nodes).filter((node) => node?.kind === 'Metric').flatMap(namesOf);
  const texts = [...activeClaims(payload).map((claim) => claim?.proposition ?? ''), ...metricNames];
  return texts.some((text) => textHits(fact.value_patterns, text) && textHits(fact.status_patterns, text));
}

function queryEntries(row) {
  return [...list(row?.before_chat?.queries), ...list(row?.after_chat?.queries)];
}

function queryHit(entries, targets) {
  if (!entries.length || entries.some((entry) => entry?.status && entry.status !== 'completed')) return false;
  const texts = entries.flatMap((entry) => stringsIn(entry?.result));
  return targets.some((entity) => texts.some((text) => entity.name_patterns.some((pattern) => patternHits(pattern, text, { contained: true }))));
}

function blankMetrics() {
  return {
    entity_recall: null, type_accuracy: null, duplicate_entities: 0,
    original_alias_rate: null, identifier_fill: null, relation_pair_recall: null,
    canonical_relation_rate: null, relation_names_per_100_claims: null, relates_to_share: null,
    non_english_fact_share: null, numeric_fidelity: null, measurement_node_share: null,
    query_hit_rate: { en: null, ko: null },
  };
}

function withRates(counts) {
  const claims = counts.claims;
  return {
    entity_recall: ratio(counts.entitiesMatched, counts.entitiesExpected),
    type_accuracy: ratio(counts.typesCorrect, counts.entitiesMatched),
    duplicate_entities: counts.duplicateEntities,
    original_alias_rate: ratio(counts.aliasCarried, counts.aliasExpected),
    identifier_fill: ratio(counts.identifierFilled, counts.identifierExpected),
    relation_pair_recall: ratio(counts.relationsJoined, counts.relationsExpected),
    canonical_relation_rate: ratio(counts.canonicalClaims, counts.canonicalClaimOpportunities),
    relation_names_per_100_claims: claims ? (counts.relationNames.size * 100) / claims : null,
    relates_to_share: ratio(counts.relatesTo, claims),
    non_english_fact_share: ratio(counts.hangulClaims, claims),
    numeric_fidelity: ratio(counts.numericFound, counts.numericExpected),
    measurement_node_share: counts.nodes ? counts.measurementNodes / counts.nodes : 0,
    query_hit_rate: {
      en: ratio(counts.queryHits.en, counts.queryExpected.en),
      ko: ratio(counts.queryHits.ko, counts.queryExpected.ko),
    },
    counts: {
      entities: [counts.entitiesMatched, counts.entitiesExpected],
      types: [counts.typesCorrect, counts.entitiesMatched],
      duplicates: counts.duplicateEntities,
      original_alias: [counts.aliasCarried, counts.aliasExpected],
      identifier: [counts.identifierFilled, counts.identifierExpected],
      relation_pairs: [counts.relationsJoined, counts.relationsExpected],
      canonical_relation_claims: [counts.canonicalClaims, counts.canonicalClaimOpportunities],
      relation_names: [counts.relationNames.size, claims],
      relates_to: [counts.relatesTo, claims],
      non_english_facts: [counts.hangulClaims, claims],
      numeric: [counts.numericFound, counts.numericExpected],
      measurement_nodes: [counts.measurementNodes, counts.nodes],
      query_en: [counts.queryHits.en, counts.queryExpected.en],
      query_ko: [counts.queryHits.ko, counts.queryExpected.ko],
    },
  };
}

function emptyCounts() {
  return {
    entitiesMatched: 0, entitiesExpected: 0, typesCorrect: 0, duplicateEntities: 0,
    aliasCarried: 0, aliasExpected: 0, identifierFilled: 0, identifierExpected: 0,
    relationsJoined: 0, relationsExpected: 0, canonicalClaims: 0, canonicalClaimOpportunities: 0,
    relationNames: new Set(), relatesTo: 0, hangulClaims: 0, claims: 0,
    numericFound: 0, numericExpected: 0, measurementNodes: 0, nodes: 0,
    queryHits: { en: 0, ko: 0 }, queryExpected: { en: 0, ko: 0 },
  };
}

function addCounts(total, counts) {
  for (const key of ['entitiesMatched', 'entitiesExpected', 'typesCorrect', 'duplicateEntities', 'aliasCarried', 'aliasExpected', 'identifierFilled', 'identifierExpected', 'relationsJoined', 'relationsExpected', 'canonicalClaims', 'canonicalClaimOpportunities', 'relatesTo', 'hangulClaims', 'claims', 'numericFound', 'numericExpected', 'measurementNodes', 'nodes']) {
    total[key] += counts[key];
  }
  for (const name of counts.relationNames) total.relationNames.add(name);
  total.queryHits.en += counts.queryHits.en;
  total.queryHits.ko += counts.queryHits.ko;
  total.queryExpected.en += counts.queryExpected.en;
  total.queryExpected.ko += counts.queryExpected.ko;
  return total;
}

function worldPayload(row) {
  const snapshot = row?.after_chat?.records ? row.after_chat : row?.before_chat;
  const worlds = list(snapshot?.records).filter((entry) => entry?.snapshot?.kind === 'world');
  return worlds.at(-1)?.snapshot?.payload ?? null;
}

function usageOf(call, kind) {
  const usage = call?.usage;
  if (usage == null || typeof usage !== 'object') return null;
  const aliases = kind === 'input'
    ? ['input_tokens', 'input', 'inputTokens']
    : ['output_tokens', 'output', 'outputTokens'];
  for (const name of aliases) if (finite(usage[name])) return usage[name];
  return null;
}

function episodeUsage(rows) {
  let seen = false;
  const calls = new Map();
  for (const row of rows) {
    if (!Object.hasOwn(row, 'world_inference')) continue;
    seen = true;
    if (!Array.isArray(row.world_inference)) return { world_model_calls: null, world_input_tokens: null, world_output_tokens: null, world_model_duration_ms: null };
    for (const call of row.world_inference) calls.set(call?.call_index ?? `${calls.size}`, call);
  }
  if (!seen) return { world_model_calls: null, world_input_tokens: null, world_output_tokens: null, world_model_duration_ms: null };
  const recorded = [...calls.values()];
  const sum = (pick) => {
    const values = recorded.map(pick);
    return values.every(finite) ? values.reduce((total, value) => total + value, 0) : null;
  };
  return {
    world_model_calls: recorded.length,
    world_input_tokens: sum((call) => usageOf(call, 'input')),
    world_output_tokens: sum((call) => usageOf(call, 'output')),
    world_model_duration_ms: sum((call) => finite(call?.duration_ms) ? call.duration_ms : null),
  };
}

function gradeTrial(scenario, expected, rows) {
  const expectedSteps = new Set(scenario.steps.map((step) => step.id));
  const byStep = new Map(rows.map((row) => [row.step_id, row]));
  const complete = [...expectedSteps].every((id) => byStep.get(id)?.status === 'completed');
  let payload = null;
  let snapshotStep = null;
  for (const row of rows) {
    const next = worldPayload(row);
    if (next) { payload = next; snapshotStep = row.step_id; }
  }
  const nodes = list(payload?.nodes);
  const claims = payload ? activeClaims(payload) : [];
  const counts = emptyCounts();
  counts.nodes = nodes.length;
  counts.measurementNodes = nodes.filter(isMeasurementNode).length;
  const entityDetail = [];
  const matchedIds = new Map();
  counts.entitiesExpected = expected.entities.length;
  for (const entity of expected.entities) {
    const matched = matchingNodes(entity, nodes);
    matchedIds.set(entity.id, new Set(matched.map((node) => node.id)));
    const duplicate = matched.length > 1;
    const typed = matched.some((node) => node.kind === entity.kind);
    const alias = entity.original_name ? matched.some((node) => carriesOriginal(node, entity.original_name)) : null;
    const identifier = entity.identifier ? matched.some((node) => holdsIdentifier(node, entity.identifier)) : null;
    if (matched.length) counts.entitiesMatched += 1;
    if (matched.length && typed) counts.typesCorrect += 1;
    if (duplicate && NAMED_KINDS.has(entity.kind)) counts.duplicateEntities += 1;
    if (entity.original_name) {
      counts.aliasExpected += 1;
      if (alias) counts.aliasCarried += 1;
    }
    if (entity.identifier) {
      counts.identifierExpected += 1;
      if (identifier) counts.identifierFilled += 1;
    }
    entityDetail.push({ id: entity.id, matched_nodes: matched.length, kinds: [...new Set(matched.map((node) => node.kind ?? null))], original_alias: alias, identifier });
  }
  const relationDetail = [];
  counts.relationsExpected = expected.relations.length;
  for (const relation of expected.relations) {
    const sourceIds = matchedIds.get(relation.source) ?? new Set();
    const targetIds = matchedIds.get(relation.target) ?? new Set();
    const accepted = new Set(relation.directions ?? ['forward', 'reverse']);
    const joining = claims.filter((claim) => accepted.has(pairDirection(claim, sourceIds, targetIds)));
    const canonical = relation.relation === 'open' ? [] : joining.filter((claim) => claim.relation === relation.relation);
    if (joining.length) counts.relationsJoined += 1;
    if (relation.relation !== 'open') {
      counts.canonicalClaimOpportunities += joining.length;
      counts.canonicalClaims += canonical.length;
    }
    relationDetail.push({ id: relation.id, joined: joining.length > 0, joining_claims: joining.length, canonical_claims: relation.relation === 'open' ? null : canonical.length });
  }
  for (const claim of claims) {
    counts.claims += 1;
    if (typeof claim.relation === 'string' && claim.relation) counts.relationNames.add(claim.relation);
    if (claim.relation === 'RELATES_TO') counts.relatesTo += 1;
    if (HANGUL.test(String(claim.proposition ?? ''))) counts.hangulClaims += 1;
  }
  const numericDetail = [];
  counts.numericExpected = expected.numeric_facts.length;
  for (const fact of expected.numeric_facts) {
    const found = payload ? numericHit(fact, payload) : false;
    if (found) counts.numericFound += 1;
    numericDetail.push({ id: fact.id, found });
  }
  const queryDetail = [];
  const entitiesById = new Map(expected.entities.map((entity) => [entity.id, entity]));
  for (const query of expected.queries) {
    const row = byStep.get(query.step_id);
    const language = query.language === 'ko' ? 'ko' : 'en';
    counts.queryExpected[language] += 1;
    const present = Boolean(row);
    const hit = present && queryHit(queryEntries(row), query.target_entity_ids.map((id) => entitiesById.get(id)));
    if (hit) counts.queryHits[language] += 1;
    queryDetail.push({ step_id: query.step_id, language, present, hit });
  }
  const metrics = payload ? withRates(counts) : blankMetrics();
  if (!payload) metrics.duplicate_entities = null;
  return {
    trial: rows[0]?.trial ?? null,
    attempt_id: rows[0]?.attempt_id ?? null,
    complete,
    snapshot_step_id: snapshotStep,
    nodes: nodes.length,
    claims: claims.length,
    metrics,
    entities: entityDetail,
    relations: relationDetail,
    numeric_facts: numericDetail,
    queries: queryDetail,
    usage: episodeUsage(rows),
    _counts: payload ? counts : emptyCounts(),
  };
}

function meanOf(values) {
  return values.length && values.every(finite) ? values.reduce((total, value) => total + value, 0) / values.length : null;
}

function poolTrials(trials) {
  if (!trials.length || trials.some((trial) => !trial.snapshot_step_id)) {
    const metrics = blankMetrics();
    metrics.duplicate_entities = null;
    metrics.relation_names_per_100_claims = null;
    return metrics;
  }
  const counts = trials.reduce((total, trial) => addCounts(total, trial._counts), emptyCounts());
  const metrics = withRates(counts);
  metrics.duplicate_entities = meanOf(trials.map((trial) => trial.metrics.duplicate_entities));
  metrics.duplicate_entities_total = counts.duplicateEntities;
  metrics.relation_names_per_100_claims = meanOf(trials.map((trial) => trial.metrics.relation_names_per_100_claims));
  return metrics;
}

function usageSummary(trials) {
  const field = (name) => {
    const values = trials.map((trial) => trial.usage[name]);
    return { values, mean: meanOf(values) };
  };
  return {
    episodes: trials.length,
    world_model_calls: field('world_model_calls'),
    world_input_tokens: field('world_input_tokens'),
    world_output_tokens: field('world_output_tokens'),
    world_model_duration_ms: field('world_model_duration_ms'),
  };
}

function validateInputs(suite, oracle) {
  if (suite?.schema !== 'loupist-world-suite/1' || suite.version !== 1 || !Array.isArray(suite.scenarios)) throw new Error('Unsupported World suite');
  if (oracle?.schema !== 'loupist-world-ontology-oracle/1' || oracle.version !== 2) throw new Error('Unsupported ontology oracle');
  const scenarios = new Map(suite.scenarios.map((scenario) => [scenario.id, scenario]));
  if (scenarios.size !== suite.scenarios.length) throw new Error('Duplicate scenario id');
  for (const expected of oracle.scenarios ?? []) {
    if (!scenarios.has(expected.id)) throw new Error(`Oracle scenario is not in the corpus: ${expected.id}`);
    const ids = new Set();
    for (const entity of expected.entities ?? []) {
      if (!entity.id || ids.has(entity.id) || !entity.kind || !entity.name_patterns?.length) throw new Error(`Invalid entity ${expected.id}/${entity.id}`);
      ids.add(entity.id);
      for (const pattern of entity.name_patterns) compile(pattern);
    }
    for (const relation of expected.relations ?? []) {
      if (!ids.has(relation.source) || !ids.has(relation.target) || !relation.relation) throw new Error(`Invalid relation ${expected.id}/${relation.id}`);
    }
    for (const fact of expected.numeric_facts ?? []) {
      if (!fact.value_patterns?.length || !fact.status_patterns?.length || !['reported', 'forecast'].includes(fact.status)) throw new Error(`Invalid numeric fact ${expected.id}/${fact.id}`);
      for (const pattern of [...fact.value_patterns, ...fact.status_patterns]) compile(pattern);
    }
  }
  return scenarios;
}

function gradeArm(arm, traces, scenarios) {
  const grouped = new Map();
  for (const row of traces) {
    if (String(row?.step_id ?? '').startsWith('__')) continue;
    if (!scenarios.has(row?.scenario_id)) continue;
    const key = JSON.stringify([row.scenario_id, row.trial]);
    const bucket = grouped.get(key) ?? new Map();
    const attempt = row.attempt_id ?? '';
    const rows = bucket.get(attempt) ?? [];
    rows.push(row);
    bucket.set(attempt, rows);
    grouped.set(key, bucket);
  }
  const scenarioReports = [];
  for (const expected of arm.oracleScenarios) {
    const trials = [];
    const keys = [...grouped.keys()].filter((key) => JSON.parse(key)[0] === expected.id).sort((a, b) => JSON.parse(a)[1] - JSON.parse(b)[1]);
    for (const key of keys) {
      const attempts = [...grouped.get(key).entries()];
      const [attemptId, rows] = attempts.at(-1);
      const graded = gradeTrial(scenarios.get(expected.id), expected, rows);
      graded.unscored_attempt_ids = attempts.slice(0, -1).map(([id]) => id);
      graded.attempt_id = rows[0]?.attempt_id ?? attemptId;
      trials.push(graded);
    }
    scenarioReports.push({
      scenario_id: expected.id,
      trials: trials.map(({ _counts, ...trial }) => trial),
      pooled: trials.length ? poolTrials(trials) : null,
      _trials: trials,
    });
  }
  const allTrials = scenarioReports.flatMap((scenario) => scenario._trials);
  const report = {
    arm: arm.name,
    scenarios: scenarioReports.map(({ _trials, ...scenario }) => scenario),
    pooled: allTrials.length ? poolTrials(allTrials) : null,
    usage_per_episode: usageSummary(allTrials),
  };
  return report;
}

function criterion(pass, detail) {
  return { pass: Boolean(pass), ...detail };
}

export function adoptionCriteria(report) {
  const baseline = report.arms.find((arm) => arm.arm === 'baseline');
  const candidate = report.arms.find((arm) => arm.arm === 'candidate');
  if (!baseline || !candidate) return null;
  const rate = (arm, metric) => arm.pooled?.[metric] ?? null;
  const mixed = candidate.scenarios.find((scenario) => scenario.scenario_id === 'O-MIXED');
  const mixedDuplicates = mixed?.trials.map((trial) => trial.metrics.duplicate_entities) ?? [];
  const calls = (arm) => arm.usage_per_episode.world_model_calls.mean;
  const input = (arm) => arm.usage_per_episode.world_input_tokens.mean;
  const withinBudget = (candidateValue, baselineValue) => finite(candidateValue) && finite(baselineValue) && candidateValue <= baselineValue * 1.35;
  return {
    type_accuracy: criterion(finite(rate(candidate, 'type_accuracy')) && rate(candidate, 'type_accuracy') >= 0.85, { candidate: rate(candidate, 'type_accuracy') }),
    duplicate_entities: criterion(
      finite(rate(candidate, 'duplicate_entities')) && finite(rate(baseline, 'duplicate_entities'))
        && rate(candidate, 'duplicate_entities') <= rate(baseline, 'duplicate_entities')
        && mixedDuplicates.length > 0 && mixedDuplicates.every((value) => finite(value) && value <= 1),
      { candidate_mean: rate(candidate, 'duplicate_entities'), baseline_mean: rate(baseline, 'duplicate_entities'), mixed_trials: mixedDuplicates },
    ),
    original_alias_rate: criterion(finite(rate(candidate, 'original_alias_rate')) && rate(candidate, 'original_alias_rate') >= 0.8, { candidate: rate(candidate, 'original_alias_rate') }),
    identifier_fill: criterion(finite(rate(candidate, 'identifier_fill')) && rate(candidate, 'identifier_fill') >= 0.8, { candidate: rate(candidate, 'identifier_fill') }),
    relation_pair_recall: criterion(
      finite(rate(candidate, 'relation_pair_recall')) && finite(rate(baseline, 'relation_pair_recall'))
        && rate(candidate, 'relation_pair_recall') >= rate(baseline, 'relation_pair_recall') - 0.05,
      { candidate: rate(candidate, 'relation_pair_recall'), baseline: rate(baseline, 'relation_pair_recall') },
    ),
    relates_to_and_english: criterion(
      finite(rate(candidate, 'relates_to_share')) && rate(candidate, 'relates_to_share') <= 0.05
        && finite(rate(candidate, 'non_english_fact_share')) && rate(candidate, 'non_english_fact_share') <= 0.05,
      { relates_to_share: rate(candidate, 'relates_to_share'), non_english_fact_share: rate(candidate, 'non_english_fact_share') },
    ),
    numeric_fidelity: criterion(
      finite(rate(candidate, 'numeric_fidelity')) && finite(rate(baseline, 'numeric_fidelity'))
        && rate(candidate, 'numeric_fidelity') >= rate(baseline, 'numeric_fidelity') - 0.05,
      { candidate: rate(candidate, 'numeric_fidelity'), baseline: rate(baseline, 'numeric_fidelity') },
    ),
    measurement_nodes: criterion(rate(candidate, 'measurement_node_share') === 0, {
      candidate: rate(candidate, 'measurement_node_share'), baseline: rate(baseline, 'measurement_node_share'),
    }),
    world_cost: criterion(withinBudget(calls(candidate), calls(baseline)) && withinBudget(input(candidate), input(baseline)), {
      candidate_calls_per_episode: calls(candidate), baseline_calls_per_episode: calls(baseline),
      candidate_input_tokens_per_episode: input(candidate), baseline_input_tokens_per_episode: input(baseline),
    }),
  };
}

export function gradeOntology({ suite, suiteBytes, oracle, oracleBytes, arms }) {
  const bytes = Buffer.isBuffer(suiteBytes) ? suiteBytes : Buffer.from(suiteBytes);
  if (sha256(bytes) !== oracle.input_sha256) throw new Error('Frozen corpus digest does not match oracle');
  const scenarios = validateInputs(suite, oracle);
  const graded = arms.map((arm) => gradeArm({ name: arm.arm, oracleScenarios: oracle.scenarios }, arm.traces, scenarios));
  const report = {
    schema: 'loupist-world-ontology-report/1',
    corpus_sha256: sha256(bytes),
    oracle_sha256: sha256(oracleBytes ?? Buffer.from(JSON.stringify(oracle))),
    arms: graded,
  };
  report.adoption = adoptionCriteria(report);
  return report;
}

function parseArgs(argv) {
  const args = { arms: [] };
  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index];
    const value = argv[index + 1];
    if (!value) throw new Error(`Missing value for ${name}`);
    if (name === '--suite') args.suite = value;
    else if (name === '--oracle') args.oracle = value;
    else if (name === '--out') args.out = value;
    else if (name === '--arm') {
      const split = value.indexOf('=');
      if (split <= 0) throw new Error('--arm must be name=traces.jsonl');
      args.arms.push({ arm: value.slice(0, split), traces: value.slice(split + 1) });
    } else throw new Error(`Unknown argument ${name}`);
  }
  if (!args.suite || !args.oracle || !args.out || !args.arms.length) throw new Error('Required: --suite --oracle --out --arm name=traces.jsonl');
  return args;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const suiteBytes = await readFile(args.suite);
  const oracleBytes = await readFile(args.oracle);
  const oracle = JSON.parse(oracleBytes);
  const arms = [];
  for (const arm of args.arms) {
    const text = await readFile(arm.traces, 'utf8');
    const traces = text.split('\n').filter((line) => line.trim()).map((line) => JSON.parse(line));
    arms.push({ arm: arm.arm, traces });
  }
  const report = gradeOntology({ suite: JSON.parse(suiteBytes), suiteBytes, oracle, oracleBytes, arms });
  await writeFile(args.out, `${JSON.stringify(report, null, 2)}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
