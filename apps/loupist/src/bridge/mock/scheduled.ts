import type { DeliveryRequest, FromPresetRequest, ProposalDeclineRequest, ProposalVisitRequest, TriggerReviewRequest, TriggerDryRun, TriggerEvaluation, TriggerGetRequest, TriggerRevertRequest, TriggerView } from "../../contracts/generated";
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
  /** Sessions runs made, as Rust's `scheduled-chat-` prefix marks them. */
  private made = new Set<string>();

  constructor(private state: MockBridgeState, private changed: (loupistId: string) => void = () => {}) {}

  async briefingSettingsGet({ loupistId, edition }: { loupistId: string; edition?: "morning" | "evening" | null }): Promise<BriefingSettings> {
    if (!this.state.loupists.some(loupist => loupist.id === loupistId)) throw new Error("Loupist not found.");
    const wanted = edition ?? "morning";
    const task = [...this.tasks.values()].find(task => task.loupist_id === loupistId && task.kind === "briefing" && (task.edition ?? "morning") === wanted) ?? null;
    const hour = wanted === "evening" ? 18 : 8;
    return clone({ enabled: task?.status === "active", task, schedule: task?.schedule ?? { kind: "weekly", days: [1,2,3,4,5], hour, minute: 0, timezone: Intl.DateTimeFormat().resolvedOptions().timeZone } });
  }
  async briefingSettingsSet(request: BriefingSettingsRequest): Promise<BriefingSettings> {
    const edition = request.edition ?? "morning";
    const previous = await this.briefingSettingsGet({ loupistId: request.loupistId, edition });
    const schedule = request.schedule ?? previous.schedule;
    const status = request.enabled ? "active" : "paused";
    if (previous.task) {
      const task = this.tasks.get(previous.task.id)!;
      Object.assign(task, { schedule, status, edition, revision: task.revision + 1, updated_at_ms: Date.now(), next_run_at_ms: request.enabled ? preview(schedule) : null });
    } else if (request.enabled || request.schedule) {
      const task = await this.create({ loupist_id: request.loupistId, title: edition === "evening" ? "Evening briefing" : "Briefings", prompt: "Host-authored briefing", schedule, status, target: { kind: "dedicated_chat" }, model_profile_id: "", thinking_level: "", run_options: {}, use_memory: true, update_memory: true });
      Object.assign(this.tasks.get(task.id)!, { kind: "briefing", edition });
    }
    this.changed(request.loupistId);
    return this.briefingSettingsGet({ loupistId: request.loupistId, edition });
  }
  async acceptOffer(request: { loupistId: string; offerId: string }): Promise<ScheduledTask> {
    const task = await this.fromPreset({ loupistId: request.loupistId, preset: "filings-and-results", subjects: [] });
    return task;
  }
  async dismissOffer(_request: { loupistId: string; offerId: string }) {}
  answerAsk(_request: { loupistId: string; askId: string; choice: number }) {}
  async inboxList({ loupistId, cursor, limit }: InboxRequest): Promise<InboxPage> {
    if (limit < 1 || limit > 100) throw new Error("Inbox limit must be between 1 and 100.");
    if (!this.state.loupists.some(loupist => loupist.id === loupistId)) throw new Error("Loupist not found.");
    const runs = [...this.runs.values()].filter(run => run.loupist_id === loupistId && !(run.quiet && run.status === "succeeded")).sort((a,b) => b.started_at_ms - a.started_at_ms || b.run_id.localeCompare(a.run_id));
    const start = cursor ? runs.findIndex(run => run.run_id === cursor) + 1 : 0;
    if (cursor && start === 0) throw new Error("Inbox cursor not found.");
    const page = runs.slice(start, start + limit);
    const items: InboxItem[] = page.map(run => ({ briefingContext: run.briefing_context, error: run.error, runId: run.run_id, taskId: run.task_id, kind: run.kind, title: run.task_title, status: run.status, summary: run.summary, startedAtMs: run.started_at_ms, finishedAtMs: run.finished_at_ms ?? null, readAtMs: run.read_at_ms, threadId: run.thread_id, messageId: run.message_id, skippedTools: run.skipped_tools }));
    return clone({ items, nextCursor: page.length === limit ? page.at(-1)!.run_id : null });
  }
  async inboxMarkRead({ loupistId, runIds }: { loupistId: string; runIds: string[] }) {
    if (runIds.length > 100 || runIds.some(id => this.runs.get(id)?.loupist_id !== loupistId)) throw new Error("Inbox item not found.");
    for (const id of runIds) { const run = this.runs.get(id)!; if (run.status !== "running") run.read_at_ms ??= Date.now(); }
    this.changed(loupistId);
  }
  async inboxUnreadCounts() {
    return this.state.loupists.map(loupist => ({ loupistId: loupist.id, count: [...this.runs.values()].filter(run => run.loupist_id === loupist.id && run.status !== "running" && !(run.quiet && run.status === "succeeded") && run.read_at_ms === null).length }));
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
  async confirm(request: ScheduledTaskIdentity & { expected_revision: number }) {
    const task = this.task(request);
    if (task.revision !== request.expected_revision || !task.awaiting_confirmation || task.declined_in_visit !== undefined
      || task.trigger && !task.replaces) throw new Error("This scheduled task is not waiting for confirmation.");
    Object.assign(task, { status: "active", awaiting_confirmation: false, next_run_at_ms: preview(task.schedule), revision: task.revision + 1, updated_at_ms: Date.now() });
    this.changed(task.loupist_id);
    return clone(task);
  }
  async setDeclined(request: ProposalDeclineRequest) {
    const task = this.task(request);
    if (!task.awaiting_confirmation || (task.declined_in_visit !== undefined && task.declined_in_visit !== request.visit)
      || (!request.declined && task.declined_in_visit !== request.visit)) throw new Error("This proposal cannot be changed in this visit.");
    task.declined_in_visit = request.declined ? request.visit : undefined;
    this.changed(task.loupist_id);
  }
  async endVisit(request: ProposalVisitRequest) {
    const before = this.tasks.size;
    for (const task of this.tasks.values()) {
      if (task.loupist_id === request.loupist_id && task.declined_in_visit !== undefined && task.declined_in_visit <= request.visit) this.tasks.delete(task.id);
    }
    if (this.tasks.size !== before) this.changed(request.loupist_id);
  }
  async discard(request: ScheduledTaskIdentity) {
    const task = this.task(request);
    if (!task.awaiting_confirmation) throw new Error("Only a task waiting for confirmation can be discarded here.");
    this.tasks.delete(task.id);
    this.changed(task.loupist_id);
  }
  async fromPreset(request: FromPresetRequest) {
    const subjects = request.subjects ?? [];
    const existing = [...this.tasks.values()].find(task => task.loupist_id === request.loupistId &&
      task.preset === request.preset && JSON.stringify(task.subjects) === JSON.stringify(subjects));
    if (existing) return clone(existing);
    const rows = {
      "filings-and-results": { title: "Filings and results", days: [1, 2, 3, 4, 5], hour: 18, minute: 30 },
      "thesis-lines": { title: "Thesis lines", days: [6], hour: 10, minute: 0 },
      "results-follow-up": { title: "Results follow-up", days: [], hour: 8, minute: 0 },
      "price-move": { title: "Price moves", days: [], hour: 0, minute: 0 },
      "week-in-review": { title: "Week in review", days: [5], hour: 16, minute: 0 },
    };
    const { title, days, hour, minute } = rows[request.preset];
    if (request.preset === "price-move") {
      const task = await this.create({
        loupist_id: request.loupistId, title, prompt: "Report the move.", schedule: { kind: "interval", every_minutes: 15 }, status: "paused",
        target: { kind: "new_chat" }, model_profile_id: "", thinking_level: "", run_options: {}, use_memory: true, update_memory: false,
      });
      const saved = this.tasks.get(task.id)!;
      Object.assign(saved, {
        preset: request.preset, subjects, report: "every_run", terms: "A move of more than 5% from one close to the next", awaiting_confirmation: true,
        trigger: {
          script: { name: "price-band", version: 1, sha256: "ab".repeat(32) }, summary: "A move of more than 5% from one close to the next",
          tools: [{ name: "price" }, { name: "followed" }], args: { band_percent: 5 }, state: "proposed", script_state: {},
          evaluations: [], counters: { today: { day: "", evaluations: 0, reads: 0, met: 0 }, recent: [], agent_runs_today: 0, error_streak: 0, no_data_streak: 0, backoff_ms: 0, reads_blocked: false },
          approved_versions: [], stale_after_days: 5, reads_per_check: 1,
          proposal_test: { today_met: false, today_reading: "No asset beyond the band", today_outcome: "not_met", fired_on: ["2026-11-12"], replayed: 18, unreplayed: 2, days: [{ date: "2026-11-12", outcome: "met", reading: "Northwind +6.1%" }], reads: 4 },
        },
      });
      this.changed(task.loupist_id);
      return clone(saved);
    }
    const once = request.preset === "results-follow-up";
    const schedule: ScheduledCadence = once
      ? { kind: "once", at_ms: Date.parse(`${request.params?.results_date}T08:00:00`) + 86_400_000 }
      : { kind: "weekly", days, hour, minute, timezone: Intl.DateTimeFormat().resolvedOptions().timeZone };
    const task = await this.create({
      loupist_id: request.loupistId, title, prompt: title, schedule, status: "active",
      target: { kind: "new_chat" }, model_profile_id: "", thinking_level: "", run_options: {}, use_memory: true, update_memory: true,
    });
    Object.assign(this.tasks.get(task.id)!, {
      preset: request.preset, subjects, report: once || request.preset === "week-in-review" ? "every_run" : "when_met",
      terms: title,
    });
    this.changed(task.loupist_id);
    return clone(this.tasks.get(task.id)!);
  }
  async triggerGet(request: TriggerGetRequest): Promise<TriggerView> {
    const task = this.task(request);
    const trigger = task.trigger;
    if (!trigger) throw new Error("This task has no check.");
    const version = request.version ?? trigger.pending?.version ?? trigger.script.version;
    return {
      code: "# saved check\n", summary: trigger.summary, version,
      sha256: (trigger.pending?.version === version ? trigger.pending.sha256 : trigger.script.sha256),
      dry_run: trigger.pending?.version === version ? trigger.pending.test : trigger.approved_versions.find(item => item.version === version)?.test ?? trigger.proposal_test,
      task: clone(task),
    };
  }
  async triggerApprove(request: TriggerReviewRequest) {
    const task = this.task(request);
    const trigger = task.trigger;
    const selected = trigger?.pending ?? (task.awaiting_confirmation ? trigger?.script : undefined);
    if (!trigger || task.revision !== request.expected_revision || selected?.version !== request.version
        || selected.sha256 !== request.sha256) {
      throw new Error("Turn on the version shown on the card.");
    }
    const test = trigger.pending ? trigger.pending.test : trigger.proposal_test;
    const configuration = trigger.pending ? trigger.pending.configuration : {
      args: trigger.args, tools: trigger.tools, reads_per_check: trigger.reads_per_check, active_hours: trigger.active_hours,
    };
    if (!test?.scores || !configuration) throw new Error("Test this version before turning it on.");
    Object.assign(trigger, configuration);
    trigger.approved = { version: request.version, sha256: request.sha256, at_ms: Date.now(), test, configuration };
    trigger.script = { ...trigger.script, version: request.version, sha256: request.sha256 };
    trigger.approved_versions = [...trigger.approved_versions, trigger.approved];
    trigger.pending = undefined;
    trigger.state = "on";
    task.awaiting_confirmation = false;
    task.status = "active";
    task.next_run_at_ms = Date.now() + 15 * 60_000;
    task.revision++;
    this.changed(task.loupist_id);
    return clone(task);
  }
  async triggerRevert(request: TriggerRevertRequest) {
    const task = this.task({ loupist_id: request.loupistId, task_id: request.taskId });
    const prior = task.trigger?.approved_versions.find(version => version.version === request.toVersion);
    if (!task.trigger || !prior?.configuration || !prior.test?.scores) throw new Error("Go back only to a reviewed version.");
    Object.assign(task.trigger, prior.configuration);
    task.trigger.script = { ...task.trigger.script, version: prior.version, sha256: prior.sha256 };
    task.trigger.approved = prior;
    task.trigger.pending = undefined;
    task.trigger.state = "on";
    task.revision++;
    this.changed(task.loupist_id);
    return clone(task);
  }
  async triggerTest(request: TriggerReviewRequest): Promise<TriggerEvaluation> {
    const task = this.task(request);
    const trigger = task.trigger;
    if (!trigger) throw new Error("This task has no check.");
    const selected = trigger.pending ?? trigger.script;
    if (task.revision !== request.expected_revision || task.declined_in_visit !== undefined
        || selected.version !== request.version || selected.sha256 !== request.sha256) {
      throw new Error("The check proposal changed.");
    }
    const reading = "No asset beyond the band";
    if (task.awaiting_confirmation || trigger.pending) {
      const test: TriggerDryRun = {
        today_met: false, today_reading: reading, today_outcome: "not_met", reads: 1,
        fired_on: [], replayed: 0, unreplayed: 0, days: [],
        scores: { previous_met: 0, next_met: 0, compared: 0, days: [] },
      };
      if (trigger.pending) trigger.pending.test = test;
      else trigger.proposal_test = test;
      task.revision++;
      this.changed(task.loupist_id);
    }
    return {
      at_ms: Date.now(), scheduled_for_ms: Date.now(), outcome: "not_met", reading,
      as_of: "2026-11-24", evidence: {}, version: selected.version, sha256: selected.sha256,
      reads: 1, coalesced: false, ledger: [],
    };
  }
  async triggerEvaluations(request: ScheduledTaskIdentity): Promise<TriggerEvaluation[]> {
    return clone(this.task(request).trigger?.evaluations ?? []);
  }
  async setDelivery(request: DeliveryRequest) {
    const task = this.task({ loupist_id: request.loupistId, task_id: request.taskId });
    if (task.revision !== request.expectedRevision) throw new Error("The task changed. Reload and try again.");
    task.delivery = { to: request.to ?? task.delivery?.to ?? "bot", notify: request.notify ?? task.delivery?.notify };
    task.revision++;
    this.changed(task.loupist_id);
    return clone(task);
  }
  async setStatus(request: ScheduledTaskIdentity & { expected_revision: number; status: "active" | "paused" }) {
    const task = this.task(request);
    if (task.revision !== request.expected_revision || task.awaiting_confirmation) throw new Error("The task changed. Reload and try again.");
    Object.assign(task, { status: request.status, awaiting_confirmation: false, next_run_at_ms: request.status === "active" ? preview(task.schedule) : null, revision: task.revision + 1, updated_at_ms: Date.now() });
    this.changed(task.loupist_id);
    return clone(task);
  }
  async delete(request: ScheduledTaskIdentity & { expected_revision: number }) {
    const task = this.task(request);
    if (task.kind === "briefing") throw new Error("Turn the briefing off instead of deleting it.");
    if (task.revision !== request.expected_revision) throw new Error("The task changed. Reload and try again.");
    const sessions = new Set([...this.runs.values()].filter((run) => run.task_id === task.id).map((run) => run.thread_id));
    if (task.session_thread_id) sessions.add(task.session_thread_id);
    for (const run of [...this.runs.values()]) if (run.task_id === task.id) this.runs.delete(run.run_id);
    this.tasks.delete(task.id);
    this.release(task.loupist_id, sessions);
    this.changed(task.loupist_id);
  }
  /** Mirrors Rust: a session a run made goes once nothing in Schedule points at it, unless the user wrote in it. */
  private release(loupistId: string, sessions: Iterable<string | null | undefined>) {
    for (const id of sessions) {
      if (!id || !this.made.has(id)) continue;
      if ([...this.runs.values()].some((run) => run.thread_id === id) || [...this.tasks.values()].some((task) => task.session_thread_id === id)) continue;
      const threads = this.state.threads[loupistId] ?? [];
      if (threads.find((item) => item.id === id)?.messages.some((message) => message.role === "user" && !message.scheduled)) continue;
      this.state.threads[loupistId] = threads.filter((item) => item.id !== id);
      this.made.delete(id);
    }
  }
  async taskRuns(request: ScheduledTaskIdentity) {
    this.task(request);
    return clone([...this.runs.values()].filter((run) => run.task_id === request.task_id).sort((a, b) => b.started_at_ms - a.started_at_ms));
  }
  /** Mirrors Rust: a finished run posts one short message into the home Thread. */
  deliver(run: ScheduledRun) {
    if (run.status !== "failed" && (run.status !== "succeeded" || run.quiet)) return;
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
    if (task.awaiting_confirmation) throw new Error("This scheduled task is waiting for approval.");
    if ([...this.runs.values()].some((run) => run.task_id === task.id && run.status === "running")) throw new Error("This task is already running.");
    const thread = task.target.kind === "new_chat" || (task.target.kind === "dedicated_chat" && !task.session_thread_id)
      ? await chatCreate(this.state, { loupist_id: task.loupist_id, title: task.title })
      : this.state.threads[task.loupist_id]?.find((item) => item.id === (task.target.kind === "existing_chat" ? task.target.thread_id : task.session_thread_id));
    if (thread && task.target.kind === "dedicated_chat") task.session_thread_id = thread.id;
    if (!thread) throw new Error("The target Chat is unavailable.");
    if (task.target.kind !== "existing_chat") this.made.add(thread.id);
    const now = Date.now();
    const run: ScheduledRun = { quiet: false, briefing_context: task.kind === "briefing" ? { symbols: [], principlesChecked: 0, newFacts: 0, disputedEntityIds: [], changed: { names: 0, relations: 0, conflicts: 0, rows: [] }, ahead: [] } : null, kind: task.kind, summary: "", read_at_ms: null, message_id: null, skipped_tools: [], id: makeId("scheduled-run"), task_id: task.id, loupist_id: task.loupist_id, task_revision: task.revision, task_title: task.title, thread_id: thread.id, run_id: makeId("chat-scheduled"), scheduled_for_ms: now, started_at_ms: now, status: "running", finished_at_ms: null, error: null };
    this.runs.set(run.run_id, run);
    // As in Rust, history keeps 50 runs per task and a run leaving it takes its session along.
    const pruned = [...this.runs.values()].filter((item) => item.task_id === task.id).sort((a, b) => b.started_at_ms - a.started_at_ms).slice(50);
    for (const old of pruned) this.runs.delete(old.run_id);
    this.release(task.loupist_id, pruned.map((old) => old.thread_id));
    this.events.set(run.run_id, []);
    task.last_run_at_ms = now;
    task.updated_at_ms = now;
    try {
      await send({ run_id: run.run_id, loupist_id: task.loupist_id, thread_id: thread.id, prompt: task.prompt, use_memory: task.use_memory, update_memory: task.update_memory, model_profile_id: task.model_profile_id, thinking_level: task.thinking_level, run_options: task.run_options, attachments: [] }, (event) => {
        this.events.get(run.run_id)?.push(clone(event));
        // Mock Chat saves its user message just after started, before the first progress event.
        if (event.type !== "started" && !this.baselines.has(run.run_id)) {
          const saved = this.state.threads[task.loupist_id]!.find((item) => item.id === thread.id)!;
          // As in Rust, the request names the run that sent it.
          const request = [...saved.messages].reverse().find((message) => message.role === "user");
          if (request) request.scheduled = { task_id: task.id, run_id: run.run_id, task_title: task.title, task_kind: task.kind };
          this.baselines.set(run.run_id, clone(saved));
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
