import { LEARNING_PROGRAM_LIMITS, type LearningProgramUndoAllRequest, type LearningProgramUpdateRequest, type ProgramField, type ProgramUndo } from "../../contracts/scheduled";
import type { LearnedMark, PageView } from "../../contracts/lens";
import type { ScheduledTask } from "../../types";
import { commandError } from "./commandError";
import { mockNewestSectionOps, mockUndoMessages, recordedMessageUndo } from "./knowledge";
import type { MockScheduledTasks } from "./scheduled";
import { clone, type MockBridgeState } from "./state";

const bytes = (text: string) => new TextEncoder().encode(text).length;

/** The host words `reason` for the log; the editor shows its own line for the field. */
const refuse = (field: ProgramField, reason: string) => commandError("invalid_request", reason, { field, reason });

/** Mirrors `learning_program_update`: one validation table, the revision guard, and no wider scope than the Loupist grants. */
export async function learningProgramUpdate(scheduled: MockScheduledTasks, request: LearningProgramUpdateRequest): Promise<ScheduledTask> {
  const current = await scheduled.get({ loupist_id: request.loupistId, task_id: request.taskId });
  if (!current) throw commandError("not_found", "learning program");
  if (current.kind !== "learning" || !current.program) throw commandError("invalid_request", "Only a learning program is edited here");
  if (current.revision !== request.revision) throw commandError("stale_revision", "This program changed.");
  const { patch } = request;
  if (Object.values(patch).every(value => value === undefined || value === null)) throw commandError("invalid_request", "Change at least one field");
  if (patch.title != null && (!patch.title.trim() || bytes(patch.title) > LEARNING_PROGRAM_LIMITS.titleBytes)) throw refuse("title", "title must be 1 to 200 bytes");
  if (patch.prompt != null && (!patch.prompt.trim() || bytes(patch.prompt) > LEARNING_PROGRAM_LIMITS.briefBytes)) throw refuse("prompt", "learning briefs may be at most 8 KiB");
  if (patch.subjects && patch.subjects.length > LEARNING_PROGRAM_LIMITS.subjects) throw refuse("subjects", "a task may have at most eight subjects");
  if (patch.schedule?.kind === "weekly" && !patch.schedule.days.length) throw refuse("schedule", "a weekly schedule needs a day");
  if (patch.schedule?.kind === "once" && patch.schedule.at_ms <= Date.now()) throw refuse("schedule", "an active one-time schedule must be in the future");
  const program = { ...current.program, ...(patch.depth ? { depth: patch.depth } : {}), ...(patch.sources ? { scope: patch.sources } : {}), ...(patch.keeps ? { keeps: patch.keeps } : {}) };
  const granted = current.program.scope.connections;
  if (granted !== "all_enabled" && (program.scope.connections === "all_enabled" || program.scope.connections.some(id => !granted.includes(id)))) {
    throw refuse("sources", "A program can use only the connections this Loupist allows");
  }
  const subjects = patch.subjects ?? current.subjects ?? [];
  if (program.keeps.asset_reads && !subjects.some(subject => "asset" in subject)) throw refuse(patch.keeps ? "keeps" : "subjects", "keeping asset reads requires an asset subject");
  const next: ScheduledTask = {
    ...current,
    ...(patch.title != null ? { title: patch.title.trim() } : {}),
    ...(patch.prompt != null ? { prompt: patch.prompt } : {}),
    ...(patch.subjects ? { subjects: patch.subjects } : {}),
    ...(patch.schedule ? { schedule: patch.schedule } : {}),
    ...(patch.report ? { report: patch.report } : {}),
    ...(patch.delivery ? { delivery: patch.delivery } : {}),
    program,
    revision: current.revision + 1,
    updated_at_ms: Date.now(),
  };
  scheduled.installTask(next);
  return clone(next);
}

/** Mirrors `learning_program_undo_all`: every run newest first, in one transaction, or only the counts. */
export async function learningProgramUndoAll(state: MockBridgeState, scheduled: MockScheduledTasks, request: LearningProgramUndoAllRequest): Promise<ProgramUndo> {
  const runs = (await scheduled.taskRuns({ loupist_id: request.loupistId, task_id: request.taskId }))
    .filter(run => run.message_id && !recordedMessageUndo(state, run.message_id));
  const counts = mockUndoMessages(state, request.loupistId, runs.map(run => run.message_id!), !request.preview);
  return { runs: runs.length, ...counts };
}

/** Adds `learned` to each section whose newest op a learning run filed, as the host computes it on read. */
export async function withLearnedMarks(state: MockBridgeState, scheduled: MockScheduledTasks, loupistId: string, page: PageView): Promise<PageView> {
  if (page.body.kind === "principle") return page;
  const learning = (await scheduled.list({ loupist_id: loupistId })).filter(task => task.kind === "learning");
  const runs = new Map<string, LearnedMark>();
  for (const task of learning) {
    for (const run of await scheduled.taskRuns({ loupist_id: loupistId, task_id: task.id })) {
      if (run.message_id && !recordedMessageUndo(state, run.message_id)) runs.set(run.message_id, { atMs: run.finished_at_ms ?? run.started_at_ms, taskId: task.id, runId: run.run_id, programTitle: task.title });
    }
  }
  const newest = mockNewestSectionOps(state, loupistId, page.recordId);
  const sections = page.body.sections.map(section => {
    const { learned: _stale, ...rest } = section;
    const mark = runs.get(newest.get(section.key)?.messageId ?? "");
    return mark ? { ...rest, learned: mark } : rest;
  });
  return { ...page, body: { ...page.body, sections } };
}
