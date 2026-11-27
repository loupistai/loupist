/**
 * Agreement between two judges on the same items. No model call.
 *
 * A criterion is the rubric question, identified by its id wherever it is asked, so a question
 * asked of many scenarios and trials pools its items. One on which the judges agree on fewer
 * than 85% of the shared items, or that has fewer than ten shared items, is unreliable: it is
 * reported and must not decide a keep, a revert or a release.
 */
import { groupBy } from './eval-contract.mjs';

export const MIN_AGREEMENT = 0.85;
export const MIN_ITEMS = 10;

function verdicts(rows, name) {
  const byItem = new Map();
  const problems = [];
  for (const [id, group] of groupBy(rows, (row) => row?.item_id)) {
    const [row] = group;
    if (typeof id !== 'string' || !id || typeof row.pass !== 'boolean' || typeof row.criterion_id !== 'string' || !row.criterion_id) {
      problems.push(`${name}: a row lacks item_id, criterion_id or a boolean pass`);
    } else if (group.length !== 1) problems.push(`${name}: duplicate item ${id}`);
    else byItem.set(id, row);
  }
  return { byItem, problems };
}

const where = (row) => Object.fromEntries(['suite', 'scenario_id', 'session_id', 'turn_index', 'trial', 'tool', 'refusal_reason']
  .filter((field) => row[field] !== undefined && row[field] !== null).map((field) => [field, row[field]]));

/** `a` and `b` are judgment rows of two judges over the same traces. */
export function judgeAgreement(a, b) {
  const left = verdicts(a, 'a');
  const right = verdicts(b, 'b');
  const problems = [...left.problems, ...right.problems];
  if (problems.length) throw new Error(`Judgments cannot be compared:\n${problems.join('\n')}`);
  const shared = [...left.byItem.keys()].filter((id) => right.byItem.has(id));
  const mismatched = shared.filter((id) => left.byItem.get(id).criterion_id !== right.byItem.get(id).criterion_id);
  if (mismatched.length) throw new Error(`The two files name different criteria for ${mismatched.length} item(s)`);
  const disagreements = shared.filter((id) => left.byItem.get(id).pass !== right.byItem.get(id).pass).map((id) => {
    const [first, second] = [left.byItem.get(id), right.byItem.get(id)];
    return { item_id: id, criterion_id: first.criterion_id, ...where(first),
      a: { pass: first.pass, reason: first.reason ?? null }, b: { pass: second.pass, reason: second.reason ?? null } };
  });
  const criteria = [...groupBy(shared, (id) => left.byItem.get(id).criterion_id)].map(([criterion_id, ids]) => {
    const agreed = ids.filter((id) => left.byItem.get(id).pass === right.byItem.get(id).pass).length;
    const rate = agreed / ids.length;
    const reasons = [rate < MIN_AGREEMENT && `agreement under ${MIN_AGREEMENT * 100}%`, ids.length < MIN_ITEMS && `fewer than ${MIN_ITEMS} items`].filter(Boolean);
    return { criterion_id, items: ids.length, agreed, agreement: rate, reliable: reasons.length === 0, reasons };
  }).sort((one, other) => one.criterion_id.localeCompare(other.criterion_id));
  const models = (rows) => [...new Set(rows.map((row) => row.model).filter(Boolean))].sort();
  return {
    judges: { a: models(a), b: models(b) },
    items: { a: left.byItem.size, b: right.byItem.size, shared: shared.length,
      only_a: left.byItem.size - shared.length, only_b: right.byItem.size - shared.length },
    agreement: shared.length ? (shared.length - disagreements.length) / shared.length : null,
    criteria,
    unreliable: criteria.filter((criterion) => !criterion.reliable).map((criterion) => criterion.criterion_id),
    disagreements,
  };
}
