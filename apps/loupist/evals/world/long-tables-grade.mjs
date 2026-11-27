#!/usr/bin/env node
/**
 * Counts the labelled figures of a long-table smoke among the claims it published. No model call.
 *
 * A labelled cell is `found` when one active claim from its Source names the cell's row and
 * column and states its value; `wrong` when a claim names the row and column and states only
 * other figures; `missing` otherwise. A claim names the row's entity in its text or as one of
 * its endpoints, since a claim may leave its subject to the node. A long table loses its header past the first chunk
 * unless the chunker carries it, so the counts compare two chunkers on the same Sources.
 */
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

const list = (value) => (Array.isArray(value) ? value : []);
// Words that say what kind of thing a row is, not which one: a proposition may drop them.
const GENERIC = new Set(['and', 'of', 'the', 'before', 'for', 'unit', 'segment', 'number']);
const tokens = (text) => String(text).toLowerCase().match(/[\p{L}\p{N}]+/gu) ?? [];
const key = (text) => tokens(text).filter((word) => !GENERIC.has(word) && word.length > 2);

/** A period label as the patterns a sentence may state it by, and the finer periods of the same year. */
export function periodOf(label) {
  const match = /^(?:(Q[1-4]|H[12])\s+)?(?:FY\s*)?(\d{4})[-/](\d{2})$/u.exec(label.trim());
  if (!match) throw new Error(`Unsupported period label ${label}`);
  const [, part, start, end] = match;
  const year = `(?:fy\\s*)?${start}\\s*[-–/]\\s*(?:20)?${end}`;
  const ordinal = { 1: 'first', 2: 'second', 3: 'third', 4: 'fourth' };
  const parts = {
    Q: (n) => `(?:q${n}|${n}q|${ordinal[n]}[- ]quarter|quarter ${n})`,
    H: (n) => `(?:h${n}|${n}h|${ordinal[n]}[- ]half${n === '1' ? '|half[- ]year|six months' : ''})`,
  };
  const finer = '(?:q[1-4]|[1-4]q|h[12]|[12]h|(?:first|second|third|fourth)[- ](?:quarter|half)|half[- ]year|six months)';
  // A part and its year stand together ("Q2 FY2026-27", "the second quarter of FY2026-27").
  const near = (first, second) => new RegExp(`${first}[^.;\\d]{0,24}${second}|${second}[^.;\\d]{0,24}${first}`, 'iu');
  return part
    ? { states: (text) => near(parts[part[0]](part[1]), year).test(text) }
    : { states: (text) => new RegExp(year, 'iu').test(text) && !near(finer, year).test(text) };
}

/** Amounts a proposition states, without the digits of its period labels and dates. */
export function amounts(proposition) {
  const text = String(proposition)
    .replace(/\b(?:fy\s*)?\d{4}\s*[-–/]\s*\d{2,4}\b/giu, ' ')
    .replace(/\b(?:q[1-4]|[1-4]q|h[12]|[12]h)\b/giu, ' ')
    .replace(/\b\d{1,2}\s+(?:january|february|march|april|may|june|july|august|september|october|november|december)\s+\d{4}\b/giu, ' ')
    .replace(/\b(?:19|20)\d{2}\b/gu, ' ');
  return (text.match(/\d[\d,]*(?:\.\d+)?/gu) ?? []).map((value) => Number(value.replaceAll(',', ''))).filter(Number.isFinite);
}

// The same amount in the scales a proposition may restate it in.
const SCALES = { crore: [1, 10, 0.01], lakh: [1, 0.1, 0.01], million: [1, 1000, 0.001], billion: [1, 1000], count: [1] };
const same = (left, right) => Math.abs(left - right) <= 1e-9 * Math.max(1, Math.abs(left), Math.abs(right));

function gradeCell(cell, claims) {
  const [entity, ...measure] = cell.row.split(': ');
  const [name, measureWords] = [key(entity)[0], key(measure.join(' '))];
  const period = periodOf(cell.column);
  const accepted = (SCALES[cell.unit] ?? [1]).map((factor) => cell.value * factor);
  const about = claims.filter((claim) => {
    const words = new Set(tokens(claim.proposition));
    return (words.has(name) || claim.endpointWords.has(name)) && measureWords.some((word) => words.has(word))
      && period.states(claim.proposition);
  });
  const stated = about.filter((claim) => amounts(claim.proposition).some((value) => accepted.some((want) => same(value, want))));
  if (stated.length) return { ...cell, outcome: 'found', claim_id: stated[0].id };
  const other = about.find((claim) => amounts(claim.proposition).length > 0);
  return other ? { ...cell, outcome: 'wrong', claim_id: other.id, proposition: other.proposition } : { ...cell, outcome: 'missing' };
}

function gradeSource(scenario, rows) {
  const [step] = scenario.steps;
  const [fixture] = step.sources;
  const stage = rows.find((row) => row.step_id === step.id);
  const records = list(stage?.before_chat?.records);
  const spans = new Set(records.filter(({ record }) => record?.kind === 'source' && record.title === fixture.title)
    .flatMap(({ canonical_materials }) => list(canonical_materials)).map((item) => item.span_id).filter(Boolean));
  const payload = records.find(({ snapshot }) => snapshot?.kind === 'world')?.snapshot.payload;
  const nodes = new Map(list(payload?.nodes).map((node) => [node.id, node]));
  const claims = list(payload?.claims).filter((claim) => claim.status === 'active' && list(claim.evidence).some((item) => spans.has(item.spanId)))
    .map((claim) => ({ ...claim, endpointWords: new Set(list(claim.endpoints).map((end) => nodes.get(end.nodeId))
      .flatMap((node) => [node?.name, ...list(node?.aliases)].filter(Boolean).flatMap(tokens))) }));
  const complete = rows.length > 0 && rows.every((row) => row.status === 'completed') && stage !== undefined && Array.isArray(payload?.claims);
  const cells = scenario.cells.map((cell) => gradeCell(cell, claims));
  const count = (outcome) => cells.filter((cell) => cell.outcome === outcome).length;
  return {
    scenario_id: scenario.id, profile: scenario.profile, form: scenario.form, complete,
    source_characters: fixture.text.length, claims: claims.length, calls: list(stage?.world_inference).length,
    // An incomplete trial measured nothing: its counts are unknown, not zero.
    labelled: cells.length, found: complete ? count('found') : null, wrong: complete ? count('wrong') : null, missing: complete ? count('missing') : null,
    cells: complete ? cells : [],
  };
}

/** One arm: the traces of one run of the suite. */
export function gradeLongTables(suite, rows) {
  const ids = new Set(suite.scenarios.map(({ id }) => id));
  if (rows.some((row) => row.suite !== suite.suite || !ids.has(row.scenario_id))) throw new Error('long-table traces must name the suite and its scenarios');
  const scenarios = suite.scenarios.map((scenario) => gradeSource(scenario, rows.filter((row) => row.scenario_id === scenario.id)));
  const total = (field) => (scenarios.some((item) => item[field] === null) ? null : scenarios.reduce((sum, item) => sum + item[field], 0));
  return { scenarios, labelled: total('labelled'), found: total('found'), wrong: total('wrong'), missing: total('missing'), calls: total('calls') };
}

async function main(argv) {
  const options = { arms: [] };
  for (let index = 0; index < argv.length; index += 2) {
    const [flag, value] = [argv[index], argv[index + 1]];
    if (flag === '--suite') options.suite = value;
    else if (flag === '--out') options.out = value;
    else if (flag === '--arm' && value?.includes('=')) options.arms.push({ name: value.slice(0, value.indexOf('=')), path: value.slice(value.indexOf('=') + 1) });
    else throw new Error(`unknown flag ${flag}`);
  }
  if (!options.suite || !options.out || !options.arms.length) throw new Error('usage: long-tables-grade.mjs --suite SUITE --arm NAME=TRACE.jsonl [--arm NAME=TRACE.jsonl] --out NEW-REPORT.json');
  const suite = JSON.parse(await readFile(options.suite, 'utf8'));
  const arms = [];
  for (const arm of options.arms) {
    const rows = (await readFile(arm.path, 'utf8')).split('\n').filter((line) => line.trim()).map((line) => JSON.parse(line));
    arms.push({ arm: arm.name, ...gradeLongTables(suite, rows) });
  }
  await writeFile(options.out, `${JSON.stringify({ suite: suite.suite, arms }, null, 1)}\n`, { flag: 'wx' });
  for (const arm of arms) console.log(arm.arm, JSON.stringify({ labelled: arm.labelled, found: arm.found, wrong: arm.wrong, missing: arm.missing, calls: arm.calls }));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
