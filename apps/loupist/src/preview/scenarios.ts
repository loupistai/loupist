import i18n from "../i18n";
import { growthReplayPending, prepareGrowthReplay } from "./growth-replay";
import { prepareCoreSchema } from "./core-schema";
import { semiconductorTrackingView } from "../bridge/mock/worldSchema";
import { advancePreviewClock, previewDelivery, previewKo, prepareConflictsFixture, prepareConnectionSetupFixture, prepareJournalFixture, prepareLearningFixture, prepareCustomConnectionFixture, prepareFixture, prepareMorningFixture, prepareUpcomingFixture, PreviewBridge } from "./fixtures";
import type { ChatApproval, ChatProgressItem, ChatRunPhase } from "../contracts/chat";
import type { GrowthEvent, KnowledgeMapEdge, KnowledgeMapNode } from "../contracts/knowledge";
import { MIN_VISIT_MS, recordVisit } from "../app/loupistVisit";
import { MOCK_PACK_PREVIEW } from "../bridge/mock/sharing";
import { rememberFocal, rememberLegendOpen, rememberScope } from "../components/map/mapVisit";
import type { MapScope } from "../components/map/localView";
import { workspaceOverlayMedia } from "../components/workspace/useWorkspacePanelResize";
import { signInTiming } from "../components/settings/signIn";
import type { ProviderConnectFailure } from "../contracts/model";
import { assetScenarios } from "./assetScenarios";
import { answerCheckScenarios } from "./answerCheckScenarios";
import { gridScenarios } from "./gridScenarios";
import { financeScenarios } from "./financeScenarios";
import { koreaScenarios } from "./koreaScenarios";
import { holdingsScenarios } from "./holdingsScenarios";
import { dataScenarios } from "./dataScenarios";
import { browserScenarios } from "./browserScenarios";
import { quoteEvidenceScenarios } from "./quoteEvidenceScenarios";
import { settingsScenarios } from "./settingsScenarios";
import { readerScenarios } from "./readerScenarios";
import { liveResearchRun } from "./liveScenario";

export type PreviewScenario = {
  id: string;
  title: string;
  setup: (bridge: PreviewBridge) => Promise<void>;
  enter: (root: HTMLElement) => Promise<void>;
  landmark: () => Element | null;
  /** Captures must retain a live DOM range after fonts and lazy chunks settle. */
  selection?: boolean;
  /** "print": the capture emulates print media, as the print dialog draws the page. */
  media?: "print";
};

// Check rendered visibility without requiring layout geometry (jsdom has none).
export function visible(element: Element | null): element is HTMLElement {
  if (!element || element.closest('[hidden], [aria-hidden="true"], [inert]')) return false;
  for (let node: Element | null = element; node; node = node.parentElement) {
    const style = getComputedStyle(node);
    if (style.display === "none" || style.visibility === "hidden" || style.opacity === "0") return false;
  }
  // Closed details and overflow-folded controls can still report display:block.
  // jsdom has no layout; a browser must actually render a box for the landmark.
  if (document.documentElement.clientWidth > 0 && ![...element.getClientRects()].some(rect => rect.width > 0 && rect.height > 0)) return false;
  return true;
}
const select = (selector: string, root: ParentNode = document) => [...root.querySelectorAll(selector)].find(visible) ?? null;
// jsdom has no layout: the Map canvas never mounts and every box measures zero.
const hasLayout = () => document.documentElement.clientWidth > 0;
// A mounted message can still be the lazy Markdown fallback. Gate interactions
// and landmarks so scrolling and measurements use the final rendered answer.
const messagesRendered = () => !hasLayout() || (!document.querySelector(".message-response-fallback") && [...document.querySelectorAll(".message .message-content")].every(body => !body.textContent?.includes("**") && !body.textContent?.includes("](#evidence-")));
const mapCanvas = () => !hasLayout() || select(".map-canvas canvas");
export async function until<T>(read: () => T | null | false | undefined, reason: string, timeout = 7500): Promise<T> {
  const deadline = performance.now() + timeout;
  while (performance.now() < deadline) {
    const result = read();
    if (result) return result;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(reason);
}
async function click(selector: string, root: ParentNode = document) {
  await until(messagesRendered, "Message Markdown did not finish rendering");
  const target = await until(() => {
    const node = select(selector, root);
    return node && !node.hasAttribute("disabled") ? node : null;
  }, `Control not visible: ${selector}`);
  target.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
  (target as HTMLElement).click();
}
/** Opens a Radix menu: it opens on pointer down, as a mouse press does (jsdom has no PointerEvent). */
function pressMenu(trigger: Element | null | undefined) {
  const init = { bubbles: true, button: 0 };
  trigger?.dispatchEvent(typeof PointerEvent === "function" ? new PointerEvent("pointerdown", { ...init, pointerType: "mouse" }) : new MouseEvent("pointerdown", init));
}
/** Clicks a Map toolbar action, through "More" when a narrow toolbar has folded it there. */
async function clickMapAction(selector: string, root: ParentNode = document) {
  await until(() => root.querySelector(selector), `Map action did not initialize: ${selector}`);
  if ([...root.querySelectorAll(selector)].some(visible)) return click(selector, root);
  const more = await until(() => select("[data-map-more]", root), "More actions not visible");
  // The menu opens on pointer down, as a mouse press does.
  pressMenu(more);
  await click(`[role="menu"] ${selector}`);
}
async function openNavigation(selector: string) {
  if (!select(selector)) await click('[data-slot="sidebar-trigger"]');
}
async function openLoupistSettings() {
  await until(() => select(".loupist-header-identity"), "Loupist did not initialize");
  await click("[data-header-loupist-settings]");
}
async function selectPanel(kind: "lens" | "world" | "materials") {
  pressMenu(await until(() => select(".header-panel-toggle"), "Panel control did not initialize"));
  await click(`[data-workspace-kind="${kind}"]`);
}
async function navigate(root: HTMLElement, tab: "map" | "lens" | "schedule" | "settings") {
  await until(() => root.querySelector('textarea[data-chat-composer]'), "Chat did not initialize");
  if (tab === "map" || tab === "lens") { await selectPanel(tab === "map" ? "world" : "lens"); return; }
  if (tab === "schedule") {
    await openNavigation(".loupist-chat-row[data-home]");
    const more = await until(() => document.querySelector<HTMLButtonElement>("[data-bot-menu]"), "Bot menu did not initialize");
    more.focus();
    if (hasLayout()) await until(() => visible(more), "Bot menu did not become visible");
    pressMenu(more);
    await click("[data-bot-schedule]");
    return;
  }
  await openNavigation("#main-tab-settings");
  await click("#main-tab-settings");
}
/** Opens one run of a scheduled task in the Schedule reader. */
async function openScheduleRun(root: HTMLElement, runId: string) {
  await navigate(root, "schedule");
  await click('[data-schedule-task="preview-briefing"] .schedule-task-summary', root);
  await click(`[data-schedule-run="${runId}"]`, root);
}
async function settings(root: HTMLElement, section: string) {
  await navigate(root, "settings");
  await openNavigation(`#settings-section-${section}`);
  await click(`#settings-section-${section}`);
}
const idle = async () => {};
/** A reply that stays in flight on a running step; `phase: "tool"` sends no answer text. */
function liveRun(bridge: PreviewBridge, phase: ChatRunPhase) {
  bridge.chatSend = async (request, observer) => {
    const run_id = request.run_id;
    observer({ type: "started", run_id });
    observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: Date.now() - 12_000, items: [
      { id: "preview-read", kind: "tool", toolName: "world_read", category: "memory", operation: "read", action: "Read saved notes", target: "Hanul Precision operating report", status: "succeeded" },
      { id: "preview-page", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", running: "Reading hanul.example", target: "Q3 operating review · hanul.example", status: "running" },
    ] } });
    observer({ type: "phase", run_id, phase });
    if (phase === "writing") observer({ type: "delta", run_id, text: "Hanul Precision’s revenue grew 11%, while operating margin declined. I’m checking the new line’s utilization…" });
    return { run_id };
  };
}
/** A reply that stays in flight while the Loupist publishes a chart, so the answer holds its reserved card. */
function chartRun(bridge: PreviewBridge) {
  bridge.chatSend = async (request, observer) => {
    const run_id = request.run_id;
    observer({ type: "started", run_id });
    observer({ type: "phase", run_id, phase: "writing" });
    observer({ type: "delta", run_id, text: "Utilization rose through the quarter while operating margin fell. The chart below puts the two side by side." });
    observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: Date.now() - 9_000, items: [
      { id: "preview-read", kind: "tool", toolName: "world_read", category: "memory", operation: "read", action: "Read saved notes", target: "Hanul Precision operating report", status: "succeeded" },
      { id: "preview-chart", kind: "tool", toolName: "chart_publish", category: "artifact", operation: "publish", action: "Made a chart", running: "Making a chart", target: "Margin vs utilization", status: "running" },
    ] } });
    observer({ type: "phase", run_id, phase: "tool" });
    return { run_id };
  };
}
/** A research-sized reply mid-plan: one item done, one active with two steps (one building a chart), one pending. */
function planRun(bridge: PreviewBridge) {
  bridge.chatSend = async (request, observer) => {
    const run_id = request.run_id;
    const ko = document.documentElement.lang.startsWith("ko");
    const start = Date.now() - 14_000;
    const labels = ko
      ? ["3분기 영업 보고서 읽기", "가동률과 재고일수 비교하기", "원칙과 대조하기"]
      : ["Read the Q3 operating report", "Compare utilization with inventory days", "Check against your rules"];
    observer({ type: "started", run_id });
    observer({ type: "phase", run_id, phase: "writing" });
    observer({ type: "delta", run_id, text: ko
      ? "한울정밀의 매출은 11% 늘었지만 영업이익률은 낮아졌어요. 지금 가동률과 재고일수를 비교하고 있어요…"
      : "Hanul Precision’s revenue grew 11%, while operating margin declined. I’m comparing utilization with inventory days…" });
    observer({ type: "progress", run_id, sequence: 1, progress: {
      startedAtMs: start,
      plan: { items: [
        { id: "report", label: labels[0]!, status: "done", activatedAtMs: start + 500 },
        { id: "compare", label: labels[1]!, status: "active", activatedAtMs: start + 6_000 },
        { id: "principles", label: labels[2]!, status: "pending" },
      ] },
      items: [
        { id: "preview-read", kind: "tool", toolName: "world_read", category: "memory", operation: "read", action: "Read saved notes", target: "Hanul Precision operating report", status: "succeeded", startedAtMs: start + 1_000, finishedAtMs: start + 3_000 },
        { id: "preview-search", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", target: "Hanul Precision inventory days", status: "succeeded", startedAtMs: start + 6_500, finishedAtMs: start + 8_000 },
        { id: "preview-chart", kind: "tool", toolName: "chart_publish", category: "artifact", operation: "publish", action: "Made a chart", running: "Making a chart", target: "Margin vs utilization", status: "running", startedAtMs: start + 9_000 },
      ],
    } });
    observer({ type: "phase", run_id, phase: "tool" });
    return { run_id };
  };
}
/** The steps of a reply that read saved notes and then asked to use a connection or a site. */
function approvalSteps(at: number, kind: "connection" | "site", status: "running" | "succeeded"): ChatProgressItem[] {
  const finished = status === "succeeded" ? { finishedAtMs: at - 4_000 } : {};
  return [
    { id: "preview-read", kind: "tool", toolName: "world_read", category: "memory", operation: "read", action: "Read saved notes", target: "Hanul Precision operating report", status: "succeeded", startedAtMs: at - 28_000, finishedAtMs: at - 26_000 },
    kind === "site"
      ? { id: "preview-site", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", running: "Reading markets.example.com", target: "markets.example.com", status, startedAtMs: at - 9_000, ...finished }
      : { id: "preview-filings", kind: "tool", toolName: "mcp__custom-morrow__read_company_filings", category: "finance", operation: "read", action: "Read company filings", running: "Reading company filings", source: { kind: "connector", name: "Morrow research tools" }, target: "Hanul Precision · 2026 Q3", status, startedAtMs: at - 12_000, ...finished },
  ];
}
const approvalFor = (kind: "connection" | "site", state: ChatApproval["state"], at: number): ChatApproval => kind === "site"
  ? { requestId: "req-web", kind: "web_site", itemId: "preview-site", connectionId: "web", connectionName: "Web", toolName: "web_fetch", target: "markets.example.com/morrow-components/q3-2026-results", state, requestedAtMs: at - 9_000 }
  : { requestId: "req-1", itemId: "preview-filings", connectionId: "custom-morrow", connectionName: "Morrow research tools", toolName: "read_company_filings", target: "Hanul Precision · 2026 Q3", state, requestedAtMs: at - 12_000 };
/** A reply paused on a read-only connection or a new website until the user decides. */
function approvalRun(bridge: PreviewBridge, kind: "connection" | "site") {
  bridge.chatSend = async (request, observer) => {
    const run_id = request.run_id;
    const at = Date.now();
    observer({ type: "started", run_id });
    observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: at - 30_000, items: approvalSteps(at, kind, "running"), approvals: [approvalFor(kind, "waiting", at)] } });
    observer({ type: "phase", run_id, phase: "tool" });
    return { run_id };
  };
}
/**
 * The preview Chat exists only in the fixture's workspace, and the mock reads
 * artifacts from its own store. Back it with a stored Chat so the published
 * chart can be read into its card.
 */
async function inlineChartFixture(bridge: PreviewBridge) {
  await prepareFixture(bridge, "answer");
  const [loupist] = await bridge.loupistList();
  const stored = await bridge.chatCreate({ loupist_id: loupist!.id, title: "Reading Hanul Precision’s margin decline" });
  const storedId = (id: string) => id === "preview-chat" ? stored.id : id;
  const send = bridge.chatSend.bind(bridge);
  bridge.chatSend = (request, observer) => send({ ...request, thread_id: storedId(request.thread_id) }, observer);
  const read = bridge.chatArtifactRead.bind(bridge);
  bridge.chatArtifactRead = (loupistId, chatId, artifactId, revisionId) => read(loupistId, storedId(chatId), artifactId, revisionId);
}
/** The saved answer three times, so the thread overflows even at 1680×1050 and the reader can scroll away. */
function longThread(bridge: PreviewBridge) {
  const select = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async (loupistId) => {
    const workspace = await select(loupistId);
    const repeat = (messages: typeof workspace.threads[number]["messages"]) => [1, 2].flatMap(copy => messages.map(message => ({ ...message, id: `earlier-${copy}-${message.id}` })));
    return { ...workspace, threads: workspace.threads.map((thread, index) => index ? thread : { ...thread, messages: [...repeat(thread.messages), ...thread.messages] }) };
  };
}
async function submitPrompt(root: HTMLElement, prompt = previewKo() ? "한울정밀의 가동률과 재고일수를 비교해 줘." : "Compare Hanul Precision’s utilization and inventory days.") {
  const composer = await until(() => root.querySelector<HTMLTextAreaElement>('textarea[data-chat-composer]'), "Composer did not appear");
  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, prompt);
  composer.dispatchEvent(new Event("input", { bubbles: true }));
  await click('.composer-shell button[type="submit"]', root);
}
/**
 * The print sheet shows only in print media, which the capture emulates; jsdom
 * has no print media, so there the sheet's printed document stands in, present
 * and filled, though hidden on screen.
 */
const printLandmark = () => {
  const printed = document.querySelector<HTMLElement>(".print-sheet .print-document");
  if (!hasLayout()) return printed?.textContent?.trim() && document.documentElement.hasAttribute("data-printing") ? select(".chat-workspace, .briefings-reader") : null;
  return select(".print-sheet .print-document");
};
/** jsdom has no scroll geometry; those runs check the scenario reaches the thread. */
// The live dot is aria-hidden, so landmarks use its visible run header.
const runningHeader = () => select('.chat-progress[data-run-status="streaming"]') ?? select(".message.assistant.streaming");
const stalledHeader = () => { const header = runningHeader(); return header?.querySelector("[data-heartbeat]") ? header : null; };
async function scrollThreadUp() {
  const thread = await until(() => select(".message-scroll"), "Thread did not appear");
  if (!hasLayout()) return;
  await until(() => thread.scrollHeight - thread.clientHeight > 120, "Thread did not overflow");
  thread.scrollTop = 0;
  thread.dispatchEvent(new Event("scroll"));
  // Let the pill's 180 ms entrance finish before the capture.
  await new Promise((resolve) => setTimeout(resolve, 300));
}

/** This chat's research map: four names around one hub, with `events` as the growth this chat added. */
function chatResearchMap(bridge: PreviewBridge, events: () => GrowthEvent[], names = ["Hanul Precision", "Morrow Components", "Capacity utilization", "Operating margin"]) {
  const readMap = bridge.knowledgeMap.bind(bridge);
  bridge.knowledgeMap = async request => {
    const map = await readMap(request);
    map.nodes = names.map((label, index) => ({ id: `chat-node-${index}`, kind: "entity", label, entityKind: index < 2 ? "Organization" : "Entity", degree: index === 0 ? 3 : 1, watched: false, held: false }));
    map.edges = [1, 2, 3].map(index => ({ id: `chat-edge-${index}`, kind: "claim", source: "chat-node-0", target: `chat-node-${index}`, relation: "related_to", status: "active" }));
    map.worldRevisionId = "chat-research-map";
    return map;
  };
  bridge.knowledgeGrowthEvents = async () => ({ events: events(), nextCursor: null });
}
const chatGrowth = (index: number, originMessageId: string, atMs: number): GrowthEvent => ({ eventId: `chat-growth-${index}`, loupistId: "loupist-quality", originChatId: "preview-chat", originMessageId, atMs, kind: "entity_added", subjectId: `chat-node-${index}`, recordId: "preview-world", actor: "loupist", summary: "", beforeSummary: null, afterSummary: null });
/** The chat's first answer is the one that adds names; its growth appears once it completes. */
const firstMapChange = { messageId: "" };

/** What waits for the user, as Rust's `attention_list` reports it: an approval, an answer that asked a question, briefing conflicts and a check due soon. */
function prepareAttention(bridge: PreviewBridge) {
  const ko = previewKo();
  bridge.attentionItems = [
    { kind: "approval", loupistId: "loupist-quality", loupistName: "Quality Compounder", chatId: "preview-chat", title: ko ? "기업 공시 · read_filing" : "Company filings · read_filing", atMs: Date.now() },
    { kind: "question", loupistId: "loupist-quality", loupistName: "Quality Compounder", chatId: "preview-chat-1", title: ko ? "모로 부품 공급망 점검" : "Morrow Components supplier check", atMs: Date.now() - 600_000 },
    { kind: "briefingConflict", loupistId: "loupist-quality", loupistName: "Quality Compounder", count: 2, title: "", atMs: Date.now() - 3_600_000 },
    { kind: "checkDue", loupistId: "loupist-quality", loupistName: "Quality Compounder", recordId: "preview-journal", title: ko ? "한울정밀의 재고일수가 50일 아래로 내려간다" : "Hanul’s inventory days fall below 50", dueDate: new Date(Date.now() + 2 * 86_400_000).toISOString().slice(0, 10), atMs: Date.now() },
  ];
}

/** A document with three kept versions (side-panel.md §3.2); Restore appends the chosen one as the newest. */
async function versionedDocument(bridge: PreviewBridge) {
  const ko = previewKo();
  const title = ko ? "한울정밀 3분기 메모" : "Hanul Precision Q3 notes";
  const bodies = ko ? [
    "새 생산라인의 인증 비용이 늘면서 영업이익률이 14.8%로 낮아졌어요.",
    "영업이익률이 14.8%로 낮아졌어요. 인증 비용이 대부분을 설명하고, 가동률은 4분기에 회복될 것으로 보여요.",
    "영업이익률이 14.8%로 낮아졌어요. 인증 비용이 대부분을 설명하지만, 재고일수 54일은 수요가 아직 따라오지 못했다는 뜻이에요.",
  ] : [
    "Operating margin fell to 14.8% as qualification costs at the new line rose.",
    "Operating margin fell to 14.8%. Qualification costs explain most of it; utilization should recover in Q4.",
    "Operating margin fell to 14.8%. Qualification costs explain most of it, but 54 inventory days say demand has not caught up yet.",
  ];
  const day = (revision: number) => Date.parse("2026-10-26T05:30:00Z") + revision * 86_400_000;
  // Each kept revision and the version its content came from.
  const kept = new Map<number, { body: number; from?: number }>([[1, { body: 0 }], [2, { body: 1 }], [3, { body: 2 }]]);
  const current = () => Math.max(...kept.keys());
  const view = (revision: number) => ({
    artifact: { id: "work-versions", chat_session_id: "preview-chat", kind: "markdown" as const, title, current_revision: current(), created_at_ms: day(1), updated_at_ms: day(current()) },
    revision: { artifact_id: "work-versions", revision, payload: { kind: "markdown" as const, schema: "loupist-markdown/1" as const, markdown: `# ${title}\n\n${bodies[kept.get(revision)!.body]}` }, digest: String(revision).repeat(64), source_message_id: `preview-answer-${revision}`, created_at_ms: day(revision) },
  });
  await prepareFixture(bridge, "answer");
  bridge.loupistArtifactList = async () => [{ artifact: view(current()).artifact, chat_title: ko ? "한울정밀 영업이익률 하락 읽기" : "Reading Hanul Precision’s margin decline" }];
  bridge.chatArtifactRead = async (_loupistId, _chatId, _artifactId, revisionId) => view(revisionId ?? current());
  bridge.chatArtifactVersions = async () => [...kept.keys()].sort((a, b) => b - a).map(revision => ({ revision_id: revision, title, digest: String(revision).repeat(64), source_message_id: `preview-answer-${revision}`, created_at_ms: day(revision), ...(kept.get(revision)!.from ? { restored_from_revision_id: kept.get(revision)!.from } : {}) }));
  bridge.chatArtifactRestore = async ({ revision_id }) => {
    kept.set(current() + 1, { body: kept.get(revision_id)!.body, from: revision_id });
    return view(current());
  };
}
const openVersionedDocument = async (root: HTMLElement) => {
  await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
  await selectPanel("materials");
  await click("#workspace-tab-work");
  await click(".work-list button.work-row");
  await until(() => select("[data-work-versions]"), "The version control did not show");
};
/** The menu opens on pointer down, as a mouse press does. */
const openVersionMenu = async () => {
  (await until(() => select("[data-work-versions]"), "The version control did not show")).dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
  await until(() => select('.work-version-menu [role="menuitemradio"]'), "The versions did not list");
};
const chooseEarliestVersion = async () => {
  await openVersionMenu();
  [...document.querySelectorAll<HTMLElement>('.work-version-menu [role="menuitemradio"]')].at(-1)!.click();
  await until(() => select("[data-work-version-bar]"), "The earlier version did not open");
};

/** The badge dot itself is aria-hidden; the tab that carries it is the visible landmark. */
const badgedTab = () => [...document.querySelectorAll(".workspace-tab-badge")].map(badge => badge.closest('[role="tab"]')).find((tab): tab is HTMLElement => visible(tab)) ?? null;
/** The sourced answer with a long retained excerpt whose cited passage sits mid-document.
 *  A website citation opens the browser; a filing opens the retained reader directly. */
async function citedAnswerFixture(bridge: PreviewBridge, secondSource = false, web = true) {
  await prepareFixture(bridge, "answer");
  const original = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async loupistId => {
    const workspace = await original(loupistId);
    const message = workspace.threads[0]?.messages.find(item => item.id === "preview-answer");
    const evidence = message?.evidence_refs?.[0];
    if (!message || !evidence) return workspace;
    const encoder = new TextEncoder();
    const filler = Array.from({ length: 14 }, (_, index) => `Section ${index + 1}. Fictional operating detail for layout review: shipments, staffing and supplier qualification remained on plan for the quarter.`).join("\n\n");
    const passage = "Operating margin was 14.8%, down from 16.2%, mainly because of qualification costs at the new production line.";
    const excerpt = `${filler}\n\n${passage}\n\n${filler}`;
    const start = encoder.encode(`${filler}\n\n`).length;
    Object.assign(evidence, { excerpt, excerpt_match: { start, end: start + encoder.encode(passage).length }, ...(web ? {} : { tool_name: "finance_market_data" }) });
    if (secondSource) {
      const id = `ev_${"c".repeat(32)}`;
      const survey = "Morrow Components lead times fell by two days.";
      message.evidence_refs!.push({ ...evidence, id, title: "Morrow Components · supplier capacity survey", url: "https://morrow.example/capacity-survey", provider: "Morrow Components", excerpt: `${survey}\n\nNo change in announced capacity.`, excerpt_match: { start: 0, end: encoder.encode(survey).length } });
      message.content = message.content.replace(/\n\n/, ` [2](#evidence-${id})\n\n`);
    }
    return workspace;
  };
}
/** A website citation keeps its saved passage behind this explicit control. */
async function viewSavedPassage() {
  const label = i18n.t("chat:browserWorkspaceView.viewSavedPassage");
  const button = await until(() => [...document.querySelectorAll("button")].find(node => visible(node) && node.textContent?.trim() === label) ?? null, "Saved passage control did not appear");
  button.click();
}
/** A semiconductor Map past sixty names: TSMC with twelve neighbors, 199 second-ring names and one name with no relationships. */
/** The Work tab's list: two charts and two documents across three chats. */
async function workListFixture(bridge: PreviewBridge) {
  await prepareFixture(bridge, "answer");
  const at = Date.parse("2026-10-28T00:00:00Z");
  const item = (id: string, chat: string, title: string, kind: "chart" | "markdown", revision: number, days: number, chatTitle: string) => ({
    artifact: { id, chat_session_id: chat, kind, title, current_revision: revision, created_at_ms: at - days * 86_400_000, updated_at_ms: at - days * 86_400_000 }, chat_title: chatTitle,
  });
  bridge.loupistArtifactList = async () => [
    item("work-margin", "preview-chat", "Margin vs utilization", "chart", 2, 0, "Reading Hanul Precision’s margin decline"),
    item("work-note", "preview-chat", "Hanul Precision Q3 notes", "markdown", 1, 1, "Reading Hanul Precision’s margin decline"),
    item("work-suppliers", "preview-chat-1", "Supplier lead times", "chart", 3, 4, "Morrow Components supplier check"),
    item("work-memo", "preview-chat-2", "Dividend coverage memo", "markdown", 1, 9, "Dividend income review"),
  ];
}
/** `lens`: two Lens entries applied to TSMC, for the Relationships view's dotted rows. */
function localViewMap(bridge: PreviewBridge, { lens = false } = {}) {
  const original = bridge.knowledgeMap.bind(bridge);
  bridge.knowledgeMap = async request => {
    const map = await original(request);
    const start = Date.now() - 30 * 86_400_000;
    const node = (id: string, label: string, entityKind: string, index: number, extra: Partial<KnowledgeMapNode> = {}): KnowledgeMapNode => ({ id, label, kind: "entity", entityKind, degree: 0, watched: false, held: false, firstSeenAtMs: start + (index % 30) * 86_400_000, ...extra });
    const suppliers = ["ASML", "Applied Materials", "Tokyo Electron", "Shin-Etsu Chemical", "SUMCO"];
    const customers = ["Apple", "NVIDIA", "AMD", "Qualcomm", "Broadcom"];
    const peers = ["Samsung Foundry", "Intel Foundry"];
    const ring = [...suppliers, ...customers, ...peers];
    const nodes: KnowledgeMapNode[] = [node("local-tsmc", "TSMC", "Organization", 0, { watched: true }), ...ring.map((label, index) => node(`local-ring-${index}`, label, "Organization", index + 1, { held: label === "NVIDIA" })), node("local-kioxia", "Kioxia", "Organization", 13)];
    const edges: KnowledgeMapEdge[] = ring.map((_, index) => {
      const id = `local-ring-${index}`;
      const relation = index < suppliers.length ? "SUPPLIES" : index < suppliers.length + customers.length ? "SUPPLIES" : "COMPETES_WITH";
      return index < suppliers.length ? { id: `local-claim-${id}`, kind: "claim", source: id, target: "local-tsmc", relation, status: "active" } : { id: `local-claim-${id}`, kind: "claim", source: "local-tsmc", target: id, relation, status: index === 6 ? "disputed" : "active" };
    });
    const kinds = ["Wafer", "Gas", "Chemical", "Equipment", "Module", "Design", "Packaging", "Memory"];
    for (let index = 0; index < 199; index++) {
      const id = `local-outer-${index}`;
      nodes.push(node(id, `${kinds[index % kinds.length]} supplier ${String(index + 1).padStart(3, "0")}`, index % 3 === 2 ? "Product" : "Organization", index));
      edges.push({ id: `local-claim-${id}`, kind: "claim", source: id, target: `local-ring-${index % ring.length}`, relation: "SUPPLIES", status: "active" });
    }
    if (lens) {
      const ko = previewKo();
      nodes.push({ id: "local-lens-cash", recordId: "local-lens-cash", label: ko ? "이익보다 현금흐름 먼저" : "Cash flow first", kind: "lens_principle", degree: 0, watched: false, held: false },
        { id: "local-lens-moat", recordId: "local-lens-moat", label: ko ? "경쟁우위 비교" : "Moat comparison", kind: "lens_method", degree: 0, watched: false, held: false });
      edges.push({ id: "local-applies-cash", kind: "applies_to", source: "local-lens-cash", target: "local-tsmc" }, { id: "local-applies-moat", kind: "applies_to", source: "local-lens-moat", target: "local-tsmc" });
    }
    for (const edge of edges) for (const id of [edge.source, edge.target]) nodes.find(item => item.id === id)!.degree++;
    map.nodes = nodes; map.edges = edges;
    for (const [kind, count] of Object.entries(map.counts)) { count.total = count.shown = nodes.filter(item => item.kind === kind).length || edges.filter(item => item.kind === kind).length; count.truncated = false; }
    map.worldRevisionId = "local-view-preview";
    return map;
  };
}
/** Last visit two days ago and five changes since: two names, a relationship, conflicting evidence and a learned method. */
function whatsNewFixture(bridge: PreviewBridge, withChanges: boolean) {
  const now = Date.now(), hour = 3_600_000;
  const lastSeenAtMs = now - (withChanges ? 50 : 24) * hour;
  recordVisit("loupist-quality", lastSeenAtMs - MIN_VISIT_MS, lastSeenAtMs);
  const event = (index: number, kind: GrowthEvent["kind"], subjectId: string | null, atMs: number, summary: string, extra: Partial<GrowthEvent> = {}): GrowthEvent => ({ eventId: `whats-new-${index}`, loupistId: "loupist-quality", atMs, kind, subjectId, recordId: "map-world", originChatId: null, originMessageId: null, actor: "loupist", summary, beforeSummary: null, afterSummary: null, ...extra });
  const events = withChanges ? [
    event(0, "lens_created", null, now - 27 * hour, "Compare competitive advantages", { recordId: "map-lens-1", origin: { kind: "learning_review", reviewId: "preview-review", worldJobId: "preview-job" } }),
    event(1, "entity_added", "map-entity-09", now - 26 * hour, "SK hynix"),
    event(2, "source_retained", null, now - hour, "Q3 operating review", { originMessageId: "preview-answer" }),
    event(3, "entity_added", "map-entity-03", now - hour, "NVIDIA", { originMessageId: "preview-answer" }),
    event(4, "claim_added", "map-claim-01", now - 50 * 60_000, "Apple depends on TSMC for its most advanced chips", { originMessageId: "preview-answer" }),
    event(5, "claim_disputed", "map-claim-08", now - 20 * 60_000, "TSMC packaging capacity reserved for SK hynix HBM (54% vs 48%)"),
  ] : [];
  bridge.knowledgeGrowthEvents = async request => ({ events: events.filter(item => request.sinceMs === undefined || item.atMs >= request.sinceMs), nextCursor: null });
}
/** On a first visit the legend opens on a wide Map and starts collapsed on a narrow one. */
function legendAtFirstVisit() {
  const legend = select("details.map-legend");
  const width = select(".map-view")?.clientWidth ?? 0;
  return legend instanceof HTMLDetailsElement && legend.open === (!hasLayout() || width >= 900);
}
function rememberPreviewMap({ focalId, scope, legendOpen }: { focalId?: string; scope?: MapScope; legendOpen?: boolean }) {
  if (focalId) rememberFocal("loupist-quality", focalId);
  if (scope) rememberScope("loupist-quality", scope);
  if (legendOpen !== undefined) rememberLegendOpen(legendOpen);
}
async function enterMap(root: HTMLElement) {
  await navigate(root, "map");
  const map = await until(() => select(".map-view"), "Map did not load");
  if (map.clientWidth > 0) {
    await until(() => map.querySelector("canvas"), "Map canvas did not mount");
    for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
    await clickMapAction("[data-map-fit]", map);
  }
  return map;
}

/** Selects `count` words from word `start` of an answer paragraph's first text run, as a person dragging would. */
export async function selectAnswerWords(start: number, count: number, paragraphIndex = 0) {
  await until(messagesRendered, "Message Markdown did not finish rendering");
  // Let a previous quote finish moving focus to the composer before selecting again.
  await new Promise((resolve) => setTimeout(resolve, 60));
  // An absent fallback before the answer mounts does not mean Markdown is ready.
  // Wait for a final paragraph in the same poll; selecting the lazy fallback
  // leaves a detached range when React replaces it, especially in narrow cells.
  const paragraph = await until(() => [...document.querySelectorAll(".message.assistant .message-content p:not(.message-response-fallback)")].filter(visible)[paragraphIndex], "Answer Markdown did not load");
  const walker = document.createTreeWalker(paragraph, NodeFilter.SHOW_TEXT);
  const node = walker.nextNode() as Text | null;
  if (!node) throw new Error("Answer paragraph has no text");
  const words = [...node.data.matchAll(/\S+/gu)];
  const first = words[Math.min(start, words.length - 1)]!;
  const last = words[Math.min(start + count - 1, words.length - 1)]!;
  // Pressing into the answer takes focus away from the composer, as a real drag does.
  (document.activeElement as HTMLElement | null)?.blur();
  const range = document.createRange();
  range.setStart(node, first.index!);
  range.setEnd(node, last.index! + last[0].length);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
  await until(() => select(".quote-ask"), "Quote button did not appear");
}
// Preview fixtures may already hold a principle with the same words; reuse it.
const ensurePrinciple = async (bridge: PreviewBridge, request: Parameters<PreviewBridge["principleCreate"]>[0]) =>
  (await bridge.principleList({ loupistId: request.loupistId })).find(item => item.statement === request.statement) ?? await bridge.principleCreate(request);
const i18nHistory = () => i18n.t("knowledge:memoryPage.tabs.history");
/** Scrolls the feature under review into view, so narrow captures show it rather than the thread's end. */
const centerOn = (selector: string, block: ScrollLogicalPosition = "center", readySelector?: string) => async () => {
  const target = await until(() => messagesRendered() && (!readySelector || select(readySelector)) ? select(selector) : null, `Not ready to frame: ${selector}`);
  if (hasLayout()) {
    await document.fonts.ready;
    // Morning news can finish after the host-posted message mounts. Let its
    // settled effect and the conversation's resize scroll finish before framing.
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    target.scrollIntoView({ block });
  }
};
const base = (id: string, title: string, landmark: PreviewScenario["landmark"], enter: PreviewScenario["enter"] = idle): PreviewScenario => ({ id, title, setup: (bridge) => prepareFixture(bridge), enter: async root => { await enter(root); await until(messagesRendered, "Message Markdown did not finish rendering"); }, landmark: () => messagesRendered() ? landmark() : null });

export const scenarios: readonly PreviewScenario[] = [
  { ...base("growth-replay", "Recorded Semiconductors growth", () => select("[data-growth-fixture-pending]") ?? select(".map-replay-bar"), async root => {
    await navigate(root, "map");
    const map = await until(() => select(".map-view"), "Growth Map did not load");
    if (growthReplayPending) {
      const pending = document.createElement("p");
      pending.dataset.growthFixturePending = "true";
      pending.textContent = "Fixture pending";
      map.prepend(pending);
      return;
    }
    if (hasLayout()) await until(mapCanvas, "Growth Map canvas did not mount");
    await clickMapAction("[data-map-replay]", map);
    const scrubber = await until(() => map.querySelector<HTMLInputElement>(".map-replay-scrubber"), "Growth replay did not open");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(scrubber, scrubber.max);
    scrubber.dispatchEvent(new Event("input", { bubbles: true }));
    scrubber.dispatchEvent(new Event("change", { bubbles: true }));
    if (hasLayout()) {
      for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
      await clickMapAction("[data-map-fit]", map);
    }
  }), setup: prepareGrowthReplay },
  { ...base("map-conflicts", "Conflicting evidence", () => select(".map-conflicts"), async root => {
    await navigate(root, "map");
    const input = await until(() => select('.map-view [role="combobox"]'), "Map search missing") as HTMLInputElement;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "Hanul");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await click('[role="option"]');
    await until(() => select(".map-conflicts"), "Conflicting evidence did not load");
    if (hasLayout()) { await until(mapCanvas, "Map canvas missing"); select(".map-conflicts")?.scrollIntoView({ block: "start" }); }
  }), setup: prepareConflictsFixture },
  { ...base("map-core-schema", "Core World kinds", () => select("#map-inspector-title"), async root => {
    await navigate(root, "map");
    const input = await until(() => select('.map-view [role="combobox"]'), "Map search missing") as HTMLInputElement;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "Hanbit");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await click('[role="option"]');
    await until(() => select("#map-inspector-title"), "Core schema inspector did not open");
    if (hasLayout()) {
      await until(mapCanvas, "Core schema canvas did not mount");
      const legend = document.querySelector("details.map-legend");
      if (legend instanceof HTMLDetailsElement && !legend.open) legend.querySelector("summary")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      await clickMapAction("[data-map-fit]", root);
      for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
    }
  }), setup: prepareCoreSchema },
  { ...base("briefing-conflicts-link", "Briefing links to conflicting evidence", () => select("[data-briefings-conflicts]"), async root => {
    await openScheduleRun(root, "preview-briefing-1");
    const link = await until(() => select("[data-briefings-conflicts]"), "Conflicting evidence link missing");
    if (hasLayout()) link.scrollIntoView({ block: "center" });
  }), setup: prepareConflictsFixture },
  ...["notes-journal", "journal-new-entry", "journal-reader-outcome"].map((id): PreviewScenario => ({
    id, title: "Your journal and research notes", setup: prepareJournalFixture,
    landmark: () => select(id === "journal-new-entry" ? ".journal-check-fields" : id === "journal-reader-outcome" ? ".journal-outcome-card" : ".journal-entry-list"),
    enter: async root => {
      await navigate(root, "map");
      await clickMapAction("[data-map-library]", root);
      await click('[role="tab"][id$="-trigger-note"]', root);
      await until(() => root.querySelectorAll(".journal-library-row").length === 3, "Journal entries did not load");
      if (id === "journal-new-entry") {
        await click(".journal-library-section .source-library-heading button", root);
        await click(".journal-check-toggle", root);
      } else if (id === "journal-reader-outcome") await click('[data-research-id="preview-journal-1"]', root);
      if (hasLayout()) { await until(mapCanvas, "Map canvas did not load"); for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
        const target = select(id === "journal-new-entry" ? ".journal-check-fields" : id === "journal-reader-outcome" ? ".journal-outcome-card" : ".journal-entry-list");
        const viewport = target?.closest<HTMLElement>(id === "journal-new-entry" ? '[role="tabpanel"]' : ".knowledge-research-reader");
        if (target && viewport && target.getBoundingClientRect().bottom > viewport.getBoundingClientRect().bottom) viewport.scrollTop += target.getBoundingClientRect().top - viewport.getBoundingClientRect().top - 12;
      }
    },
  })),
  ...["map-learned-method", "map-learned-toast", "memory-history", "lens-then-now-stacked"].map((id): PreviewScenario => {
    let bridge: PreviewBridge;
    let review: Awaited<ReturnType<typeof prepareLearningFixture>>;
    return {
      id, title: "How your Loupist learned a method",
      setup: async candidate => { bridge = candidate; review = await prepareLearningFixture(candidate, 3); },
      landmark: () => select(id === "map-learned-toast" ? ".learning-toast" : id === "memory-history" ? ".memory-history" : id === "lens-then-now-stacked" ? ".lens-then-now" : ".method-learning"),
      enter: async root => {
        if (id === "memory-history") {
          // Learning history lives on "What {Loupist} knows".
          await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
          await selectPanel("lens");
          // Choose the tab once the page has loaded; a click during loading can be lost to the first render.
          await until(() => select(".memory-page-body"), "What it knows did not load");
          const history = await until(() => [...document.querySelectorAll<HTMLElement>('.memory-page-tabs [role="tab"]')].find(tab => tab.textContent?.includes(i18nHistory())), "History tab missing");
          history.click();
          await until(() => select(".memory-history"), "History did not load");
          return;
        }
        await navigate(root, "map");
        bridge.emitLearningReview(review);
        await until(() => select(".learning-toast"), "Learning toast did not appear");
        if (id !== "map-learned-toast") {
          await click(".learning-toast button");
          await until(() => select(".lens-then-now"), "Learning details did not load");
          if (id === "lens-then-now-stacked") {
            const inspector = select(".workspace-lens-content .memory-reader")!;
            inspector.style.width = "340px";
            inspector.style.height = "100%";
            inspector.style.left = "auto";
            select(".lens-history")?.scrollIntoView({ block: "start" });
          }
        }
        if (hasLayout() && id === "map-learned-toast") { await until(mapCanvas, "Map canvas did not load"); for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame); }
      },
    };
  }),
  ...["connections-custom-empty", "connections-custom-list", "connection-add-url-signin", "connection-add-json-review", "connection-review-tools", "connection-detail", "chat-needs-approval"].map((id): PreviewScenario => ({
    id, title: id, setup: bridge => prepareCustomConnectionFixture(bridge, id),
    landmark: () => select(id === "chat-needs-approval" ? ".approval-cards" : id === "connection-detail" ? ".custom-detail" : id === "connection-add-url-signin" ? "[data-custom-signing-in]" : id === "connection-add-json-review" || id === "connection-review-tools" ? ".custom-add .custom-tools" : ".custom-connections"),
    enter: async root => {
      if (id === "chat-needs-approval") {
        await until(() => select(".approval-cards"), "Approval activity did not load");
        if (hasLayout()) {
          await until(() => root.querySelector("button.loupist-citation"), "Answer rendering did not finish");
          const activity = select(".approval-cards");
          const viewport = activity?.closest<HTMLElement>(".message-scroll");
          if (activity && viewport) {
            viewport.scrollTop += activity.getBoundingClientRect().top - viewport.getBoundingClientRect().top;
            viewport.dispatchEvent(new Event("scroll", { bubbles: true }));
            for (let parent = viewport.parentElement; parent; parent = parent.parentElement) parent.scrollTop = 0;
            window.scrollTo(0, 0);
          }
        }
        return;
      }
      await settings(root, "connections");
      if (id === "connection-detail") { await click('[data-custom-connection="preview-custom-changed"]'); return; }
      if (!id.startsWith("connection-add") && id !== "connection-review-tools") return;
      await click("[data-custom-add]");
      const fill = async (selector: string, value: string) => {
        const input = await until(() => select(selector), "Custom connection field did not load") as HTMLInputElement | HTMLTextAreaElement;
        Object.getOwnPropertyDescriptor(input.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, "value")!.set!.call(input, value);
        input.dispatchEvent(new Event("input", { bubbles: true }));
      };
      if (id === "connection-add-json-review") {
        await click('.custom-add [role="radio"][data-state="unchecked"]');
        await fill(".custom-add textarea", JSON.stringify({ mcpServers: { investor: { url: "https://research.example/mcp" }, archive: { command: "npx", args: ["@example/research-archive@1.2.3", "--read-only"] } } }));
      } else await fill('.custom-add input[type="url"]', "https://research.example/mcp");
      await click('.custom-add [data-slot="dialog-footer"] button');
    },
  })),

  ...(["needs_setup", "ready"] as const).map((state): PreviewScenario => ({
    ...base(`chat-connection-setup-${state.replace("_", "-")}`, "A connection set up from Chat", () => select(`.connection-setup-card[data-state="${state}"]`), async () => {
      await until(() => select(`.connection-setup-card[data-state="${state}"]`), "Setup card did not load");
      select(".connection-setup-card")?.scrollIntoView({ block: "center" });
    }),
    setup: bridge => prepareConnectionSetupFixture(bridge, state),
  })),
  ...["chat-feedback-principle", "chat-memory-principle"].map((id): PreviewScenario => ({
    ...base(id, "A principle saved from Chat", () => select(id === "chat-feedback-principle" ? ".principle-saved" : ".memory-principle"), async root => {
      await until(() => root.querySelector("article.message.assistant.complete"), "Answer did not load");
      if (id === "chat-feedback-principle") {
        await click("[data-feedback-principle]", root);
        await click('.principle-form button[type="submit"]', root);
        await until(() => select(".principle-saved"), "Principle did not save");
      }
      if (hasLayout()) {
        await until(() => root.querySelector("button.loupist-citation"), "Answer rendering did not finish");
        select(id === "chat-feedback-principle" ? ".answer-correction" : ".memory-principle")?.scrollIntoView({ block: "center" });
      }
    }),
    setup: async bridge => {
      await prepareFixture(bridge, "answer");
      const principle = (await bridge.principleList({ loupistId: "loupist-quality" }))[0];
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const message = workspace.threads[0].messages.find(item => item.id === "preview-answer")!;
        if (id === "chat-feedback-principle") message.feedback = { rating: "down", correction: "Compare cash conversion over several reporting periods before interpreting margin growth.", updatedAt: Date.now() };
        else message.memory_update = { status: "applied", commitId: "preview-principle", changes: [{ kind: "principle", statement: principle.statement, recordId: principle.recordId, operation: "create", title: principle.statement, lesson: "", basis: "", futureUse: "", knowledge: { kind: "lens", recordId: principle.recordId, revisionId: principle.revisionId, objectId: principle.recordId } }] };
        return workspace;
      };
    },
  })),
  ...["welcome", "notice"].map((state): PreviewScenario => ({
    ...base(`onboarding-${state}`, `Onboarding ${state}`, () => select('[data-onboarding="1"] h1'), async () => {
      if (state === "notice" && hasLayout()) select("[data-onboarding-notice]")?.scrollIntoView({ block: "center" });
    }),
    setup: bridge => prepareFixture(bridge, "provider"),
  })),
  { ...base("onboarding-bot", "First launch opens Bot after the notice", () => document.getElementById("current-location")?.textContent === i18n.t("app:thread.title") ? select(".chat-layout") : null, async root => {
    await click("[data-notice-acknowledgment]", root);
    await click("[data-onboarding-start]", root);
    await click("[data-onboarding-create]", root);
    await until(() => select("textarea[data-chat-composer]"), "Bot did not initialize");
  }), setup: async bridge => {
    await prepareFixture(bridge, "loupist");
    bridge.scheduleTasks = "none";
    bridge.inboxList = async () => ({ items: [], nextCursor: null });
    bridge.inboxUnreadCounts = async () => [];
  } },
  // W17.1/W17.4: onboarding step 2. ChatGPT sign-in and its waiting, paste and device cards, error rows, and the connect sheet.
  ...(() => {
    const states = ["connect", "connect-waiting", "connect-paste", "connect-device", "connect-declined", "connect-not-eligible", "connect-port-busy", "connect-key", "connect-openai-replaces", "connect-local", "connect-local-none", "connect-custom-test", "connect-custom-error"] as const;
    type ConnectState = (typeof states)[number];
    const failures: Partial<Record<ConnectState, ProviderConnectFailure>> = { "connect-declined": "plan_permission_declined", "connect-not-eligible": "plan_not_eligible", "connect-port-busy": "port_in_use" };
    const sheet = (kind: string, selector: string) => () => select(`[data-connect-sheet="${kind}"] ${selector}`);
    const landmarks: Record<ConnectState, () => Element | null> = {
      connect: () => select('[data-onboarding="2"] .onboarding-other[open] [data-connect-row="custom"]'),
      "connect-waiting": () => select('[data-onboarding="2"] [data-signin="waiting"] [data-paste-toggle]'),
      "connect-paste": () => select('[data-onboarding="2"] [data-signin="waiting"] .signin-paste input'),
      "connect-device": () => select('[data-onboarding="2"] [data-signin="device"] .signin-device-code'),
      "connect-declined": () => select('[data-onboarding="2"] .signin-error[data-reason="plan_permission_declined"]'),
      "connect-not-eligible": () => select('[data-onboarding="2"] .signin-error[data-reason="plan_not_eligible"]'),
      "connect-port-busy": () => select('[data-onboarding="2"] .signin-error[data-reason="port_in_use"]'),
      "connect-key": sheet("api_key", '[data-key-sheet="google"] input'),
      "connect-openai-replaces": sheet("api_key", "[data-replaces-chatgpt]"),
      "connect-local": () => select('[data-connect-sheet="local"] [data-engine="ollama"] input[type="radio"]:checked') && select('[data-connect-sheet="local"] [data-engine="ollama"] input[type="radio"]:disabled'),
      "connect-local-none": sheet("local", "[data-local-none]"),
      "connect-custom-test": () => select('[data-connect-sheet="custom"] .connect-test-result[data-outcome="ok"]') && select('[data-connect-sheet="custom"] .connect-test-models input:checked'),
      "connect-custom-error": sheet("custom", '.connect-test-result[data-outcome="not_found"]'),
    };
    const typeInto = (input: HTMLInputElement, value: string) => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    };
    const openOtherWays = async () => {
      const disclosure = await until(() => select('[data-onboarding="2"] details.onboarding-other'), "Other ways did not appear") as HTMLDetailsElement;
      if (!disclosure.open) (disclosure.querySelector("summary") as HTMLElement).click();
      await until(() => select('[data-onboarding="2"] [data-connect-row="custom"]'), "Other ways did not open");
    };
    return states.map((state): PreviewScenario => ({
      ...base(`onboarding-${state}`, `Onboarding ${state}`, landmarks[state], async root => {
        await click("[data-notice-acknowledgment]", root);
        await click("[data-onboarding-start]", root);
        if (state === "connect-waiting" || state === "connect-paste" || failures[state]) {
          await click("[data-onboarding-connect]", root);
          if (state === "connect-paste") await click("[data-paste-toggle]");
          return;
        }
        if (state === "connect-device") { await click('[data-onboarding-connect="xai"]', root); return; }
        await openOtherWays();
        if (state === "connect") return;
        const row = state === "connect-key" ? "key-google" : state === "connect-openai-replaces" ? "key-openai" : state.startsWith("connect-local") ? "local" : "custom";
        await click(`[data-connect-row="${row}"]`);
        if (state === "connect-custom-test" || state === "connect-custom-error") {
          const form = await until(() => select("[data-custom-sheet]"), "Custom endpoint sheet did not open");
          typeInto(form.querySelector<HTMLInputElement>('input:not([type="radio"]):not([type="url"])')!, "Office gateway");
          typeInto(form.querySelector<HTMLInputElement>('input[type="url"]')!, "https://gateway.example/v1");
          await click("[data-custom-test]");
        }
      }),
      setup: async bridge => {
        await prepareFixture(bridge, "provider");
        // The paste route normally appears 20 s after the browser opens.
        signInTiming.pasteRevealMs = state === "connect-waiting" || state === "connect-paste" ? 0 : 20_000;
        const failure = failures[state];
        if (state === "connect-waiting" || state === "connect-paste") {
          bridge.connectScript = async (_provider, observer) => {
            observer({ type: "opening_browser", message: "", opened: true, url: null });
            observer({ type: "manual_entry" });
            await new Promise(() => {});
          };
        } else if (state === "connect-device") {
          bridge.connectScript = async (_provider, observer) => {
            observer({ type: "device_code", user_code: "WDJB-MJHT", verification_uri: "https://accounts.x.ai/oauth2/device", expires_in_s: 900 });
            await new Promise(() => {});
          };
        } else if (failure) {
          bridge.connectScript = async (_provider, observer) => {
            observer({ type: "failed", reason: failure });
            throw { code: "provider_connect_failed", message: failure };
          };
        }
        // A ChatGPT plan stays signed in on this Mac while an OpenAI key is being added.
        if (state === "connect-openai-replaces") bridge.setProviderStatus("openai", { credential_kind: "plan", state: "connected" });
        if (state === "connect-local-none") bridge.localDiscovery = { engines: bridge.localDiscovery.engines.map(engine => ({ ...engine, running: false, models: [] })) };
        if (state === "connect-custom-error") bridge.customTestResult = { outcome: "not_found", models: [] };
      },
    }));
  })(),
  base("new-loupist-dialog", "New Loupist templates", () => select(".loupist-template-fields"), async () => {
    await until(() => select('textarea[data-chat-composer]'), "Chat did not initialize");
    await openNavigation(".loupist-header-new");
    await click(".loupist-header-new");
  }),
  { ...base("loupist-import-dialog", "Open a Loupist folder", () => {
    const dialog = select("[data-loupist-import-dialog]");
    return dialog?.querySelector(".workspace-path") && dialog.querySelector('input[type="checkbox"]:not(:checked)') ? dialog : null;
  }, async root => {
    await settings(root, "loupists");
    await click("[data-loupist-import]", root);
    await click("[data-loupist-import-dialog] .loupist-folder-heading button");
  }), setup: async bridge => {
    await prepareFixture(bridge);
    bridge.loupistChooseFolder = async () => "/Users/research/Backups/Quality Compounder";
  } },
  base("chat-empty", "Empty chat: the Loupist's morning message", () => select('textarea[data-chat-composer]') && select('.morning-message[aria-busy="false"]')),
  base("composer-focus", "Composer keyboard focus is one stroke on the field edge", () => {
    const field = select('.composer [data-slot="input-group"]:has(textarea:focus-visible)');
    if (!field) return null;
    if (!hasLayout()) return field;
    const style = getComputedStyle(field);
    const transparent = style.borderTopColor === "rgba(0, 0, 0, 0)" || style.borderTopColor === "transparent"; // token-exception: CSSOM serializes the transparent border in a preview assertion
    return style.outlineStyle !== "none" && style.outlineOffset.trim().startsWith("-") && transparent && style.boxShadow === "none" ? field : null;
  }, async () => {
    const prompt = await until(() => select("textarea[data-chat-composer]"), "Composer did not appear");
    prompt.focus({ focusVisible: true });
  }),
  ...([["thread-morning", "morning", "Morning message: changes since the last visit"], ["thread-first-day", "first-day", "Morning message: a new Loupist's first day"], ["thread-nothing-changed", "nothing-changed", "Morning message: nothing new since yesterday"]] as const).map(([id, state, title]): PreviewScenario => ({
    ...base(id, title, () => select('textarea[data-chat-composer]') && select('.morning-message[aria-busy="false"] .morning-ask')),
    setup: bridge => prepareMorningFixture(bridge, state),
  })),
  // W15.9e: the event strip above the lead, and the same message with nothing ahead (no strip).
  { ...base("thread-upcoming", "Morning message: what is coming up for what you follow", () => select('.morning-message[aria-busy="false"] .morning-upcoming li:nth-child(5) .morning-upcoming-row')), setup: bridge => prepareUpcomingFixture(bridge, "events") },
  { ...base("thread-upcoming-none", "Morning message: nothing ahead, so no event strip", () => {
    const message = select('.morning-message[aria-busy="false"]');
    return message?.querySelector(".morning-ask") && !message.querySelector(".morning-upcoming") ? message : null;
  }), setup: bridge => prepareUpcomingFixture(bridge, "none") },
  base("chat-private", "Private chat with persistent disclosure", () => select(".composer-private-status") && select(".composer-disclosure"), async (root) => {
    await click('.composer-private-toggle[aria-pressed="false"]', root);
  }),
  { ...base("chat-answer", "Completed sourced answer", () => {
    const answer = select("article.message.assistant.complete");
    return answer && answer.querySelectorAll(".message-content p").length >= 2 && answer.querySelector(".chat-receipt") && answer.querySelector("button.loupist-citation") ? answer : null;
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
  { ...base("chat-no-account", "Saved answer after the last AI account is removed", () => {
    const connect = select("button.composer-model-menu:not([aria-label])");
    return connect && select("article.message.assistant.complete");
  }),
    setup: async (bridge) => {
      await prepareFixture(bridge, "answer");
      for (const provider of (await bridge.modelCatalogGet()).providers) await bridge.providerDisconnect(provider.id);
    } },
  { ...base("chat-streaming", "Answer in progress", () => select('.message.assistant.streaming [data-streamdown="strong"]') && select("button.composer-stop-action"), submitPrompt), setup: (bridge) => prepareFixture(bridge, "streaming") },
  // W15.18 and W15.19/W15.22: guidance and a queued message during a run; what waits across Loupists.
  // W15.26–W15.28: actions on hover, the citation card, and the error shapes.
  { ...base("chat-hover-actions", "Earlier answers show their actions on hover; the newest keeps them", () => select(".message.assistant[data-latest] .message-action-bar")), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const thread = workspace.threads[0];
      const [question, answer] = thread.messages;
      if (question && answer) {
        thread.messages.push(
          { ...question, id: "preview-follow-up", content: previewKo() ? "재고일수는 어떻게 봐야 해?" : "How should I read the inventory days?", created_at: "2026-10-28T00:05:00Z" },
          { ...answer, id: "preview-follow-up-answer", content: previewKo() ? "재고일수 54일은 같은 업종 평균보다 길어요. 수요가 현금으로 바뀌는 속도가 아직 느리다는 뜻이에요." : "At 54 days, inventory sits above the peer average. Demand has not yet turned into cash as fast as revenue suggests.", evidence_refs: [], memory_update: undefined, created_at: "2026-10-28T00:05:12Z" },
        );
      }
      return workspace;
    };
  } },
  { ...base("chat-citation-hover", "A citation previews its source", () => select(".citation-preview"), async () => {
    await until(messagesRendered, "Message Markdown did not finish rendering");
    // Keyboard focus opens the card the same way a short hover does.
    (await until(() => select("button.loupist-citation"), "Citation did not render") as HTMLButtonElement).focus();
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
  // W15.28 and W17 (providers §2.4): each failed answer names its account problem and one next step.
  ...([
    ["chat-error-quota", "rate_limited", "A busy AI account counts down to a retry", "openai", "button"],
    ["chat-error-signin", "needs_reconnect", "A sign-in that stopped working asks to reconnect", "openai", '[data-error-action="reconnect"]'],
    ["chat-error-plan-limit", "plan_limit", "A ChatGPT plan at its limit never retries on its own", "openai", '[data-error-action="manage_usage"]'],
    ["chat-error-not-eligible", "plan_not_eligible", "A plan other apps can't use offers an API key", "openai", '[data-error-action="use_api_key"]'],
    ["chat-error-no-credit", "out_of_credit", "A key account out of credit", "anthropic", "button"],
  ] as const).map(([id, kind, title, provider, action]): PreviewScenario => ({
    ...base(id, title, () => select(`.message-run-state ${action}`)),
    setup: async (bridge) => {
      await prepareFixture(bridge, "answer");
      if (kind === "out_of_credit") await bridge.providerConfigure({ provider: "anthropic", api_key: "preview-key" });
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const answer = workspace.threads[0].messages.find(item => item.id === "preview-answer");
        if (answer) Object.assign(answer, {
          status: "error", failure_kind: kind, evidence_refs: [], memory_update: undefined,
          // A busy provider said how long to wait; Rust passed the bounded hint on.
          ...(kind === "rate_limited" ? { retry_after_seconds: 45 } : {}),
          content: previewKo() ? "한울정밀의 영업이익률은 16.2%에서 14.8%로" : "Hanul Precision’s operating margin fell from 16.2% to",
          execution_model: provider === "anthropic"
            ? { profile_id: "anthropic/claude-sonnet-4-5", name: "Claude Sonnet 4.5", provider, model: "claude-sonnet-4-5", thinking_level: "medium", run_options: {} }
            : { profile_id: "model-test", name: "GPT-5.6 Luna", provider, model: "gpt-5.6-luna", thinking_level: "medium", run_options: {} },
        });
        return workspace;
      };
    },
  })),
  // W17.5: Rust retried a retired model once on Auto within the turn; the answer says which model replaced it.
  { ...base("chat-model-gone", "A retired model is replaced once and the answer says so", () => select(".message-model-note")), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const answer = workspace.threads[0].messages.find(item => item.id === "preview-answer");
      if (answer) Object.assign(answer, {
        replaced_model: "GPT-5.4",
        execution_model: { profile_id: "openai/gpt-5.6-luna", name: "GPT-5.6 Luna", provider: "openai", model: "gpt-5.6-luna", thinking_level: "medium", run_options: {} },
      });
      return workspace;
    };
  } },
  // W17.5: Auto resolved to a different model because the old one left the list.
  { ...base("chat-default-changed", "Chat names the model it now uses, once", () => select(".model-notice-line")), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    bridge.setModelNotice({ id: "preview-notice", kind: "now_using", model: "GPT-6.1 Sol" });
  } },
  { ...base("chat-offline", "Offline: the thread says so and a sent message waits", () => select(".chat-offline-banner") && select(".chat-pending-card"), async (root) => {
    await submitPrompt(root, previewKo() ? "다시 연결되면 마진을 비교해 줘." : "Compare the margins once I'm back online.");
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    Object.defineProperty(window.navigator, "onLine", { configurable: true, get: () => false });
  } },
  { ...base("chat-steer-queued", "Guidance waits in the timeline and a message waits its turn", () => select(".chat-guiding-row") && select(".chat-pending-card") && select(".chat-pending-heading"), async (root) => {
    await submitPrompt(root);
    await until(runningHeader, "Run header did not appear");
    await until(() => select("button.composer-stop-action") && !select("button.composer-queue-action") && !select("button.composer-steer-action"), "An empty composer should show only stop");
    const composer = await until(() => root.querySelector<HTMLTextAreaElement>("textarea[data-chat-composer]"), "Composer did not appear");
    const type = (text: string) => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, text);
      composer.dispatchEvent(new Event("input", { bubbles: true }));
    };
    type("Focus on the new line only");
    await until(() => select("button.composer-queue-action") && select("button.composer-steer-action") && select("button.composer-stop-action"), "Typing should offer queue, guide and stop");
    await click("button.composer-steer-action", root);
    await until(() => select(".chat-guiding-row"), "The guiding row did not appear");
    type("Then compare it with Morrow Components");
    await click("button.composer-queue-action", root);
    await until(() => select(".chat-pending-card") && select(".chat-pending-heading"), "The queued row did not appear");
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "streaming");
    bridge.chatSteer = async (request) => ({ message_id: "preview-steer", prompt: request.prompt, created_at: new Date().toISOString(), mode: "steer" });
  } },
  { ...base("chat-steer-guided", "Guidance a finished reply accepted reads as a Guided row", () => {
    const row = select(".chat-guided");
    return row && !select(".composer-guidance[data-state='pending']") ? row : null;
  }, async () => {
    const row = await until(() => select(".chat-guided"), "The guided row did not appear");
    await until(() => !select(".composer-guidance[data-state='pending']"), "A finished reply still shows waiting guidance");
    // Narrow windows open at the answer; bring the question, the row and the answer's start into view.
    if (hasLayout()) row.scrollIntoView({ block: "center" });
  }), setup: async (bridge) => {
    bridge.guidedAnswer = true;
    await prepareFixture(bridge, "answer");
  } },
  { ...base("shell-needs-attention", "The sidebar lists what waits for the user", () => select('.needs-attention .needs-attention-row[data-kind="approval"]'), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
    if (hasLayout() && document.documentElement.clientWidth < 768) await openNavigation(".needs-attention");
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    prepareAttention(bridge);
  } },
  { ...base("chat-heartbeat", "Run silent for 16 seconds", () => stalledHeader() ?? (hasLayout() ? null : runningHeader()), async (root) => {
    await submitPrompt(root);
    await until(runningHeader, "Run header did not appear");
    // The standalone preview freezes the clock; jsdom runs keep real time and only check the run header.
    if (!hasLayout()) return;
    advancePreviewClock(16_000);
    await until(stalledHeader, "Heartbeat did not appear", 3000);
  }), setup: async (bridge) => { await prepareFixture(bridge, "streaming"); liveRun(bridge, "tool"); } },
  { ...base("chat-status-pill", "Scrolled up while a reply runs", () => select(".chat-status-pill[data-running]") ?? (hasLayout() ? null : runningHeader()), async (root) => {
    await submitPrompt(root);
    await until(runningHeader, "Run header did not appear");
    await scrollThreadUp();
  }), setup: async (bridge) => { await prepareFixture(bridge, "answer"); longThread(bridge); liveRun(bridge, "writing"); } },
  { ...base("chat-status-pill-idle", "Scrolled up after the reply", () => select(".chat-status-pill:not([data-running])") ?? (hasLayout() ? null : select(".message-scroll .message.assistant")), scrollThreadUp), setup: async (bridge) => { await prepareFixture(bridge, "answer"); longThread(bridge); } },
  { ...base("chat-chart-building", "A chart reserves its card while it is built", () => select('.chat-artifact-block[data-state="building"]'), root => submitPrompt(root, "Chart Hanul Precision’s margin against utilization.")), setup: async (bridge) => { await prepareFixture(bridge, "answer"); chartRun(bridge); } },
  { ...base("chat-chart-failed", "A failed chart leaves a compact explanation below the answer", () => select('.chat-artifact-block[data-state="failed"] [role="alert"]'), centerOn('.chat-artifact-block[data-state="failed"]')), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const answer = workspace.threads[0]?.messages.find(message => message.role === "assistant");
      if (answer) answer.progress = { startedAtMs: Date.now() - 4_000, finishedAtMs: Date.now(), items: [
        { id: "failed-chart", kind: "tool", toolName: "chart_publish", category: "artifact", operation: "publish", action: "Made a chart", running: "Making a chart", target: previewKo() ? "한울정밀 · 최근 3년 일봉" : "Hanul Precision · three years of daily prices", status: "failed" },
      ] };
      return workspace;
    };
  } },
  { ...base("chat-plan-running", "A multi-step reply works through its plan", () => {
    const active = select('.chat-plan [aria-current="step"]');
    return active && select('.chat-progress-toggle[data-tool-status="running"]') && select('.chat-artifact-block[data-state="building"]') ? select(".chat-plan") : null;
  }, async (root) => {
    await submitPrompt(root, "Why did Hanul Precision’s margin fall while revenue grew?");
    await click("button[data-research-summary]", root);
    await until(() => select(".chat-plan"), "Plan did not appear");
    // A short window follows the reserved chart card; scroll back to the plan, which shows the status pill.
    const thread = select(".message-scroll");
    if (!hasLayout() || !thread || thread.scrollHeight - thread.clientHeight <= 120) return;
    thread.scrollTop = 0;
    thread.dispatchEvent(new Event("scroll"));
    await new Promise((resolve) => setTimeout(resolve, 300));
  }), setup: async (bridge) => { await prepareFixture(bridge, "streaming"); planRun(bridge); } },
  // A clear scheduling request produces an active saved receipt in Bot.
  { ...base("chat-routine-proposed", "A routine saved from a clear request shows its next run", () => select(".message-schedule"), async () => {
    await until(() => select(".message-schedule-open"), "The saved routine did not render");
    if (hasLayout()) select(".message-schedules")?.scrollIntoView({ block: "center" });
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const schedule = { kind: "weekly" as const, days: [1], hour: 8, minute: 0, timezone: "Asia/Seoul" };
    const task = { id: "preview-routine", revision: 1, loupist_id: "loupist-quality", kind: "custom" as const, title: "Compare inventory days", prompt: "Compare inventory days across the assets I follow and flag any above 60 days.",
      schedule, target: { kind: "new_chat" as const }, model_profile_id: "preview", thinking_level: "medium", run_options: {}, use_memory: true, update_memory: false, status: "active" as const,
      created_at_ms: Date.now(), updated_at_ms: Date.now(), next_run_at_ms: Date.now() + 86_400_000 };
    bridge.installScheduledTask(task);
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const [question, answer] = workspace.threads[0].messages;
      if (question?.role === "user") question.content = "Every Monday at 8, compare inventory days for what I follow.";
      if (answer?.role === "assistant") {
        Object.assign(answer, { content: "Saved **Compare inventory days** for every Monday at 8:00, Seoul time. The app must be open for it to run.", evidence_refs: [], memory_update: undefined });
        answer.schedule_receipts = [{ task_id: task.id, title: task.title, revision: task.revision, action: "created", status: "active", schedule, target: { kind: "new_chat" }, next_run_at_ms: task.next_run_at_ms, use_memory: true, update_memory: false }];
      }
      return workspace;
    };
  } },
  // Narrow windows hide the line on purpose; those cells show the header without it.
  { ...base("shell-market-status", "The header shows the markets the user follows", () => select(".market-status") ?? (document.documentElement.clientWidth < 1100 ? select(".app-header") : null)), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    for (const entry of [{ symbol: "005930", market: "KRX", displayName: "Samsung Electronics" }, { symbol: "000660", market: "KRX", displayName: "SK hynix" }, { symbol: "AAPL", market: "NASDAQ", displayName: "Apple" }, { symbol: "BTC", market: "CRYPTO", displayName: "Bitcoin" }]) await bridge.watchlistAdd(entry);
  } },
  // W17.3: the run-profile menu names the plan behind the answer under its model list.
  { ...base("composer-plan-line", "The model menu says which plan pays", () => select(".composer-model-menu-panel [data-plan-line]"), async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    const trigger = await until(() => select(".composer-model-menu"), "The run-profile control did not render");
    trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
  }) },
  { ...base("composer-model-menu", "The composer shows Auto and its menu offers a model", () => select(".composer-model-menu-panel"), async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    // Radix menus open on pointer down, not on click.
    const trigger = await until(() => select(".composer-model-menu"), "The run-profile control did not render");
    trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
  }) },
  { ...base("composer-command-menu", "/ offers finance commands, then Skills", () => select('.composer-assist[data-symbol="/"]'), async root => {
    const composer = await until(() => root.querySelector<HTMLTextAreaElement>("textarea[data-chat-composer]"), "Composer did not appear");
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, "/");
    composer.setSelectionRange(composer.value.length, composer.value.length);
    composer.dispatchEvent(new Event("input", { bubbles: true }));
  }), setup: bridge => prepareFixture(bridge, "answer") },
  { ...base("chat-ticker-text", "Historical ticker references remain ordinary message text", () =>
      [...document.querySelectorAll(".message-user-text")].find(message => visible(message) && message.textContent?.includes("$AAPL:NASDAQ (Apple)")) ?? null),
    setup: async bridge => {
      await prepareFixture(bridge, "answer");
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const question = workspace.threads[0].messages.find(item => item.role === "user");
        if (question) question.content = previewKo() ? "$AAPL:NASDAQ (Apple) 뭐가 달라졌어? 비용은 $5였어." : "$AAPL:NASDAQ (Apple) What changed? It cost $5.";
        const answer = workspace.threads[0].messages.find(item => item.role === "assistant");
        if (answer) Object.assign(answer, { content: previewKo() ? "애플의 영업이익률이 올랐어요." : "Apple's operating margin increased.", evidence_refs: [], memory_update: undefined });
        return workspace;
      };
    },
  },
  // A pending decision remains actionable with the activity summary folded.
  ...([["chat-approval-card", "connection"], ["chat-approval-web-site", "site"]] as const).map(([id, kind]): PreviewScenario => ({
    ...base(id, kind === "site" ? "After third-party text, a new website waits for the user's OK" : "A read-only connection waits for the user's OK",
      () => select(kind === "site" ? ".chat-progress .approval-card[data-kind='web_site']" : ".chat-progress .approval-card"),
      async root => { await submitPrompt(root, "Read Hanul Precision’s Q3 filings and tell me what changed."); await centerOn(".approval-card")(); }),
    setup: async (bridge) => { await prepareFixture(bridge, "answer"); approvalRun(bridge, kind); },
  })),
  {
    ...base("chat-approval-decided", "Inspect a settled approval", () => select(".approval-line"), async root => {
      await click("button[data-research-summary]", root);
      await centerOn(".approval-line")();
    }),
    setup: async (bridge) => {
      await prepareFixture(bridge, "answer");
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const answer = workspace.threads[0].messages.find(item => item.id === "preview-answer");
        if (answer) {
          const at = Date.now();
          answer.progress = { startedAtMs: at - 30_000, finishedAtMs: at, items: approvalSteps(at, "connection", "succeeded"), approvals: [approvalFor("connection", "allowed_once", at)] };
        }
        return workspace;
      };
    },
  },
  { ...base("chat-private-marker", "A private chat stays marked in the sidebar", () => select(".loupist-private-mark"), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
    await openNavigation(".loupist-private-mark");
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      Object.assign(workspace.threads[0], { use_memory: false, update_memory: false });
      // Rust refuses Memory writes in a private chat, so its answers carry no saved items.
      for (const message of workspace.threads[0].messages) delete message.memory_update;
      return workspace;
    };
  } },
  { ...base("chat-title-editing", "A session's menu opens its rename dialog", () => select("#rename-thread-name"), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
    await openNavigation('[data-thread-id="preview-chat"]');
    const row = await until(() => document.querySelector('[data-thread-id="preview-chat"]')?.closest(".loupist-chat-row"), "Session did not initialize");
    const trigger = row.querySelector<HTMLButtonElement>(".loupist-chat-menu")!;
    trigger.focus();
    pressMenu(trigger);
    const rename = await until(() => [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(item => item.textContent?.trim() === i18n.t("app:loupistChatItem.rename")), "Rename action did not appear");
    rename.click();
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    // The home Thread keeps its name; this scenario opens the answer as an ordinary topic Chat.
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      Object.assign(workspace.threads[0], { home: false });
      return workspace;
    };
  } },
  { ...base("shell-unsent-draft", "A chat with unsent text shows a dot in the sidebar", () => select(".loupist-draft-dot"), async root => {
    const composer = await until(() => root.querySelector<HTMLTextAreaElement>("textarea[data-chat-composer]"), "Composer did not appear");
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, "Check inventory days against utilization next");
    composer.dispatchEvent(new Event("input", { bubbles: true }));
    // Narrow windows keep the sidebar behind its toggle.
    if (hasLayout() && document.documentElement.clientWidth < 768) await openNavigation(".loupist-draft-dot");
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
  // W15.24–W15.25: "In this chat" rail, its in-flow row on narrow chats, and the composer's + menu.
  { ...base("chat-rail-learning", "A finished reply's sources are still being reviewed", () => select(".chat-rail-processing") ?? (!hasLayout() ? select(".message.assistant") : null)), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    const read = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async id => {
      const workspace = await read(id);
      const answer = workspace.threads[0]?.messages.find(message => message.id === "preview-answer");
      const source = answer?.memory_update?.changes.find(change => change.kind === "Source");
      if (source) Object.assign(source, { worldJobId: "preview-job", worldStatus: "queued" });
      return workspace;
    };
    bridge.knowledgeGraphJob = async ({ loupistId, jobId }) => ({
      loupistId, jobId, status: "running", worldRecordId: "world", worldRevisionId: "rev", payloadDigest: "", createdAtMs: Date.now(), hostMayRedrive: false,
    });
  } },
  { ...base("chat-rail-context", "Current connections and learned names, including a long label", () => select(".chat-rail-chip") ?? (!hasLayout() ? select(".message.assistant") : null), async () => {
    if (!hasLayout()) return;
    await until(() => select(".chat-rail"), "Chat context did not appear");
    for (const card of ["using", "learned"]) {
      const header = await until(() => select(`[data-card="${card}"] .chat-rail-card-header`), "Context section did not appear");
      if (header.getAttribute("aria-expanded") === "false") (header as HTMLButtonElement).click();
    }
  }), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    const at = Date.now();
    chatResearchMap(bridge, () => [0, 1, 2, 3].map(index => chatGrowth(index, "preview-answer", at)),
      ["Hanul Precision", "Morrow Components", "Quarterly capacity utilization at the new packaging line", "HBM4"]);
    const read = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async id => {
      const workspace = await read(id);
      const answer = workspace.threads[0]?.messages.find(message => message.id === "preview-answer");
      if (answer) {
        answer.progress = { startedAtMs: at - 12_000, finishedAtMs: at, items: approvalSteps(at, "connection", "succeeded") };
        answer.lens_receipts = [...answer.lens_receipts ?? [], {
          recordId: "preview-rail-method", revisionId: "rev", kind: "method", delivery: "context",
          title: previewKo() ? "고급 메모리 수요와 출하량 비교 · Advanced-Memory Demand-to-Shipments Conversion" : "Advanced-Memory Demand-to-Shipments Conversion",
        }];
      }
      return workspace;
    };
  } },
  ...(["chat-rail-files", "chat-rail-needs-you", "chat-rail-compact", "chat-rail-bottom", "chat-header-resuming"] as const).map((id): PreviewScenario => ({
    ...base(id, id === "chat-header-resuming" ? "A resumed response keeps its status above the chat body and context" : id === "chat-rail-files" ? "Files and images in this chat sit in the upper-right column" : id === "chat-rail-needs-you" ? "A connection waiting for the user's OK shows in the rail" : "Context keeps its own space above a narrow or split conversation",
      // jsdom has no layout, so the chat column has no width and the rail stays away; there the answer stands in.
      () => !hasLayout() ? select(".message.assistant") : id === "chat-header-resuming" ? select(".thread-statuses .memory-state") : id === "chat-rail-needs-you" ? select(".chat-rail-needs") : select('.chat-rail [data-card="files"]'),
      async root => {
        if (id === "chat-rail-needs-you") await submitPrompt(root, "Read Hanul Precision’s Q3 filings and tell me what changed.");
        if (!hasLayout()) return;
        // Exercise both split directions with a context card present.
        const canSplit = document.documentElement.clientWidth > 1060;
        if (canSplit && (id === "chat-rail-compact" || id === "chat-rail-bottom")) await selectPanel("materials");
        if (canSplit && id === "chat-rail-bottom") await click('[data-workspace-split="bottom"]');
        await until(() => select(".chat-rail"), "The rail did not appear");
      }),
    setup: async (bridge) => {
      await prepareFixture(bridge, "answer");
      if (id === "chat-header-resuming") bridge.runActivityList = async () => [{
        run_id: "preview-resumed-run", loupist_id: "loupist-quality", kind: "chat",
        target: "preview-chat", started_at_ms: Date.now() - 30_000,
      }];
      if (id === "chat-rail-needs-you") approvalRun(bridge, "connection");
      const swatch = (color: string) => `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect width="96" height="96" fill="${color}"/><path d="M12 72 L36 44 L52 58 L84 22" stroke="white" stroke-width="6" fill="none"/></svg>`)}`;
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const [question, answer] = workspace.threads[0].messages;
        if (question?.role === "user") {
          question.attachments = [
            { id: "rail-1", filename: "utilization.png", media_type: "image/svg+xml", size_bytes: 48_200, url: swatch("steelblue") },
            { id: "rail-2", filename: "margin-bridge.png", media_type: "image/svg+xml", size_bytes: 51_900, url: swatch("darkgoldenrod") },
            { id: "rail-3", filename: "inventory.png", media_type: "image/svg+xml", size_bytes: 39_400, url: swatch("seagreen") },
            { id: "rail-4", filename: "holdings.csv", media_type: "text/csv", size_bytes: 14_300 },
          ];
        }
        if (answer?.role === "assistant") {
          answer.content = `${answer.content}\n\n${previewKo() ? "비교 내용은 [q3-memo.md](notes/q3-memo.md)에 적어 두었어요." : "I wrote the comparison to [q3-memo.md](notes/q3-memo.md)."}`;
        }
        return workspace;
      };
    },
  })),
  // B83/B85: one lightbox for every image in the chat; "Add to question" puts the image back in the tray.
  ...(["chat-lightbox", "chat-lightbox-added"] as const).map((id): PreviewScenario => ({
    ...base(id, id === "chat-lightbox" ? "The lightbox on the second of three images, sent in two questions" : "Add to question puts the image in the composer's tray, unsent",
      () => id === "chat-lightbox" ? select(".lightbox") : select(".composer-attachments .composer-thumb"),
      async root => {
        await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
        const thumbnails = await until(() => { const found = [...document.querySelectorAll<HTMLButtonElement>("button.sent-image")].filter(visible); return found.length >= 3 ? found : null; }, "Sent images did not render");
        thumbnails[1]!.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
        thumbnails[1]!.click();
        await until(() => select(".lightbox img"), "The lightbox did not show the image");
        if (id === "chat-lightbox-added") {
          // Step to the image sent with the later question, then add it.
          (await until(() => select(".lightbox"), "The lightbox did not open")).dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
          await click("[data-lightbox-add]");
          await until(() => !select(".lightbox") && select(".composer-attachments .composer-thumb"), "The image did not reach the composer");
        }
      }),
    setup: async (bridge) => {
      await prepareFixture(bridge, "answer");
      const swatch = (color: string) => `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="640" height="400"><rect width="640" height="400" fill="${color}"/><path d="M60 320 L230 190 L360 250 L580 80" stroke="white" stroke-width="16" fill="none"/></svg>`)}`;
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const thread = workspace.threads[0]!;
        const [question, answer] = thread.messages;
        if (question?.role === "user" && answer) {
          question.attachments = [
            { id: "lightbox-1", filename: "utilization.png", media_type: "image/svg+xml", size_bytes: 48_200, url: swatch("steelblue") },
            { id: "lightbox-2", filename: "margin-bridge.png", media_type: "image/svg+xml", size_bytes: 51_900, url: swatch("darkgoldenrod") },
          ];
          thread.messages.push(
            { ...question, id: "lightbox-follow-up", content: previewKo() ? "이 재고 차트도 같이 봐 줘." : "Look at this inventory chart too.", created_at: "2026-10-28T00:05:00Z",
              attachments: [{ id: "lightbox-3", filename: "inventory-days.png", media_type: "image/svg+xml", size_bytes: 39_400, url: swatch("seagreen") }] },
            { ...answer, id: "lightbox-follow-up-answer", content: previewKo() ? "재고일수는 54일로, 같은 업종 평균보다 길어요." : "Inventory sits at 54 days, above the peer average.", evidence_refs: [], memory_update: undefined, created_at: "2026-10-28T00:05:12Z" },
          );
        }
        return workspace;
      };
    },
  })),
  // B83/B85 (chat-run §2 breakpoints): at 700 px a press and hold opens the citation card; wider windows open it on hover.
  { ...base("chat-citation-longpress", "A press and hold on a citation opens its card in a narrow window", () => select(".citation-preview"), async () => {
    await until(messagesRendered, "Message Markdown did not finish rendering");
    const chip = await until(() => select("button.loupist-citation"), "Citation did not render") as HTMLButtonElement;
    if (hasLayout() && window.matchMedia("(max-width: 720px)").matches) {
      chip.scrollIntoView({ block: "center" });
      chip.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "touch" }));
      try {
        await until(() => select(".citation-preview"), "The card did not open on a long press", 2000);
      } catch (error) {
        throw new Error(`The card did not open on a long press (citation connected: ${chip.isConnected})`, { cause: error });
      }
      chip.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, button: 0, pointerType: "touch" }));
      chip.click();
    } else chip.focus();
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
  // B92: a step Rust refused after the answer read other sites explains itself in the step, with no override.
  { ...base("chat-step-paused", "A command paused because the answer read other sites", () => select(".chat-step-paused"), async root => {
    await click("button[data-research-summary]", root);
    await centerOn(".chat-step-paused")();
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const answer = workspace.threads[0]!.messages.find((message) => message.role === "assistant");
      const at = Date.parse("2026-10-28T00:00:00Z");
      if (answer) answer.progress = { startedAtMs: at, finishedAtMs: at + 12_000, items: [
        { id: "paused-search", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", target: "Hanul Precision third-quarter margin", status: "succeeded", startedAtMs: at + 1_000, finishedAtMs: at + 3_000 },
        { id: "paused-page", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", target: "Q3 operating review · hanul.example", href: "https://hanul.example/quarterly-review", status: "succeeded", startedAtMs: at + 3_000, finishedAtMs: at + 6_000 },
        { id: "paused-bash", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", target: "python3 margin_model.py", status: "failed", refusal: "untrusted_text_paused", startedAtMs: at + 7_000, finishedAtMs: at + 7_100 },
      ] };
      return workspace;
    };
  } },
  // X9 (B82): Print or save as PDF draws a sheet for paper; the capture emulates print media to show the page itself.
  { ...base("chat-print-view", "A chat printed for Save as PDF", printLandmark, async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    window.print = () => undefined;
    // The open Thread has no sidebar menu; the palette prints the chat on screen.
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, ctrlKey: !navigator.userAgent.includes("Mac"), bubbles: true }));
    const input = await until(() => select(".command-palette-input") as HTMLInputElement | null, "Palette did not open");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, i18n.t("app:palette.actions.printChat"));
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await click('[data-palette-row="action:print-chat"]');
    await until(() => document.querySelector(".print-sheet .print-document [data-streamdown]"), "The print sheet did not render");
  }), setup: (bridge) => prepareFixture(bridge, "answer"), media: "print" },
  { ...base("briefing-print-view", "A briefing printed for Save as PDF", printLandmark, async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    window.print = () => undefined;
    await openScheduleRun(root, "preview-briefing-1");
    await click("[data-briefing-print]");
    await until(() => document.querySelector(".print-sheet .print-document"), "The print sheet did not render");
  }), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    bridge.includeBriefingBody = true;
    const [loupist] = await bridge.loupistList();
    await bridge.briefingSettingsSet({ loupistId: loupist!.id, enabled: true });
  }, media: "print" },
  { ...base("composer-attach-tray", "Attached files show their real sizes in the tray", () => select(".composer-attachments .composer-file-size"), async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    const column = await until(() => select(".chat-workspace"), "The chat column did not render");
    // A drop on the column attaches, as dragging files from Finder does.
    const drop = new Event("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", { value: { types: ["Files"], files: [
      new File([new Uint8Array(1_258_291)], "q3-deck.pdf", { type: "application/pdf" }),
      new File(["symbol,shares\n".repeat(1_000)], "holdings.csv", { type: "text/csv" }),
    ] } });
    column.dispatchEvent(drop);
    await until(() => select(".composer-attachments .composer-file-size"), "The tray did not show sizes");
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
  { ...base("composer-attach-menu", "The + menu attaches files, a pasted image, the map or a saved source", () => select(".composer-add-menu"), async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    const trigger = await until(() => select(".composer-add-trigger"), "The + control did not render");
    trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
  // W15.36: reading details. Dividers mark a new day and what finished while the chat was out of sight; long answers get an outline.
  { ...base("chat-dividers", "Dividers mark a new day and what finished while you were away", () => select('.chat-divider[data-kind="new"]'), async () => {
    // Frame the combined day/new-answer divider after the Markdown mounts,
    // rather than leaving it beyond a narrow capture's top scroll edge.
    await centerOn('.chat-divider[data-kind="new"]')();
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    // The user last had this chat in view the evening before the answer finished.
    try { localStorage.setItem("loupist.chatSeen.v1", JSON.stringify({ "preview-chat": Date.parse("2026-10-27T12:00:00Z") })); } catch { /* preview without storage */ }
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const question = workspace.threads.find(thread => thread.id === "preview-chat")?.messages.find(message => message.role === "user");
      if (question) question.created_at = "2026-10-27T09:00:00Z";
      return workspace;
    };
  } },
  { ...base("chat-branch", "An edited question links to its earlier version", () => select(".message.user .message-version-link"), centerOn(".message.user .message-version-link")), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const thread = workspace.threads.find(item => item.id === "preview-chat");
      // The question was edited; its first wording stays as an earlier version.
      if (thread) Object.assign(thread, { versions: [{ id: "preview-version", after_message_id: null, first_message_at_ms: Date.parse("2026-10-27T23:50:00Z"), message_count: 2 }] });
      return workspace;
    };
  } },
  { ...base("chat-outline", "A long answer opens with its outline", () => select(".answer-outline[open]"), async () => {
    await click(".answer-outline > summary");
    await centerOn(".answer-outline[open]", "start")();
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const ko = previewKo();
    const sections = ko
      ? ["## 매출", "매출은 11% 늘어 1,842억 원이에요.", "## 영업이익률", "영업이익률은 16.2%에서 14.8%로 낮아졌어요.", "### 인증 비용", "새 생산라인의 인증 비용이 하락의 대부분이에요.", "## 다음에 볼 것", "가동률과 재고일수를 비교해 보세요."]
      : ["## Revenue", "Revenue grew 11% to ₩184.2 billion.", "## Operating margin", "The operating margin fell from 16.2% to 14.8%.", "### Qualification costs", "Most of the decline came from qualification costs at the new line.", "## What to watch next", "Compare utilization against inventory days."];
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const answer = workspace.threads.find(thread => thread.id === "preview-chat")?.messages.find(message => message.id === "preview-answer");
      if (answer) answer.content = sections.join("\n\n");
      return workspace;
    };
  } },
  { ...base("chat-chart-inline", "A finished chart sits in the answer", () => {
    const card = select('.chat-artifact-block[data-state="ready"]');
    const chart = card?.querySelector(".artifact-chart-shell");
    // jsdom cannot draw the chart engine; there the chart figure stands in for its canvas.
    return chart && !select(".workspace-panel") && (!hasLayout() || chart.querySelector("canvas")) ? card : null;
  }, async (root) => {
    await submitPrompt(root, "Chart Hanul Precision’s price and volume.");
    await until(() => select('.chat-artifact-block[data-state="ready"]'), "Chart card did not fill");
  }), setup: inlineChartFixture },
  // B73: Rust keeps only a chart's current revision; an earlier answer says its chart changed instead of drawing the newer one.
  { ...base("chat-chart-revised", "An earlier answer's chart was revised by a later answer", () => {
    const card = select('.chat-artifact-block[data-state="revised"]');
    const chart = card?.querySelector(".artifact-chart-shell");
    // Revision metadata arrives before the lazy chart engine finishes loading.
    return chart && (!hasLayout() || chart.querySelector(".chart-financial-canvas canvas")) ? card : null;
  }, async (root) => {
    await submitPrompt(root, "Chart Hanul Precision’s price and volume.");
    await until(() => select('.chat-artifact-block[data-state="revised"]'), "The chart card did not notice the later revision");
    await centerOn('.chat-artifact-block[data-state="revised"]')();
  }), setup: async (bridge) => {
    await inlineChartFixture(bridge);
    // A later answer published revision 2, the only one Rust keeps; this answer recorded revision 1.
    const read = bridge.chatArtifactRead.bind(bridge);
    bridge.chatArtifactRead = async (loupistId, chatId, artifactId, revisionId) => {
      const view = await read(loupistId, chatId, artifactId, revisionId);
      return { ...view, artifact: { ...view.artifact, current_revision: view.artifact.current_revision + 1 } };
    };
  } },
  // W15.20: one chip grammar; a phrase selected in an answer becomes a quote chip.
  { ...base("chat-quote-ask", "Selecting a phrase in an answer offers to ask about it", () => document.getSelection()?.toString().trim() ? select(".quote-ask") : null, async () => {
    await selectAnswerWords(0, 6);
  }), selection: true, setup: (bridge) => prepareFixture(bridge, "answer") },
  { ...base("composer-inline-context", "Inline file reference and a Lens mode in a saved question and draft", () =>
    document.querySelector(".composer-prompt-highlight [data-inline-reference]") && select(".message-user-context .message-tool-label")), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    const read = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async id => {
      const workspace = await read(id);
      const thread = workspace.threads.find(t => t.home);
      if (thread) thread.messages = [{ id: "preview-lens-question", role: "user", status: "complete", created_at: new Date().toISOString(),
        content: previewKo() ? "$lens\n\n지금 어떤 판단 기준을 쓰고 있어?" : "$lens\n\nWhich research principles are active?" }];
      return workspace;
    };
    const token = "@report.md";
    const prompt = previewKo() ? `이 기준으로 ${token} 실적을 살펴봐 줘.` : `Apply these principles to ${token} earnings.`;
    const start = prompt.indexOf(token);
    await bridge.chatDraftWrite({ loupist_id: "loupist-quality", thread_id: "preview-chat" }, {
      prompt, context: { pins: [], asOf: "2026-10-15", chips: [
        { key: "skill:lens", kind: "skill", id: "lens", label: "Lens" },
        { key: "file:report", kind: "file", id: "reports/report.md", label: "report.md" },
      ], inlineReferences: [{ key: "chip:file:report", kind: "file", text: token, start, end: start + token.length }] },
    });
  } },
  { ...base("composer-chips", "Quoted passages stay inline with the question", () => {
    const prompt = select("textarea[data-chat-composer]") as HTMLTextAreaElement | null;
    return document.querySelectorAll('.composer-prompt-highlight [data-inline-reference="quote"]').length === 5 && prompt?.value.trim() ? prompt : null;
  }, async () => {
    // Five different phrases; the Korean answer is short, so they overlap.
    for (const start of [0, 2, 4, 6, 8]) {
      await selectAnswerWords(start, 3, 1);
      await click(".quote-ask");
    }
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
  // W15.15–W15.17: "What {Loupist} knows", the inline correction under an answer, and the once-per-chat decline.
  ...(["memory-page", "memory-page-empty", "memory-paused", "memory-reset-dialog"] as const).map((id): PreviewScenario => ({
    ...base(id, id === "memory-page" ? "What the Loupist knows, with a principle open" : id === "memory-paused" ? "Learning is paused" : "A new Loupist knows only what you follow",
      () => select(id === "memory-reset-dialog" ? ".memory-reset-dialog" : id === "memory-page" ? ".memory-reader:not(.is-empty)" : id === "memory-paused" ? ".memory-page-paused" : ".memory-about"), async root => {
        await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
        if (id === "memory-reset-dialog") {
          await openLoupistSettings();
          await click('[data-loupist-section="learning"] > summary');
          await click("[data-memory-reset]");
        } else {
          await selectPanel("lens");
          if (id === "memory-page") await click(".memory-page-list .memory-row");
          if (id === "memory-page-empty") await click('.memory-page-tabs [role="tab"]:first-child');
        }
      }),
    setup: async (bridge) => {
      await prepareFixture(bridge, id === "memory-page" ? "answer" : "empty");
      for (const entry of [{ symbol: "005930", market: "KRX", displayName: "Samsung Electronics" }, { symbol: "000660", market: "KRX", displayName: "SK hynix" }, { symbol: "AAPL", market: "NASDAQ", displayName: "Apple" }]) await bridge.watchlistAdd(entry);
      if (id === "memory-page") {
        const first = await ensurePrinciple(bridge, { loupistId: "loupist-quality", statement: "Check operating cash flow before trusting reported profit.", why: "Profit can rise on accruals while cash lags.", origin: "feedback", originChatId: "preview-chat", originMessageId: "preview-answer" });
        (bridge as unknown as { state: { memoryUsage?: Record<string, unknown> } }).state.memoryUsage = { "loupist-quality": [{ recordId: first.recordId, usedCount: 4, lastUsedAtMs: Date.parse("2026-10-28T00:00:00Z"), usedIn: [{ chatId: "preview-chat", atMs: Date.parse("2026-10-28T00:00:00Z") }] }] };
      } else {
        bridge.principleList = async () => [];
        bridge.learningReviewList = async () => [];
        bridge.knowledgeGrowthEvents = async () => ({ events: [], nextCursor: null });
        // A new Loupist has learned no methods either.
        const catalog = bridge.knowledgeCatalog.bind(bridge);
        bridge.knowledgeCatalog = async (request) => ({ ...(await catalog(request)), lenses: [] });
      }
      if (id === "memory-reset-dialog") bridge.knowledgeMemoryResetPreview = async () => ({ principles: 2, pages: 3, sections: 5, names: 12, relationships: 18, sources: 4 });
      if (id === "memory-paused") {
        const scope = { kind: "project" as const, loupistId: "loupist-quality" };
        const current = await bridge.configRead({ scope });
        await bridge.configUpdate({ scope, expectedRevision: current.revision, set: { chat: { use_memory: false, update_memory: false } }, unset: [] });
        await bridge.memoryLearningSettingsSet({ loupistId: "loupist-quality", learnMethods: false });
      }
    },
  })),
  { ...base("memory-pages", "What the Loupist knows: a topic page open with every kind of basis", () => {
    const reader = select(".memory-reader .page-reader");
    return reader && reader.querySelector(".page-contested") && reader.querySelector('[data-chip="said"]') && reader.querySelector('[data-chip="view"]') ? reader : null;
  }, async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    await selectPanel("lens");
    await until(() => select(".memory-page-list .memory-row"), "Pages did not load");
    await click('.memory-page-list .memory-row[aria-label^="Hanul Precision"], .memory-page-list .memory-row[aria-label^="한울정밀"]');
    await until(() => select(".page-reader .page-section"), "The page did not open");
  }), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    bridge.knowledgeGrowthEvents = async () => ({ events: [], nextCursor: null });
    bridge.learningReviewList = async () => [];
  } },
  ...(["chat-used-principle-correction", "chat-save-declined"] as const).map((id): PreviewScenario => ({
    ...base(id, id === "chat-save-declined" ? "A secret was not saved to memory" : "A principle that shaped the answer can be set aside",
      () => select(id === "chat-save-declined" ? ".memory-declined" : ".used-memory"), async () => {
        if (id === "chat-used-principle-correction") await click("[data-receipt-used]");
        if (hasLayout()) select(id === "chat-save-declined" ? ".memory-declined" : ".used-memory")?.scrollIntoView({ block: "center" });
      }),
    setup: async (bridge) => {
      await prepareFixture(bridge, "answer");
      const principle = await ensurePrinciple(bridge, { loupistId: "loupist-quality", statement: "Check operating cash flow before trusting reported profit.", origin: "settings" });
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const answer = workspace.threads[0].messages.find(item => item.id === "preview-answer")!;
        if (id === "chat-used-principle-correction") {
          answer.lens_receipts = [{ recordId: principle.recordId, revisionId: principle.revisionId, kind: "principle", title: principle.statement, delivery: "context" }];
          answer.content += previewKo()
            ? `\n\n원칙에 따라 영업이익률을 보기 전에 [영업현금흐름을 먼저 확인했어요](lens:${principle.recordId}).`
            : `\n\nFollowing your principle, I [checked operating cash flow first](lens:${principle.recordId}) before reading the margin.`;
        } else {
          // The user pasted something secret-looking; Rust refused to keep it.
          const question = workspace.threads[0].messages.find(item => item.role === "user");
          if (question) question.content = previewKo() ? "데이터 피드용 API 키 sk-test-4f9a2c7e를 기억해 줘." : "Remember my data feed API key sk-test-4f9a2c7e.";
          answer.content = previewKo() ? "그 키는 기억에 저장하지 않았어요. 연결 설정에 넣으면 안전하게 보관돼요." : "I didn't keep that key in memory. Put it in the connection's settings, where it is stored safely.";
          answer.evidence_refs = [];
          answer.memory_update = undefined;
          answer.tool_refusals = [{ connection_id: "memory", connection_name: "Memory", tool_name: "note_save", reason: "sensitive_memory" }];
        }
        return workspace;
      };
    },
  })),
  { ...base("panel-closed-text-answer", "A sourced text answer leaves the panel closed", () => {
    const answer = select("article.message.assistant.complete");
    return answer?.querySelector("button.loupist-citation") && !select(".workspace-panel") ? answer : null;
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
  ...(["panel-source-selection", "panel-source-selected"] as const).map((id): PreviewScenario => ({
    ...base(id, id === "panel-source-selection" ? "A retained Source selection offers Add to question" : "A retained Source selection stays inline in the question without sending",
      () => id === "panel-source-selection" ? (document.getSelection()?.toString().trim() ? select(".quote-ask") : null) : document.querySelector('.composer-prompt-highlight [data-inline-reference="source_span"]') ? select("textarea[data-chat-composer]") : null, async () => {
        const passage = await until(() => select(".workspace-panel [data-quote-retained-source]"), "Saved Source passage did not open");
        const node = passage.firstChild!;
        (document.activeElement as HTMLElement | null)?.blur();
        const range = document.createRange();
        range.setStart(node, 0); range.setEnd(node, Math.min(node.textContent!.length, 64));
        document.getSelection()!.removeAllRanges(); document.getSelection()!.addRange(range);
        document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
        await until(() => select(".quote-ask"), "Source selection action did not appear");
        if (id === "panel-source-selected") await click(".quote-ask");
      }),
    selection: id === "panel-source-selection",
    setup: async bridge => {
      await prepareFixture(bridge, "answer");
      const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
      const title = previewKo() ? "한울정밀 3분기 보고서" : "Hanul Precision quarterly report";
      const saved = await bridge.knowledgeSourceRetain({ loupistId: "loupist-quality", title, originLocator: "Pasted text", coverage: "included_full", text: previewKo() ? "매출은 11% 늘었지만 영업이익률은 14.8%로 낮아졌어요. 새 생산라인의 인증 비용이 늘었어요. 재고일수와 가동률을 함께 확인해야 해요." : "Revenue grew 11% while operating margin fell to 14.8%. Qualification costs at the new production line increased. Compare inventory days with utilization." });
      catalog.sources.unshift({ ...catalog.sources[0], recordId: saved.recordId, revisionId: saved.revisionId, title, objects: [{ id: saved.sourceId, title }] });
      bridge.knowledgeCatalog = async ({ loupistId }) => structuredClone({ ...catalog, loupistId });
      await bridge.workspaceUiWrite({ version: 1, open: true, placement: "side", width: 480, height: 360, sessions: [{ loupist_id: "loupist-quality", thread_id: "preview-chat", active_tab_id: "saved-source", tabs: [{ id: "saved-source", kind: "memory", title, record_id: saved.recordId, location: { loupistId: "loupist-quality", surface: "source", recordId: saved.recordId, revisionId: saved.revisionId, selection: { kind: "source", sourceId: saved.sourceId, spanId: saved.spanId } } }] }] });
    },
  })),
  { ...base("panel-evidence-passage", "A citation opens its retained passage", () => select(".workspace-evidence .evidence-match"), async root => {
    await click("button.loupist-citation", root);
    await viewSavedPassage();
    await until(() => select(".workspace-evidence .evidence-match"), "Evidence passage did not open");
  }), setup: bridge => citedAnswerFixture(bridge) },
  { ...base("panel-evidence-history", "Evidence keeps one reader with history", () => select('.workspace-status button:not([disabled])') && select(".workspace-evidence"), async root => {
    await until(() => root.querySelectorAll("button.loupist-citation").length >= 2, "Second citation did not render");
    await click("button.loupist-citation", root);
    await until(() => select(".workspace-evidence"), "Evidence did not open");
    const citations = [...root.querySelectorAll<HTMLButtonElement>("button.loupist-citation")];
    citations[1]!.click();
    await until(() => select('.workspace-status button:not([disabled])'), "Evidence history did not advance");
  }), setup: bridge => citedAnswerFixture(bridge, true, false) },
  { ...base("panel-badge-not-focus", "New work marks its tab while the reader stays", () => badgedTab() && select(".workspace-evidence"), async root => {
    await click("button.loupist-citation", root);
    const panel = await until(() => select(".workspace-panel"), "Panel did not open");
    panel.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    // The narrow sheet covers the composer: close it, send, and reopen it; work published under the sheet is marked, never focused.
    const sheet = panel.getAttribute("role") === "dialog";
    if (sheet) {
      root.querySelector<HTMLElement>(".artifact-panel-backdrop")!.click();
      await until(() => !select(".workspace-panel"), "Sheet did not close");
    }
    const composer = await until(() => root.querySelector<HTMLTextAreaElement>('textarea[data-chat-composer]'), "Composer did not appear");
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, "Draft a short document on Hanul Precision’s margin.");
    composer.dispatchEvent(new Event("input", { bubbles: true }));
    await click('.composer-shell button[type="submit"]', root);
    if (sheet) await selectPanel("materials");
    await until(badgedTab, "New work did not mark its tab", 20_000);
  }), setup: bridge => citedAnswerFixture(bridge, false, false) },
  ...["chat-mini-map", "chat-mini-map-empty"].map((id): PreviewScenario => ({
    ...base(id, "Chat research map", () => select(".chat-mini-map .map-empty") ?? (mapCanvas() ? select(".chat-mini-map .map-canvas") : null), async root => {
      if (id === "chat-mini-map") await click('.chat-receipt [data-receipt="names"] button', root);
      else {
        await selectPanel("materials");
        await click("#workspace-tab-chat-map", root);
      }
      await until(() => select(".chat-mini-map .map-empty") ?? (mapCanvas() ? select(".chat-mini-map .map-canvas") : null), "Chat map did not load");
      if (hasLayout()) for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
    }),
    setup: async bridge => {
      await prepareFixture(bridge, id === "chat-mini-map" ? "answer" : "empty");
      const readMap = bridge.knowledgeMap.bind(bridge);
      bridge.knowledgeMap = async request => {
        const map = await readMap(request);
        map.nodes = ["Hanul Precision", "Morrow Components", "Capacity utilization", "Operating margin"].map((label, index) => ({ id: `chat-node-${index}`, kind: "entity", label, entityKind: index < 2 ? "Organization" : "Entity", degree: index === 0 ? 3 : 1, watched: false, held: false }));
        map.edges = [1, 2, 3].map(index => ({ id: `chat-edge-${index}`, kind: "claim", source: "chat-node-0", target: `chat-node-${index}`, relation: "related_to", status: "active" }));
        map.worldRevisionId = "chat-mini-map";
        return map;
      };
      bridge.knowledgeGrowthEvents = async () => ({ events: id === "chat-mini-map-empty" ? [] : [0, 1, 2].map(index => ({ eventId: `chat-growth-${index}`, loupistId: "loupist-quality", originChatId: "preview-chat", originMessageId: "preview-answer", atMs: Date.now(), kind: "entity_added", subjectId: `chat-node-${index}`, recordId: "preview-world", actor: "loupist", summary: "", beforeSummary: null, afterSummary: null })), nextCursor: null });
    },
  })),
  { ...base("chat-live-map", "Names a running step reads glow on the mini map", () => document.querySelector(".chat-progress-names") && (mapCanvas() ? select(".chat-mini-map .map-canvas") : null), async root => {
    await submitPrompt(root);
    await click(".message.assistant.streaming .chat-progress-toggle", root);
    await until(() => document.querySelector(".chat-progress-names"), "Step names did not appear");
    await selectPanel("materials");
    await click("#workspace-tab-chat-map", root);
    await until(() => mapCanvas() ? select(".chat-mini-map .map-canvas") : null, "Chat map did not load");
    if (hasLayout()) for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
  }), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    // Earlier growth is settled, so only the running step's names glow.
    chatResearchMap(bridge, () => [0, 1, 2].map(index => chatGrowth(index, "preview-answer", Date.now() - 60_000)));
    bridge.chatSend = async (request, observer) => {
      const run_id = request.run_id;
      observer({ type: "started", run_id });
      observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: Date.now() - 9_000, items: [
        { id: "preview-map-read", kind: "tool", category: "memory", operation: "read", action: "Read the map", status: "succeeded", entityIds: ["chat-node-0", "chat-node-2"] },
        { id: "preview-page", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", running: "Reading hanul.example", target: "Q3 operating review · hanul.example", status: "running" },
      ] } });
      observer({ type: "phase", run_id, phase: "tool" });
      return { run_id };
    };
  } },
  // W15.29, W15.32, W15.33: the inspector's Relationships view, the panel's Work list, and charts docked below the chat.
  { ...base("map-relationships", "A company's suppliers, customers and peers in the inspector", () => select(".map-relationships"), async (root) => {
    const map = await enterMap(root);
    const input = await until(() => map.querySelector<HTMLInputElement>('[role="combobox"]'), "Map search did not load");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "TSMC");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await click('[role="option"]', map);
    await until(() => select(".map-inspector"), "Inspector did not open");
    await click(".map-inspector-views button:last-child");
    // The dotted Lens line closes the view; bring it into the inspector's scroll.
    (await until(() => select(".map-relationship-lenses"), "The Lens line did not render")).scrollIntoView({ block: "nearest" });
  }), setup: async (bridge) => { await prepareFixture(bridge); localViewMap(bridge, { lens: true }); rememberPreviewMap({ scope: "1" }); } },
  { ...base("panel-charts-list", "The Work tab lists the Loupist's charts and documents", () => select(".work-list .work-row"), async (root) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    await selectPanel("materials");
    await click("#workspace-tab-work");
  }), setup: workListFixture },
  // B84: the open item's version menu, an earlier version with Restore asked, and the restored newest version.
  { ...base("panel-work-version", "An open document lists its kept versions", () => select('.work-version-menu [role="menuitemradio"]'), async (root) => {
    await openVersionedDocument(root);
    await openVersionMenu();
  }), setup: versionedDocument },
  { ...base("panel-work-version-restore", "An earlier version is read-only and Restore asks first", () => select(".work-version-confirm"), async (root) => {
    await openVersionedDocument(root);
    await chooseEarliestVersion();
    await click("[data-work-version-bar] .work-version-actions button");
    await until(() => select(".work-version-confirm"), "Restore did not ask first");
  }), setup: versionedDocument },
  { ...base("panel-work-version-restored", "A restored version becomes the newest one", () => select("[data-work-version-restored]"), async (root) => {
    await openVersionedDocument(root);
    await chooseEarliestVersion();
    await click("[data-work-version-bar] .work-version-actions button");
    await click(".work-version-confirm .work-version-actions button");
  }), setup: versionedDocument },
  // Expand reads the open citation in one column. A web page opens in the browser; other evidence keeps the reader. The narrow sheet has no Expand.
  { ...base("panel-expand-evidence", "Expand reads the open item in one column with Back to chat", () => {
    const body = select(".workspace-evidence") ?? select(".browser-toolbar");
    if (!body) return null;
    return window.matchMedia(workspaceOverlayMedia).matches || select(".workspace-panel[data-expanded]") ? body : null;
  }, async (root) => {
    await click("button.loupist-citation", root);
    await until(() => select(".workspace-evidence") ?? select(".browser-toolbar"), "The citation did not open");
    const actions = select(".workspace-dock-options");
    if (!actions) return;
    pressMenu(actions);
    await click("[data-workspace-expand]");
  }), setup: bridge => citedAnswerFixture(bridge) },
  // The panel menu pins one placement for every kind. The overlay under 1060 px
  // offers no panel menu, so there the Work list stands in.
  { ...base("panel-dock-menu", "The Dock control's menu with Always dock here", () => select(".workspace-dock-menu") ?? (window.matchMedia(workspaceOverlayMedia).matches ? select(".work-list .work-row") : null), async (root) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    await selectPanel("materials");
    await click("#workspace-tab-work");
    if (window.matchMedia(workspaceOverlayMedia).matches) return;
    pressMenu(await until(() => select(".workspace-dock-options"), "The Dock control's menu did not render"));
  }), setup: workListFixture },
  // The docked chart is the subject: a capture waits for its canvas, not just the Work tab's status row.
  // jsdom cannot draw a canvas, so there the status row stays the landmark.
  { ...base("panel-dock-chart", "A chart opened from Work docks below the chat", () => {
    if (!hasLayout()) return select(".workspace-work-tab .workspace-status");
    const shell = select(".workspace-work-tab .artifact-chart-shell");
    return shell?.querySelector("canvas") ? shell : null;
  }, async (root) => {
    await submitPrompt(root, "Chart Hanul Precision’s price and volume.");
    // The chart run is the slow part; give it room when the machine is busy.
    await until(() => select('.chat-artifact-block[data-state="ready"]'), "Chart card did not fill", 15_000);
    await selectPanel("materials");
    await click("#workspace-tab-work");
    await click(".work-list button.work-row");
  }), setup: inlineChartFixture },
  // B84 / side-panel.md X13: files an answer wrote and linked sit under it as chips; ⋯ shows one in Finder.
  { ...base("panel-delivered-files", "An answer with two delivered files and the chip row", () => select(".message-file-menu"), async (root) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    const row = await until(() => select(".message-files"), "The file chips did not render");
    row.scrollIntoView({ block: "center" });
    pressMenu(row.querySelector(".message-file-chip-more"));
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async (loupistId) => {
      const workspace = await original(loupistId);
      const answer = workspace.threads[0].messages.at(-1);
      if (answer?.role === "assistant") {
        const at = Date.parse(answer.created_at);
        const wrote = (id: string, filePath: string): ChatProgressItem => ({ id, kind: "tool", toolName: "write", category: "files", operation: "write", action: "Wrote a file", filePath, status: "succeeded", startedAtMs: at - 4_000, finishedAtMs: at - 3_000 });
        answer.progress = { startedAtMs: at - 9_000, finishedAtMs: at, ...answer.progress, items: [...(answer.progress?.items ?? []), wrote("preview-write-memo", "notes/q3-margin-memo.md"), wrote("preview-write-table", "exports/hanul-margins-2024-2026.csv")] };
        answer.content = `${answer.content}\n\n${previewKo() ? "비교 메모는 [q3-margin-memo.md](notes/q3-margin-memo.md)에, 분기별 수치는 [hanul-margins-2024-2026.csv](exports/hanul-margins-2024-2026.csv)에 저장했어요." : "I saved the comparison to [q3-margin-memo.md](notes/q3-margin-memo.md) and the quarterly figures to [hanul-margins-2024-2026.csv](exports/hanul-margins-2024-2026.csv)."}`;
      }
      return workspace;
    };
  } },
  { ...base("panel-opens-first-map-change", "A chat's first names open the Map beside it", () => {
    const names = select('article.message.assistant.complete .chat-receipt [data-receipt="names"]');
    if (!names) return null;
    // Below 1280 px, or as a sheet, the panel stays closed and the receipt carries the names.
    if (window.innerWidth < 1280) return select(".workspace-panel") ? null : names;
    return select('#workspace-tab-chat-map[aria-selected="true"]') && (mapCanvas() ? select(".chat-mini-map .map-canvas") : null);
  }, async root => {
    await submitPrompt(root);
    await until(() => select('article.message.assistant.complete .chat-receipt [data-receipt="names"]'), "The answer's names did not arrive", 20_000);
    if (hasLayout() && window.innerWidth >= 1280) {
      await until(() => mapCanvas() ? select(".chat-mini-map .map-canvas") : null, "Map did not open beside the chat");
      for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
    }
  }), setup: async bridge => {
    await prepareFixture(bridge);
    firstMapChange.messageId = "";
    chatResearchMap(bridge, () => firstMapChange.messageId ? [0, 1, 2].map(index => chatGrowth(index, firstMapChange.messageId, Date.now())) : []);
    const send = bridge.chatSend.bind(bridge);
    bridge.chatSend = (request, observer) => send(request, event => {
      if (event.type === "completed") firstMapChange.messageId = event.message_id;
      observer(event);
    });
  } },
  ...["chat-receipt", "chat-receipt-empty", "chat-learned"].map((id): PreviewScenario => ({
    ...base(id, id === "chat-receipt" ? "Answer receipt with every segment" : id === "chat-learned" ? "Answer whose Learned line is open" : "Answer that saved nothing", () => {
      const receipt = select("article.message.assistant.complete .chat-receipt");
      if (!receipt) return null;
      if (id === "chat-receipt-empty") return receipt;
      if (id === "chat-learned") return select(".learned-card .learned-section p") ? receipt : null;
      return receipt.querySelectorAll(".chat-receipt-item").length >= 6 ? receipt : null;
    }, async root => {
      await until(() => root.querySelector("article.message.assistant.complete"), "Answer did not load");
      if (id === "chat-learned") {
        await click('.chat-receipt [data-receipt-learned="sections"]', root);
        await until(() => select(".learned-card .learned-section p"), "Filed pages did not load");
      }
      if (hasLayout()) {
        await until(() => id === "chat-receipt-empty" ? select(".chat-receipt") : root.querySelector("button.loupist-citation"), "Answer rendering did not finish");
        await until(() => id === "chat-receipt-empty" || select('.chat-receipt [data-receipt="names"]'), "Map growth did not reach the receipt");
        select(id === "chat-learned" ? ".learned-card" : ".chat-receipt")?.scrollIntoView({ block: "center" });
      }
    }),
    setup: async bridge => {
      await prepareFixture(bridge, "answer");
      const principle = (await bridge.principleList({ loupistId: "loupist-quality" }))[0];
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const message = workspace.threads[0].messages.find(item => item.id === "preview-answer")!;
        if (id === "chat-receipt-empty") {
          message.memory_update = { status: "no_change", commitId: null, changes: [] };
          message.evidence_refs = [];
          message.content = message.content.replace(/ \[1\]\(#evidence-[^)]+\)/, "");
          return workspace;
        }
        const [cited] = message.evidence_refs ?? [];
        if (cited) message.evidence_refs = [cited, { ...cited, id: `ev_${"c".repeat(32)}`, title: "Morrow Components supplier capacity survey", url: "https://morrow.example/capacity-survey", provider: "Morrow Components IR", content_sha256: "c".repeat(64) }];
        message.content += document.documentElement.lang.startsWith("ko")
          ? `\n\n내 원칙 점검: [${principle.statement}](lens:${principle.recordId}) 이번 분기에는 충족돼요.`
          : `\n\nChecked against your principles: [${principle.statement}](lens:${principle.recordId}) holds for this quarter.`;
        message.memory_refs = [{ read_ref: "preview-method-read", kind: "lens", title: "Separate durable margins from temporary capacity costs", excerpt: "", access: "exact_read", knowledge: { kind: "lens", recordId: "preview-method", revisionId: "preview-method-rev", objectId: "preview-method" } }];
        // The receipt's used principles and methods come from what the host delivered.
        message.lens_receipts = [
          { recordId: principle.recordId, revisionId: principle.revisionId, kind: "principle", title: principle.statement, delivery: "context" },
          { recordId: "preview-method", revisionId: "preview-method-rev", kind: "method", title: "Separate durable margins from temporary capacity costs", delivery: "exact_read" },
        ];
        const lens = (recordId: string, title: string, operation: "create") => ({ kind: "Lens" as const, recordId, operation, title, lesson: "Compare utilization with inventory days before reading margin changes.", basis: "Q3 operating review", futureUse: "Next capacity ramp", knowledge: { kind: "lens" as const, recordId, revisionId: `${recordId}-rev`, objectId: recordId } });
        message.memory_update = { status: "applied", commitId: "preview-receipt", changes: [
          ...(message.memory_update?.status === "applied" ? message.memory_update.changes : []),
          lens("preview-method-saved", "Test whether capacity costs are temporary", "create"),
        ] };
        return workspace;
      };
      if (id === "chat-receipt" || id === "chat-learned") {
        const readMap = bridge.knowledgeMap.bind(bridge);
        bridge.knowledgeMap = async request => {
          const map = await readMap(request);
          map.nodes = ["Hanul Precision", "Morrow Components", "Capacity utilization", "Operating margin"].map((label, index) => ({ id: `receipt-node-${index}`, kind: "entity", label, entityKind: index < 2 ? "Organization" : "Metric", degree: index === 0 ? 3 : 1, watched: false, held: false }));
          map.edges = [1, 2, 3].map(index => ({ id: `receipt-edge-${index}`, kind: "claim", source: "receipt-node-0", target: `receipt-node-${index}`, relation: "related_to", status: "active" }));
          map.worldRevisionId = "chat-receipt";
          return map;
        };
        const growth = (index: number, kind: "entity_added" | "claim_added", subjectId: string) => ({ eventId: `receipt-growth-${index}`, loupistId: "loupist-quality", originChatId: "preview-chat", originMessageId: "preview-answer", atMs: Date.now() - 60_000, kind, subjectId, recordId: "preview-world", actor: "loupist" as const, summary: "", beforeSummary: null, afterSummary: null });
        bridge.knowledgeGrowthEvents = async () => ({ events: [growth(0, "entity_added", "receipt-node-1"), growth(1, "entity_added", "receipt-node-2"), growth(2, "entity_added", "receipt-node-3"), growth(3, "claim_added", "receipt-edge-1")], nextCursor: null });
      }
    },
  })),
  ...["map-overview", "map-1000", "map-empty", "map-selected", "map-filtered", "map-lens-overlay", "map-lens-on-world", "map-library-sources", "map-replay-paused", "map-export-menu"].map((id): PreviewScenario => ({
    ...base(id, "Research map", () => {
      if (id === "map-export-menu") return select(".map-export-menu, .map-more-menu");
      const map = select(".map-view");
      return map?.querySelector("canvas, .map-empty, .map-list") ?? (map?.clientWidth === 0 ? map : null);
    }, async (root) => {
      await navigate(root, "map");
      const map = await until(() => select(".map-view"), "Map did not load");
      if (id === "map-selected") {
        const input = await until(() => map.querySelector<HTMLInputElement>('[role="combobox"]'), "Map search did not load");
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "Apple");
        input.dispatchEvent(new Event("input", { bubbles: true }));
        await click('[role="option"]', map);
        await until(() => select(".map-inspector"), "Inspector did not open");
      }
      if (id === "map-replay-paused") {
        await clickMapAction("[data-map-replay]", map);
        const scrubber = await until(() => map.querySelector<HTMLInputElement>(".map-replay-scrubber"), "Replay did not open");
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(scrubber, String((Number(scrubber.min) + Number(scrubber.max)) / 2));
        scrubber.dispatchEvent(new Event("input", { bubbles: true }));
        scrubber.dispatchEvent(new Event("change", { bubbles: true }));
      }
      if (id === "map-library-sources") await clickMapAction("[data-map-library]", map);
      if (id === "map-lens-overlay") {
        await click("[data-map-filter]", map);
        await click(".map-lens-filter summary", document.body);
        await click('[data-lens-id="map-lens-0"]', document.body);
        await until(() => select(".map-lens-banner"), "Lens overlay did not open");
      }
      if (id === "map-lens-on-world") {
        await click("[data-map-filter]", map);
        await click(".map-lens-filter summary", document.body);
        await click('[data-lens-id="map-lens-1"]', document.body);
        await until(() => select('[data-lens-applied="map-lens-1"]'), "Selected Lens did not mark the World");
      }
      if (id === "map-filtered") {
        await click("[data-map-filter]", map);
        const product = await until(() => document.querySelector<HTMLInputElement>('[data-map-filter-kind="product"]'), "Product filter did not open");
        product.click();
        product.focus();
        product.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      }
      if (map.clientWidth > 0 && id !== "map-empty") {
        await until(() => map.querySelector("canvas"), "Map canvas did not mount");
        for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
        if (id !== "map-replay-paused" && id !== "map-selected") await clickMapAction("[data-map-fit]", map);
      }
      if (id === "map-export-menu") {
        const trigger = map.querySelector("[data-map-export]");
        if (visible(trigger)) await click("[data-map-export]", map);
        else pressMenu(await until(() => select("[data-map-more]"), "More actions did not load"));
        await until(() => select(".map-export-menu, .map-more-menu"), "Export choices did not open");
      }
    }),
    setup: async (bridge) => {
      await prepareFixture(bridge);
      // The draw budget is measured on all 1,000 names, not the local view a large Map opens on.
      if (id === "map-1000") rememberScope("loupist-quality", "all");
      if (id === "map-export-menu") bridge.exportSave = async request => {
        window.dispatchEvent(new CustomEvent("map-preview-export", { detail: request }));
        return `/preview/${request.suggestedName}`;
      };
      const original = bridge.knowledgeMap.bind(bridge);
      bridge.knowledgeMap = async (request) => {
        const map = await original(request);
        if (id === "map-replay-paused") {
          const start = Date.now() - 28 * 86_400_000;
          map.nodes.forEach((node, index) => { node.firstSeenAtMs = start + index / Math.max(1, map.nodes.length - 1) * 28 * 86_400_000; });
          for (const edge of map.edges) edge.firstSeenAtMs = Math.max(map.nodes.find(node => node.id === edge.source)?.firstSeenAtMs ?? start, map.nodes.find(node => node.id === edge.target)?.firstSeenAtMs ?? start);
        }
        if (id === "map-empty") {
          // A new Loupist already has its standing Lens, but no World names.
          map.nodes = map.nodes.filter((node) => node.id === "map-lens-0"); map.edges = [];
          for (const count of Object.values(map.counts)) { count.total = 0; count.shown = 0; count.truncated = false; }
          map.counts.lens_perspective.total = map.counts.lens_perspective.shown = map.nodes.length;
        }
        if (id === "map-1000") {
          map.nodes = Array.from({ length: 1000 }, (_, i) => ({ id: `synthetic-${i}`, kind: "entity" as const, label: `Research ${String(i + 1).padStart(4, "0")}`, entityKind: ["Organization", "Product", "Person", "Event", "Entity"][i % 5], degree: 0, watched: i < 3, held: i === 3, firstSeenAtMs: Date.UTC(2026, 9, 1 + i % 28) }));
          map.edges = Array.from({ length: 1500 }, (_, i) => ({ id: `synthetic-edge-${i}`, kind: "claim" as const, source: `synthetic-${i % 1000}`, target: `synthetic-${(i * 31 + 7) % 1000}`, relation: "related_to", status: i % 97 === 0 ? "disputed" as const : "active" as const }));
          for (const edge of map.edges) for (const nodeId of [edge.source, edge.target]) map.nodes[Number(nodeId.split("-")[1])].degree++;
          for (const [kind, count] of Object.entries(map.counts)) { count.total = count.shown = kind === "entity" ? 1000 : kind === "claim" ? 1500 : 0; count.truncated = false; }
          map.worldRevisionId = "synthetic-1000";
        }
        return map;
      };
    },
  })),
  { ...base("map-local-1hop", "A large Map opens around one company, legend open on a first visit (collapsed below 900 px)", () => select("[data-map-around]") && mapCanvas() && legendAtFirstVisit() ? select(".map-stats") : null, async root => { await enterMap(root); }),
    setup: async bridge => { await prepareFixture(bridge); localViewMap(bridge); rememberPreviewMap({ scope: "1" }); } },
  { ...base("map-local-2hops-bounded", "Two hops stop at 150 names around the company", () => select("[data-map-around]") && mapCanvas() ? select(".map-stats") : null, async root => { await enterMap(root); }),
    setup: async bridge => { await prepareFixture(bridge); localViewMap(bridge); rememberPreviewMap({ scope: "2" }); } },
  { ...base("map-local-no-neighbors", "A company with no relationships yet", () => mapCanvas() ? select(".map-no-neighbors") : null, async root => { await enterMap(root); }),
    setup: async bridge => { await prepareFixture(bridge); localViewMap(bridge); rememberPreviewMap({ focalId: "local-kioxia", scope: "1" }); } },
  ...(["map-whats-new", "map-whats-new-empty"] as const).map((id): PreviewScenario => ({
    ...base(id, id === "map-whats-new" ? "What changed since the last visit" : "Nothing new since the last visit", () => mapCanvas() ? select(id === "map-whats-new" ? ".map-whats-new .map-whats-new-row" : ".map-whats-new .map-whats-new-state") : null, async root => {
      const map = await enterMap(root);
      await click("[data-map-whats-new]", map);
    }),
    setup: async bridge => { await prepareFixture(bridge); whatsNewFixture(bridge, id === "map-whats-new"); },
  })),
  { ...base("map-legend-compact", "Legend with at most six rows", () => mapCanvas() ? select("details.map-legend[open] .map-legend-more") : null, async root => { await enterMap(root); }),
    setup: async bridge => {
      await prepareFixture(bridge);
      rememberPreviewMap({ legendOpen: true });
      // The World canvas draws no Lens nodes, so overflow needs the World categories the seed omits.
      const read = bridge.knowledgeMap.bind(bridge);
      bridge.knowledgeMap = async request => {
        const map = await read(request);
        const company = map.nodes.find(node => node.kind === "entity" && node.entityKind === "Organization");
        const person: KnowledgeMapNode = { id: "legend-mina-cho", kind: "entity", label: "Mina Cho", entityKind: "Person", degree: 1, watched: false, held: false };
        const journal: KnowledgeMapNode = { id: "legend-journal", kind: "journal", label: "Inventory days below 50", degree: 0, watched: false, held: false };
        map.nodes = [...map.nodes, person, journal];
        if (company) map.edges = [...map.edges, { id: "legend-person-claim", kind: "claim", source: company.id, target: person.id, relation: "leads", status: "active" }];
        return map;
      };
    } },
  base("world-sources", "Retained sources and processing", () => {
    const list = select(".source-library > ul");
    return list && list.children.length >= 6 && mapCanvas() ? list : null;
  }, async (root) => { await navigate(root, "map"); await clickMapAction("[data-map-library]", root); }),
  { ...base("lens-reader", "A Lens page read as sections", () => select('.workspace-lens-content .page-reader .page-section'), async root => {
    await navigate(root, "lens");
    const method = await until(() => [...document.querySelectorAll<HTMLButtonElement>(".memory-row")].find(row => row.querySelector(".memory-marker.is-method")), "Method did not list");
    method.click();
    await until(() => select(".workspace-lens-content .page-reader .page-section"), "Method did not open");
  }), setup: prepareFixture },
  { ...base("lens-reader-many", "A long page list keeps its page readable when the Lens is expanded", () => select('.workspace-lens-content .page-reader'), async root => {
    await navigate(root, "lens");
    const method = await until(() => [...document.querySelectorAll<HTMLButtonElement>(".memory-row")].find(row => row.querySelector(".memory-marker.is-method")), "Method did not list");
    method.click();
    await until(() => select('.workspace-lens-content .page-reader'), "Method did not open");
    if (!window.matchMedia(workspaceOverlayMedia).matches) await click('.workspace-kind-heading button:first-child');
  }), setup: async bridge => {
    await prepareFixture(bridge);
    const read = bridge.knowledgeCatalog.bind(bridge);
    bridge.knowledgeCatalog = async request => {
      const catalog = await read(request);
      const method = catalog.lenses.find(item => item.lensKind === "method")!;
      return { ...catalog, lenses: [...catalog.lenses, ...Array.from({ length: 20 }, (_, index) => ({ ...method, recordId: `many-method-${index}`, title: `${method.title} · ${index + 1}` }))] };
    };
  } },
  // W15.23 and W15.6c: the Schedule tab manages scheduled work; results arrive in the home Thread.
  ...([
    ["schedule-list", "Scheduled tasks with saved names, cadence and a task menu", ".schedule-list"],
    ["schedule-watching-setup", "Asset watch history keeps the saved choices read-only", ".watching-reader .watching-status"],
    ["schedule-empty", "Nothing scheduled yet", ".schedule-list .briefings-empty"],
    ["schedule-create", "Request scheduled work in Bot", ".composer-shell textarea"],
    ["schedule-active-filter", "Active filter hides a paused task", "[data-schedule-filter='active']"],
    ["schedule-dialog", "Saved briefing cadence and run history", ".schedule-task-detail"],
    ["briefing-structured", "A briefing's sections above its answer", ".briefing-sections"],
    ["briefing-failed", "A briefing that could not finish", ".briefings-failure"],
    ["briefing-suggested", "A briefing keeps the changes that prompted it", ".briefing-sections"],
  ] as const).map(([id, title, landmark]): PreviewScenario => ({
    ...base(id, title, () => {
      if (id === "schedule-list") {
        const list = select(".schedule-list");
        return list && !list.closest(".briefings-layout")?.hasAttribute("data-reader-open") && list.querySelector('[data-schedule-task="watching"][data-status="setup"]') ? list : null;
      }
      if (id === "schedule-active-filter") {
        const rows = [...document.querySelectorAll(".schedule-task:not(.watching-item)")];
        const paused = previewKo() ? "주간 공급 능력 비교" : "Weekly supplier capacity comparison";
        return rows.length > 0 && rows.every(row => !row.textContent?.includes(paused)) ? select(landmark) : null;
      }
      if (id === "schedule-create") return (select(landmark) as HTMLTextAreaElement | null)?.value ? select(landmark) : null;
      return select(landmark);
    }, async root => {
      if (id === "briefing-structured" || id === "briefing-failed" || id === "briefing-suggested") { await openScheduleRun(root, id === "briefing-failed" ? "preview-briefing-2" : "preview-briefing-1"); return; }
      if (id === "schedule-create") {
        await openNavigation(".loupist-chat-row[data-home]");
        await click(".loupist-chat-row[data-home] .loupist-chat-button");
        const composer = await until(() => select("textarea[data-chat-composer]") as HTMLTextAreaElement | null, "Bot composer did not initialize");
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, previewKo() ? "매주 월요일 아침 8시에 재고일수를 비교해 줘." : "Every Monday at 8, compare inventory days for what I follow.");
        composer.dispatchEvent(new Event("input", { bubbles: true }));
        return;
      }
      await navigate(root, "schedule");
      if (id === "schedule-watching-setup") await click('[data-schedule-task="watching"] .schedule-task-summary');
      if (id === "schedule-dialog") await click('[data-schedule-task="preview-briefing"] .schedule-task-summary');
    }),
    setup: async bridge => {
      await prepareFixture(bridge, "answer");
      bridge.includeBriefingBody = true;
      if (id === "schedule-empty") { bridge.scheduleTasks = "none"; bridge.inboxList = async () => ({ items: [], nextCursor: null }); bridge.inboxUnreadCounts = async () => []; return; }
      await bridge.briefingSettingsSet({ loupistId: "loupist-quality", enabled: true });
      // The briefing follows the same assets the user follows, named in the cell's language.
      const ko = previewKo();
      const followed = [["005930", "KRX", ko ? "삼성전자" : "Samsung Electronics"], ["000660", "KRX", ko ? "SK하이닉스" : "SK hynix"], ["AAPL", "NASDAQ", "Apple"]] as const;
      bridge.watchlistList = async () => followed.map(([symbol, market, displayName]) => ({ symbol, market, displayName, addedAtMs: Date.now() - 30 * 86_400_000 }));
      const map = await bridge.knowledgeMap({ loupistId: "loupist-quality", maxNodes: 1000 });
      const entity = (node: { id: string }, index: number) => ({ eventId: `briefing-growth-${index}`, loupistId: "loupist-quality", originChatId: "preview-chat", originMessageId: "preview-answer", atMs: Date.now(), kind: "entity_added" as const, subjectId: node.id, recordId: "preview-world", actor: "loupist" as const, summary: "", beforeSummary: null, afterSummary: null });
      const disputed = id === "briefing-suggested" ? [{ ...entity({ id: "map-claim-28" }, 9), kind: "claim_disputed" as const, subjectId: "map-claim-28", atMs: Date.now() + 3_600_000 }] : [];
      bridge.knowledgeGrowthEvents = async () => ({ events: [...map.nodes.filter(node => node.kind === "entity").slice(0, 3).map(entity), ...disputed], nextCursor: null });
    },
  })),
  { ...base("thread-home", "The home Thread: a delivered result and the morning news", () => select(".thread-delivery") && select('.morning-message[aria-busy="false"]'), async () => {
    const news = await until(() => select('.morning-message[aria-busy="false"]'), "Morning news did not appear");
    if (hasLayout()) news.scrollIntoView({ block: "end" });
  }), setup: async bridge => {
    await prepareMorningFixture(bridge, "morning");
    bridge.conversation = "answer";
    const select = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await select(loupistId);
      workspace.threads.find(thread => thread.home)?.messages.push(previewDelivery());
      return workspace;
    };
  } },
  { ...base("chat-topic-empty", "A new topic Chat offers only questions", () => select(".morning-ask-only .morning-question"), async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    // ⌘N starts a topic Chat at every width; the sidebar's row control is hover-only and hidden when narrow.
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "n", metaKey: true, ctrlKey: !navigator.userAgent.includes("Mac"), bubbles: true }));
  }) },
  base("loupists", "Loupists in Settings", () => select(".settings-loupists"), (root) => settings(root, "loupists")),
  base("loupist-settings", "Editable Loupist profile", () => select(".loupist-settings-page #loupist-profile-focus"), async () => { await openLoupistSettings(); await until(() => select("#loupist-profile-name"), "Profile editor did not open"); }),
  { ...base("loupist-settings-tracking", "What the map tracks", () => select(".loupist-tracking-changes"), async () => {
    await openLoupistSettings();
    await click('[data-loupist-section="world"] > summary');
    await until(() => select(".loupist-tracking-list"), "Tracking list did not load");
    select(".loupist-tracking")?.scrollIntoView({ block: "start" });
  }), setup: async bridge => { await prepareFixture(bridge); bridge.installWorldSchema("loupist-quality", semiconductorTrackingView()); } },
  { ...base("chat-all-chats", "Five recent sessions stay alongside the current chat", () => {
    const sessions = select(".loupist-navigation");
    return sessions && sessions.querySelectorAll('.loupist-chat-row:not([data-home])').length === 5 ? sessions : null;
  }, async () => {
    await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
    await openNavigation('[data-thread-id="preview-chat"]');
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true }));
    const search = await until(() => select(".command-palette-input") as HTMLInputElement | null, "Search did not open");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(search, "Capital expenditure plans");
    search.dispatchEvent(new Event("input", { bubbles: true }));
    await until(() => select('[data-palette-row="chat:loupist-quality:preview-chat-6"]'), "An older session did not appear in search");
    search.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await until(() => !select(".command-palette"), "Search did not close");
  }), setup: async bridge => { await prepareFixture(bridge); bridge.manyChats = true; } },
  base("shell-titlebar-grown", "A taller titlebar keeps the sidebar and composer inside the remaining body", () => select(".app-titlebar"), async () => {
    await until(() => select('textarea[data-chat-composer]'), "Chat did not initialize");
    // Exercise the height contract independently of the current platform's titlebar size.
    (select(".app-titlebar") as HTMLElement).style.minHeight = "72px";
  }),
  base("shell-bot-rail", "Collapsed navigation with centered icons, or the navigation sheet in a narrow window", () => select(hasLayout() && window.matchMedia("(max-width: 899px)").matches ? '[data-mobile="true"] .navigation-search' : '[data-collapsible="icon"] .navigation-search'), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Bot did not initialize");
    if (!select('[data-collapsible="icon"] .navigation-search')) await click('[data-slot="sidebar-trigger"]');
  }),
  base("shell-sidebar-home", "Selected Loupist and Bot", () => select(".loupist-header-identity"), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Bot did not initialize");
  }),
  { ...base("shell-sidebar-many", "Six Loupists with activity and unread briefings", () => select(".loupist-header-profiles"), async () => {
    await until(() => select('textarea[data-chat-composer]'), "Chat did not initialize");
    await until(() => document.querySelectorAll(".loupist-header-profiles .loupist-header-identity").length === 6, "Loupist list did not load");
    // The visual dot is presentational; its live status is a separate sibling.
    await until(() => { const dot = document.querySelector('.loupist-header-identity:nth-of-type(2) .loupist-run-indicator'); return dot && (!hasLayout() || dot.getBoundingClientRect().width > 0); }, "Background activity did not reach the header");
  }), setup: async bridge => {
    await prepareFixture(bridge);
    const names = ["Semiconductors — memory chips, foundries, advanced packaging", "US Big Tech", "Dividend Loupist", "Energy transition and industrial infrastructure", previewKo() ? "한국 소비재와 유통" : "Korean consumer goods and retail", "Global supply chains"];
    const [first] = await bridge.loupistList();
    await bridge.loupistProfileUpdate({ loupistId: first!.id, name: names[0], focus: "Memory chips & foundries" });
    for (const name of names.slice(1)) await bridge.loupistCreate({ name, focus: "Long-term research and primary evidence" });
    bridge.manyLoupists = true;
    const profiles = await bridge.loupistList();
    bridge.runActivityList = async () => [{ run_id: "preview-background-run", loupist_id: profiles[1]!.id, kind: "chat", target: "preview-chat", started_at_ms: Date.now() - 30_000 }];
  } },
  base("connections", "Connections", () => select("#marketplace-title"), (root) => settings(root, "connections")),
  ...settingsScenarios({ base, select, settings, until }),
  ...readerScenarios({ base, select, navigate, selectPanel, click, clickMapAction, until }),
  ...dataScenarios({ base, select, click, settings }),
  // W17.3 (providers §2.3): one card per account; ChatGPT plan line, key accounts.
  ...([
    ["settings-models", "Models & providers", '[data-provider-card="anthropic"]'],
    ["settings-models-plan-limit", "A ChatGPT plan at its limit", '[data-provider-card="openai"] [data-notice="plan_limit"]'],
    ["settings-models-needs-reconnect", "A sign-in that needs reconnecting", '[data-provider-card="openai"][data-state="needs_reconnect"]'],
    ["settings-models-disconnected", "A disconnected account schedules still use", '[data-provider-card="xai"][data-state="disconnected"]'],
    ["settings-models-expanded", "A card's models with In menu switches", '[data-provider-card="openai"] .provider-card-models .provider-card-switch'],
  ] as const).map(([id, title, landmark]): PreviewScenario => ({
    ...base(id, title, () => select(landmark), async (root) => {
      await settings(root, "model");
      if (id === "settings-models-expanded") await click('[data-provider-card="openai"] [aria-controls="provider-models-openai"]');
      if (id === "settings-models-disconnected") await centerOn(landmark)();
    }),
    setup: async (bridge) => {
      await prepareFixture(bridge);
      await bridge.providerConfigure({ provider: "anthropic", api_key: "preview-key" });
      await bridge.providerConfigure({ provider: "google", api_key: "preview-key" });
      const hour = 60 * 60 * 1000;
      if (id === "settings-models-plan-limit") bridge.setProviderStatus("openai", { notice: { kind: "plan_limit", at_ms: Date.now() - hour }, last_limit_at_ms: Date.now() - hour });
      if (id === "settings-models-needs-reconnect") bridge.setProviderStatus("openai", { state: "needs_reconnect" });
      if (id === "settings-models-disconnected") bridge.setProviderStatus("xai", { state: "disconnected", credential_kind: "none", connected_at_ms: null, used_by: { schedules: 2, chat_default: false, world_models: 0 } });
    },
  })),
  { ...base("settings-models-custom", "Custom model provider", () => select('[data-provider-card^="custom-"]'), async (root) => { await settings(root, "model"); }), setup: async bridge => { await prepareFixture(bridge); await bridge.providerCustomSave({ name: "Office Ollama", base_url: "http://localhost:11434/v1", api: "openai-completions", model_ids: ["qwen2.5:7b", "llama3.1:8b"] }); } },
  base("custom-provider-form", "Add a custom provider", () => select("[data-custom-sheet]"), async (root) => {
    await settings(root, "model");
    await click("[data-provider-connect]");
    await click('[data-connect-row="custom"]');
    const form = await until(() => select("[data-custom-sheet]"), "Custom provider sheet did not open");
    const fields = [...form.querySelectorAll<HTMLInputElement>("input:not([type=password]):not([type=radio])")];
    for (const [field, value] of fields.map((field, index) => [field, ["Office Ollama", "http://localhost:11434/v1"][index]] as const)) {
      if (value === undefined) continue;
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(field, value);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    }
  }),
  { ...base("existing-user-notice", "Existing user notice", () => select("[data-notice-dialog]")), setup: async bridge => { await prepareFixture(bridge); bridge.noticeSettingsGet = async () => ({ notice_acknowledged_version: 0 }); } },
  base("settings-application", "Application notice", () => select("[data-notice-settings]"), root => settings(root, "agent")),
  // W15.37: the ⌘K command palette and Settings › Keyboard.
  { ...base("palette-open", "The command palette searches what needs attention, chats, names and actions", () => select('.command-palette-list [data-palette-row^="attention:"]') && select(".command-palette-list"), async (root) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, ctrlKey: !navigator.userAgent.includes("Mac"), bubbles: true }));
    const input = await until(() => select(".command-palette-input") as HTMLInputElement | null, "Palette did not open");
    // One letter reaches every group in the fixture: needs attention, chats, Map names, principles and actions.
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "a");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }), setup: async (bridge) => { await prepareFixture(bridge, "answer"); prepareAttention(bridge); } },
  // B82: the palette searches every Loupist; each group lists the active Loupist's rows first, each captioned.
  { ...base("palette-loupists", "The command palette finds names and principles in every Loupist", () => select('.command-palette-list [data-palette-row^="principle:"]') && select('.command-palette-list [role="group"] [data-palette-row^="map:"]') && select(".command-palette-list"), async (root) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, ctrlKey: !navigator.userAgent.includes("Mac"), bubbles: true }));
    const input = await until(() => select(".command-palette-input") as HTMLInputElement | null, "Palette did not open");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, previewKo() ? "삼성" : "samsung");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    // Both Loupists' Maps answer before the capture.
    await until(() => document.querySelectorAll('.command-palette-list [data-palette-row^="map:"]').length >= 2, "The other Loupist's names did not arrive");
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const ko = previewKo();
    const cycle = await bridge.loupistCreate({ name: ko ? "사이클 리더" : "Cycle Reader", template: "semiconductors" });
    await bridge.principleCreate({ loupistId: cycle.id, statement: ko ? "삼성전자의 메모리 사이클은 마진보다 먼저 돈다" : "Samsung's memory cycle turns before its margins do", origin: "settings" });
    // A new Loupist's mock Map is empty; it shares the seeded names so both Maps answer.
    const read = bridge.knowledgeMap.bind(bridge);
    bridge.knowledgeMap = async (request) => read(request.loupistId === cycle.id ? { ...request, loupistId: "loupist-quality" } : request);
  } },
  base("settings-keyboard", "Keyboard shortcuts", () => select(".keyboard-table"), (root) => settings(root, "keyboard")),
  base("settings-world", "World processing", () => select(".world-settings-card"), async () => { await openLoupistSettings(); await click('[data-loupist-section="world"] > summary'); select(".world-settings-card")?.scrollIntoView({ block: "start" }); }),
  // W16.1–W16.2: asset pages and "What changed" (finance-asset-page.md §8).
  ...assetScenarios({ base, select, until, click, enterMap, hasLayout, centerOn }),
  ...answerCheckScenarios({ base, select, until, click, clickMapAction, hasLayout, submitPrompt, navigate }),
  ...holdingsScenarios({ base, select, until, click }),
  ...sharingScenarios(),
  ...gridScenarios({ base, select, until, click, selectPanel, hasLayout, submitPrompt, navigate }),
  // W16.6–W16.7: results before and after, local watchers (finance-earnings-snapshot.md §8, finance-watchers.md §8).
  ...financeScenarios({ base, select, until, click, hasLayout, navigate, centerOn }),
  // W16.4, W16.8: Korean asset-page depth and the sensitivity card (finance-korea-retail-disagreement.md §8).
  ...koreaScenarios({ base, select, until, click, hasLayout }),
  ...browserScenarios({ base, select, until, click }),
  ...quoteEvidenceScenarios({ base, select, until, click }),
];

/** W19 sharing: Share sheet, link or file, review before adding, suggestions, origin and deep links. */
function sharingScenarios(): PreviewScenario[] {
  const openMemoryPage = async (root: HTMLElement) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    await selectPanel("lens");
    await until(() => select(".memory-page-tabs"), "What the Loupist knows did not load");
  };
  const openShare = async (root: HTMLElement) => { await openMemoryPage(root); await click("[data-share-loupist]"); };
  const publishFromSheet = async () => {
    await click('[data-share-sheet="loupist"] [data-share-publish]');
    await click('.share-sheet[data-step="repo"] .share-primary');
  };
  const openLinkDialog = async () => {
    await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
    await openNavigation(".loupist-header-new");
    await click(".loupist-header-new");
    await click("[data-loupist-shared-entry] [data-use-link-or-file]");
  };
  /** W19.6: "+ New Loupist" › Browse the Gallery, in a build with a community index. */
  const withIndex = (bridge: PreviewBridge) => { bridge.sharing.account = { ...bridge.sharing.account, indexConfigured: true }; };
  const openGallery = async () => {
    await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
    await openNavigation(".loupist-header-new");
    await click(".loupist-header-new");
    await click("[data-loupist-shared-entry] [data-browse-gallery]");
    await until(() => select("[data-gallery]"), "The Gallery did not open");
  };
  /** W19.5: the shared Loupists section on "What {Loupist} knows". */
  const openSharedSection = async (root: HTMLElement) => {
    await openMemoryPage(root);
    await click('.memory-page-tabs [role="tab"]:nth-child(1)');
    const section = await until(() => select("[data-shared-packs]"), "Shared Loupists you added did not show");
    // The update check runs as the section opens; scroll once its result is in place.
    await until(() => select("[data-pack-update], [data-pack-removed-banner]"), "No update or banner showed");
    section.scrollIntoView?.({ block: "start" });
  };
  const openReview = async () => {
    await openLinkDialog();
    await click("[data-pack-link-dialog] .share-outputs button");
    await until(() => select("[data-pack-review]"), "Review before adding did not open");
  };
  const sharing = (title: string, id: string, landmark: () => Element | null, enter: (root: HTMLElement) => Promise<void>, configure: (bridge: PreviewBridge) => void | Promise<void> = () => {}): PreviewScenario => ({
    ...base(id, title, landmark, enter),
    setup: async bridge => { await prepareFixture(bridge); await configure(bridge); },
  });
  const signedIn = (bridge: PreviewBridge) => { bridge.sharing.account = { ...bridge.sharing.account, login: "kimj" }; };
  /** The License field sits under the sections; bring it into the sheet's view. */
  const openShareAt = (target: string) => async (root: HTMLElement) => {
    await openShare(root);
    (await until(() => select(target), `${target} did not show`)).scrollIntoView?.({ block: "start" });
  };
  return [
    sharing("Share sheet", "share-sheet", () => select('[data-share-sheet="loupist"] .share-count'), openShare),
    sharing("Share sheet with private details to check", "share-sheet-findings", () => select("[data-share-findings]"), openShare, bridge => bridge.sharing.setFindings()),
    sharing("Share sheet with nothing to share", "share-sheet-nothing", () => select("[data-share-nothing]"), openShare,
      bridge => { bridge.sharing.summaryOverrides = { counts: { perspective: 0, methods: 0, principles: 0, names: 0, links: 0, sources: 0, leftOut: 0, leftOutLicense: 0 } }; }),
    sharing("Sign in to GitHub with a device code", "share-sheet-github-code", () => select("[data-device-code]"),
      async root => { await openShare(root); await click('[data-share-sheet="loupist"] [data-share-publish]'); }, bridge => { bridge.sharing.signInMode = "wait"; }),
    sharing("Publishing to GitHub", "share-sheet-publishing", () => select("[data-share-progress]")?.textContent?.match(/\d/) ? select("[data-share-progress]") : null,
      async root => { await openShare(root); await publishFromSheet(); }, bridge => { signedIn(bridge); bridge.sharing.holdPublish = true; }),
    sharing("Published to GitHub", "share-sheet-published", () => select("[data-share-published]"), async root => { await openShare(root); await publishFromSheet(); }, signedIn),
    sharing("Update the recorded repository", "share-sheet-update-repo", () => select('.share-sheet[data-step="repo"] .share-primary'),
      async root => { await openShare(root); await click('[data-share-sheet="loupist"] [data-share-publish]'); },
      bridge => { signedIn(bridge); bridge.sharing.summaryOverrides = { repo: { fullName: "kimj/loupist-quality-compounder", url: "https://github.com/kimj/loupist-quality-compounder", version: 4 }, nextVersion: 5 }; }),
    // Licensing: Rust's NC-SA default with items whose license keeps them out, and two shared items whose licenses clash.
    sharing("Share sheet license with the NC-SA default", "share-sheet-license", () => select("[data-share-license-summary]"), openShareAt("[data-share-left-out-license]"),
      bridge => { bridge.sharing.summaryOverrides = { counts: { perspective: 1, methods: 14, principles: 6, names: 312, links: 1204, sources: 88, leftOut: 41, leftOutLicense: 2 } }; }),
    sharing("Share sheet with licenses that can't be shared together", "share-sheet-license-conflict", () => select("[data-share-license-conflict]"), openShareAt("[data-share-license-conflict]"),
      bridge => {
        bridge.sharing.licensedItems = [
          { key: "m-5a1c9e2b7d3f4a60", name: "Dividend growth check", license: "CC-BY-SA-4.0" },
          { key: "p-6b2d0f3c8e4a5b71", name: "Never size a position on one quarter.", license: "CC-BY-NC-4.0" },
        ];
      }),
    sharing("Agree to the Gallery listing terms", "share-sheet-listing-terms", () => select("[data-gallery-terms]"), async root => { await openShare(root); await publishFromSheet(); },
      bridge => { signedIn(bridge); bridge.sharing.account = { ...bridge.sharing.account, indexConfigured: true, galleryTermsUrl: "https://example.org/loupist/gallery-terms" }; }),
    sharing("No GitHub client in this build", "share-sheet-no-github", () => select("[data-github-missing]"), openShare,
      bridge => { bridge.sharing.account = { clientConfigured: false, indexConfigured: false, galleryTermsUrl: null, login: null }; }),
    sharing("Share this method", "share-method", () => select('[data-share-sheet="item"] .share-item-row .share-section-note'), async root => {
      await openMemoryPage(root);
      const row = await until(() => [...document.querySelectorAll<HTMLButtonElement>(".memory-page-list .memory-row")].find(button => button.querySelector(".memory-marker.is-method")), "No method row");
      row.click();
      await click(".memory-reader [data-share-item]");
    }),
    sharing("Start from a shared Loupist", "new-loupist-shared-entry", () => select("[data-loupist-shared-entry]"), async () => {
      await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
      await openNavigation(".loupist-header-new");
      await click(".loupist-header-new");
    }),
    sharing("Open a shared Loupist from a link or file", "pack-link-dialog", () => select("[data-pack-link-dialog] input"), openLinkDialog),
    sharing("Review before adding", "pack-review", () => select("[data-pack-review] .pack-review-sections"), openReview),
    sharing("Review a method with a warning", "pack-review-warnings", () => select(".pack-review-item-body [data-flagged]"), async () => {
      await openReview();
      await click('.pack-review-items li[data-flagged] .pack-review-item');
    }),
    sharing("Add to an existing Loupist", "pack-review-merge", () => select('.pack-review-section[data-disabled]'), async () => {
      await openReview();
      await click('.pack-review-merge [role="radio"]');
    }),
    sharing("A pack that can't be added", "pack-review-invalid", () => select("[data-pack-rejected]"), openReview, bridge => bridge.sharing.rejectWith("unsafe")),
    sharing("Review a Personal-licensed pack", "pack-review-personal", () => select("[data-pack-not-redistributable]"), openReview, bridge => {
      const manifest = { ...MOCK_PACK_PREVIEW.manifest, license: "LicenseRef-Loupist-Personal" as const, licenseUrl: "https://example.org/loupist/personal-terms", redistributable: false };
      bridge.sharing.openResult = { kind: "preview", preview: { ...MOCK_PACK_PREVIEW, manifest } };
    }),
    sharing("A pack removed from the Gallery", "pack-review-blocked", () => select("[data-pack-blocked]"), openReview,
      bridge => { bridge.sharing.openResult = { kind: "preview", preview: { ...MOCK_PACK_PREVIEW, blocked: "reported" } }; }),
    sharing("Suggestions from a shared Loupist", "pack-suggestions", () => select("[data-pack-suggestions] .pack-suggestion"), openMemoryPage,
      bridge => { bridge.sharing.seedImport("loupist-quality"); }),
    sharing("A shared name in the Map inspector", "map-inspector-shared", () => select(".map-inspector [data-shared-origin]"), async root => {
      await navigate(root, "map");
      const map = await until(() => select(".map-view"), "Map did not load");
      const input = await until(() => map.querySelector<HTMLInputElement>('[role="combobox"]'), "Map search did not load");
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "Apple");
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await click('[role="option"]', map);
      await until(() => select(".map-inspector"), "Inspector did not open");
    }, bridge => { bridge.packOrigin = async () => ({ importId: "imp-1", packName: "Semiconductor supply chain", author: "Kim J." }); }),
    // A link held from a cold start: Rust asks once onboarding is done, and the dialog opens over Chat.
    sharing("Open a shared Loupist from a link", "deep-link-confirm", () => select("[data-deep-link-dialog]"), idle, bridge => { bridge.sharing.receiveDeepLink({ status: "ok", source: "github.com/kimj/loupist-semiconductor-supply-chain" }, true); }),
    // W19.6: the community Gallery under "+ New Loupist".
    sharing("Gallery of shared Loupists", "gallery", () => select("[data-gallery-card] .gallery-card-image img"), openGallery, withIndex),
    sharing("Gallery while offline", "gallery-offline", () => select("[data-gallery-offline]"), openGallery, bridge => {
      withIndex(bridge);
      bridge.sharing.gallery = { ...bridge.sharing.gallery, status: "offline", fetchedAtMs: Date.parse("2026-10-26T18:00:00Z") };
    }),
    sharing("Gallery with no match", "gallery-no-match", () => select("[data-gallery-empty]"), async () => {
      await openGallery();
      const input = await until(() => select("[data-gallery] input[type=search]") as HTMLInputElement | null, "Gallery search did not load");
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "tanker freight");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    }, withIndex),
    // W19.5: a newer version of a shared Loupist, checked when the section opens.
    sharing("Update from a shared Loupist", "pack-update-diff", () => select("[data-pack-update]"), openSharedSection,
      bridge => { bridge.sharing.seedImport("loupist-quality", { version: 3 }); }),
    sharing("A shared Loupist removed from the Gallery", "pack-removed-banner", () => select("[data-pack-removed-banner]"), openSharedSection,
      bridge => { withIndex(bridge); bridge.sharing.updateCheck = { kind: "upToDate" }; bridge.sharing.seedImport("loupist-quality", { blockedReason: "reported" }); }),
  ];
}

/**
 * Scenarios on a real clock, outside the capture matrix: their frames depend on
 * timing, so they are driven by `scripts/probe-streaming.mjs`, not screenshotted.
 */
export const liveScenarios: readonly PreviewScenario[] = [
  {
    id: "chat-live-research",
    title: "A research reply streams on a real clock",
    setup: async (bridge) => { await prepareFixture(bridge, "answer"); longThread(bridge); liveResearchRun(bridge); },
    enter: async (root) => { await until(messagesRendered, "Message Markdown did not finish rendering"); await submitPrompt(root, "Why did Hanul Precision’s margin fall while revenue grew?"); },
    landmark: () => select(".message.user:last-of-type") ?? select(".message.user"),
  },
];

