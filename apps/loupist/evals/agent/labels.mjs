/** Human labels for judged items, plus agreement against the judge. */
import { turnIdentity, groupBy, hasTurnIdentity, isRecord } from './eval-contract.mjs';
import { itemId } from './judge.mjs';
import { wilsonInterval } from './report.mjs';

const clip = (value, max = 800) => {
  const text = String(value ?? '');
  return text.length <= max ? text : `${text.slice(0, max)}…`;
};

export function labelViews(judgments, traces) {
  const byTurn = new Map(traces.map((trace) => [turnIdentity(trace), trace]));
  return judgments.map((judgment) => {
    const trace = byTurn.get(turnIdentity(judgment));
    return {
      item_id: judgment.item_id, criterion_id: judgment.criterion_id, metric: judgment.metric ?? null,
      criterion: judgment.criterion ?? judgment.criterion_id, user: trace?.user ?? '', answer: trace?.answer ?? '',
      memory_item: judgment.memory_item ?? null,
    };
  });
}

export function formatLabelItem(item, index, total) {
  const lines = [
    `[${index + 1}/${total}] ${item.criterion_id} (${item.metric ?? 'unscoped'})`,
    item.criterion, '',
    `User: ${clip(item.user)}`,
    `Answer: ${clip(item.answer)}`,
  ];
  if (item.memory_item) lines.push(`Memory: ${item.memory_item.kind ?? ''} ${item.memory_item.title ?? ''} — ${clip(item.memory_item.summary, 400)}`);
  lines.push('', 'p pass | f fail | s skip | q quit, with an optional note after the command');
  return lines.join('\n');
}

export function parseLabelLine(line) {
  const match = /^(p|pass|f|fail|s|skip|q|quit)\b\s*(.*)$/iu.exec(String(line ?? '').trim());
  if (!match) return null;
  const command = match[1].toLowerCase();
  if (command === 'q' || command === 'quit') return { action: 'quit' };
  if (command === 's' || command === 'skip') return { action: 'skip' };
  return { action: 'label', pass: command === 'p' || command === 'pass', note: match[2].trim() };
}

export async function labelInteractive({ items, ask, now = () => new Date().toISOString() }) {
  const labels = [];
  for (let index = 0; index < items.length; index += 1) {
    let attempts = 0;
    for (;;) {
      const parsed = parseLabelLine(await ask(formatLabelItem(items[index], index, items.length)));
      if (!parsed) {
        attempts += 1;
        if (attempts >= 5) throw new Error('Unrecognized label command');
        continue;
      }
      if (parsed.action === 'quit') return labels;
      if (parsed.action === 'skip') break;
      labels.push({
        item_id: items[index].item_id, criterion_id: items[index].criterion_id,
        human_pass: parsed.pass, note: parsed.note, labeled_at: now(),
      });
      break;
    }
  }
  return labels;
}

export function importLabels(text) {
  const labels = [];
  const ids = new Set();
  for (const [index, line] of String(text).split('\n').entries()) {
    if (!line.trim()) continue;
    let value;
    try { value = JSON.parse(line); } catch { throw new Error(`Label line ${index + 1} is not JSON`); }
    if (!isRecord(value)) throw new Error(`Label line ${index + 1} must be an object`);
    if (typeof value.item_id !== 'string' || !value.item_id) throw new Error(`Label line ${index + 1} missing item_id`);
    if (typeof value.criterion_id !== 'string' || !value.criterion_id) throw new Error(`Label line ${index + 1} missing criterion_id`);
    if (typeof value.human_pass !== 'boolean') throw new Error(`Label line ${index + 1} human_pass must be boolean`);
    if (ids.has(value.item_id)) throw new Error(`Label line ${index + 1} has duplicate item_id`);
    ids.add(value.item_id);
    labels.push({
      item_id: value.item_id, criterion_id: value.criterion_id, human_pass: value.human_pass,
      note: typeof value.note === 'string' ? value.note : '', labeled_at: typeof value.labeled_at === 'string' ? value.labeled_at : null,
    });
  }
  return labels;
}

function confusion(pairs) {
  let tp = 0;
  let tn = 0;
  let fp = 0;
  let fn = 0;
  for (const { judgment, label } of pairs) {
    if (judgment.pass && label.human_pass) tp += 1;
    else if (!judgment.pass && !label.human_pass) tn += 1;
    else if (judgment.pass && !label.human_pass) fp += 1;
    else fn += 1;
  }
  const matched = pairs.length;
  const positives = tp + fn;
  const negatives = tn + fp;
  return {
    counts: { matched, positives, negatives, tp, tn, fp, fn },
    accuracy: matched === 0 ? null : (tp + tn) / matched,
    tpr: positives === 0 ? null : tp / positives,
    tnr: negatives === 0 ? null : tn / negatives,
    wilson_95: { accuracy: wilsonInterval(tp + tn, matched), tpr: wilsonInterval(tp, positives), tnr: wilsonInterval(tn, negatives) },
  };
}

export function agreement(judgments, labels) {
  const judgmentGroups = groupBy(judgments, (row) => row?.item_id);
  const labelGroups = groupBy(labels, (row) => row?.item_id);
  const pairs = [];
  const invalid = { unmatched: 0, invalid_judgments: 0, invalid_labels: 0, duplicate_judgments: 0, duplicate_labels: 0, criterion_mismatches: 0 };
  const validJudgments = new Map();
  for (const [id, rows] of judgmentGroups) {
    if (rows.length !== 1) { invalid.duplicate_judgments += 1; continue; }
    const row = rows[0];
    if (!hasTurnIdentity(row) || typeof row.pass !== 'boolean' || typeof row.criterion_id !== 'string'
      || !row.criterion_id || id !== itemId(row, row.criterion_id)) { invalid.invalid_judgments += 1; continue; }
    validJudgments.set(id, row);
  }
  for (const [id, rows] of labelGroups) {
    if (rows.length !== 1) { invalid.duplicate_labels += 1; continue; }
    const label = rows[0];
    if (!isRecord(label) || typeof id !== 'string' || !id || typeof label.criterion_id !== 'string'
      || !label.criterion_id || typeof label.human_pass !== 'boolean') { invalid.invalid_labels += 1; continue; }
    const judgment = validJudgments.get(id);
    if (!judgment) { invalid.unmatched += 1; continue; }
    if (label.criterion_id !== judgment.criterion_id) { invalid.criterion_mismatches += 1; continue; }
    pairs.push({ judgment, label });
  }
  const overall = confusion(pairs);
  const breakdown = (key) => Object.fromEntries([...groupBy([...validJudgments.values()], key).keys()].sort().map((id) => {
    const summary = confusion(pairs.filter((pair) => key(pair.judgment) === id));
    const status = summary.counts.matched < 10 ? 'insufficient_labels'
      : summary.tpr >= 0.8 && summary.tnr >= 0.8 ? 'pass' : 'fail';
    return [id, { ...summary, status }];
  }));
  const reasons = [];
  if (overall.counts.matched < 30) reasons.push('matched < 30');
  if (overall.counts.positives < 10) reasons.push('positives < 10');
  if (overall.counts.negatives < 10) reasons.push('negatives < 10');
  if (overall.tpr == null || overall.tpr < 0.8) reasons.push('TPR < 0.8 or unavailable');
  if (overall.tnr == null || overall.tnr < 0.8) reasons.push('TNR < 0.8 or unavailable');
  for (const [name, count] of Object.entries(invalid)) if (count) reasons.push(`${name}: ${count}`);
  return {
    ...overall,
    counts: { ...overall.counts, labels: labels.length, ...invalid },
    ready: reasons.length === 0,
    validated: reasons.length === 0,
    reasons,
    per_criterion: breakdown((row) => row.criterion_id),
    per_metric: breakdown((row) => row.metric ?? 'unscoped'),
  };
}
