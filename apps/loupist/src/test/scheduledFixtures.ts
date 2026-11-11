import type { ScheduledRun } from "../types";

export const scheduledRunFixture = (overrides: Partial<ScheduledRun> = {}): ScheduledRun => ({
  kind: "custom", summary: "", read_at_ms: null, message_id: null, skipped_tools: [],
  id: "scheduled-run-one", task_id: "schedule-one", loupist_id: "loupist-quality", task_revision: 3, task_title: "Daily source review",
  thread_id: "scheduled-chat", run_id: "chat-scheduled-one", scheduled_for_ms: 1000, started_at_ms: 1000, status: "running",
  briefing_context: null, finished_at_ms: null, error: null,
  ...overrides,
});
