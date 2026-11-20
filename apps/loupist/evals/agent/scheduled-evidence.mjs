import { createHash } from 'node:crypto';

const present = value => typeof value === 'string' && value.trim().length > 0;
const fail = detail => ({ pass: false, detail });
const unknown = detail => ({ pass: null, detail });

function existingTaskEvidence(trace, task, admission, traces) {
  const earlier = traces.filter(row => row.run_id === trace.run_id && row.suite === trace.suite
    && row.scenario_id === trace.scenario_id && row.session_id === trace.session_id
    && row.trial === trace.trial && row.turn_index < trace.turn_index);
  const creations = earlier.filter(row => row.turn_index === 0);
  if (creations.length !== 1 || !Array.isArray(creations[0].scheduled_tasks)) return unknown('task creation snapshot unavailable');
  const [created] = creations[0].scheduled_tasks;
  if (creations[0].status !== 'completed' || creations[0].scheduled_tasks.length !== 1
    || created?.task_id !== task.id || (created.prompt_sha256
      ? created.prompt_sha256 !== createHash('sha256').update(task.prompt).digest('hex') : created.prompt !== task.prompt)) {
    return fail('Run now did not execute the task and prompt saved on the creation turn');
  }
  if (earlier.some(row => row.admission?.run_id === admission.run_id)) return fail('Run now reused a previous occurrence');
  return null;
}

export function scheduledDeliveryEvidence(trace, scenario, turn, traces = [], quiet = false) {
  const { saved_task: task, admission, runs, deliveries, session_answer: source, home_thread_id: home, terminal_event: terminal } = trace;
  if (!task || !admission || !source || !present(home) || !Array.isArray(runs) || !Array.isArray(deliveries)) {
    return unknown('scheduled delivery provenance unavailable');
  }
  if (runs.length !== 1) return fail('expected one history record for this occurrence');
  const [run] = runs;
  if (!run || !present(task.id) || !present(admission.run_id) || !present(admission.thread_id)
    || trace.resolved_appearance !== scenario.language || admission.surface !== 'scheduled'
    || admission.run_id === trace.run_id || admission.thread_id === home
    || task.kind !== turn.scheduled.kind || run.kind !== task.kind
    || admission.task_id !== task.id || run.task_id !== task.id || run.run_id !== admission.run_id
    || run.thread_id !== admission.thread_id || source.thread_id !== run.thread_id) {
    return fail('task, run, surface, appearance or thread identity does not match');
  }
  if (task.kind === 'custom') {
    if (!present(task.prompt) || admission.prompt !== task.prompt) return fail('admission differs from the saved custom request');
    if (turn.scheduled.run_existing) {
      const mismatch = existingTaskEvidence(trace, task, admission, traces);
      if (mismatch) return mismatch;
    } else if (task.prompt !== turn.user) return fail('fixture did not run the requested prompt');
  } else {
    const prefix = `${task.prompt}\n`;
    if (!present(task.prompt) || !present(admission.prompt) || !admission.prompt.startsWith(prefix)) {
      return fail('admitted briefing prompt lacks the saved host template');
    }
    let data;
    try { data = JSON.parse(admission.prompt.slice(prefix.length)); }
    catch { return fail('admitted briefing context is invalid'); }
    if (data?.appLanguage !== trace.resolved_appearance) return fail('admitted briefing appLanguage differs from appearance');
  }
  if (run.status !== 'succeeded'
    || ![run.scheduled_for_ms, run.started_at_ms, run.finished_at_ms].every(Number.isSafeInteger)
    || run.scheduled_for_ms > run.started_at_ms || run.started_at_ms > run.finished_at_ms
    || !present(run.message_id) || run.message_id !== source.message_id || source.status !== 'complete'
    || terminal?.status !== 'completed' || terminal.message_id !== source.message_id
    || source.host_composed !== false) return fail('successful scheduled model reply missing from history');

  if (quiet) {
    const inbox = trace.inbox_run_ids;
    if (!Array.isArray(inbox) || !inbox.every(present)) return unknown('inbox evidence unavailable');
    if (deliveries.length || inbox.includes(run.run_id)) return fail('quiet occurrence was delivered to Bot or the inbox');
    return { pass: true, detail: 'successful occurrence in history without a Bot or inbox delivery' };
  }
  if (deliveries.length !== 1) return fail('expected one Bot delivery');
  const [delivery] = deliveries;
  if (!delivery || delivery.task_id !== task.id || delivery.run_id !== run.run_id
    || delivery.thread_id !== home || delivery.run_thread_id !== run.thread_id || delivery.kind !== task.kind
    || delivery.status !== 'succeeded' || delivery.finished_at_ms !== run.finished_at_ms
    || !present(source.content) || !present(delivery.message_id) || delivery.message_id === source.message_id
    || !present(run.summary) || delivery.content !== run.summary || trace.answer !== delivery.content) {
    return fail('persisted Bot delivery does not match the completed scheduled reply');
  }
  return { pass: true, detail: 'one scheduled model reply delivered to Bot' };
}
