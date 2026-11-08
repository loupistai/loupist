import type { InboxItem, InboxRequest, InboxPage, ScheduledRun, ScheduledTask } from "../contracts/scheduled";
import { MockWorldSchema } from "../bridge/mock/worldSchema";
import { MockLoupistBridge } from "../bridge/mockBridge";
import { mockAssetId } from "../bridge/mock/assets";
import { previewLearned, previewPages } from "./pages";
import { MIN_VISIT_MS, recordVisit } from "../app/loupistVisit";
import type { ChatMessage, ChatSendRequest, ChatStreamEvent, GraphEdge, GraphNode, KnowledgeCatalog, KnowledgeLensReadRequest, LearnedDigest, LensKind, PageSummary, PageView, StreamObserver, WorldGraphQuery } from "../types";

export const previewKo = () => typeof document !== "undefined" && document.documentElement.lang.startsWith("ko");

/** The preview's frozen "now"; the capture script also freezes the page clock to it before any module runs. */
export const FIXTURE_TIME = Date.parse("2026-10-28T00:00:00Z");
const LONG_TITLE = "Hanul Precision — 2026 third-quarter operating review: capacity expansion, customer concentration and the cash conversion cycle";
const METHOD_TITLE = "Separate durable margins from temporary capacity costs — a research method for Hanul Precision and its overseas suppliers";
const TASK_TITLE = "Morning review · Hanul Precision filings";
const KOREAN_TASK_TITLE = "아침 검토 · 한울정밀 공시";
const revision = { revisionId: "preview-revision", revisionDigest: "a".repeat(64), readOnly: false, indexingState: "ready" as const };
const sourceTitles = [LONG_TITLE, "Hanul Precision 2026 third-quarter results presentation", "Morrow Components · supplier capacity survey", "Bluehaven Logistics — freight and inventory update", "Eunbit Materials: new plant utilization and orders", "Cedar Robotics · customer demand outlook"];
const previewCatalog: KnowledgeCatalog = {
  loupistId: "loupist-quality",
  worlds: [{ ...revision, recordId: "preview-world", kind: "world", title: "Industrial supply chain", objects: [] }],
  lenses: [
    { ...revision, recordId: "preview-perspective", kind: "lens", lensKind: "perspective", title: "Evidence before conviction", objects: [] },
    { ...revision, recordId: "preview-method", kind: "lens", lensKind: "method", title: METHOD_TITLE, summary: "Compare margins, utilization and cash conversion before interpreting a single quarter.", updatedAtMs: FIXTURE_TIME, objects: [] },
    { ...revision, recordId: "preview-subject", kind: "lens", lensKind: "subject", title: "Hanul Precision", summary: "A precision-parts maker adding capacity while margins thin.", updatedAtMs: FIXTURE_TIME - 3_600_000, learned: true, objects: [] },
    { ...revision, recordId: "preview-idea", kind: "lens", lensKind: "idea", title: "Margins recover once utilization passes 80%", summary: "An unverified link: fixed costs dilute as the new line fills.", updatedAtMs: FIXTURE_TIME - 3_000_000, learned: true, objects: [] },
  ],
  sources: [{ ...revision, recordId: "preview-sources", kind: "source", title: "Company filings and field notes", objects: sourceTitles.map((title, i) => ({ id: `preview-source-${i}`, title, summary: i === 0 ? "Revenue ₩184.2bn, operating margin 14.8%; new capacity enters qualification." : "Fictional research material for layout review.", originLabel: i % 2 ? "Hanul Research" : "Company investor relations", acquiredAt: { precision: "instant", value: `2026-10-${28 - i}T00:00:00Z` }, publicationTime: { precision: "date", value: `2026-10-${27 - i}` }, processing: { status: i === 1 ? "running" : "succeeded" } })) }],
};
const companies = ["Hanul Precision", "Morrow Components", "Bluehaven Logistics", "Cedar Robotics", "Eunbit Materials", "Northstar Foundry", "Orchard Sensors", "Dawn Circuits"];
const concepts = ["Operating margin", "Capacity utilization", "Customer concentration", "Inventory days", "Cash conversion", "Export demand", "New production line", "Freight costs", "Order backlog", "Pricing power", "Working capital", "Automation", "Supplier qualification", "Energy costs", "Return on capital", "Quality yield"];
const previewNodes: GraphNode[] = [...companies, ...concepts].map((name, i) => ({ id: `preview-node-${i}`, name, aliases: [], description: `${name} in the fictional industrial supply chain.`, kind: i < companies.length ? "Organization" : "Entity", sourceRole: i < companies.length ? "organization" : "concept", properties: {} }));
const previewEdges: GraphEdge[] = Array.from({ length: 32 }, (_, i) => {
  const source = previewNodes[i % 24];
  const target = previewNodes[i < 24 ? (i + 1) % 24 : 8 + i % 16];
  const relation = i % 2 ? "depends_on" : "influences";
  const endpoints = [{ nodeId: source.id, role: "subject" }, { nodeId: target.id, role: "object" }];
  return { id: `preview-claim-${i}`, relation, endpoints, status: "active", claim: { id: `preview-claim-${i}`, proposition: `${source.name} ${relation.replaceAll("_", " ")} ${target.name.toLowerCase()}.`, attribution: "Fictional Q3 operating review", relation, endpoints, qualifiers: [], evidence: [{ spanId: "preview-span", role: "supports", attribution: "Company report" }], status: "active", publicationTime: { precision: "date", value: "2026-10-27" } } };
});
const evidenceId = `ev_${"a".repeat(32)}`;
const answerMessages: ChatMessage[] = [
  { id: "preview-user", role: "user", status: "complete", created_at: "2026-10-28T00:00:00Z", content: `Review ${LONG_TITLE}. What explains the margin decline?` },
  { id: "preview-answer", role: "assistant", status: "complete", created_at: "2026-10-28T00:00:12Z", content: `Hanul Precision’s operating margin fell from **16.2% to 14.8%**, while revenue grew 11% to ₩184.2 billion. The report attributes most of the decline to qualification costs at the new production line. [1](#evidence-${evidenceId})\n\nThe next useful comparison is utilization against inventory days. Inventory is 54 days, so stronger reported demand has not yet translated into faster cash conversion.`, evidence_refs: [{ id: evidenceId, title: LONG_TITLE, kind: "original", url: "https://hanul.example/quarterly-review", provider: "Hanul Precision IR", tool_name: "web_fetch", retrieved_at: "2026-10-28T00:00:00Z", published_at: "2026-10-27", excerpt: "Revenue ₩184.2bn; operating margin 14.8%; inventory days 54.", excerpt_truncated: false, content_sha256: "b".repeat(64), warnings: [] }], memory_update: { status: "applied", commitId: "preview-commit", changes: [{ kind: "Source", recordId: "preview-sources", operation: "create", title: LONG_TITLE, lesson: "", basis: "Quarterly operating report", futureUse: "Compare the next quarter", knowledge: { kind: "source", recordId: "preview-sources", revisionId: revision.revisionId, objectId: "preview-source-0", spanId: "preview-span" } }] } },
];
const previewInboxItems: InboxItem[] = [
  { briefingContext: { symbols: ["005930", "000660", "AAPL"], principlesChecked: 2, newFacts: 14, disputedEntityIds: [],
    changed: { names: 5, relations: 2, conflicts: 1, rows: [
      { kind: "claim_added", subjectId: null, summary: "Hanul Precision supplies Morrow Components" },
      { kind: "claim_disputed", subjectId: null, summary: "Capacity timing differs between two reports" },
      { kind: "source_retained", subjectId: null, summary: "Q3 operating review" },
    ] },
    ahead: [{ journalId: "preview-journal-1", title: "Inventory days below 50", byDate: "2026-11-04" }] }, runId: "preview-briefing-1", taskId: "preview-briefing", kind: "briefing", title: "Briefings", status: "succeeded", summary: "October 28: Hanul Precision reported higher qualification costs and a lower operating margin. The new capacity figures conflict with the saved utilization rule.", startedAtMs: FIXTURE_TIME, finishedAtMs: FIXTURE_TIME + 12000, readAtMs: null, threadId: "preview-chat", messageId: "preview-answer", skippedTools: [] },
  { runId: "preview-custom-1", taskId: "preview-task-0", kind: "custom", title: TASK_TITLE, status: "succeeded", summary: "October 27: Supplier delivery times fell by two days. The scheduled run used retained Sources; one requested check was unavailable.", startedAtMs: FIXTURE_TIME - 86400000, finishedAtMs: FIXTURE_TIME - 86390000, readAtMs: null, threadId: "preview-chat", messageId: "preview-answer", skippedTools: [{ name: "bash", reason: "Native tools are unavailable in unattended runs." }] },
  { runId: "preview-briefing-2", taskId: "preview-briefing", kind: "briefing", title: "Briefings", status: "failed", error: "The model provider could not be reached. Please try again.", summary: "", startedAtMs: FIXTURE_TIME - 2*86400000, finishedAtMs: FIXTURE_TIME - 2*86400000 + 5000, readAtMs: FIXTURE_TIME - 86400000, threadId: "preview-chat", messageId: null, skippedTools: [] },
  { runId: "preview-briefing-3", taskId: "preview-briefing", kind: "briefing", title: "Briefings", status: "succeeded", summary: "October 25: No material changes appeared in the available company Sources.", startedAtMs: FIXTURE_TIME - 3*86400000, finishedAtMs: FIXTURE_TIME - 3*86400000 + 12000, readAtMs: FIXTURE_TIME - 2*86400000, threadId: "preview-chat", messageId: "preview-answer", skippedTools: [] },
  { runId: "preview-custom-2", taskId: "preview-task-1", kind: "custom", title: "Weekly supplier capacity comparison", status: "succeeded", summary: "October 24: Morrow Components disclosed an additional production shift; the announcement did not specify the added capacity.", startedAtMs: FIXTURE_TIME - 4*86400000, finishedAtMs: FIXTURE_TIME - 4*86400000 + 12000, readAtMs: FIXTURE_TIME - 3*86400000, threadId: "preview-chat", messageId: "preview-answer", skippedTools: [] },
];

const previewLearningReviews: import("../contracts/knowledge").LearningReview[] = [
  { reviewId: "preview-review-created", status: "created", lensRecordId: "map-lens-2", rationale: "Three dated capacity ramps link lower utilization to temporary margin pressure; compare utilization and cash conversion next quarter.", evidenceCount: 6, createdAtMs: FIXTURE_TIME },
  { reviewId: "preview-review-deferred-1", status: "deferred", reason: "no_reusable_pattern", rationale: "The office move and director appointments are unrelated one-off facts.", evidenceCount: 0, createdAtMs: FIXTURE_TIME - 21600000 },
  { reviewId: "preview-review-deferred-2", status: "deferred", reason: "no_reusable_pattern", rationale: "The current cash-conversion method already covers these facts.", evidenceCount: 0, createdAtMs: FIXTURE_TIME - 43200000 },
];

function localizedAnswers() {
  const messages = structuredClone(answerMessages);
  if (!previewKo()) return messages;
  const answer = messages.find(message => message.id === "preview-answer");
  if (answer) answer.content = `한울정밀의 영업이익률이 **16.2%에서 14.8%**로 낮아졌고, 매출은 11% 늘어 1,842억 원이에요. 보고서에는 새 생산라인의 인증 비용이 하락의 대부분이라고 나와요. [1](#evidence-${evidenceId})\n\n가동률과 재고일수를 비교하는 것이 다음으로 유용해요. 재고일수는 54일이에요. 보고된 수요가 늘어도 현금 전환은 아직 빨라지지 않았어요.`;
  return messages;
}

/** The morning briefing as the Loupist posts it into the home Thread. */
export function previewDelivery(): ChatMessage {
  const summary = localizeInbox(previewInboxItems)[0]!.summary;
  return {
    id: "preview-delivery", role: "assistant", status: "complete", host_composed: true, created_at: new Date(FIXTURE_TIME - 3_600_000).toISOString(),
    content: summary.replace(/^[^:]{1,20}:\s*/u, ""),
    delivery: { run_id: "preview-briefing-1", task_id: "preview-briefing", task_title: "Briefings", task_kind: "briefing", schedule: { kind: "weekly", days: [1, 2, 3, 4, 5], hour: 8, minute: 0, timezone: "Asia/Seoul" }, run_thread_id: "preview-briefings", status: "succeeded", finished_at_ms: FIXTURE_TIME - 3_600_000 },
  };
}

/** The first time after the frozen preview clock a weekly or daily Seoul schedule fires (UTC+9 all year). */
function nextSeoulRun(schedule: ScheduledTask["schedule"]) {
  if (schedule.kind !== "weekly" && schedule.kind !== "daily") return FIXTURE_TIME + 86_400_000;
  const days = schedule.kind === "weekly" ? schedule.days : [0, 1, 2, 3, 4, 5, 6];
  const offsetMs = 9 * 3_600_000;
  for (let day = 0; day < 8; day += 1) {
    const local = new Date(FIXTURE_TIME + offsetMs + day * 86_400_000);
    if (!days.includes(local.getUTCDay())) continue;
    const at = Date.UTC(local.getUTCFullYear(), local.getUTCMonth(), local.getUTCDate(), schedule.hour, schedule.minute) - offsetMs;
    if (at > FIXTURE_TIME) return at;
  }
  return FIXTURE_TIME + 86_400_000;
}

function briefingBodyMessage(): ChatMessage {
  const content = previewKo()
    ? "10월 28일 브리핑: 한울정밀의 인증 비용이 늘었고 영업이익률은 14.8%예요. 재고일수는 54일이에요."
    : "October 28 briefing: Hanul Precision’s qualification costs rose, and the operating margin settled at 14.8%. Inventory is 54 days.";
  return { id: "preview-briefing-body", role: "assistant", status: "complete", created_at: "2026-10-28T00:00:20Z", content };
}

function localizeInbox(items: InboxItem[]) {
  const copy = structuredClone(items);
  if (!previewKo()) return copy;
  const summaries: Record<string, string> = {
    "preview-briefing-1": "10월 28일: 한울정밀이 더 높은 인증 비용과 더 낮은 영업이익률을 보고했어요. 새 생산능력 수치는 저장된 가동률 원칙과 어긋나요.",
    "preview-custom-1": "10월 27일: 공급사의 납기가 이틀 줄었어요. 예약 실행은 보관한 자료를 사용했고, 요청한 확인 하나는 쓸 수 없었어요.",
    "preview-briefing-3": "10월 25일: 확인할 수 있는 회사 자료에서 중요한 변화는 없었어요.",
    "preview-custom-2": "10월 24일: Morrow Components가 생산 교대를 추가했다고 밝혔고, 늘어난 생산능력은 적지 않았어요.",
  };
  for (const item of copy) {
    if (item.title === TASK_TITLE) item.title = KOREAN_TASK_TITLE;
    if (summaries[item.runId]) item.summary = summaries[item.runId];
    // Korean cells read Korean rows and checks, like a Korean user's own briefing.
    const context = item.briefingContext;
    if (context?.changed) {
      const rows: Record<string, string> = {
        "Hanul Precision supplies Morrow Components": "한울정밀 → Morrow Components (공급) 연결",
        "Capacity timing differs between two reports": "두 보고서의 증설 시점이 서로 달라요",
        "Q3 operating review": "3분기 영업 검토 자료",
      };
      for (const row of context.changed.rows) row.summary = rows[row.summary] ?? row.summary;
    }
    for (const check of context?.ahead ?? []) if (check.title === "Inventory days below 50") check.title = "재고일수 50일 미만";
  }
  return copy;
}

function localizeCatalog(catalog: KnowledgeCatalog) {
  if (!previewKo()) return catalog;
  const titles = ["한울정밀 2026년 3분기 실적 설명자료", "은빛소재: 신규 공장 가동률 및 수주 동향"];
  for (const source of catalog.sources) for (const object of source.objects) {
    if (object.title === sourceTitles[1]) object.title = titles[0];
    if (object.title === sourceTitles[4]) object.title = titles[1];
    if (object.originLabel === "Hanul Research") object.originLabel = "한울 리서치";
  }
  return catalog;
}

/** Preview-owned responses only: no changes to the shared mock's state or defaults. */
export class PreviewBridge extends MockLoupistBridge {
  includeBriefingBody = false;
  private learningHandlers = new Set<(event: import("../contracts/knowledge").LearningReviewFinishedEvent) => void>();
  override onLearningReviewFinished(handler: (event: import("../contracts/knowledge").LearningReviewFinishedEvent) => void) {
    this.learningHandlers.add(handler);
    return () => { this.learningHandlers.delete(handler); };
  }
  emitLearningReview(review: import("../contracts/knowledge").LearningReview) {
    this.learningHandlers.forEach(handler => handler({ loupistId: "loupist-quality", review }));
  }

  private readonly worldSchema = new MockWorldSchema();
  installWorldSchema(loupistId: string, view: import("../types").WorldSchemaView) { this.worldSchema.install(loupistId, view); }
  worldSchemaGet(request: { loupistId: string }) { return Promise.resolve(this.worldSchema.get(request.loupistId)); }
  worldSchemaUndo(request: { loupistId: string; changeId: string }) { return Promise.resolve(this.worldSchema.undo(request.loupistId, request.changeId)); }
  worldSchemaRemove(request: { loupistId: string; kind: "type" | "relation"; name: string }) { return Promise.resolve(this.worldSchema.remove(request.loupistId, request.kind, request.name)); }
  onWorldSchemaChanged(handler: (event: { loupistId: string }) => void) { return this.worldSchema.subscribe(handler); }

  override async learningReviewList(request: import("../contracts/knowledge").LearningReviewQuery) {
    await super.learningReviewList(request);
    return request.loupistId === "loupist-quality" ? structuredClone(previewLearningReviews.slice(0, request.limit ?? 50)) : [];
  }
  override async knowledgeMap(request: import("../contracts/knowledge").KnowledgeMapQuery) {
    const map = await super.knowledgeMap(request);
    const learned = map.nodes.find(node => node.id === "map-lens-2");
    if (learned && request.loupistId === "loupist-quality") {
      learned.learnedAtMs = FIXTURE_TIME;
      learned.learnedRationale = previewLearningReviews[0]!.rationale!;
      map.generatedAtMs = FIXTURE_TIME;
    }
    for (const edge of map.edges) if (edge.relation === "supplies" && edge.source === "map-entity-00" && edge.target === "map-entity-01") edge.relation = "associated_with";
    return map;
  }
  private previewInboxHandlers = new Set<(event: import("../types").InboxChangedEvent) => void>();
  override onInboxChanged(handler: (event: import("../types").InboxChangedEvent) => void) {
    const off = super.onInboxChanged(handler); this.previewInboxHandlers.add(handler);
    return () => { off(); this.previewInboxHandlers.delete(handler); };
  }
  private readonly previewInbox = localizeInbox(previewInboxItems);
  override async inboxList({ cursor, limit }: InboxRequest): Promise<InboxPage> {
    const start = cursor ? this.previewInbox.findIndex(item => item.runId === cursor) + 1 : 0;
    const items = this.previewInbox.slice(start, start + limit).map(item => this.includeBriefingBody && item.kind === "briefing" && item.messageId === "preview-answer" ? { ...item, messageId: "preview-briefing-body" } : item);
    return structuredClone({ items, nextCursor: start + limit < this.previewInbox.length ? items.at(-1)!.runId : null });
  }
  override async inboxMarkRead({ runIds }: { loupistId: string; runIds: string[] }) {
    for (const item of this.previewInbox) if (runIds.includes(item.runId)) item.readAtMs ??= Date.now();
    this.previewInboxHandlers.forEach(handler => handler({ loupistId: "loupist-quality" }));
  }
  override async inboxUnreadCounts() {
    const loupists = await this.loupistList();
    return loupists.map((loupist, index) => ({ loupistId: loupist.id, count: index === 0 ? this.previewInbox.filter(item => item.readAtMs === null).length : this.manyLoupists && index === 2 ? 103 : 0 }));
  }

  /** Tasks for the Schedule tab, built from the same runs the inbox lists. */
  scheduleTasks: "preview" | "none" = "preview";
  private previewRuns(): ScheduledRun[] {
    return this.previewInbox.map(item => ({
      id: item.runId, run_id: item.runId, task_id: item.taskId, loupist_id: "loupist-quality", task_revision: 1, task_title: item.title,
      thread_id: item.threadId, scheduled_for_ms: item.startedAtMs, started_at_ms: item.startedAtMs, finished_at_ms: item.finishedAtMs,
      status: item.status, error: item.error ?? null, briefing_context: item.briefingContext ?? null, kind: item.kind, summary: item.summary,
      read_at_ms: item.readAtMs, message_id: this.includeBriefingBody && item.kind === "briefing" && item.messageId === "preview-answer" ? "preview-briefing-body" : item.messageId,
      skipped_tools: item.skippedTools,
    }));
  }
  override async scheduledTasksList({ loupist_id }: { loupist_id: string }): Promise<ScheduledTask[]> {
    if (this.scheduleTasks === "none") return [];
    const runs = this.previewRuns();
    const weekly = { kind: "weekly" as const, days: [1, 2, 3, 4, 5], hour: 8, minute: 0, timezone: "Asia/Seoul" };
    const task = (id: string, kind: "briefing" | "custom", title: string, schedule: ScheduledTask["schedule"], status: ScheduledTask["status"]): ScheduledTask => ({
      kind, id, loupist_id, revision: 1, title, prompt: title, schedule, target: kind === "briefing" ? { kind: "dedicated_chat" } : { kind: "new_chat" },
      model_profile_id: "preview", thinking_level: "medium", run_options: {}, use_memory: true, update_memory: kind === "briefing", status,
      next_run_at_ms: status === "active" ? nextSeoulRun(schedule) : null, last_run_at_ms: runs.find(run => run.task_id === id)?.started_at_ms ?? null,
      created_at_ms: FIXTURE_TIME - 30 * 86_400_000, updated_at_ms: FIXTURE_TIME, latest_run: runs.find(run => run.task_id === id) ?? null,
    });
    return [
      task("preview-briefing", "briefing", "Briefings", weekly, "active"),
      task("preview-task-0", "custom", previewKo() ? KOREAN_TASK_TITLE : TASK_TITLE, { ...weekly, days: [1], hour: 7, minute: 30 }, "active"),
      task("preview-task-1", "custom", previewKo() ? "주간 공급 능력 비교" : "Weekly supplier capacity comparison", { kind: "weekly", days: [5], hour: 17, minute: 0, timezone: "Asia/Seoul" }, "paused"),
    ];
  }
  override async scheduledTaskRuns({ task_id }: { loupist_id: string; task_id: string }) {
    return this.previewRuns().filter(run => run.task_id === task_id);
  }

  conversation: "empty" | "answer" | "streaming" = "empty";
  /** The answer accepted guidance while it ran. */
  guidedAnswer = false;
  manyLoupists = false;
  manyChats = false;
  private stopStream?: () => void;
  override async loupistSelect(loupistId: string) {
    const workspace = await super.loupistSelect(loupistId);
    const titles = this.manyChats || this.manyLoupists ? ["Q3 margin review", "HBM supply map", "Foundry capacity outlook", "Quarterly cash conversion", "Customer concentration", "Advanced packaging demand", "Capital expenditure plans"] : [this.conversation === "answer" ? "Reading Hanul Precision’s margin decline" : ""];
    const messages = this.conversation === "answer" ? localizedAnswers() : [];
    if (this.conversation === "answer" && this.guidedAnswer) {
      // Guidance the reply accepted while it ran: Rust stores it before the answer, marked as a steer.
      const at = messages.findIndex(message => message.id === "preview-answer");
      messages.splice(at < 0 ? messages.length : at, 0, { id: "preview-steer", role: "user", status: "complete", steer: true,
        content: previewKo() ? "새 생산라인만 봐 줘" : "Focus on the new line only", created_at: "2026-10-28T00:00:30Z" });
    }
    if (this.includeBriefingBody) messages.push(briefingBodyMessage());
    // The first preview conversation is the Loupist's home Thread, where a Loupist opens.
    return { ...workspace, threads: titles.map((title, i) => ({ id: i ? `preview-chat-${i}` : "preview-chat", loupist_id: loupistId, home: i === 0, title, updated_at: `2026-10-${28 - i}T00:00:00Z`, use_memory: true, update_memory: true, messages: structuredClone(i === 0 ? messages : []) })) };

  }
  override async knowledgeCatalog({ loupistId }: { loupistId: string }) {
    const principles = (await super.knowledgeCatalog({ loupistId })).lenses.filter(lens => lens.lensKind === "principle");
    return localizeCatalog(structuredClone({ ...previewCatalog, loupistId, lenses: [...previewCatalog.lenses, ...principles] }));
  }
  override async knowledgeWorldQuery(request: WorldGraphQuery) {
    return structuredClone({ recordId: request.recordId, ...revision, nodes: previewNodes, edges: previewEdges, coverage: "sealed_snapshot" as const, truncated: false });
  }
  private pageStore?: Map<string, PageView>;
  private pageLog: Array<{ recordId: string; before?: import("../types").SectionView; entry: import("../types").LensLogEntryView }> = [];
  private pages() {
    this.pageStore ??= new Map(previewPages(previewKo(), METHOD_TITLE).map(page => [page.recordId, page]));
    return this.pageStore;
  }
  private pageView(page: PageView): PageView {
    const own = this.pageLog.filter(row => row.recordId === page.recordId).map(row => row.entry);
    return structuredClone({ ...page, history: [...own, ...page.history].sort((a, b) => b.createdAtMs - a.createdAtMs) });
  }
  override async knowledgeLensRead(request: KnowledgeLensReadRequest): Promise<PageView> {
    const page = this.pages().get(request.recordId);
    if (page) return this.pageView(page);
    if ((await this.principleList({ loupistId: request.loupistId, includeArchived: true })).some(p => p.recordId === request.recordId)) return super.knowledgeLensRead(request);
    return this.pageView({ ...this.pages().get("preview-method")!, recordId: request.recordId });
  }
  override async knowledgeLensPagesForNode({ nodeId }: { loupistId: string; nodeId: string }): Promise<PageSummary[]> {
    return [...this.pages().values()].filter(page => page.anchors.some(anchor => anchor.nodeId === nodeId))
      .map(page => ({ recordId: page.recordId, title: page.title, kind: page.kind, summary: page.body.kind === "principle" ? page.body.statement : page.body.summary }));
  }
  override async knowledgeLensSearch({ query, kinds }: { loupistId: string; query: string; kinds?: LensKind[] }) {
    const terms = query.toLocaleLowerCase().split(/\s+/u).filter(Boolean);
    return [...this.pages().values()].filter(page => (!kinds?.length || kinds.includes(page.kind)) && terms.every(term => JSON.stringify(page.body).toLocaleLowerCase().includes(term) || page.title.toLocaleLowerCase().includes(term)))
      .map(page => ({ recordId: page.recordId, revisionId: page.revisionId, kind: page.kind, title: page.title, summary: page.body.kind === "principle" ? page.body.statement : page.body.summary, snippet: "" }));
  }
  override async memoryLearned({ messageIds }: { loupistId: string; messageIds: string[] }): Promise<LearnedDigest[]> {
    return messageIds.map(messageId => messageId === previewLearned([]).messageId ? previewLearned([...this.pages().values()]) : { messageId, pages: [] });
  }
  override async memorySectionForget({ recordId, key }: { loupistId: string; recordId: string; key: string }): Promise<PageView> {
    const page = this.pages().get(recordId);
    if (!page || page.body.kind === "principle") throw new Error("Page not found.");
    const before = page.body.sections.find(section => section.key === key);
    page.body.sections = page.body.sections.filter(section => section.key !== key);
    this.pageLog.push({ recordId, before, entry: { opId: `preview-forget-${this.pageLog.length}`, op: "retire", outcome: "retired", key, origin: "user", createdAtMs: FIXTURE_TIME } });
    return this.pageView(page);
  }
  override async memoryOpUndo({ opId }: { loupistId: string; opId: string }): Promise<PageView> {
    const row = this.pageLog.find(item => item.entry.opId === opId);
    const page = row && this.pages().get(row.recordId);
    if (!row?.before || !page || page.body.kind === "principle") throw new Error("Nothing to undo.");
    page.body.sections.push(row.before);
    const order = ["overview", "value-chain", "goals", "views", "risks", "claim", "use-when", "steps", "beliefs"];
    page.body.sections.sort((a, b) => order.indexOf(a.key) - order.indexOf(b.key));
    this.pageLog.push({ recordId: row.recordId, entry: { opId: `preview-undo-${this.pageLog.length}`, op: "put", outcome: "added", key: row.entry.key, origin: "user", createdAtMs: FIXTURE_TIME + 1, undoesOpId: opId } });
    return this.pageView(page);
  }
  override async chatSend(request: ChatSendRequest, observer: StreamObserver<ChatStreamEvent>) {
    if (this.conversation !== "streaming") return super.chatSend(request, observer);
    const run_id = request.run_id;
    observer({ type: "started", run_id });
    const draft = previewKo()
      ? "한울정밀의 매출은 **11% 증가**했지만 영업이익률은 낮아졌어요.\n\n새 생산라인의 가동률과 재고일수를 함께 보면"
      : "Hanul Precision’s revenue **grew 11%**, while operating margin declined.\n\nThe new line’s utilization and the change in inventory days suggest";
    observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: Date.now() - 2400, items: [{ id: "preview-read", kind: "tool", toolName: "world_read", category: "memory", operation: "read", action: "Read saved notes", status: "succeeded" }] } });
    observer({ type: "phase", run_id, phase: "writing" });
    observer({ type: "progress_patch", run_id, patch: { sequence: 2, upsertItems: [{ id: "preview-draft", kind: "commentary", text: draft }] } });
    this.stopStream = () => { observer({ type: "aborted", run_id }); };
    return { run_id };
  }
  override async chatAbort(runId: string) { if (this.stopStream) { this.stopStream(); this.stopStream = undefined; } else await super.chatAbort(runId); }
}

export async function prepareFixture(bridge: PreviewBridge, mode: "provider" | "loupist" | "empty" | "answer" | "streaming" = "empty") {
  for (const loupist of await bridge.loupistList()) if (mode === "provider" || mode === "loupist" || loupist.id !== "loupist-quality") await bridge.loupistDelete({ loupist_id: loupist.id });
  if (mode === "provider") for (const provider of (await bridge.modelCatalogGet()).providers) await bridge.providerDisconnect(provider.id);
  bridge.conversation = mode === "answer" || mode === "streaming" ? mode : "empty";
}

export async function configureTheme(bridge: MockLoupistBridge, theme: "light" | "dark") {
  const config = await bridge.configRead({ scope: { kind: "global" } });
  await bridge.configUpdate({ scope: config.scope, expectedRevision: config.revision, set: { appearance: { theme } }, unset: [] });
}

let previewClockOffsetMs = 0;
/** Moves the frozen preview clock forward, e.g. to show a run that has been silent for a while. */
export function advancePreviewClock(milliseconds: number) { previewClockOffsetMs += milliseconds; }

/** Keep relative dates and the live progress timer stable in the standalone preview. */
export function freezePreviewClock() {
  const NativeDate = globalThis.Date;
  const now = () => FIXTURE_TIME + previewClockOffsetMs;
  globalThis.Date = new Proxy(NativeDate, {
    construct(target, args) { return Reflect.construct(target, args.length ? args : [now()]); },
    apply() { return new NativeDate(now()).toString(); },
    get(target, key, receiver) { return key === "now" ? now : Reflect.get(target, key, receiver); },
  });
  return () => { globalThis.Date = NativeDate; };
}

function customConnectionFixture(id = "preview-custom-ready"): import("../contracts/customConnections").CustomConnection {
  return { id, kind: "custom", name: "Hanul investor research", transport: { kind: "http", url: "https://research.example/mcp" }, command_line: null, dont_save_to_memory: true, pending: false, enabled: true, needs_review: false, auth_state: "not_required", env_keys: [], header_names: [], last_error: null,
    tools: [
      { name: "read_company_filings", server_description: "Read public company filings and quarterly operating results. Returns dated source links and reporting periods.", input_schema: { type: "object" }, annotations: { readOnlyHint: true }, definition_hash: "a".repeat(64), definition_changed: false, eligibility: "read_only", approval: "always" },
      { name: "save_research_note", server_description: "Save a research note to the server’s shared workspace.", input_schema: { type: "object" }, annotations: null, definition_hash: "b".repeat(64), definition_changed: false, eligibility: "not_read_only", approval: null },
      { name: "place_payment", server_description: "Send a payment to the selected account.", input_schema: { type: "object" }, annotations: { readOnlyHint: true }, definition_hash: "c".repeat(64), definition_changed: false, eligibility: "financial_action", approval: null },
    ] };
}

export async function prepareCustomConnectionFixture(bridge: PreviewBridge, scenario: string) {
  await prepareFixture(bridge, scenario === "chat-needs-approval" ? "answer" : undefined);
  const ready = customConnectionFixture();
  const changed = { ...customConnectionFixture("preview-custom-changed"), name: "Morrow research tools", needs_review: true };
  changed.tools[0] = { ...changed.tools[0], approval: null, definition_changed: true, definition_hash: "d".repeat(64) };
  const signIn = { ...customConnectionFixture("preview-custom-signin"), name: "Private company research", auth_state: "sign_in_needed" as const, tools: [] };
  const records = scenario === "connections-custom-empty" ? [] : [ready, changed, signIn];
  const copy = <T,>(value: T) => structuredClone(value);
  bridge.customConnectionList = async () => copy(records);
  bridge.customConnectionAddUrl = async request => {
    const record = { ...customConnectionFixture("preview-custom-new"), name: request.name || "Company research server", transport: { kind: "http" as const, url: request.url }, pending: true, tools: [] };
    records.push(record); return copy(record);
  };
  bridge.customConnectionConfirm = async ({ connection_id }) => {
    const record = records.find(item => item.id === connection_id)!;
    Object.assign(record, { pending: scenario === "connection-add-url-signin", needs_review: true, auth_state: scenario === "connection-add-url-signin" ? "sign_in_needed" : "not_required", tools: scenario === "connection-add-url-signin" ? [] : customConnectionFixture().tools.map(tool => ({ ...tool, approval: null })) });
    return copy(record);
  };
  let cancelSignIn: (() => void) | undefined;
  bridge.customConnectionSignIn = ({ connection_id }) => new Promise(resolve => {
    const record = records.find(item => item.id === connection_id)!;
    record.auth_state = "in_progress";
    cancelSignIn = () => { record.auth_state = "sign_in_needed"; resolve(copy(record)); };
  });
  bridge.customConnectionSignInCancel = async () => { cancelSignIn?.(); };
  bridge.customConnectionAddJson = async () => {
    const remote = { ...customConnectionFixture("preview-import-remote"), name: "Company investor data", pending: true, tools: [], header_names: ["Authorization"] };
    const local = { ...customConnectionFixture("preview-import-local"), name: "Local research archive", pending: true, transport: { kind: "stdio" as const, command: "npx", args: ["@example/research-archive@1.2.3", "--read-only"] }, command_line: "npx @example/research-archive@1.2.3 --read-only", env_keys: ["RESEARCH_API_KEY"], tools: [] };
    records.push(remote, local); return copy([remote, local]);
  };
  bridge.customToolApprove = async request => {
    const record = records.find(item => item.id === request.connection_id)!;
    record.tools = record.tools.map(tool => tool.name === request.tool_name ? { ...tool, approval: request.choice, definition_changed: false } : tool);
    record.needs_review = record.tools.some(tool => tool.eligibility === "read_only" && !tool.approval);
    return copy(record);
  };
  bridge.customConnectionSetEnabled = async request => { const record = records.find(item => item.id === request.connection_id)!; record.enabled = request.enabled; return copy(record); };
  bridge.customConnectionSetMemory = async request => { const record = records.find(item => item.id === request.connection_id)!; record.dont_save_to_memory = request.dont_save_to_memory; return copy(record); };
  bridge.customConnectionRemove = async request => { const index = records.findIndex(item => item.id === request.connection_id); if (index >= 0) records.splice(index, 1); };
  if (scenario === "chat-needs-approval") {
    const select = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => { const workspace = await select(loupistId); const answer = workspace.threads[0].messages.find(item => item.id === "preview-answer"); if (answer) Object.assign(answer, { tool_refusals: [{ connection_id: changed.id, connection_name: changed.name, tool_name: changed.tools[0].name, reason: "needs_approval" }] }); return workspace; };
  }
}

/** The answer set up FRED from Chat: its `connection_setup` step leaves a card whose state the card reads live. */
export async function prepareConnectionSetupFixture(bridge: PreviewBridge, state: "needs_setup" | "ready") {
  await prepareFixture(bridge, "answer");
  if (state === "ready") {
    await bridge.marketplaceCredentialSave({ entry_id: "fred.macro", secrets: { api_key: "preview-fred-key-0123" } });
    await bridge.marketplaceCredentialVerify({ entry_id: "fred.macro" });
    await bridge.loupistCapabilityEnable({ loupist_id: "loupist-quality", entry_id: "fred.macro" });
  }
  const select = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async loupistId => {
    const workspace = await select(loupistId);
    const answer = workspace.threads[0].messages.find(item => item.id === "preview-answer");
    if (answer) Object.assign(answer, { progress: { startedAtMs: FIXTURE_TIME - 8000, finishedAtMs: FIXTURE_TIME - 2000, items: [
      { id: "preview-setup", kind: "tool", toolName: "connection_setup", category: "capability", operation: "generic", action: "Prepared a connection", running: "Preparing a connection", target: "fred.macro", source: { kind: "connector", name: "FRED" }, status: "succeeded", startedAtMs: FIXTURE_TIME - 8000, finishedAtMs: FIXTURE_TIME - 7000 },
    ] } });
    return workspace;
  };
}

/** One coherent World/Lens history for the learning UI capture matrix. */
export async function prepareLearningFixture(bridge: PreviewBridge, evidenceCount = 6) {
  await prepareFixture(bridge);
  const title = "Test whether capacity costs are temporary";
  const refinedTitle = "Check cash conversion across a capacity ramp";
  const at = Date.now() - 3600000;
  const reviews: import("../contracts/knowledge").LearningReview[] = [
    { reviewId: "learning-created", status: "created", lensRecordId: "map-lens-2", evidenceCount, createdAtMs: at, rationale: "Across three companies, low utilization and temporary qualification costs preceded margin recovery. Compare cash conversion before calling the pressure temporary." },
    { reviewId: "learning-refined", status: "refined", lensRecordId: "map-lens-1", evidenceCount: 4, createdAtMs: at - 86400000, rationale: "Inventory rose faster than sales, so the method now checks working capital." },
    { reviewId: "learning-deferred", status: "deferred", evidenceCount: 0, createdAtMs: at - 2 * 86400000, reason: "insufficient_turns" },
    { reviewId: "learning-failed", status: "failed", evidenceCount: 0, createdAtMs: at - 3 * 86400000, reason: "not_sent" },
  ];
  bridge.learningReviewList = async ({ limit }) => structuredClone(reviews.slice(0, limit ?? 50));
  const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
  catalog.lenses.push(...[title, refinedTitle].map((name, index) => ({ ...revision, recordId: `map-lens-${2 - index}`, title: name, lensKind: "method" as const, kind: "lens" as const, learned: true, updatedAtMs: at - index * 86400000, objects: [] })));
  bridge.knowledgeCatalog = async ({ loupistId }) => structuredClone({ ...catalog, loupistId });
  const baseMap = await bridge.knowledgeMap({ loupistId: "loupist-quality" });
  baseMap.nodes = baseMap.nodes.filter(node => node.kind !== "lens_principle");
  baseMap.counts.lens_principle = { shown: 0, total: 0, truncated: false };
  const learned = baseMap.nodes.find(node => node.id === "map-lens-2")!;
  learned.label = title; learned.learnedAtMs = at; learned.learnedRationale = reviews[0].rationale!;
  const refined = baseMap.nodes.find(node => node.id === "map-lens-1")!;
  refined.label = refinedTitle; refined.learnedAtMs = at - 86400000; refined.learnedRationale = reviews[1].rationale!;
  const evidenceNodes = baseMap.nodes.filter(node => node.kind === "entity").slice(0, 6);
  evidenceNodes.forEach((node, i) => { node.label = companies[i]; });
  const statements = ["Operating margin fell during a new production line’s qualification.", "Utilization reached 68% as qualification costs began to decline.", "Inventory days fell as utilization recovered in the next quarter.", "Temporary ramp labor accounted for most of the margin decline.", "Cash conversion improved alongside higher utilization.", "Pricing remained stable while ramp costs and scrap declined."];
  const claims = statements.slice(0, evidenceCount).map((proposition, i) => ({ ...previewEdges[i], id: `learning-claim-${i}`, endpoints: [{ nodeId: evidenceNodes[i].id, role: "subject" }], claim: { ...previewEdges[i].claim, id: `learning-claim-${i}`, proposition, endpoints: [{ nodeId: evidenceNodes[i].id, role: "subject" }], publicationTime: { precision: "date" as const, value: `2026-10-${20 + i}` } } }));
  bridge.knowledgeWorldQuery = async request => structuredClone({ recordId: request.recordId, ...revision, revisionId: request.revisionId, nodes: evidenceNodes.map(node => ({ id: node.id, name: node.label, aliases: [], description: "", kind: "Organization", sourceRole: "organization", properties: {} })), edges: claims, coverage: "sealed_snapshot" as const, truncated: false });
  bridge.knowledgeMap = async () => structuredClone(baseMap);
  const originalRead = bridge.knowledgeLensRead.bind(bridge);
  bridge.knowledgeLensRead = async request => ({ ...await originalRead(request), title: catalog.lenses.find(lens => lens.recordId === request.recordId)?.title ?? title, kind: request.recordId.startsWith("map-lens-") ? "method" : "perspective" });
  const growth: import("../contracts/knowledge").GrowthEvent[] = [
    { eventId: "learning-revision", loupistId: catalog.loupistId, recordId: learned.recordId!, subjectId: learned.recordId!, kind: "lens_revised", atMs: at, actor: "loupist", originChatId: null, originMessageId: null, summary: "Added cash confirmation", beforeSummary: "Compare utilization and margin across the ramp. Separate qualification costs from recurring expenses.", afterSummary: "Compare utilization, margins and inventory days. Confirm recovery with operating cash flow, and test whether pricing or product mix explains the change.", rationale: "Margin recovery alone did not establish that demand had turned into cash.", origin: { kind: "learning_review", reviewId: reviews[0].reviewId } },
    { eventId: "learning-prior", loupistId: catalog.loupistId, recordId: learned.recordId!, subjectId: learned.recordId!, kind: "lens_revised", atMs: at - 86400000, actor: "loupist", originChatId: null, originMessageId: null, summary: "Separated ramp costs", beforeSummary: "Track reported margins.", afterSummary: "Compare utilization and margin across the ramp.", rationale: "Three companies reported temporary qualification costs." },
    { eventId: "learning-original", loupistId: catalog.loupistId, recordId: learned.recordId!, subjectId: learned.recordId!, kind: "lens_created", atMs: at - 2 * 86400000, actor: "loupist", originChatId: null, originMessageId: null, summary: title, beforeSummary: null, afterSummary: "Track reported margins." },
  ];
  bridge.knowledgeGrowthEvents = async () => ({ events: structuredClone(growth), nextCursor: null });
  return reviews[0];
}


export async function prepareJournalFixture(bridge: PreviewBridge) {
  await prepareFixture(bridge);
  const entries: import("../types").JournalRecord[] = [
    { judgment: "I think Hanul’s gross margin will stay above 55% through Q1.", reasoning: "Capacity utilization is recovering while qualification costs are easing.", check: { claim: "Gross margin stays above 55%", metric: "Gross margin", direction: "above", threshold: 55, unit: "%", byDate: "2027-03-31", confidence: 70 } },
    { judgment: "I expect inventory days to fall below 50 days.", reasoning: "The new orders should reduce the finished goods backlog.", check: { claim: "Inventory days fall below 50", metric: "Inventory days", direction: "below", threshold: 50, unit: " days", byDate: "2026-10-01" }, outcome: { observed: 54, observedAt: "2026-10-02", result: "did_not", note: "The quarterly filing reported 54 inventory days." } },
    { judgment: "I think operating margin will exceed 16%.", reasoning: "Higher shipments could spread fixed costs across more units.", check: { claim: "Operating margin exceeds 16%", metric: "Operating margin", direction: "above", threshold: 16, unit: "%", byDate: "2026-10-01" }, outcome: { observed: 14.8, observedAt: "2026-10-02", result: "did_not", note: "Qualification costs continued during the quarter." } },
  ].map((entry, index) => ({ ...entry, loupistId: "loupist-quality", journalId: `preview-journal-${index}`, researchId: "", originChatId: "preview-chat", entities: [], createdAtMs: Math.min(FIXTURE_TIME - (index + 1) * 86400000, Date.parse(entry.check.byDate) - 86400000), updatedAtMs: FIXTURE_TIME, archived: false, inputDigest: "preview", inputs: { sources: [], worlds: [], lenses: [], earlier: [] } })) as import("../types").JournalRecord[];
  if (previewKo()) for (const [index, copy] of [
    ["Hanul의 매출총이익률이 1분기까지 55% 위로 유지될 것 같아요.", "가동률이 회복되고 인증 비용은 줄고 있어요."],
    ["재고일수가 50일 아래로 줄어들 것 같아요.", "새 주문이 완제품 재고를 줄일 것 같아요."],
    ["영업이익률이 16%를 넘을 것 같아요.", "출하가 늘면 고정비가 더 많은 수량에 나뉘어요."],
  ].entries()) {
    entries[index].judgment = copy[0];
    entries[index].reasoning = copy[1];
    if (entries[index].outcome?.note) entries[index].outcome.note = index === 1 ? "분기 보고서의 재고일수는 54일이에요." : "분기 동안 인증 비용이 이어졌어요.";
  }
  bridge.journalList = async () => structuredClone(entries);
  const read = bridge.knowledgeJournalRead.bind(bridge);
  bridge.knowledgeJournalRead = async request => structuredClone(entries.find(entry => entry.journalId === request.journalId) ?? await read(request));
}

/**
 * The empty Chat's morning message. "morning": two days since the last visit with new names,
 * relationships, an unread briefing and two Journal checks; "first-day": a new Loupist's seed anchors;
 * "nothing-changed": a visit yesterday and nothing since.
 */
export async function prepareMorningFixture(bridge: PreviewBridge, state: "morning" | "first-day" | "nothing-changed") {
  await prepareFixture(bridge);
  const ko = previewKo();
  const hour = 3_600_000;
  const lastSeenAtMs = state === "morning" ? FIXTURE_TIME - 2 * 86_400_000 : state === "nothing-changed" ? FIXTURE_TIME - 86_400_000 : undefined;
  if (lastSeenAtMs !== undefined) recordVisit("loupist-quality", lastSeenAtMs - MIN_VISIT_MS, lastSeenAtMs);
  const localNames: Record<string, string[]> = { "map-entity-00": ["한울정밀"], "map-entity-04": ["은빛소재"], "map-entity-29": ["영업이익률"] };
  const map = bridge.knowledgeMap.bind(bridge);
  bridge.knowledgeMap = async request => {
    const result = await map(request);
    for (const node of result.nodes) {
      const index = Number(/^map-entity-(\d+)$/.exec(node.id)?.[1] ?? Number.NaN);
      if (index < companies.length) { node.label = companies[index]!; node.localNames = localNames[node.id]; }
      if (localNames[node.id] && index >= companies.length) node.localNames = localNames[node.id];
    }
    return result;
  };
  const event = (kind: import("../contracts/knowledge").GrowthEvent["kind"], subjectId: string | null, atMs: number, summary = ""): import("../contracts/knowledge").GrowthEvent =>
    ({ eventId: `morning-${kind}-${subjectId ?? atMs}`, loupistId: "loupist-quality", atMs, kind, subjectId, recordId: "preview-world", originChatId: null, originMessageId: null, actor: "loupist", summary, beforeSummary: null, afterSummary: null });
  const events = state === "morning" ? [
    event("entity_added", "map-entity-00", FIXTURE_TIME - hour), event("claim_added", "map-claim-04", FIXTURE_TIME - 2 * hour),
    event("entity_added", "map-entity-05", FIXTURE_TIME - 3 * hour), event("entity_added", "map-entity-29", FIXTURE_TIME - 4 * hour),
    event("claim_added", "map-claim-28", FIXTURE_TIME - 5 * hour), ...["03", "04", "06", "07"].map((id, i) => event("entity_added", `map-entity-${id}`, FIXTURE_TIME - (6 + i) * hour)),
    event("source_retained", null, FIXTURE_TIME - 10 * hour, ko ? "한울정밀 2026년 3분기 실적 설명자료" : sourceTitles[1]), event("schema_extended", null, FIXTURE_TIME - 11 * hour),
  ] : state === "first-day" ? ["00", "01", "02", "04", "29"].map((id, i) => event("entity_added", `map-entity-${id}`, FIXTURE_TIME - 10 * 60_000 - i)) : [];
  bridge.knowledgeGrowthEvents = async request => ({ events: events.filter(item => request.sinceMs == null || item.atMs >= request.sinceMs).sort((a, b) => a.atMs - b.atMs), nextCursor: null });
  const watchlist = [{ symbol: "HNP", market: "KRX", displayName: ko ? "한울정밀" : "Hanul Precision", addedAtMs: FIXTURE_TIME - 30 * 86_400_000 }];
  bridge.watchlistList = async () => structuredClone(watchlist);
  if (state !== "morning") {
    // A briefing already read before the last visit is not news.
    const inbox = bridge.inboxList.bind(bridge);
    bridge.inboxList = async request => {
      const page = await inbox(request);
      return { ...page, items: state === "first-day" ? [] : page.items.filter(item => item.runId === "preview-briefing-3") };
    };
    bridge.journalList = async () => [];
  } else {
    const checks: [string, string, string][] = ko
      ? [["한울정밀의 재고일수가 50일 아래로 내려간다", "재고일수", "2026-10-26"], ["한울정밀의 매출총이익률이 55% 이상을 유지한다", "매출총이익률", "2026-11-14"]]
      : [["Hanul’s inventory days fall below 50", "inventory days", "2026-10-26"], ["Hanul’s gross margin stays above 55%", "gross margin", "2026-11-14"]];
    bridge.journalList = async () => checks.map(([claim, metric, byDate], index) => ({
      journalId: `preview-morning-check-${index}`, researchId: "", loupistId: "loupist-quality", judgment: claim, reasoning: "", entities: [],
      check: { claim, metric, direction: index ? "above" : "below", threshold: index ? 55 : 50, byDate }, outcome: null, originChatId: "preview-chat",
      createdAtMs: FIXTURE_TIME - 30 * 86_400_000, updatedAtMs: FIXTURE_TIME - 30 * 86_400_000, archived: false, inputDigest: "preview",
      inputs: { sources: [], worlds: [], lenses: [], earlier: [] },
    })) as import("../types").JournalRecord[];
  }
  if (state === "first-day") {
    const philosophy = ko ? "산업 공급망" : "industrial supply chains";
    const loupists = bridge.loupistList.bind(bridge);
    bridge.loupistList = async () => (await loupists()).map(loupist => loupist.id === "loupist-quality" ? { ...loupist, philosophy } : loupist);
    const select = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => { const workspace = await select(loupistId); return loupistId === "loupist-quality" ? { ...workspace, loupist: { ...workspace.loupist, philosophy } } : workspace; };
  }
}

/**
 * The morning message's event strip (W15.9e) on the fixture day (Wed Oct 28):
 * five events in the next two weeks, as Rust orders them, or nothing ahead.
 */
export async function prepareUpcomingFixture(bridge: PreviewBridge, state: "events" | "none") {
  await prepareMorningFixture(bridge, "morning");
  if (state === "none") { bridge.assets.installUpcoming("loupist-quality", []); return; }
  const ko = previewKo();
  const asset = (symbol: string, market: string, name: string, entityId: string) => ({ assetId: mockAssetId(symbol, market), symbol, market, name, entityId });
  const hanul = asset("HNP", "KRX", "Hanul Precision", "map-entity-00");
  const eunbit = asset("EBM", "KRX", "Eunbit Materials", "map-entity-04");
  const backlog = ko ? "모로우의 수주 잔고가 계속 늘어난다" : "Morrow’s order backlog keeps growing";
  // The check in the strip is the Journal's own; the message does not repeat it as a bullet.
  const journal = bridge.journalList.bind(bridge);
  bridge.journalList = async request => [...await journal(request), {
    journalId: "preview-upcoming-check", researchId: "", loupistId: "loupist-quality", judgment: backlog, reasoning: "", entities: [],
    check: { claim: backlog, metric: ko ? "수주 잔고" : "order backlog", direction: "above", threshold: 0, byDate: "2026-11-03" }, outcome: null, originChatId: "preview-chat",
    createdAtMs: FIXTURE_TIME - 20 * 86_400_000, updatedAtMs: FIXTURE_TIME - 20 * 86_400_000, archived: false, inputDigest: "preview",
    inputs: { sources: [], worlds: [], lenses: [], earlier: [] },
  } as import("../types").JournalRecord];
  bridge.assets.installUpcoming("loupist-quality", [
    { kind: "results", date: "2026-10-29", asset: asset("MRRW", "NASDAQ", "Morrow Components", "map-entity-01"), dateSource: "calendar", eventId: "ern_0123456789abcdef", period: "Q3 2026" },
    { kind: "exDividend", date: "2026-10-30", asset: asset("BLHV", "NYSE", "Bluehaven Logistics", "map-entity-02") },
    { kind: "results", date: "2026-11-02", asset: hanul, dateSource: "ir_notice", eventId: "ern_1123456789abcdef", period: "Q3 2026" },
    { kind: "check", date: "2026-11-03", journalId: "preview-upcoming-check", claim: backlog },
    { kind: "recordDate", date: "2026-11-09", asset: eunbit },
  ]);
}

/** Paired attributed reports, shared by the inspector and briefing navigation previews. */
export async function prepareConflictsFixture(bridge: PreviewBridge) {
  await prepareFixture(bridge, "answer");
  const map = await bridge.knowledgeMap({ loupistId: "loupist-quality" });
  const nodes = structuredClone(previewNodes);
  const endpoints = [{ nodeId: nodes[0].id, role: "subject" }, { nodeId: nodes[1].id, role: "object" }];
  const edges = ["Hanul Precision expanded supplier capacity by 18% in the third quarter.", "Morrow Components reports unchanged capacity for Hanul Precision in the same quarter."].map((proposition, index): GraphEdge => ({
    id: `conflict-${index}`, relation: "SUPPLIES", endpoints, status: "disputed",
    claim: { id: `conflict-${index}`, relation: "SUPPLIES", endpoints, status: "disputed", proposition, attribution: sourceTitles[index], qualifiers: [], evidence: [{ spanId: `conflict-span-${index}`, role: "supports", attribution: sourceTitles[index] }], publicationTime: { precision: "date", value: `2026-10-${27 - index}` } },
  }));
  map.nodes = nodes.map(node => ({ id: node.id, label: node.name, kind: "entity", entityKind: node.kind, degree: 3, watched: node.id === nodes[0].id, held: false }));
  map.edges = [...previewEdges.slice(2), ...edges].map(edge => ({ id: edge.id, kind: "claim", source: edge.endpoints[0].nodeId, target: edge.endpoints[1].nodeId, status: edge.status, relation: edge.relation }));
  map.worldRevisionId = "preview-conflicts";
  map.counts.entity = { total: nodes.length, shown: nodes.length, truncated: false };
  map.counts.claim = { total: map.edges.length, shown: map.edges.length, truncated: false };
  bridge.knowledgeMap = async () => structuredClone(map);
  bridge.knowledgeWorldQuery = async request => structuredClone({ recordId: request.recordId, ...revision, revisionId: request.revisionId, nodes, edges, coverage: "sealed_snapshot", truncated: false, sourceTargets: Object.fromEntries(edges.map((_, index) => [`conflict-span-${index}`, { recordId: "preview-sources", revisionId: revision.revisionId, sourceId: `preview-source-${index}`, spanId: `conflict-span-${index}`, worldRecordId: request.recordId, worldRevisionId: request.revisionId }])) });
  const inbox = bridge.inboxList.bind(bridge);
  bridge.inboxList = async request => { const page = await inbox(request); return { ...page, items: page.items.map(item => item.briefingContext ? { ...item, briefingContext: { ...item.briefingContext, disputedEntityIds: endpoints.map(endpoint => endpoint.nodeId) } } : item) }; };
  const runs = bridge.scheduledTaskRuns.bind(bridge);
  bridge.scheduledTaskRuns = async request => (await runs(request)).map(run => run.briefing_context ? { ...run, briefing_context: { ...run.briefing_context, disputedEntityIds: endpoints.map(endpoint => endpoint.nodeId) } } : run);
}
