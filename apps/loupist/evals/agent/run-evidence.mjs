/** Deterministic evidence from the runner's scheduled-run, dry-run and progress records. No model call. */
import { isRecord, validDate } from './eval-contract.mjs';
import { answerLanguageEvidence } from './language-evidence.mjs';
import { hostResult } from './tool-evidence.mjs';

const fail = (detail) => ({ pass: false, detail });
const unknown = (detail) => ({ pass: null, detail });
const RUN_STATUSES = new Set(['succeeded', 'failed', 'interrupted']);
const BUDGET_OUTCOMES = new Set(['none', 'soft', 'refused', 'cancelled']);
const names = (value, label) => {
  if (value === undefined) return [];
  if (!Array.isArray(value) || !value.length || value.some((item) => typeof item !== 'string' || !item)) throw new Error(`${label} must be a list of names`);
  return value;
};

/**
 * `scheduled_run` states what the occurrence's stored run record must say: any of status, quiet
 * and budget. `after_soft_note: true` is for a fixture built to reach the soft level: the run
 * passes when it finished there, fails when it went on to the hard level, and is unmeasured
 * when it never reached the note, because the fixture's own precondition did not hold.
 */
export function scheduledRunAssertion(assertion) {
  const { status, quiet, budget } = assertion;
  if (assertion.after_soft_note !== undefined && (assertion.after_soft_note !== true || budget !== undefined)) throw new Error('scheduled_run after_soft_note must be true and replaces budget');
  if (status === undefined && quiet === undefined && budget === undefined && !assertion.after_soft_note) throw new Error('scheduled_run requires status, quiet, budget or after_soft_note');
  if (status !== undefined && !RUN_STATUSES.has(status)) throw new Error('scheduled_run status must be succeeded, failed or interrupted');
  if (quiet !== undefined && typeof quiet !== 'boolean') throw new Error('scheduled_run quiet must be a boolean');
  if (budget !== undefined && !BUDGET_OUTCOMES.has(budget)) throw new Error('scheduled_run budget must be none, soft, refused or cancelled');
  return { status, quiet, budget };
}

export function scheduledRunEvidence(assertion, trace) {
  const expected = scheduledRunAssertion(assertion);
  const run = trace.scheduled_run;
  if (!isRecord(run)) return unknown('scheduled run record unavailable');
  const observed = { status: run.status, quiet: run.quiet, budget: isRecord(run.budget) ? run.budget.outcome : run.budget };
  const wanted = Object.entries(expected).filter(([, value]) => value !== undefined);
  const missing = wanted.filter(([field]) => observed[field] === undefined || observed[field] === null).map(([field]) => field);
  if (missing.length) return unknown(`scheduled run record lacks ${missing.join(', ')}`);
  const wrong = wanted.filter(([field, value]) => observed[field] !== value).map(([field, value]) => `${field} ${observed[field]}, expected ${value}`);
  if (assertion.after_soft_note) {
    if (!BUDGET_OUTCOMES.has(observed.budget)) return unknown('scheduled run record lacks budget');
    if (observed.budget !== 'soft' && observed.budget !== 'none') wrong.push(`budget ${observed.budget} after the soft note`);
    if (!wrong.length && observed.budget === 'none') return unknown('the run stayed under its soft level, so the note was never sent');
  }
  return wrong.length ? fail(wrong.join('; '))
    : { pass: true, detail: [...wanted.map(([field, value]) => `${field} ${value}`), ...(assertion.after_soft_note ? ['finished after the soft note'] : [])].join(', ') };
}

/** `check_dry_run` names days a saved check must (not) have been met on and tools its reads must (not) include. */
export function checkDryRunAssertion(assertion) {
  const dates = (value, label) => {
    const list = names(value, `check_dry_run ${label}`);
    if (list.some((date) => !validDate(date))) throw new Error(`check_dry_run ${label} must be YYYY-MM-DD dates`);
    return list;
  };
  const spec = { metOn: dates(assertion.met_on, 'met_on'), notMetOn: dates(assertion.not_met_on, 'not_met_on'),
    tools: names(assertion.tools, 'check_dry_run tools'), noTools: names(assertion.no_tools, 'check_dry_run no_tools') };
  if (!Object.values(spec).some((list) => list.length)) throw new Error('check_dry_run requires met_on, not_met_on, tools or no_tools');
  return spec;
}

/** The replay of a `kind: "check"` turn, else the dry run a successful `trigger_save` returned in this turn. */
function dryRunOf(trace, calls) {
  if (isRecord(trace.trigger_dry_run)) return trace.trigger_dry_run;
  const saved = (calls ?? []).filter((call) => call.name === 'trigger_save' && call.outcome === 'ok').at(-1);
  const dry = saved ? hostResult(saved.result)?.dry_run : null;
  return isRecord(dry) ? dry : null;
}

const day = (entry) => (typeof entry === 'string' ? entry : entry?.date ?? entry?.day ?? null);

export function checkDryRunEvidence(assertion, trace, calls) {
  const spec = checkDryRunAssertion(assertion);
  const dry = dryRunOf(trace, calls);
  if (!dry) return unknown('dry run evidence unavailable');
  // A harness replay lists the met days; a `trigger_save` result lists them as `fired_on` beside every replayed day.
  const met = Array.isArray(dry.fired_on) ? dry.fired_on
    : Array.isArray(dry.days) ? dry.days.filter((entry) => typeof entry === 'string' || entry?.met === true || entry?.met > 0).map(day) : null;
  const tools = Array.isArray(dry.tools) ? dry.tools.map((tool) => (typeof tool === 'string' ? tool : tool?.name)) : null;
  if ((spec.metOn.length || spec.notMetOn.length) && met === null) return unknown('dry run lists no days');
  if ((spec.tools.length || spec.noTools.length) && tools === null) return unknown('dry run lists no tools');
  const problems = [
    ...spec.metOn.filter((date) => !met.includes(date)).map((date) => `not met on ${date}`),
    ...spec.notMetOn.filter((date) => met.includes(date)).map((date) => `met on ${date}`),
    ...spec.tools.filter((tool) => !tools.includes(tool)).map((tool) => `did not read ${tool}`),
    ...spec.noTools.filter((tool) => tools.includes(tool)).map((tool) => `read ${tool}`),
  ];
  return problems.length ? fail(problems.join('; ')) : { pass: true, detail: `met on ${met?.join(', ') || 'no day'}${tools ? `; read ${tools.join(', ') || 'nothing'}` : ''}` };
}

/** `progress_language` reads the progress notes the user saw during the turn. `min` defaults to one note. */
export function progressLanguageAssertion(assertion) {
  const min = assertion.min ?? 1;
  if (!Number.isSafeInteger(min) || min < 0) throw new Error('progress_language min must be a non-negative integer');
  return min;
}

export function progressLanguageEvidence(assertion, trace) {
  const min = progressLanguageAssertion(assertion);
  if (!Array.isArray(trace.progress_notes)) return unknown('progress notes unavailable');
  const notes = trace.progress_notes.map((note) => note?.text).filter((text) => typeof text === 'string' && text.trim());
  if (notes.length < min) return fail(`${notes.length} progress notes, need at least ${min}`);
  if (!notes.length) return unknown('no progress note to read');
  // Notes are short, so the language is read over all of them together.
  const read = answerLanguageEvidence({ language: assertion.language }, notes.join('\n\n'));
  return { pass: read.pass, detail: `${notes.length} progress notes: ${read.detail}` };
}
