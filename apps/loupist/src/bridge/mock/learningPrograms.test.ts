import type { PageView } from "../../contracts/lens";
import type { ScheduledRun, ScheduledTask } from "../../types";
import { MockAssets } from "./assets";
import { knowledgeLensRead, memoryMessageUndo, mockFileSection } from "./knowledge";
import { learningProgramUndoAll, learningProgramUpdate, withLearnedMarks } from "./learningPrograms";
import { MockScheduledTasks } from "./scheduled";
import { createMockBridgeState } from "./state";

const LOUPIST = "loupist-quality";
const PAGE = "lens-subject-tcp";

const program = (patch: Partial<ScheduledTask> = {}): ScheduledTask => ({
  id: "learn-kyushu", loupist_id: LOUPIST, kind: "learning", title: "Kyushu port throughput", prompt: "Monthly container volumes at the Kyushu ports.",
  schedule: { kind: "weekly", days: [1, 3, 5], hour: 7, minute: 15, timezone: "Asia/Tokyo" }, target: { kind: "dedicated_chat" },
  model_profile_id: "", thinking_level: "", run_options: {}, use_memory: true, update_memory: true, status: "active", revision: 4,
  next_run_at_ms: null, last_run_at_ms: null, created_at_ms: 0, updated_at_ms: 0, last_error: null, session_thread_id: null, awaiting_confirmation: false,
  program: { depth: "standard", scope: { connections: ["fixture.port-stats", "fixture.customs"], web: true }, keeps: { sources: true, lens: true, asset_reads: false } },
  subjects: [{ name: "Kyushu ports" }], report: "when_new", delivery: { to: "bot" },
  ...patch,
});

const run = (id: string, messageId: string, at: number): ScheduledRun => ({
  quiet: false, briefing_context: null, kind: "learning", summary: "", read_at_ms: null, message_id: messageId, skipped_tools: [], id, task_id: "learn-kyushu",
  loupist_id: LOUPIST, task_revision: 4, task_title: "Kyushu port throughput", thread_id: null, run_id: id, scheduled_for_ms: at,
  started_at_ms: at, finished_at_ms: at + 1, status: "succeeded", error: null,
});

function setup() {
  const state = createMockBridgeState(0);
  const assets = new MockAssets(() => [], () => {}, () => state.loupists.map(loupist => loupist.id));
  const scheduled = new MockScheduledTasks(state, assets);
  return { state, scheduled };
}

let clock = Date.parse("2026-11-02T00:00:00Z");
beforeEach(() => { vi.useFakeTimers({ toFake: ["Date"] }); });
afterEach(() => { vi.useRealTimers(); });

/** Files one section a second after the last, as the host's ops are ordered. */
function file(state: ReturnType<typeof createMockBridgeState>, key: string, markdown: string, messageId: string) {
  clock += 1_000;
  vi.setSystemTime(clock);
  return mockFileSection(state, LOUPIST, PAGE, { key, heading: key, markdown, messageId });
}

const sectionText = (page: PageView, key: string) => page.body.kind === "principle" ? undefined : page.body.sections.find(section => section.key === key)?.markdown;

it("refuses a stale revision, a wider scope and an over-long brief, and saves a valid edit", async () => {
  const { scheduled } = setup();
  scheduled.installTask(program());
  const request = { loupistId: LOUPIST, taskId: "learn-kyushu" };
  await expect(learningProgramUpdate(scheduled, { ...request, revision: 3, patch: { title: "Older" } })).rejects.toMatchObject({ code: "stale_revision" });
  await expect(learningProgramUpdate(scheduled, { ...request, revision: 4, patch: { sources: { connections: "all_enabled", web: true } } }))
    .rejects.toMatchObject({ code: "invalid_request", detail: { field: "sources", reason: "A program can use only the connections this Loupist allows" } });
  await expect(learningProgramUpdate(scheduled, { ...request, revision: 4, patch: { prompt: "港".repeat(2731) } }))
    .rejects.toMatchObject({ code: "invalid_request", detail: { field: "prompt" } });
  const saved = await learningProgramUpdate(scheduled, { ...request, revision: 4, patch: { depth: "deep", schedule: { kind: "daily", hour: 18, minute: 45, timezone: "Asia/Tokyo" } } });
  expect(saved).toMatchObject({ revision: 5, program: { depth: "deep" }, schedule: { kind: "daily", hour: 18, minute: 45 } });
  expect(await scheduled.get({ loupist_id: LOUPIST, task_id: "learn-kyushu" })).toMatchObject({ revision: 5, program: { depth: "deep" } });
});

it("undoes every run newest first in one step, counts a section changed since once, and is idempotent", async () => {
  const { state, scheduled } = setup();
  await knowledgeLensRead(state, { loupistId: LOUPIST, recordId: PAGE });
  const original = sectionText(await knowledgeLensRead(state, { loupistId: LOUPIST, recordId: PAGE }), "overview");
  file(state, "overview", "Throughput rose in October.", "msg-run-1");
  file(state, "volumes", "Volumes by port.", "msg-run-1");
  file(state, "overview", "Throughput rose again in November.", "msg-run-2");
  file(state, "volumes", "A reader's own note.", "msg-user");
  scheduled.installTask(program(), [run("run-1", "msg-run-1", 1_000), run("run-2", "msg-run-2", 2_000)]);
  const request = { loupistId: LOUPIST, taskId: "learn-kyushu" };

  const preview = await learningProgramUndoAll(state, scheduled, { ...request, preview: true });
  expect(preview).toEqual({ runs: 2, undone: 2, kept: 1 });
  expect(sectionText(await knowledgeLensRead(state, { loupistId: LOUPIST, recordId: PAGE }), "overview")).toBe("Throughput rose again in November.");

  expect(await learningProgramUndoAll(state, scheduled, request)).toEqual(preview);
  const after = await knowledgeLensRead(state, { loupistId: LOUPIST, recordId: PAGE });
  expect(sectionText(after, "overview")).toBe(original);
  expect(sectionText(after, "volumes")).toBe("A reader's own note.");
  expect(await learningProgramUndoAll(state, scheduled, request)).toEqual({ runs: 0, undone: 0, kept: 0 });
});

it("skips a run already undone on its own", async () => {
  const { state, scheduled } = setup();
  file(state, "outlook", "Exportaciones de vino de Rioja al alza.", "msg-rioja-1");
  file(state, "prices", "Precios estables.", "msg-rioja-2");
  scheduled.installTask(program(), [run("run-1", "msg-rioja-1", 1_000), run("run-2", "msg-rioja-2", 2_000)]);
  await memoryMessageUndo(state, { loupistId: LOUPIST, messageId: "msg-rioja-2" });
  expect(await learningProgramUndoAll(state, scheduled, { loupistId: LOUPIST, taskId: "learn-kyushu" })).toEqual({ runs: 1, undone: 1, kept: 0 });
});

it("marks a section a learning run filed until a later op touches it", async () => {
  const { state, scheduled } = setup();
  file(state, "exports", "반도체 수출이 증가했습니다.", "msg-run-1");
  scheduled.installTask(program(), [run("run-1", "msg-run-1", 5_000)]);
  const marked = await withLearnedMarks(state, scheduled, LOUPIST, await knowledgeLensRead(state, { loupistId: LOUPIST, recordId: PAGE }));
  const section = marked.body.kind === "principle" ? undefined : marked.body.sections.find(item => item.key === "exports");
  expect(section?.learned).toEqual({ atMs: 5_001, taskId: "learn-kyushu", runId: "run-1", programTitle: "Kyushu port throughput" });
  file(state, "exports", "A reader's correction.", "msg-user");
  const edited = await withLearnedMarks(state, scheduled, LOUPIST, await knowledgeLensRead(state, { loupistId: LOUPIST, recordId: PAGE }));
  expect(edited.body.kind === "principle" ? undefined : edited.body.sections.find(item => item.key === "exports")?.learned).toBeUndefined();
});
