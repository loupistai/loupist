#!/usr/bin/env node
/**
 * Grades the structured figures a World run kept against a corpus whose every figure is labelled. No model call.
 *
 * A kept structure is the `figure` qualifier of an active claim that cites the scenario's Source.
 * It is right when a labelled cell has its amount (value times scale, in its currency), its period
 * end and its basis. Precision over the kept structures is the gate; recall is reported.
 */
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

export const MIN_PRECISION = 0.95;
const list = (value) => (Array.isArray(value) ? value : []);
const fold = (text) => String(text ?? '').normalize('NFKC').toLowerCase().replace(/\s+/gu, ' ').trim();
const SCALE_WORDS = new Map(Object.entries({
  thousand: 1e3, k: 1e3, million: 1e6, mn: 1e6, m: 1e6, 'mio.': 1e6, mio: 1e6, milhões: 1e6, milhão: 1e6, billion: 1e9, bn: 1e9, 'mrd.': 1e9, mrd: 1e9,
  bilhões: 1e9, bilhão: 1e9, trillion: 1e12, lakh: 1e5, crore: 1e7, 만: 1e4, 억: 1e8, 조: 1e12, 万: 1e4, 萬: 1e4, 億: 1e8, 亿: 1e8, 百万: 1e6,
}));
// A minor unit is its currency at a fraction, so 18.6 pence and 0.186 pounds are one amount.
const MINOR = new Map([['GBX', ['GBP', 0.01]], ['GBP PENCE', ['GBP', 0.01]]]);
// A company's own outlook is called a forecast in one market and guidance in another.
const BASIS = new Map([['actual', 'actual'], ['forecast', 'outlook'], ['guidance', 'outlook'], ['estimate', 'estimate']]);

function scaleOf(scale) {
  if (scale === undefined || scale === null || scale === '') return 1;
  if (typeof scale === 'number') return scale;
  const number = Number(scale);
  return Number.isFinite(number) ? number : SCALE_WORDS.get(fold(scale)) ?? null;
}

/** A structure or a label as an amount in its currency, or null when it cannot be read. */
export function amountOf({ value, scale, currency }) {
  const [number, factor] = [Number(value), scaleOf(scale)];
  if (value === null || value === undefined || value === '' || !Number.isFinite(number) || factor === null) return null;
  const code = typeof currency === 'string' && currency.trim() ? currency.trim().toUpperCase() : null;
  const [major, minor] = MINOR.get(code) ?? [code, 1];
  return { amount: number * factor * minor, currency: major };
}

const same = (left, right) => Math.abs(left - right) <= 1e-9 * Math.max(Math.abs(left), Math.abs(right), 1e-12);
const sameAmount = (structure, cell) => same(structure.amount, cell.amount) && (structure.currency === null || cell.currency === null || structure.currency === cell.currency);

/** The stated period end at the label's precision; a kept date finer than the Source states is noted, not failed. */
function periodOutcome(structure, cell) {
  if (cell.period_end === null) return fold(structure.period) === fold(cell.period) ? 'equal' : 'differs';
  const kept = typeof structure.period_end === 'string' ? structure.period_end.trim() : '';
  if (kept === cell.period_end) return 'equal';
  return kept.startsWith(`${cell.period_end}-`) ? 'finer' : 'differs';
}

function figureOf(claim) {
  const stored = list(claim.qualifiers).find((item) => item.key === 'figure')?.value;
  if (stored === undefined) return null;
  if (stored !== null && typeof stored === 'object') return stored;
  try {
    const parsed = JSON.parse(stored);
    return parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : { unreadable: true };
  } catch { return { unreadable: true }; }
}

function gradeStructure(structure, cells) {
  const kept = structure.unreadable ? null : amountOf(structure);
  if (kept === null) return { outcome: 'unreadable' };
  const candidates = cells.map((cell, index) => ({ cell, index })).filter(({ cell }) => sameAmount(kept, amountOf(cell)));
  if (!candidates.length) return { outcome: 'value' };
  const graded = candidates.map(({ cell, index }) => ({ index, cell, period: periodOutcome(structure, cell),
    basis: BASIS.has(structure.basis) && BASIS.get(structure.basis) === BASIS.get(cell.basis) }));
  const right = graded.find((item) => item.period !== 'differs' && item.basis);
  if (right) return { outcome: 'right', cell: right.index, finer_period_end: right.period === 'finer', metric_as_stated: fold(structure.metric) === fold(right.cell.metric) };
  return { outcome: graded.some((item) => item.period !== 'differs') ? 'basis' : 'period', cell: graded[0].index };
}

function gradeSource(scenario, rows) {
  const [step] = scenario.steps;
  const [fixture] = step.sources;
  const stage = rows.find((row) => row.step_id === step.id);
  const records = list(stage?.before_chat?.records);
  const spans = new Set(records.filter(({ record }) => record?.kind === 'source' && record.title === fixture.title)
    .flatMap(({ canonical_materials }) => list(canonical_materials)).map((item) => item.span_id).filter(Boolean));
  const payload = records.find(({ snapshot }) => snapshot?.kind === 'world')?.snapshot.payload;
  const complete = rows.length > 0 && rows.every((row) => row.status === 'completed') && stage !== undefined && Array.isArray(payload?.claims);
  const claims = list(payload?.claims).filter((claim) => claim.status === 'active' && list(claim.evidence).some((item) => spans.has(item.spanId)));
  const structures = claims.map((claim) => ({ claim, figure: figureOf(claim) })).filter(({ figure }) => figure !== null)
    .map(({ claim, figure }) => ({ claim_id: claim.id, proposition: claim.proposition ?? null, figure, ...gradeStructure(figure, scenario.cells) }));
  const count = (outcome) => structures.filter((item) => item.outcome === outcome).length;
  const recalled = new Set(structures.filter((item) => item.outcome === 'right').map((item) => item.cell));
  return {
    scenario_id: scenario.id, profile: scenario.profile, complete, claims: claims.length, calls: list(stage?.world_inference).length,
    labelled: scenario.cells.length,
    kept: complete ? structures.length : null,
    right: complete ? count('right') : null,
    wrong: complete ? { value: count('value'), period: count('period'), basis: count('basis'), unreadable: count('unreadable') } : null,
    recalled: complete ? recalled.size : null,
    finer_period_end: complete ? structures.filter((item) => item.finer_period_end).length : null,
    metric_as_stated: complete ? structures.filter((item) => item.metric_as_stated).length : null,
    // Every structure that is not right, with what the Source says, for the reader.
    not_right: complete ? structures.filter((item) => item.outcome !== 'right') : [],
  };
}

/** One run of the corpus. `pass` is the hard gate: complete, something kept, and precision at or above 95%. */
export function gradeFigures(suite, rows) {
  const ids = new Set(suite.scenarios.map(({ id }) => id));
  if (rows.some((row) => row.suite !== suite.suite || !ids.has(row.scenario_id))) throw new Error('figure traces must name the suite and its scenarios');
  const scenarios = suite.scenarios.map((scenario) => gradeSource(scenario, rows.filter((row) => row.scenario_id === scenario.id)));
  const complete = scenarios.every((item) => item.complete);
  const total = (field) => (complete ? scenarios.reduce((sum, item) => sum + item[field], 0) : null);
  const [labelled, kept, right, recalled] = [scenarios.reduce((sum, item) => sum + item.labelled, 0), total('kept'), total('right'), total('recalled')];
  const precision = kept ? right / kept : null;
  return {
    suite: suite.suite, complete, labelled, kept, right, precision, recall: complete ? recalled / labelled : null,
    by_profile: Object.fromEntries([...new Set(scenarios.map((item) => item.profile))].map((profile) => {
      const own = scenarios.filter((item) => item.profile === profile);
      const sum = (field) => (own.every((item) => item.complete) ? own.reduce((value, item) => value + item[field], 0) : null);
      return [profile, { labelled: own.reduce((value, item) => value + item.labelled, 0), kept: sum('kept'), right: sum('right'), recalled: sum('recalled') }];
    })),
    min_precision: MIN_PRECISION,
    pass: complete && precision !== null && precision >= MIN_PRECISION,
    scenarios,
  };
}

async function main([suitePath, tracePath, out]) {
  if (!suitePath || !tracePath || !out) throw new Error('usage: figures-grade.mjs SUITE TRACE.jsonl[,MORE.jsonl] NEW-REPORT.json');
  const suite = JSON.parse(await readFile(suitePath, 'utf8'));
  const rows = [];
  for (const path of tracePath.split(',')) for (const line of (await readFile(path, 'utf8')).split('\n')) if (line.trim()) rows.push(JSON.parse(line));
  const report = gradeFigures(suite, rows);
  await writeFile(out, `${JSON.stringify(report, null, 1)}\n`, { flag: 'wx' });
  console.log(JSON.stringify({ labelled: report.labelled, kept: report.kept, right: report.right, precision: report.precision, recall: report.recall, pass: report.pass }));
  if (!report.pass) process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
