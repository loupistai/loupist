import test from 'node:test';
import assert from 'node:assert/strict';
import { scheduleStateEvidence } from './grade.mjs';

test('schedule grading uses persisted state, never a successful-sounding answer', () => {
  const expected = { count: 1, tasks: [{ title: 'Review', target: { kind: 'dedicated_chat' }, schedule: { timezone: 'Asia/Seoul', hour: 9 }, use_memory: false }] };
  assert.equal(scheduleStateEvidence(expected, { answer: 'Scheduled successfully.' }).pass, null);
  assert.equal(scheduleStateEvidence(expected, { scheduled_tasks: [], answer: 'Scheduled successfully.' }).pass, false);
  const task = { task_id: 'a', status: 'active', title: 'Review', target: { kind: 'dedicated_chat' }, schedule: { kind: 'daily', timezone: 'Asia/Seoul', hour: 9, minute: 0 }, use_memory: false };
  assert.equal(scheduleStateEvidence(expected, { scheduled_tasks: [task] }).pass, true);
  assert.equal(scheduleStateEvidence(expected, { scheduled_tasks: [{ ...task, schedule: { ...task.schedule, timezone: 'UTC' } }] }).pass, false);
  assert.equal(scheduleStateEvidence(expected, { scheduled_tasks: [task, { ...task, task_id: 'b' }] }).pass, false);
});

test('several expectations must match distinct saved tasks without relying on list order', () => {
  const tasks = ['a', 'b'].map((task_id) => ({ task_id, title: task_id, status: 'paused' }));
  assert.equal(scheduleStateEvidence({ count: 2, tasks: [{ title: 'b' }, { title: 'a' }] }, { scheduled_tasks: tasks }).pass, true);
  assert.equal(scheduleStateEvidence({ count: 2, tasks: [{ title: 'a' }, { title: 'a' }] }, { scheduled_tasks: tasks }).pass, false);
  assert.equal(scheduleStateEvidence({ count: 2 }, { scheduled_tasks: [tasks[0], tasks[0]] }).pass, false);
  assert.equal(scheduleStateEvidence({ count: 2, tasks: [{ status: 'paused' }, { title: 'a' }] }, { scheduled_tasks: tasks }).pass, true);
});

test('saved instructions must retain the requested input, not just a matching title', () => {
  const expected = { count: 1, tasks: [{ title: 'Review', prompt_includes: ['morning.md'] }] };
  const task = { task_id: 'a', title: 'Review', status: 'active', prompt: 'Summarize morning.md' };
  assert.equal(scheduleStateEvidence(expected, { scheduled_tasks: [task] }).pass, true);
  assert.equal(scheduleStateEvidence(expected, { scheduled_tasks: [{ ...task, prompt: 'Summarize afternoon.md' }] }).pass, false);
});
