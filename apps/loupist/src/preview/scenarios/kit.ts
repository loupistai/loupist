/** Shared preview scenario helpers: landmarks, waits, navigation and the `base` scenario. */
import { prepareFixture, PreviewBridge, previewKo } from "../fixtures";
import type { ChatApproval, ChatProgressItem } from "../../contracts/chat";
import type { GrowthEvent, KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
import type { MapScope } from "../../components/map/localView";
import { rememberFocal, rememberLegendOpen, rememberScope } from "../../components/map/mapVisit";

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

export const select = (selector: string, root: ParentNode = document) => [...root.querySelectorAll(selector)].find(visible) ?? null;

// jsdom has no layout: the Map canvas never mounts and every box measures zero.
export const hasLayout = () => document.documentElement.clientWidth > 0;

// A mounted message can still be the lazy Markdown fallback. Gate interactions
// and landmarks so scrolling and measurements use the final rendered answer.
export const messagesRendered = () => !hasLayout() || (!document.querySelector(".message-response-fallback") && [...document.querySelectorAll(".message .message-content")].every(body => !body.textContent?.includes("**") && !body.textContent?.includes("](#evidence-")));

export const mapCanvas = () => !hasLayout() || select(".map-canvas canvas");

export async function until<T>(read: () => T | null | false | undefined, reason: string, timeout = 7500): Promise<T> {
  const deadline = performance.now() + timeout;
  while (performance.now() < deadline) {
    const result = read();
    if (result) return result;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(reason);
}

export async function click(selector: string, root: ParentNode = document) {
  await until(messagesRendered, "Message Markdown did not finish rendering");
  const target = await until(() => {
    const node = select(selector, root);
    return node && !node.hasAttribute("disabled") ? node : null;
  }, `Control not visible: ${selector}`);
  target.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
  (target as HTMLElement).click();
}

/** Opens a Radix menu: it opens on pointer down, as a mouse press does (jsdom has no PointerEvent). */
export function pressMenu(trigger: Element | null | undefined) {
  const init = { bubbles: true, button: 0 };
  trigger?.dispatchEvent(typeof PointerEvent === "function" ? new PointerEvent("pointerdown", { ...init, pointerType: "mouse" }) : new MouseEvent("pointerdown", init));
}

/** Clicks a Map toolbar action, through "More" when a narrow toolbar has folded it there. */
export async function clickMapAction(selector: string, root: ParentNode = document) {
  await until(() => root.querySelector(selector), `Map action did not initialize: ${selector}`);
  if ([...root.querySelectorAll(selector)].some(visible)) return click(selector, root);
  const more = await until(() => select("[data-map-more]", root), "More actions not visible");
  // The menu opens on pointer down, as a mouse press does.
  pressMenu(more);
  await click(`[role="menu"] ${selector}`);
}

export async function openNavigation(selector: string) {
  if (!select(selector)) await click('[data-slot="sidebar-trigger"]');
}

export async function openLoupistSettings() {
  await until(() => select(".loupist-header-identity"), "Loupist did not initialize");
  await click("[data-header-loupist-settings]");
}

export async function selectPanel(kind: "lens" | "world" | "materials") {
  pressMenu(await until(() => select(".header-panel-toggle"), "Panel control did not initialize"));
  await click(`[data-workspace-kind="${kind}"]`);
}

export async function navigate(root: HTMLElement, tab: "map" | "lens" | "schedule" | "settings") {
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
export async function openScheduleRun(root: HTMLElement, runId: string) {
  await navigate(root, "schedule");
  await click('[data-schedule-task="preview-briefing"] .schedule-task-summary', root);
  await click(`[data-schedule-run="${runId}"]`, root);
}

export async function settings(root: HTMLElement, section: string) {
  await navigate(root, "settings");
  await openNavigation(`#settings-section-${section}`);
  await click(`#settings-section-${section}`);
}

export const idle = async () => {};

/** The steps of a reply that read saved notes and then asked to use a connection. */
export function approvalSteps(at: number, status: "running" | "succeeded"): ChatProgressItem[] {
  const finished = { finishedAtMs: status === "succeeded" ? at - 4_000 : null };
  return [
    { id: "preview-read", kind: "tool", toolName: "world_read", category: "memory", operation: "read", action: "Read saved notes", target: "Hanul Precision operating report", status: "succeeded", startedAtMs: at - 28_000, finishedAtMs: at - 26_000, href: null },
    { id: "preview-filings", kind: "tool", toolName: "mcp__custom-morrow__read_company_filings", category: "finance", operation: "read", action: "Read company filings", running: "Reading company filings", source: { kind: "connector", name: "Morrow research tools" }, target: "Hanul Precision · 2026 Q3", status, startedAtMs: at - 12_000, ...finished, href: null },
  ];
}

export const approvalFor = (state: ChatApproval["state"], at: number): ChatApproval =>
  ({ requestId: "req-1", itemId: "preview-filings", connectionId: "custom-morrow", connectionName: "Morrow research tools", toolName: "read_company_filings", target: "Hanul Precision · 2026 Q3", state, requestedAtMs: at - 12_000 });

/** A reply paused on a read-only connection until the user decides. */
export function approvalRun(bridge: PreviewBridge) {
  bridge.chatSend = async (request, observer) => {
    const run_id = request.run_id;
    const at = Date.now();
    observer({ type: "started", run_id });
    observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: at - 30_000, items: approvalSteps(at, "running"), approvals: [approvalFor("waiting", at)], finishedAtMs: null } });
    observer({ type: "phase", run_id, phase: "tool" });
    return { run_id };
  };
}

/**
 * The preview Chat exists only in the fixture's workspace, and the mock reads
 * artifacts from its own store. Back it with a stored Chat so the published
 * chart can be read into its card.
 */
export async function inlineChartFixture(bridge: PreviewBridge) {
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
export function longThread(bridge: PreviewBridge) {
  const select = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async (loupistId) => {
    const workspace = await select(loupistId);
    const repeat = (messages: typeof workspace.threads[number]["messages"]) => [1, 2].flatMap(copy => messages.map(message => ({ ...message, id: `earlier-${copy}-${message.id}` })));
    return { ...workspace, threads: workspace.threads.map((thread, index) => index ? thread : { ...thread, messages: [...repeat(thread.messages), ...thread.messages] }) };
  };
}

export async function submitPrompt(root: HTMLElement, prompt = previewKo() ? "한울정밀의 가동률과 재고일수를 비교해 줘." : "Compare Hanul Precision’s utilization and inventory days.") {
  const composer = await until(() => root.querySelector<HTMLTextAreaElement>('textarea[data-chat-composer]'), "Composer did not appear");
  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, prompt);
  composer.dispatchEvent(new Event("input", { bubbles: true }));
  await click('.composer-shell button[type="submit"]', root);
}

/** jsdom has no scroll geometry; those runs check the scenario reaches the thread. */
// The live dot is aria-hidden, so landmarks use its visible run header.
export const runningHeader = () => select(".message.assistant.streaming .chat-progress") ?? select(".message.assistant.streaming");

/** This chat's research map: four names around one hub, with `events` as the growth this chat added. */
export function chatResearchMap(bridge: PreviewBridge, events: () => GrowthEvent[], names = ["Hanul Precision", "Morrow Components", "Capacity utilization", "Operating margin"]) {
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

export const chatGrowth = (index: number, originMessageId: string, atMs: number): GrowthEvent => ({ eventId: `chat-growth-${index}`, loupistId: "loupist-quality", originChatId: "preview-chat", originMessageId, atMs, kind: "entity_added", subjectId: `chat-node-${index}`, recordId: "preview-world", actor: "loupist", summary: "", beforeSummary: null, afterSummary: null });

/** What waits for the user, as Rust's `attention_list` reports it: an approval, an answer that asked a question, briefing conflicts and a check due soon. */
export function prepareAttention(bridge: PreviewBridge) {
  const ko = previewKo();
  bridge.attentionItems = [
    { kind: "approval", loupistId: "loupist-quality", loupistName: "Quality Compounder", chatId: "preview-chat", title: ko ? "기업 공시 · read_filing" : "Company filings · read_filing", atMs: Date.now() },
    { kind: "question", loupistId: "loupist-quality", loupistName: "Quality Compounder", chatId: "preview-chat-1", title: ko ? "모로 부품 공급망 점검" : "Morrow Components supplier check", atMs: Date.now() - 600_000 },
    { kind: "briefingConflict", loupistId: "loupist-quality", loupistName: "Quality Compounder", count: 2, title: "", atMs: Date.now() - 3_600_000 },
    { kind: "checkDue", loupistId: "loupist-quality", loupistName: "Quality Compounder", recordId: "preview-journal", title: ko ? "한울정밀의 재고일수가 50일 아래로 내려간다" : "Hanul’s inventory days fall below 50", dueDate: new Date(Date.now() + 2 * 86_400_000).toISOString().slice(0, 10), atMs: Date.now() },
  ];
}

/**
 * A semiconductor Map past sixty names: TSMC with twelve neighbors, 199 second-ring
 * names and one name with no relationships. `lens`: two Lens entries applied to TSMC,
 * for the Relationships view's dotted rows.
 */
export function localViewMap(bridge: PreviewBridge, { lens = false } = {}) {
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

export function rememberPreviewMap({ focalId, scope, legendOpen }: { focalId?: string; scope?: MapScope; legendOpen?: boolean }) {
  if (focalId) rememberFocal("loupist-quality", focalId);
  if (scope) rememberScope("loupist-quality", scope);
  if (legendOpen !== undefined) rememberLegendOpen(legendOpen);
}

export async function enterMap(root: HTMLElement) {
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

/** Scrolls the feature under review into view, so narrow captures show it rather than the thread's end. */
export const centerOn = (selector: string, block: ScrollLogicalPosition = "center", readySelector?: string) => async () => {
  const target = await until(() => messagesRendered() && (!readySelector || select(readySelector)) ? select(selector) : null, `Not ready to frame: ${selector}`);
  if (hasLayout()) {
    await document.fonts.ready;
    // Morning news can finish after the host-posted message mounts. Let its
    // settled effect and the conversation's resize scroll finish before framing.
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    target.scrollIntoView({ block });
  }
};

export const base = (id: string, title: string, landmark: PreviewScenario["landmark"], enter: PreviewScenario["enter"] = idle): PreviewScenario => ({ id, title, setup: (bridge) => prepareFixture(bridge), enter: async root => { await enter(root); await until(messagesRendered, "Message Markdown did not finish rendering"); }, landmark: () => messagesRendered() ? landmark() : null });
