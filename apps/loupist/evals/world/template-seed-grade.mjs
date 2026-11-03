#!/usr/bin/env node
/**
 * Deterministic grader for the asset-class template seed check (W14.3
 * caution, finance-crypto-and-templates.md §7). No model calls. A template seed
 * ships only if, on its fixture, it leaves no fewer typed core relations and no
 * lower typed share than the same Sources processed without the seed.
 */
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { sha256 } from './ontology-grade.mjs';

/** The twelve core relations every Loupist's World starts with. */
export const CORE_RELATIONS = new Set([
  'SUPPLIES', 'COMPETES_WITH', 'PARTNERS_WITH', 'SUBSIDIARY_OF', 'ACQUIRED', 'INVESTED_IN',
  'LEADS', 'OFFERS', 'USES', 'REPORTS', 'LOCATED_IN', 'AFFECTS',
]);
const ARMS = ['none', 'template'];
const list = (value) => Array.isArray(value) ? value : [];
const finite = (value) => typeof value === 'number' && Number.isFinite(value);
const mean = (values) => values.length && values.every(finite) ? values.reduce((total, value) => total + value, 0) / values.length : null;

function worldPayload(row) {
  const snapshot = row?.after_chat?.records ? row.after_chat : row?.before_chat;
  const worlds = list(snapshot?.records).filter((entry) => entry?.snapshot?.kind === 'world');
  return worlds.at(-1)?.snapshot?.payload ?? null;
}

/** Relation names the seed added, from the harness's `__seed` record. */
function seedRelations(rows) {
  const seed = rows.find((row) => row?.step_id === '__seed');
  return new Set(list(seed?.world_schema?.extension?.relations).map((relation) => relation?.name).filter(Boolean));
}

function trialReport(scenario, rows) {
  const complete = scenario.steps.every((step) => rows.some((row) => row.step_id === step.id && row.status === 'completed'));
  let payload = null;
  for (const row of rows) {
    if (String(row.step_id ?? '').startsWith('__')) continue;
    payload = worldPayload(row) ?? payload;
  }
  const seeded = seedRelations(rows);
  const claims = list(payload?.claims).filter((claim) => claim?.status !== 'withdrawn');
  const core = claims.filter((claim) => CORE_RELATIONS.has(claim?.relation)).length;
  const extension = claims.filter((claim) => seeded.has(claim?.relation)).length;
  return {
    complete: complete && payload !== null,
    seed_relations: [...seeded].sort(),
    claims: claims.length,
    typed_core_relations: core,
    seed_relation_claims: extension,
    typed_share: claims.length ? (core + extension) / claims.length : null,
  };
}

function armReport(scenario, rows) {
  const trials = new Map();
  for (const row of rows) {
    if (row?.scenario_id !== scenario.id || !Number.isInteger(row?.trial)) continue;
    // A retried trial is graded on its latest attempt.
    const attempts = trials.get(row.trial) ?? new Map();
    const attempt = row.attempt_id ?? '';
    attempts.set(attempt, [...(attempts.get(attempt) ?? []), row]);
    trials.set(row.trial, attempts);
  }
  const graded = [...trials.entries()].sort(([a], [b]) => a - b).map(([trial, attempts]) => ({ trial, ...trialReport(scenario, [...attempts.values()].at(-1)) }));
  const complete = graded.filter((trial) => trial.complete);
  return {
    trials: graded,
    complete_trials: complete.length,
    typed_core_relations: mean(complete.map((trial) => trial.typed_core_relations)),
    typed_share: mean(complete.map((trial) => trial.typed_share).filter(finite)),
  };
}

export function gradeTemplateSeed({ suite, suiteBytes, arms }) {
  if (suite?.schema !== 'loupist-world-suite/1' || suite.version !== 1) throw new Error('Unsupported World suite');
  for (const name of ARMS) if (!Array.isArray(arms?.[name])) throw new Error(`Missing arm: ${name}`);
  const scenarios = suite.scenarios.map((scenario) => {
    if (!scenario.template) throw new Error(`Scenario has no template: ${scenario.id}`);
    const none = armReport(scenario, arms.none);
    const template = armReport(scenario, arms.template);
    const measured = none.complete_trials > 0 && template.complete_trials > 0
      && finite(none.typed_core_relations) && finite(template.typed_core_relations);
    const keepsCore = measured && template.typed_core_relations >= none.typed_core_relations;
    const keepsTyped = measured && finite(template.typed_share) && finite(none.typed_share) && template.typed_share >= none.typed_share;
    return {
      id: scenario.id, template: scenario.template, none, template_arm: template,
      decision: !measured ? 'unmeasured' : keepsCore && keepsTyped ? 'ship' : 'hold',
    };
  });
  return {
    schema: 'loupist-world-template-seed-report/1',
    input_sha256: sha256(suiteBytes),
    scenarios,
    ship: Object.fromEntries(scenarios.map((scenario) => [scenario.template, scenario.decision === 'ship'])),
  };
}

async function jsonl(path) {
  return (await readFile(path, 'utf8')).split('\n').filter((line) => line.trim()).map((line) => JSON.parse(line));
}

async function main(argv) {
  const options = { arms: {} };
  for (let index = 0; index < argv.length; index += 2) {
    const [flag, value] = [argv[index], argv[index + 1]];
    if (!value) throw new Error(`Missing value for ${flag}`);
    if (flag === '--suite') options.suite = value;
    else if (flag === '--out') options.out = value;
    else if (flag === '--arm') {
      const [name, path] = value.split('=');
      if (!ARMS.includes(name) || !path) throw new Error('--arm takes none=<jsonl> or template=<jsonl>');
      options.arms[name] = await jsonl(path);
    } else throw new Error(`Unknown option ${flag}`);
  }
  if (!options.suite || !options.out) throw new Error('Usage: template-seed-grade.mjs --suite <corpus> --arm none=<jsonl> --arm template=<jsonl> --out <report>');
  const suiteBytes = await readFile(options.suite);
  const report = gradeTemplateSeed({ suite: JSON.parse(suiteBytes), suiteBytes, arms: options.arms });
  await writeFile(options.out, `${JSON.stringify(report, null, 2)}\n`, { flag: 'wx' });
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main(process.argv.slice(2)).catch((error) => { console.error(error.message); process.exitCode = 1; });
}
