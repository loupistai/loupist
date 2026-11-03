#!/usr/bin/env node
/** Deterministic grader for the custom-focus seed comparison. No model calls. */
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { entityMatchesNode, normalizeName, patternHits, sha256 } from './ontology-grade.mjs';

const NAMED_KINDS = new Set(['Organization', 'Person', 'Asset', 'Product', 'Place', 'Policy']);
const CORE_ALIASES = new Set([
  'organization', 'person', 'asset', 'product', 'place', 'policy', 'event', 'metric', 'entity',
  'company', 'stock', 'executive', 'drug', 'country',
]);
const SOURCE_ROLES = new Set(['source', 'from', 'subject']);
const TARGET_ROLES = new Set(['target', 'to', 'object']);

const finite = (value) => typeof value === 'number' && Number.isFinite(value);
const list = (value) => Array.isArray(value) ? value : [];
const ratio = (numerator, denominator) => denominator ? numerator / denominator : null;
const compile = (pattern) => new RegExp(pattern, 'iu');

function namesOf(node) {
  return [node?.name, ...list(node?.aliases)].filter((value) => typeof value === 'string');
}

function activeClaims(payload) {
  return list(payload?.claims).filter((claim) => claim?.status !== 'withdrawn');
}

function endpointNodeId(endpoint) {
  return endpoint?.nodeId ?? endpoint?.node_id ?? null;
}

function directedEnds(claim) {
  const endpoints = list(claim?.endpoints);
  const sourceEnds = endpoints.filter((endpoint) => SOURCE_ROLES.has(endpoint?.role));
  const targetEnds = endpoints.filter((endpoint) => TARGET_ROLES.has(endpoint?.role));
  if (sourceEnds.length || targetEnds.length) {
    return [sourceEnds.map(endpointNodeId).filter(Boolean), targetEnds.map(endpointNodeId).filter(Boolean)];
  }
  const ids = endpoints.map(endpointNodeId).filter(Boolean);
  return [ids.slice(0, 1), ids.slice(1, 2)];
}

function distinctDirected(from, to, fromSet, toSet) {
  for (const sourceId of from) {
    if (!fromSet.has(sourceId)) continue;
    for (const targetId of to) {
      if (toSet.has(targetId) && sourceId !== targetId) return true;
    }
  }
  return false;
}

function directHit(claim, sourceIds, targetIds, accepted) {
  const [from, to] = directedEnds(claim);
  if (accepted.has('forward') && distinctDirected(from, to, sourceIds, targetIds)) return true;
  if (accepted.has('reverse') && distinctDirected(from, to, targetIds, sourceIds)) return true;
  return false;
}

function claimLinks(claim, left, right) {
  if (!left || !right || left === right) return false;
  const ids = new Set(list(claim?.endpoints).map(endpointNodeId).filter(Boolean));
  return ids.has(left) && ids.has(right);
}

function textHits(patterns, text) {
  return patterns.some((pattern) => patternHits(pattern, text, { contained: true }));
}

function numericHit(fact, payload) {
  const metricNames = list(payload?.nodes).filter((node) => node?.kind === 'Metric').flatMap(namesOf);
  const texts = [...activeClaims(payload).map((claim) => claim?.proposition ?? ''), ...metricNames];
  return texts.some((text) => textHits(fact.value_patterns, text) && textHits(fact.status_patterns, text));
}

function stringsIn(value, found = []) {
  if (typeof value === 'string') found.push(value);
  else if (Array.isArray(value)) value.forEach((item) => stringsIn(item, found));
  else if (value && typeof value === 'object') Object.values(value).forEach((item) => stringsIn(item, found));
  return found;
}

function worldPayload(row) {
  const snapshot = row?.after_chat?.records ? row.after_chat : row?.before_chat;
  const worlds = list(snapshot?.records).filter((entry) => entry?.snapshot?.kind === 'world');
  return worlds.at(-1)?.snapshot?.payload ?? null;
}

function usageOf(call, kind) {
  const usage = call?.usage;
  if (usage == null || typeof usage !== 'object') return null;
  const aliases = kind === 'input' ? ['input_tokens', 'input', 'inputTokens'] : ['output_tokens', 'output', 'outputTokens'];
  for (const name of aliases) if (finite(usage[name])) return usage[name];
  return null;
}

function episodeUsage(rows) {
  const calls = new Map();
  let seen = false;
  for (const row of rows) {
    const lists = [];
    if (Object.hasOwn(row, 'world_inference')) {
      if (!Array.isArray(row.world_inference)) return null;
      lists.push(row.world_inference);
    }
    if (Array.isArray(row.world_schema?.seed_inference)) lists.push(row.world_schema.seed_inference);
    if (!lists.length) continue;
    seen = true;
    for (const call of lists.flat()) calls.set(call?.call_index ?? `missing-${calls.size}`, call);
  }
  if (!seen) return { world_model_calls: null, world_input_tokens: null, world_output_tokens: null };
  const recorded = [...calls.values()];
  const sum = (pick) => {
    const values = recorded.map(pick);
    return values.every(finite) ? values.reduce((total, value) => total + value, 0) : null;
  };
  return {
    world_model_calls: recorded.length,
    world_input_tokens: sum((call) => usageOf(call, 'input')),
    world_output_tokens: sum((call) => usageOf(call, 'output')),
  };
}

const PHRASE_BOUNDARY = /[:;,(]|\s[-–—]\s|\b(?:in|of|for|with|on|at|by|from|to|under|against|versus|vs)\b/iu;

/** Last word before the first phrase boundary. A trailing possessive is dropped. */
function nounHead(name) {
  const text = String(name ?? '');
  const boundary = PHRASE_BOUNDARY.exec(text);
  const prefix = (boundary ? text.slice(0, boundary.index) : text).trim();
  const words = prefix.split(/\s+/u).filter(Boolean);
  return (words.at(-1) ?? '').replace(/(?:'s|’s)$/iu, '');
}

/** Last PascalCase word. ClinicalTrial is Trial. TrialPhase is Phase. */
function kindHead(kind) {
  const parts = String(kind ?? '').match(/[A-Z]+(?![a-z])|[A-Z]?[a-z]+|\d+/gu);
  return parts?.at(-1) ?? '';
}

function kindHits(patterns, kind) {
  const head = kindHead(kind);
  if (!head || CORE_ALIASES.has(head.toLowerCase())) return false;
  return list(patterns).some((pattern) => patternHits(pattern, head));
}

function contained(patterns, name) {
  return list(patterns).some((pattern) => patternHits(pattern, name, { contained: true }));
}

function cueHeadMatches(patterns, name) {
  const head = nounHead(name);
  if (!head) return false;
  return list(patterns).some((pattern) => {
    const body = String(pattern).replace(/^\^/, '').replace(/\$$/u, '');
    return patternHits(`^(?:${body})$`, head);
  });
}

/** (a) whole name_patterns, (b) kind head plus a contained anchor, or (c) anchor plus a cue on the noun head. */
function conceptInstance(concept, node) {
  const cues = list(concept.cue_patterns);
  return namesOf(node).some((raw) => {
    const name = normalizeName(raw);
    if (list(concept.name_patterns).some((pattern) => patternHits(pattern, name))) return true;
    if (!contained(concept.anchor_patterns, name)) return false;
    if (kindHits(concept.kind_patterns, node.kind)) return true;
    return cues.length > 0 && cueHeadMatches(cues, name);
  });
}

function duplicatesCore(name) {
  return CORE_ALIASES.has(String(name ?? '').trim().toLowerCase());
}

function proposedTypes(schema) {
  if (schema?.proposal && typeof schema.proposal === 'object') return list(schema.proposal.types);
  return list(schema?.extension?.types);
}

function seedReport(rows, concepts) {
  const seedRow = [...rows].reverse().find((row) => row.world_schema) ?? null;
  const schema = seedRow?.world_schema ?? null;
  const types = proposedTypes(schema).filter((item) => item && typeof item.name === 'string');
  const names = types.map((item) => item.name);
  return {
    status: schema?.proposal?.status ?? null,
    mode: schema?.mode ?? null,
    count: names.length,
    names,
    duplicates_core: names.some(duplicatesCore),
    domain_kind: names.some((name) => concepts.some((concept) => kindHits(concept.kind_patterns, name))),
    types: types.map((item) => ({
      name: item.name,
      category: item.category ?? null,
      description: item.description ?? '',
      labels: item.labels ?? null,
      blurbs: item.blurbs ?? null,
    })),
  };
}

function matchingNodes(spec, nodes, conceptIds) {
  const matches = conceptIds.has(spec.id)
    ? (node) => conceptInstance(spec, node)
    : (node) => entityMatchesNode(spec, node);
  return nodes.filter(matches);
}

function nodeIdSet(spec, nodes, conceptIds) {
  return new Set(matchingNodes(spec, nodes, conceptIds).map((node) => node.id));
}

function viaNodeIds(relation, matchedIds) {
  const ids = [];
  const seen = new Set();
  for (const conceptId of relation.via ?? []) {
    for (const nodeId of matchedIds.get(conceptId) ?? []) {
      if (!seen.has(nodeId)) {
        seen.add(nodeId);
        ids.push(nodeId);
      }
    }
  }
  return ids;
}

function twoHop(claims, nodes, sourceIds, targetIds, viaIds) {
  const names = new Map(nodes.map((node) => [node.id, typeof node.name === 'string' ? node.name : null]));
  for (const viaId of viaIds) {
    let sourceHit = null;
    let targetHit = null;
    for (const claim of claims) {
      for (const sourceId of sourceIds) {
        if (sourceId !== viaId && claimLinks(claim, viaId, sourceId)) sourceHit = sourceId;
      }
      for (const targetId of targetIds) {
        if (targetId !== viaId && claimLinks(claim, viaId, targetId)) targetHit = targetId;
      }
    }
    if (sourceHit && targetHit && sourceHit !== targetHit) return { via_node: names.get(viaId) ?? null };
  }
  return null;
}

function gradeTrial(scenario, expected, rows) {
  const expectedSteps = new Set(scenario.steps.map((step) => step.id));
  const byStep = new Map(rows.map((row) => [row.step_id, row]));
  const complete = [...expectedSteps].every((id) => byStep.get(id)?.status === 'completed');
  let payload = null;
  let snapshotStep = null;
  for (const row of rows) {
    if (String(row.step_id ?? '').startsWith('__')) continue;
    const next = worldPayload(row);
    if (next) {
      payload = next;
      snapshotStep = row.step_id;
    }
  }
  const nodes = list(payload?.nodes);
  const claims = payload ? activeClaims(payload) : [];
  const concepts = expected.domain_concepts ?? [];
  const conceptIds = new Set(concepts.map((concept) => concept.id));
  const catalog = new Map();
  for (const entity of [...expected.entities, ...concepts]) catalog.set(entity.id, entity);
  const matchedIds = new Map();
  for (const spec of catalog.values()) matchedIds.set(spec.id, nodeIdSet(spec, nodes, conceptIds));

  let entitiesMatched = 0;
  let typesCorrect = 0;
  let duplicateEntities = 0;
  const entityDetail = [];
  for (const entity of expected.entities) {
    const matched = matchingNodes(entity, nodes, conceptIds);
    const typed = matched.some((node) => node.kind === entity.kind);
    if (matched.length) entitiesMatched += 1;
    if (matched.length && typed) typesCorrect += 1;
    if (matched.length > 1 && NAMED_KINDS.has(entity.kind)) duplicateEntities += 1;
    entityDetail.push({
      id: entity.id,
      matched_nodes: matched.length,
      kinds: [...new Set(matched.map((node) => node.kind ?? null))],
    });
  }

  let domainTyped = 0;
  let domainPresent = 0;
  const domainDetail = [];
  for (const concept of concepts) {
    const matched = matchingNodes(concept, nodes, conceptIds);
    const typed = matched.some((node) => kindHits(concept.kind_patterns, node.kind));
    if (matched.length) domainPresent += 1;
    if (typed) domainTyped += 1;
    domainDetail.push({
      id: concept.id,
      matched_nodes: matched.length,
      names: matched.map((node) => node.name ?? null),
      kinds: [...new Set(matched.map((node) => node.kind ?? null))],
      present: matched.length > 0,
      extension_typed: typed,
    });
  }

  let relationsJoined = 0;
  let relationsDirect = 0;
  const relationDetail = [];
  for (const relation of expected.relations) {
    const sourceIds = matchedIds.get(relation.source) ?? new Set();
    const targetIds = new Set(matchedIds.get(relation.target) ?? []);
    for (const extra of relation.also_targets ?? []) {
      for (const id of matchedIds.get(extra) ?? []) targetIds.add(id);
    }
    const accepted = new Set(relation.directions ?? ['forward', 'reverse']);
    const direct = claims.some((claim) => directHit(claim, sourceIds, targetIds, accepted));
    const hop = direct ? null : twoHop(claims, nodes, sourceIds, targetIds, viaNodeIds(relation, matchedIds));
    const path = direct ? 'direct' : hop ? 'two_hop' : null;
    if (path) relationsJoined += 1;
    if (path === 'direct') relationsDirect += 1;
    relationDetail.push({
      id: relation.id,
      joined: path !== null,
      path,
      via_node: path === 'two_hop' ? hop.via_node : null,
    });
  }

  let numericFound = 0;
  const numericDetail = [];
  for (const fact of expected.numeric_facts) {
    const found = payload ? numericHit(fact, payload) : false;
    if (found) numericFound += 1;
    numericDetail.push({ id: fact.id, found });
  }

  const untyped = nodes.filter((node) => node.kind === 'Entity').length;
  const queryDetail = [];
  let queryHits = 0;
  for (const query of expected.queries ?? []) {
    const row = byStep.get(query.step_id);
    const entries = list(row?.before_chat?.queries).concat(list(row?.after_chat?.queries));
    const texts = entries.flatMap((entry) => stringsIn(entry?.result));
    const targets = (query.target_entity_ids ?? []).map((id) => catalog.get(id)).filter(Boolean);
    const failed = !entries.length || entries.some((entry) => entry?.status && entry.status !== 'completed');
    const hit = !failed && targets.some((spec) => texts.some((text) => spec.name_patterns.some((pattern) => patternHits(pattern, text, { contained: true }))));
    if (hit) queryHits += 1;
    queryDetail.push({ step_id: query.step_id, hit });
  }

  const metrics = payload ? {
    domain_typed_rate: ratio(domainTyped, concepts.length),
    domain_present_rate: ratio(domainPresent, concepts.length),
    untyped_share: nodes.length ? untyped / nodes.length : null,
    entity_recall: ratio(entitiesMatched, expected.entities.length),
    type_accuracy: ratio(typesCorrect, entitiesMatched),
    relation_pair_recall: ratio(relationsJoined, expected.relations.length),
    relation_pair_recall_direct: ratio(relationsDirect, expected.relations.length),
    duplicate_entities: duplicateEntities,
    numeric_fidelity: ratio(numericFound, expected.numeric_facts.length),
    counts: {
      domain: [domainTyped, concepts.length],
      domain_present: [domainPresent, concepts.length],
      untyped: [untyped, nodes.length],
      entities: [entitiesMatched, expected.entities.length],
      types: [typesCorrect, entitiesMatched],
      relation_pairs: [relationsJoined, expected.relations.length],
      relation_pairs_direct: [relationsDirect, expected.relations.length],
      numeric: [numericFound, expected.numeric_facts.length],
      queries: [queryHits, (expected.queries ?? []).length],
    },
  } : {
    domain_typed_rate: null, domain_present_rate: null, untyped_share: null, entity_recall: null,
    type_accuracy: null, relation_pair_recall: null, relation_pair_recall_direct: null,
    duplicate_entities: null, numeric_fidelity: null, counts: null,
  };

  return {
    trial: rows[0]?.trial ?? null,
    attempt_id: rows[0]?.attempt_id ?? null,
    complete,
    snapshot_step_id: snapshotStep,
    metrics,
    entities: entityDetail,
    domain_concepts: domainDetail,
    relations: relationDetail,
    numeric_facts: numericDetail,
    queries: queryDetail,
    seed: seedReport(rows, concepts),
    usage: episodeUsage(rows),
  };
}

function meanOf(values) {
  return values.length && values.every(finite) ? values.reduce((total, value) => total + value, 0) / values.length : null;
}

function fieldMean(trials, metric) {
  return meanOf(trials.map((trial) => trial.metrics[metric]));
}

function usageMean(trials, field) {
  return meanOf(trials.map((trial) => trial.usage?.[field]));
}

function requirePatterns(values, label) {
  if (!Array.isArray(values) || !values.length || values.some((pattern) => typeof pattern !== 'string' || !pattern)) {
    throw new Error(label);
  }
  for (const pattern of values) compile(pattern);
}

function validateInputs(suite, oracle) {
  if (suite?.schema !== 'loupist-world-suite/1' || suite.version !== 1) throw new Error('Unsupported World suite');
  if (oracle?.schema !== 'loupist-world-seed-oracle/2' || oracle.version !== 2) throw new Error('Unsupported seed oracle');
  const scenarios = new Map(suite.scenarios.map((scenario) => [scenario.id, scenario]));
  if (scenarios.size !== suite.scenarios.length) throw new Error('Duplicate scenario id');
  for (const expected of oracle.scenarios ?? []) {
    const scenario = scenarios.get(expected.id);
    if (!scenario) throw new Error(`Oracle scenario is not in the corpus: ${expected.id}`);
    const ids = new Set();
    const conceptIds = new Set();
    for (const entity of expected.entities ?? []) {
      if (!entity.id || ids.has(entity.id) || !entity.kind || !entity.name_patterns?.length) throw new Error(`Invalid entity ${expected.id}/${entity.id}`);
      ids.add(entity.id);
      for (const pattern of entity.name_patterns) compile(pattern);
    }
    for (const concept of expected.domain_concepts ?? []) {
      if (!concept.id || ids.has(concept.id)) throw new Error(`Invalid domain concept ${expected.id}/${concept.id}`);
      requirePatterns(concept.name_patterns, `Invalid domain concept ${expected.id}/${concept.id}`);
      requirePatterns(concept.anchor_patterns, `Invalid domain concept ${expected.id}/${concept.id}`);
      requirePatterns(concept.kind_patterns, `Invalid domain concept ${expected.id}/${concept.id}`);
      if (concept.cue_patterns != null) requirePatterns(concept.cue_patterns, `Invalid domain concept ${expected.id}/${concept.id}`);
      ids.add(concept.id);
      conceptIds.add(concept.id);
    }
    for (const relation of expected.relations ?? []) {
      if (!ids.has(relation.source) || !ids.has(relation.target) || (relation.also_targets ?? []).some((id) => !ids.has(id))) {
        throw new Error(`Invalid relation ${expected.id}/${relation.id}`);
      }
      if ((relation.via ?? []).some((id) => !conceptIds.has(id))) throw new Error(`Invalid relation ${expected.id}/${relation.id}`);
    }
    for (const fact of expected.numeric_facts ?? []) {
      if (!fact.value_patterns?.length || !fact.status_patterns?.length) throw new Error(`Invalid numeric fact ${expected.id}/${fact.id}`);
      for (const pattern of [...fact.value_patterns, ...fact.status_patterns]) compile(pattern);
    }
    const stepIds = new Set(scenario.steps.map((step) => step.id));
    for (const query of expected.queries ?? []) {
      if (!stepIds.has(query.step_id)) throw new Error(`Unknown query step ${expected.id}/${query.step_id}`);
    }
  }
  return scenarios;
}

function gradeArm(name, traces, scenarios, oracle) {
  const grouped = new Map();
  for (const row of traces) {
    if (row?.step_id === '__setup' || row?.step_id === '__shutdown') continue;
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
  for (const expected of oracle.scenarios) {
    const trials = [];
    const keys = [...grouped.keys()].filter((key) => JSON.parse(key)[0] === expected.id).sort((a, b) => JSON.parse(a)[1] - JSON.parse(b)[1]);
    for (const key of keys) {
      const attempts = [...grouped.get(key).entries()];
      const [, rows] = attempts.at(-1);
      const graded = gradeTrial(scenarios.get(expected.id), expected, rows);
      graded.unscored_attempt_ids = attempts.slice(0, -1).map(([id]) => id);
      trials.push(graded);
    }
    scenarioReports.push({ scenario_id: expected.id, trials });
  }
  const trials = scenarioReports.flatMap((scenario) => scenario.trials);
  return {
    arm: name,
    scenarios: scenarioReports,
    means: {
      domain_typed_rate: fieldMean(trials, 'domain_typed_rate'),
      domain_present_rate: fieldMean(trials, 'domain_present_rate'),
      untyped_share: fieldMean(trials, 'untyped_share'),
      entity_recall: fieldMean(trials, 'entity_recall'),
      type_accuracy: fieldMean(trials, 'type_accuracy'),
      relation_pair_recall: fieldMean(trials, 'relation_pair_recall'),
      relation_pair_recall_direct: fieldMean(trials, 'relation_pair_recall_direct'),
      duplicate_entities: fieldMean(trials, 'duplicate_entities'),
      numeric_fidelity: fieldMean(trials, 'numeric_fidelity'),
    },
    usage_per_episode: {
      episodes: trials.length,
      world_model_calls: usageMean(trials, 'world_model_calls'),
      world_input_tokens: usageMean(trials, 'world_input_tokens'),
      world_output_tokens: usageMean(trials, 'world_output_tokens'),
    },
  };
}

function criterion(pass, detail) {
  return { pass: Boolean(pass), ...detail };
}

function atLeast(candidate, baseline, margin) {
  return finite(candidate) && finite(baseline) && candidate >= baseline - margin;
}

function withinBudget(candidate, baseline) {
  return finite(candidate) && finite(baseline) && candidate <= baseline * 1.2;
}

function scenarioMean(arm, scenarioId, metric) {
  const scenario = arm.scenarios.find((item) => item.scenario_id === scenarioId);
  if (!scenario) return null;
  return meanOf(scenario.trials.map((trial) => trial.metrics?.[metric]));
}

function totalDuplicates(arm) {
  const values = arm.scenarios.flatMap((scenario) => scenario.trials.map((trial) => trial.metrics?.duplicate_entities));
  if (!values.length || values.some((value) => !finite(value))) return null;
  return values.reduce((total, value) => total + value, 0);
}

function armReady(arm) {
  return arm.scenarios.length > 0 && arm.scenarios.every((scenario) => (
    scenario.trials.length === 3 && scenario.trials.every((trial) => trial.complete && trial.snapshot_step_id)
  ));
}

/** Pre-registered rules for the two-scenario seed comparison. Thresholds stay fixed after the runs. */
export function seedCriteria(report) {
  const none = report.arms.find((arm) => arm.arm === 'A');
  const model = report.arms.find((arm) => arm.arm === 'B');
  if (!none || !model) return null;
  const bothReady = armReady(none) && armReady(model);
  const seeds = model.scenarios.flatMap((scenario) => scenario.trials.map((trial) => trial.seed));
  const rate = (arm, metric) => arm.means[metric];
  const scenarioIds = none.scenarios.map((scenario) => scenario.scenario_id);
  const sameScenarios = scenarioIds.length === model.scenarios.length
    && scenarioIds.every((id, index) => model.scenarios[index].scenario_id === id);
  const perScenario = sameScenarios && scenarioIds.every((id) => atLeast(scenarioMean(model, id, 'relation_pair_recall'), scenarioMean(none, id, 'relation_pair_recall'), 0.1));
  const relationPass = bothReady && atLeast(rate(model, 'relation_pair_recall'), rate(none, 'relation_pair_recall'), 0.05) && perScenario;
  const totalA = totalDuplicates(none);
  const totalB = totalDuplicates(model);
  const duplicatePass = bothReady && finite(totalA) && finite(totalB) && totalB <= totalA + 1;
  const numericPass = bothReady
    && atLeast(rate(model, 'numeric_fidelity'), rate(none, 'numeric_fidelity'), 0.05)
    && atLeast(rate(model, 'type_accuracy'), rate(none, 'type_accuracy'), 0.05);
  const domainPass = bothReady && finite(rate(model, 'domain_typed_rate')) && rate(model, 'domain_typed_rate') >= 0.6;
  const untypedPass = bothReady && finite(rate(model, 'untyped_share')) && finite(rate(none, 'untyped_share')) && rate(model, 'untyped_share') <= rate(none, 'untyped_share') - 0.1;
  const costPass = bothReady
    && withinBudget(model.usage_per_episode.world_model_calls, none.usage_per_episode.world_model_calls)
    && withinBudget(model.usage_per_episode.world_input_tokens, none.usage_per_episode.world_input_tokens);
  const proposing = seeds.filter((seed) => seed?.domain_kind).length;
  const seedPass = bothReady && seeds.length === 6 && proposing >= 5 && seeds.every((seed) => !seed?.duplicates_core);
  const allPass = domainPass && untypedPass && relationPass && duplicatePass && numericPass && costPass && seedPass;
  let decision = null;
  if (bothReady) {
    if (allPass) decision = 'auto_apply';
    else if (!relationPass || !duplicatePass || !numericPass) decision = 'remove';
    else decision = 'keep_suggestion';
  }
  return {
    ready: bothReady,
    decision,
    domain_typed_rate: criterion(domainPass, { B: rate(model, 'domain_typed_rate'), floor: 0.6 }),
    untyped_share: criterion(untypedPass, {
      B: rate(model, 'untyped_share'), A: rate(none, 'untyped_share'), gap: 0.1,
    }),
    relation_pair_recall: criterion(relationPass, {
      B: rate(model, 'relation_pair_recall'), A: rate(none, 'relation_pair_recall'), margin: 0.05, per_scenario_margin: 0.1,
      per_scenario: scenarioIds.map((id) => ({
        scenario_id: id,
        B: scenarioMean(model, id, 'relation_pair_recall'),
        A: scenarioMean(none, id, 'relation_pair_recall'),
      })),
    }),
    duplicate_entities: criterion(duplicatePass, { B_total: totalB, A_total: totalA, slack: 1 }),
    numeric_and_type: criterion(numericPass, {
      B_numeric: rate(model, 'numeric_fidelity'), A_numeric: rate(none, 'numeric_fidelity'),
      B_type: rate(model, 'type_accuracy'), A_type: rate(none, 'type_accuracy'),
    }),
    world_cost: criterion(costPass, {
      B_calls: model.usage_per_episode.world_model_calls, A_calls: none.usage_per_episode.world_model_calls,
      B_input_tokens: model.usage_per_episode.world_input_tokens, A_input_tokens: none.usage_per_episode.world_input_tokens,
      cap: 1.2,
    }),
    seed_proposal: criterion(seedPass, {
      proposing_trials: proposing, required: 5, seeds: seeds.length,
      duplicates_core: seeds.some((seed) => seed?.duplicates_core),
    }),
  };
}

export function gradeSeed({ suite, suiteBytes, oracle, oracleBytes, arms }) {
  const bytes = Buffer.isBuffer(suiteBytes) ? suiteBytes : Buffer.from(suiteBytes);
  if (sha256(bytes) !== oracle.input_sha256) throw new Error('Frozen corpus digest does not match oracle');
  const scenarios = validateInputs(suite, oracle);
  const graded = arms.map((arm) => gradeArm(arm.arm, arm.traces, scenarios, oracle));
  const report = {
    schema: 'loupist-world-seed-report/2',
    corpus_sha256: sha256(bytes),
    oracle_sha256: sha256(oracleBytes ?? Buffer.from(JSON.stringify(oracle))),
    arms: graded,
  };
  report.criteria = seedCriteria(report);
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
  const arms = [];
  for (const arm of args.arms) {
    const text = await readFile(arm.traces, 'utf8');
    arms.push({
      arm: arm.arm,
      traces: text.split('\n').filter((line) => line.trim()).map((line) => JSON.parse(line)),
    });
  }
  const report = gradeSeed({
    suite: JSON.parse(suiteBytes), suiteBytes, oracle: JSON.parse(oracleBytes), oracleBytes, arms,
  });
  await writeFile(args.out, `${JSON.stringify(report, null, 2)}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
