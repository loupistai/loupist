import type {
  BriefingSettings, BriefingSettingsRequest, InboxRequest, InboxPage, InboxItem,
  ChatSendRequest, ChatStreamEvent, ScheduledCadence, ScheduledRun, ScheduledTask, ScheduledTaskListItem,
  ScheduledTaskCreateRequest, ScheduledTaskIdentity, ScheduledRunIdentity, ScheduledTaskWatch, StreamObserver,
} from "../../types";
import { clone, makeId, type MockBridgeState } from "./state";
import { chatCreate, ensureHomeThread } from "./loupistChat";

function preview(schedule: ScheduledCadence): number | null {
  const now = Date.now();
  if (schedule.kind === "once") return schedule.at_ms > now ? schedule.at_ms : null;
  if (schedule.kind === "interval") return now + schedule.every_minutes * 60_000;
  const format = new Intl.DateTimeFormat("en-US", { timeZone: schedule.timezone, hour: "2-digit", minute: "2-digit", weekday: "short", hourCycle: "h23" });
  for (let time = Math.floor(now / 60_000) * 60_000 + 60_000; time <= now + 8 * 86_400_000; time += 60_000) {
    const parts = Object.fromEntries(format.formatToParts(time).map((part) => [part.type, part.value]));
    if (Number(parts.hour) === schedule.hour && Number(parts.minute) === schedule.minute &&
      (schedule.kind === "daily" || schedule.days.includes(["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"].indexOf(parts.weekday)))) return time;
  }
  return null;
}

/** In-memory browser preview. The desktop scheduler and its clock live in Rust. */
export class MockScheduledTasks {
  private tasks = new Map<string, ScheduledTask>();
  private runs = new Map<string, ScheduledRun>();
  private events = new Map<string, ChatStreamEvent[]>();
  private observers = new Map<string, Set<StreamObserver<ChatStreamEvent>>>();
  private baselines = new Map<string, ScheduledTaskWatch["thread"]>();

  constructor(private state: MockBridgeState, private changed: (loupistId: string) => void = () => {}) {}

  async briefingSettingsGet({ loupistId }: { loupistId: string }): Promise<BriefingSettings> {
    if (!this.state.loupists.some(loupist => loupist.id === loupistId)) throw new Error("Loupist not found.");
    const task = [...this.tasks.values()].find(task => task.loupist_id === loupistId && task.kind === "briefing") ?? null;
    return clone({ enabled: task?.status === "active", task, schedule: task?.schedule ?? { kind: "weekly", days: [1,2,3,4,5], hour: 8, minute: 0, timezone: Intl.DateTimeFormat().resolvedOptions().timeZone } });
  }
  async briefingSettingsSet(request: BriefingSettingsRequest): Promise<BriefingSettings> {
    const previous = await this.briefingSettingsGet(request);
    const schedule = request.schedule ?? previous.schedule;
    const status = request.enabled ? "active" : "paused";
    if (previous.task) {
      const task = this.tasks.get(previous.task.id)!;
      Object.assign(task, { schedule, status, revision: task.revision + 1, updated_at_ms: Date.now(), next_run_at_ms: request.enabled ? preview(schedule) : null });
    } else if (request.enabled || request.schedule) {
      const task = await this.create({ loupist_id: request.loupistId, title: "Briefings", prompt: "Host-authored briefing", schedule, status, target: { kind: "dedicated_chat" }, model_profile_id: "model-test", thinking_level: "medium", run_options: {}, use_memory: true, update_memory: true });
      this.tasks.get(task.id)!.kind = "briefing";
    }
    this.changed(request.loupistId);
    return this.briefingSettingsGet(request);
  }
  async inboxList({ loupistId, cursor, limit }: InboxRequest): Promise<InboxPage> {
    if (limit < 1 || limit > 100) throw new Error("Inbox limit must be between 1 and 100.");
    if (!this.state.loupists.some(loupist => loupist.id === loupistId)) throw new Error("Loupist not found.");
    const runs = [...this.runs.values()].filter(run => run.loupist_id === loupistId).sort((a,b) => b.started_at_ms - a.started_at_ms || b.run_id.localeCompare(a.run_id));
    const start = cursor ? runs.findIndex(run => run.run_id === cursor) + 1 : 0;
    if (cursor && start === 0) throw new Error("Inbox cursor not found.");
    const page = runs.slice(start, start + limit);
    const items: InboxItem[] = page.map(run => ({ briefingContext: run.briefing_context, error: run.error, runId: run.run_id, taskId: run.task_id, kind: run.kind, title: run.task_title, status: run.status, summary: run.summary, summaryItems: run.summary_items ?? [], startedAtMs: run.started_at_ms, finishedAtMs: run.finished_at_ms ?? null, readAtMs: run.read_at_ms, threadId: run.thread_id, messageId: run.message_id, skippedTools: run.skipped_tools }));
    return clone({ items, nextCursor: page.length === limit ? page.at(-1)!.run_id : null });
  }
  async inboxMarkRead({ loupistId, runIds }: { loupistId: string; runIds: string[] }) {
    if (runIds.length > 100 || runIds.some(id => this.runs.get(id)?.loupist_id !== loupistId)) throw new Error("Inbox item not found.");
    for (const id of runIds) { const run = this.runs.get(id)!; if (run.status !== "running") run.read_at_ms ??= Date.now(); }
    this.changed(loupistId);
  }
  async inboxUnreadCounts() {
    return this.state.loupists.map(loupist => ({ loupistId: loupist.id, count: [...this.runs.values()].filter(run => run.loupist_id === loupist.id && run.status !== "running" && run.read_at_ms === null).length }));
  }

  private task(request: ScheduledTaskIdentity) {
    const task = this.tasks.get(request.task_id);
    if (!task || task.loupist_id !== request.loupist_id) throw new Error("Scheduled task not found.");
    return task;
  }
  async list({ loupist_id }: { loupist_id: string }): Promise<ScheduledTaskListItem[]> {
    const latest = new Map<string, ScheduledRun>();
    for (const run of this.runs.values()) {
      if (run.loupist_id === loupist_id && (!latest.has(run.task_id) || latest.get(run.task_id)!.started_at_ms <= run.started_at_ms)) latest.set(run.task_id, run);
    }
    return clone([...this.tasks.values()].filter((task) => task.loupist_id === loupist_id).map((task) => ({ ...task, latest_run: latest.get(task.id) ?? null })));
  }
  private async create(request: ScheduledTaskCreateRequest) {
    const now = Date.now();
    const task: ScheduledTask = { ...request, kind: "custom", id: makeId("schedule"), revision: 1, created_at_ms: now, updated_at_ms: now, next_run_at_ms: request.status === "active" ? preview(request.schedule) : null, last_run_at_ms: null, last_error: null, session_thread_id: null, awaiting_confirmation: false };
    this.tasks.set(task.id, task);
    return clone(task);
  }
  /** Preview and test helper: a task Rust created (the automatic results task) with its past runs. */
  installTask(task: ScheduledTask, runs: ScheduledRun[] = []) {
    this.tasks.set(task.id, clone({ ...task, latest_run: undefined }));
    for (const run of runs) this.runs.set(run.run_id, clone(run));
    this.changed(task.loupist_id);
  }
  /** A task the Loupist proposed in conversation: saved paused until confirmed. */
  async propose(request: ScheduledTaskCreateRequest) {
    const task = await this.create({ ...request, status: "paused" });
    this.tasks.get(task.id)!.awaiting_confirmation = true;
    return clone(this.tasks.get(task.id)!);
  }
  async get(request: ScheduledTaskIdentity) {
    const task = this.tasks.get(request.task_id);
    return task && task.loupist_id === request.loupist_id ? clone(task) : null;
  }
  async confirm(request: ScheduledTaskIdentity) {
    const task = this.task(request);
    if (!task.awaiting_confirmation) throw new Error("This scheduled task is not waiting for confirmation.");
    Object.assign(task, { status: "active", awaiting_confirmation: false, next_run_at_ms: preview(task.schedule), revision: task.revision + 1, updated_at_ms: Date.now() });
    this.changed(task.loupist_id);
    return clone(task);
  }
  async discard(request: ScheduledTaskIdentity) {
    const task = this.task(request);
    if (!task.awaiting_confirmation) throw new Error("Only a task waiting for confirmation can be discarded here.");
    this.tasks.delete(task.id);
    this.changed(task.loupist_id);
  }
  async setStatus(request: ScheduledTaskIdentity & { expected_revision: number; status: "active" | "paused" }) {
    const task = this.task(request);
    if (task.revision !== request.expected_revision) throw new Error("The task changed. Reload and try again.");
    Object.assign(task, { status: request.status, awaiting_confirmation: false, next_run_at_ms: request.status === "active" ? preview(task.schedule) : null, revision: task.revision + 1, updated_at_ms: Date.now() });
    this.changed(task.loupist_id);
    return clone(task);
  }
  async delete(request: ScheduledTaskIdentity & { expected_revision: number }) {
    const task = this.task(request);
    if (task.kind === "briefing") throw new Error("Turn the briefing off instead of deleting it.");
    if (task.kind === "earnings") throw new Error("The results task can be paused, not deleted.");
    if (task.revision !== request.expected_revision) throw new Error("The task changed. Reload and try again.");
    this.tasks.delete(task.id);
    this.changed(task.loupist_id);
  }
  async taskRuns(request: ScheduledTaskIdentity) {
    this.task(request);
    return clone([...this.runs.values()].filter((run) => run.task_id === request.task_id).sort((a, b) => b.started_at_ms - a.started_at_ms));
  }
  /** Mirrors Rust: a finished run posts one short message into the home Thread. */
  deliver(run: ScheduledRun) {
    if (run.status !== "succeeded" && run.status !== "failed") return;
    const home = ensureHomeThread(this.state, run.loupist_id);
    if (run.thread_id === home.id || home.messages.some((message) => message.delivery?.run_id === run.run_id)) return;
    const task = this.tasks.get(run.task_id);
    const finished = run.finished_at_ms ?? run.started_at_ms;
    home.messages.push({
      id: makeId("delivery"), role: "assistant", status: "complete", host_composed: true, created_at: new Date(finished).toISOString(),
      content: run.status === "succeeded" ? run.summary || `${run.task_title} finished.` : `${run.task_title} could not finish.`,
      delivery: { run_id: run.run_id, task_id: run.task_id, task_title: run.task_title, task_kind: run.kind, schedule: task ? clone(task.schedule) : null, run_thread_id: run.thread_id ?? null, status: run.status, finished_at_ms: finished },
    });
    home.updated_at = new Date(finished).toISOString();
  }
  async activity({ loupist_id }: { loupist_id: string }) {
    return (await this.list({ loupist_id })).flatMap((task) => task.latest_run ? [task.latest_run] : []);
  }

  async runNow(request: ScheduledTaskIdentity, send: (request: ChatSendRequest, observer: StreamObserver<ChatStreamEvent>) => Promise<{ run_id: string }>) {
    const task = this.task(request);
    if ([...this.runs.values()].some((run) => run.task_id === task.id && run.status === "running")) throw new Error("This task is already running.");
    if (task.kind === "earnings") {
      // The results task needs no model call: a host capture finishes in place.
      const now = Date.now();
      const run: ScheduledRun = { briefing_context: null, kind: "earnings", summary: "", summary_items: [{ kind: "earnings_nothing" }], read_at_ms: now, message_id: null, skipped_tools: [], id: makeId("scheduled-run"), task_id: task.id, loupist_id: task.loupist_id, task_revision: task.revision, task_title: task.title, thread_id: null, run_id: makeId("earnings-run"), scheduled_for_ms: now, started_at_ms: now, finished_at_ms: now, status: "succeeded", error: null };
      this.runs.set(run.run_id, run);
      task.last_run_at_ms = now;
      this.changed(task.loupist_id);
      return clone(run);
    }
    const thread = task.target.kind === "new_chat" || (task.target.kind === "dedicated_chat" && !task.session_thread_id)
      ? await chatCreate(this.state, { loupist_id: task.loupist_id, title: task.title })
      : this.state.threads[task.loupist_id]?.find((item) => item.id === (task.target.kind === "existing_chat" ? task.target.thread_id : task.session_thread_id));
    if (thread && task.target.kind === "dedicated_chat") task.session_thread_id = thread.id;
    if (!thread) throw new Error("The target Chat is unavailable.");
    const now = Date.now();
    const run: ScheduledRun = { briefing_context: task.kind === "briefing" ? { symbols: [], principlesChecked: 0, newFacts: 0, disputedEntityIds: [], changed: { names: 0, relations: 0, conflicts: 0, rows: [] }, ahead: [], assets: [] } : null, kind: task.kind, summary: "", read_at_ms: null, message_id: null, skipped_tools: [], id: makeId("scheduled-run"), task_id: task.id, loupist_id: task.loupist_id, task_revision: task.revision, task_title: task.title, thread_id: thread.id, run_id: makeId("chat-scheduled"), scheduled_for_ms: now, started_at_ms: now, status: "running", finished_at_ms: null, error: null };
    this.runs.set(run.run_id, run);
    this.events.set(run.run_id, []);
    task.last_run_at_ms = now;
    task.updated_at_ms = now;
    try {
      await send({ run_id: run.run_id, loupist_id: task.loupist_id, thread_id: thread.id, prompt: task.prompt, use_memory: task.use_memory, update_memory: task.update_memory, model_profile_id: task.model_profile_id, thinking_level: task.thinking_level, run_options: task.run_options, attachments: [] }, (event) => {
        this.events.get(run.run_id)?.push(clone(event));
        // Mock Chat saves its user message just after started, before the first progress event.
        if (event.type !== "started" && !this.baselines.has(run.run_id)) {
          this.baselines.set(run.run_id, clone(this.state.threads[task.loupist_id]!.find((item) => item.id === thread.id)!));
        }
        for (const observer of this.observers.get(run.run_id) ?? []) observer(clone(event));
        if (event.type === "completed" || event.type === "error" || event.type === "aborted") {
          run.status = event.type === "completed" ? "succeeded" : event.type === "aborted" ? "interrupted" : "failed";
          run.finished_at_ms = Date.now();
          run.error = event.type === "error" ? event.message : null;
          if (event.type === "completed") { run.summary = event.final_text.split(/\n\s*\n/)[0].replace(/[*_`#]/g, "").slice(0, 280); run.message_id = event.message_id; }
          this.deliver(run);
          this.changed(task.loupist_id);
          task.last_error = run.error;
          task.updated_at_ms = run.finished_at_ms;
          this.observers.delete(run.run_id);
        }
      });
    } catch (cause) {
      run.status = "failed";
      run.error = cause instanceof Error ? cause.message : String(cause);
      run.finished_at_ms = Date.now();
      task.last_error = run.error;
      throw cause;
    }
    return clone(run);
  }
  async watch(request: ScheduledRunIdentity, observer: StreamObserver<ChatStreamEvent>): Promise<ScheduledTaskWatch | null> {
    const run = this.runs.get(request.run_id);
    if (!run || run.loupist_id !== request.loupist_id || run.task_id !== request.task_id) return null;
    const thread = this.baselines.get(run.run_id);
    if (!thread) return null;
    const listeners = this.observers.get(run.run_id) ?? new Set();
    if (run.status === "running") { listeners.add(observer); this.observers.set(run.run_id, listeners); }
    for (const event of this.events.get(run.run_id) ?? []) observer(clone(event));
    return clone({ run, thread });
  }
}
