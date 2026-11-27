import type { DeliveryTarget, LearningConnections, LearningDepth, LearningKeeps, ReportPolicy, Schedule, TaskSubject } from "../../contracts/generated";
import { LEARNING_PROGRAM_LIMITS, type LearningProgramPatch, type ProgramField } from "../../contracts/scheduled";
import type { ScheduledTask } from "../../types";
import { commandErrorCode } from "../../lib/errors";

/** Everything the editor holds, one value per field the user sees. */
export type ProgramDraft = {
  title: string;
  prompt: string;
  subjects: TaskSubject[];
  schedule: Schedule;
  depth: LearningDepth;
  connections: LearningConnections;
  web: boolean;
  keeps: LearningKeeps;
  report: ReportPolicy;
  delivery: DeliveryTarget;
};

/** Why a field is not ready: what the editor checks itself, or `refused` when the host named the field. */
export type FieldProblem = "required" | "too_long" | "too_many" | "invalid" | "refused";
export type FieldProblems = Partial<Record<ProgramField, FieldProblem>>;

const DEFAULT_PROGRAM = { depth: "standard", scope: { connections: "all_enabled", web: true }, keeps: { sources: true, lens: true, asset_reads: false } } as const;

export function programDraft(task: ScheduledTask): ProgramDraft {
  const program = task.program ?? DEFAULT_PROGRAM;
  return {
    title: task.title,
    prompt: task.prompt,
    subjects: [...(task.subjects ?? [])],
    schedule: task.schedule,
    depth: program.depth,
    connections: program.scope.connections === "all_enabled" ? "all_enabled" : [...program.scope.connections],
    web: program.scope.web,
    keeps: { ...program.keeps },
    report: task.report ?? "when_new",
    delivery: task.delivery?.to ?? "bot",
  };
}

const same = (left: unknown, right: unknown) => JSON.stringify(left) === JSON.stringify(right);

/** Only the fields that differ from the stored task; an empty patch means the editor is unchanged. */
export function programPatch(task: ScheduledTask, draft: ProgramDraft): LearningProgramPatch {
  const stored = programDraft(task);
  const patch: LearningProgramPatch = {};
  if (draft.title.trim() !== stored.title.trim()) patch.title = draft.title.trim();
  if (draft.prompt !== stored.prompt) patch.prompt = draft.prompt;
  if (!same(draft.subjects, stored.subjects)) patch.subjects = draft.subjects;
  if (!same(draft.schedule, stored.schedule)) patch.schedule = draft.schedule;
  if (draft.depth !== stored.depth) patch.depth = draft.depth;
  if (draft.web !== stored.web || !same(draft.connections, stored.connections)) patch.sources = { connections: draft.connections, web: draft.web };
  if (!same(draft.keeps, stored.keeps)) patch.keeps = draft.keeps;
  if (draft.report !== stored.report) patch.report = draft.report;
  if (draft.delivery !== stored.delivery) patch.delivery = { ...task.delivery, to: draft.delivery };
  return patch;
}

export const patchIsEmpty = (patch: LearningProgramPatch) => Object.keys(patch).length === 0;

const utf8Bytes = (text: string) => new TextEncoder().encode(text).length;

/** Problems the editor can see without the host. The host checks the same table again on Save. */
export function draftProblems(draft: ProgramDraft): FieldProblems {
  const problems: FieldProblems = {};
  if (!draft.title.trim()) problems.title = "required";
  else if (utf8Bytes(draft.title.trim()) > LEARNING_PROGRAM_LIMITS.titleBytes) problems.title = "too_long";
  else if (/\p{Cc}/u.test(draft.title)) problems.title = "invalid";
  if (!draft.prompt.trim()) problems.prompt = "required";
  else if (utf8Bytes(draft.prompt) > LEARNING_PROGRAM_LIMITS.briefBytes) problems.prompt = "too_long";
  if (draft.subjects.length > LEARNING_PROGRAM_LIMITS.subjects) problems.subjects = "too_many";
  if (draft.schedule.kind === "weekly" && draft.schedule.days.length === 0) problems.schedule = "required";
  if (draft.keeps.asset_reads && !draft.subjects.some(subject => "asset" in subject)) problems.keeps = "invalid";
  return problems;
}

export const briefBytes = utf8Bytes;

const FIELDS: readonly ProgramField[] = ["title", "prompt", "subjects", "schedule", "depth", "sources", "keeps", "report", "delivery"];

/**
 * How a refused Save reads: the program changed under the editor, a field the host named, or neither.
 * The host's `reason` is English for the log; the editor shows its own line for the field.
 */
export function saveRefusal(cause: unknown): { kind: "stale" } | { kind: "field"; field: ProgramField } | { kind: "other" } {
  const code = commandErrorCode(cause);
  if (code === "stale_revision") return { kind: "stale" };
  const detail = cause && typeof cause === "object" && "detail" in cause ? (cause as { detail?: unknown }).detail : undefined;
  if (code === "invalid_request" && detail && typeof detail === "object" && "field" in detail) {
    const field = (detail as { field?: unknown }).field;
    if (typeof field === "string" && (FIELDS as readonly string[]).includes(field)) {
      return { kind: "field", field: field as ProgramField };
    }
  }
  return { kind: "other" };
}
