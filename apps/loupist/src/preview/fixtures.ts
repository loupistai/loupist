import type { InboxItem, InboxRequest, InboxPage, ScheduledRun, ScheduledTask, ScheduledTaskListItem } from "../contracts/scheduled";
import { MockWorldSchema } from "../bridge/mock/worldSchema";
import { MockLoupistBridge } from "../bridge/mockBridge";
import { previewLearned, previewPages } from "./pages";
import type { ChatMessage, ChatSendRequest, ChatStreamEvent, GraphEdge, GraphNode, KnowledgeCatalog, KnowledgeLensReadRequest, LearnedDigest, LensKind, PageSummary, PageView, StreamObserver, WorldGraphQuery } from "../types";

export const previewKo = () => typeof document !== "undefined" && document.documentElement.lang.startsWith("ko");

/** The preview's frozen "now"; the capture script also freezes the page clock to it before any module runs. */
export const FIXTURE_TIME = Date.parse("2026-10-28T00:00:00Z");
const LONG_TITLE = "Hanul Precision — 2026 third-quarter operating review: capacity expansion, customer concentration and the cash conversion cycle";
const METHOD_TITLE = "Separate durable margins from temporary capacity costs — a research method for Hanul Precision and its overseas suppliers";
const TASK_TITLE = "Morning review · Hanul Precision filings";
const KOREAN_TASK_TITLE = "아침 검토 · 한울정밀 공시";
export const revision = { revisionId: "preview-revision", revisionDigest: "a".repeat(64), readOnly: false, indexingState: "snapshot_only" as const };
export const sourceTitles = [LONG_TITLE, "Hanul Precision 2026 third-quarter results presentation", "Morrow Components · supplier capacity survey", "Bluehaven Logistics — freight and inventory update", "Eunbit Materials: new plant utilization and orders", "Cedar Robotics · customer demand outlook"];
const previewCatalog: KnowledgeCatalog = {
  loupistId: "loupist-quality",
  worlds: [{ ...revision, recordId: "preview-world", kind: "world", title: "Industrial supply chain", objects: [] }],
  lenses: [
    { ...revision, recordId: "preview-perspective", kind: "lens", lensKind: "perspective", title: "Evidence before conviction", objects: [] },
    { ...revision, recordId: "preview-method", kind: "lens", lensKind: "method", title: METHOD_TITLE, summary: "Compare margins, utilization and cash conversion before interpreting a single quarter.", updatedAtMs: FIXTURE_TIME, objects: [] },
    { ...revision, recordId: "preview-subject", kind: "lens", lensKind: "subject", title: "Hanul Precision", summary: "A precision-parts maker adding capacity while margins thin.", updatedAtMs: FIXTURE_TIME - 3_600_000, learned: true, objects: [] },
    { ...revision, recordId: "preview-idea", kind: "lens", lensKind: "idea", title: "Margins recover once utilization passes 80%", summary: "An unverified link: fixed costs dilute as the new line fills.", updatedAtMs: FIXTURE_TIME - 3_000_000, learned: true, objects: [] },
  ],
  sources: [{ ...revision, recordId: "preview-sources", kind: "source", title: "Company filings and field notes", objects: sourceTitles.map((title, i) => ({ id: `preview-source-${i}`, title, summary: i === 0 ? "Revenue ₩184.2bn, operating margin 14.8%; new capacity enters qualification." : "Fictional research material for layout review.", originLabel: i % 2 ? "Hanul Research" : "Company investor relations", acquiredAt: { precision: "instant", value: `2026-10-${28 - i}T00:00:00Z` }, publicationTime: { precision: "date", value: `2026-10-${27 - i}` }, processing: { jobId: null, status: i === 1 ? "running" : "succeeded", error: null } })) }],
};
export const companies = ["Hanul Precision", "Morrow Components", "Bluehaven Logistics", "Cedar Robotics", "Eunbit Materials", "Northstar Foundry", "Orchard Sensors", "Dawn Circuits"];
const concepts = ["Operating margin", "Capacity utilization", "Customer concentration", "Inventory days", "Cash conversion", "Export demand", "New production line", "Freight costs", "Order backlog", "Pricing power", "Working capital", "Automation", "Supplier qualification", "Energy costs", "Return on capital", "Quality yield"];
export const previewNodes: GraphNode[] = [...companies, ...concepts].map((name, i) => ({ id: `preview-node-${i}`, name, aliases: [], description: `${name} in the fictional industrial supply chain.`, kind: i < companies.length ? "Organization" : "Entity", sourceRole: i < companies.length ? "organization" : "concept", properties: {} }));
export const previewEdges: GraphEdge[] = Array.from({ length: 32 }, (_, i) => {
  const source = previewNodes[i % 24];
  const target = previewNodes[i < 24 ? (i + 1) % 24 : 8 + i % 16];
  const relation = i % 2 ? "depends_on" : "influences";
  const endpoints = [{ nodeId: source.id, role: "subject" }, { nodeId: target.id, role: "object" }];
  return { id: `preview-claim-${i}`, relation, endpoints, status: "active", claim: { id: `preview-claim-${i}`, proposition: `${source.name} ${relation.replaceAll("_", " ")} ${target.name.toLowerCase()}.`, attribution: "Fictional Q3 operating review", relation, endpoints, qualifiers: [], evidence: [{ spanId: "preview-span", role: "supports", attribution: "Company report" }], status: "active", recordedAtMs: null, publicationTime: { precision: "date", value: "2026-10-27" }, effectiveTime: null, validAt: null, invalidAt: null, expiredAt: null } };
});
/** The paging and scan fields of a one-page World read. */
export const previewWorldPage = { nextCursor: null, appliedCursor: null, originLabel: "", sourceTargets: {}, scan: { strategy: "preview", nodesScanned: 0, claimsScanned: 0 } };
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
    ahead: [{ journalId: "preview-journal-1", title: "Inventory days below 50", byDate: "2026-11-04" }], assets: [] }, runId: "preview-briefing-1", taskId: "preview-briefing", kind: "briefing", title: "Briefings", status: "succeeded", summary: "October 28: Hanul Precision reported higher qualification costs and a lower operating margin. The new capacity figures conflict with the saved utilization rule.", startedAtMs: FIXTURE_TIME, finishedAtMs: FIXTURE_TIME + 12000, readAtMs: null, threadId: "preview-chat", messageId: "preview-answer", skippedTools: [], error: null, summaryItems: [] },
  { runId: "preview-custom-1", taskId: "preview-task-0", kind: "custom", title: TASK_TITLE, status: "succeeded", summary: "October 27: Supplier delivery times fell by two days. The scheduled run used retained Sources; one requested check was unavailable.", startedAtMs: FIXTURE_TIME - 86400000, finishedAtMs: FIXTURE_TIME - 86390000, readAtMs: null, threadId: "preview-chat", messageId: "preview-answer", skippedTools: [], briefingContext: null, error: null, summaryItems: [] },
  { runId: "preview-briefing-2", taskId: "preview-briefing", kind: "briefing", title: "Briefings", status: "failed", error: "The model provider could not be reached. Please try again.", summary: "", startedAtMs: FIXTURE_TIME - 2*86400000, finishedAtMs: FIXTURE_TIME - 2*86400000 + 5000, readAtMs: FIXTURE_TIME - 86400000, threadId: "preview-chat", messageId: null, skippedTools: [], briefingContext: null, summaryItems: [] },
  { runId: "preview-briefing-3", taskId: "preview-briefing", kind: "briefing", title: "Briefings", status: "succeeded", summary: "October 25: No material changes appeared in the available company Sources.", startedAtMs: FIXTURE_TIME - 3*86400000, finishedAtMs: FIXTURE_TIME - 3*86400000 + 12000, readAtMs: FIXTURE_TIME - 2*86400000, threadId: "preview-chat", messageId: "preview-answer", skippedTools: [], briefingContext: null, error: null, summaryItems: [] },
  { runId: "preview-custom-2", taskId: "preview-task-1", kind: "custom", title: "Weekly supplier capacity comparison", status: "succeeded", summary: "October 24: Morrow Components disclosed an additional production shift; the announcement did not specify the added capacity.", startedAtMs: FIXTURE_TIME - 4*86400000, finishedAtMs: FIXTURE_TIME - 4*86400000 + 12000, readAtMs: FIXTURE_TIME - 3*86400000, threadId: "preview-chat", messageId: "preview-answer", skippedTools: [], briefingContext: null, error: null, summaryItems: [] },
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
  override async scheduledTasksList({ loupist_id }: { loupist_id: string }): Promise<ScheduledTaskListItem[]> {
    if (this.scheduleTasks === "none") return [];
    const runs = this.previewRuns();
    const weekly = { kind: "weekly" as const, days: [1, 2, 3, 4, 5], hour: 8, minute: 0, timezone: "Asia/Seoul" };
    const task = (id: string, kind: "briefing" | "custom", title: string, schedule: ScheduledTask["schedule"], status: ScheduledTask["status"]): ScheduledTaskListItem => ({
      kind, id, loupist_id, revision: 1, title, prompt: title, schedule, target: kind === "briefing" ? { kind: "dedicated_chat" } : { kind: "new_chat" },
      model_profile_id: "preview", thinking_level: "medium", run_options: {}, use_memory: true, update_memory: kind === "briefing", status,
      next_run_at_ms: status === "active" ? nextSeoulRun(schedule) : null, last_run_at_ms: runs.find(run => run.task_id === id)?.started_at_ms ?? null,
      created_at_ms: FIXTURE_TIME - 30 * 86_400_000, updated_at_ms: FIXTURE_TIME, latest_run: runs.find(run => run.task_id === id) ?? null,
      last_error: null,
      session_thread_id: null,
      awaiting_confirmation: false,
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
    return structuredClone({ recordId: request.recordId, ...revision, nodes: previewNodes, edges: previewEdges, coverage: "sealed_snapshot" as const, truncated: false, ...previewWorldPage });
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
    observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: Date.now() - 2400, items: [{ id: "preview-read", kind: "tool", toolName: "world_read", category: "memory", operation: "read", action: "Read saved notes", status: "succeeded", target: null, href: null, startedAtMs: 0, finishedAtMs: null }], finishedAtMs: null } });
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
