/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, PreviewBridge, previewKo } from "../fixtures";
import { workspaceOverlayMedia } from "../../components/workspace/useWorkspacePanelResize";
import type { ChatProgressItem } from "../../contracts/chat";
import type { WorkbookRead } from "../../contracts/workspace";
import i18n from "../../i18n";
import { asSession, base, chatGrowth, chatResearchMap, click, enterMap, hasLayout, inlineChartFixture, localViewMap, mapCanvas, pressMenu, select, selectPanel, submitPrompt, until, visible, type PreviewScenario } from "./kit";

/** The chat's first answer is the one that adds names; its growth appears once it completes. */
const firstMapChange = { messageId: "" };

/** A document with three kept versions (side-panel.md §3.2); Restore appends the chosen one as the newest. */
async function versionedDocument(bridge: PreviewBridge) {
  const ko = previewKo();
  const title = ko ? "한울정밀 3분기 메모" : "Hanul Precision Q3 notes";
  const bodies = ko ? [
    "새 생산라인의 인증 비용이 늘면서 영업이익률이 14.8%로 낮아졌습니다.",
    "영업이익률이 14.8%로 낮아졌습니다. 인증 비용이 대부분을 설명하고, 가동률은 4분기에 회복될 것으로 보입니다.",
    "영업이익률이 14.8%로 낮아졌습니다. 인증 비용이 대부분을 설명하지만, 재고일수 54일은 수요가 아직 따라오지 못했다는 뜻입니다.",
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
    chart_dataset: null,
  });
  await prepareFixture(bridge, "answer");
  bridge.chatArtifactList = async () => [view(current()).artifact];
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
  const label = i18n.t("chat:browserWorkspaceView.savedPassage");
  const button = await until(() => [...document.querySelectorAll("button")].find(node => visible(node) && node.textContent?.trim() === label) ?? null, "Saved passage control did not appear");
  button.click();
}

/** The Work tab's list: two charts and two documents from one chat. */
async function workListFixture(bridge: PreviewBridge) {
  await prepareFixture(bridge, "answer");
  const at = Date.parse("2026-10-28T00:00:00Z");
  bridge.chatArtifactList = async (_loupistId, threadId) => [
    { id: "work-margin", title: "Margin vs utilization", kind: "chart" as const },
    { id: "work-note", title: "Hanul Precision Q3 notes", kind: "markdown" as const },
    { id: "work-suppliers", title: "Supplier lead times", kind: "chart" as const },
    { id: "work-memo", title: "Dividend coverage memo", kind: "markdown" as const },
  ].map((item, index) => ({ ...item, chat_session_id: threadId, current_revision: 1,
    created_at_ms: at - index * 86_400_000, updated_at_ms: at - index * 86_400_000 }));
}

/** The side panel: sources, evidence, the Work list, docking and delivered files. */
/**
 * A retained quarterly table too wide for the docked panel, opened at its cited figure in a far
 * column. Synthetic figures.
 */
async function citedWideTableFixture(bridge: PreviewBridge) {
  await prepareFixture(bridge, "answer");
  const table = [
    "| (USD millions) | Q3 2025 | Q4 2025 | Q1 2026 | Q2 2026 | Q3 2026 | YoY |",
    "| --- | --- | --- | --- | --- | --- | --- |",
    "| Total revenues | 25,182 | 25,707 | 19,335 | 22,496 | 28,095 | 12% |",
    "| Income from operations | 1,409 | 941 | 398 | 923 | 718 | -49% |",
    "| Operating margin | 5.7% | 3.7% | 2.1% | 4.1% | 2.6% | -312 bp |",
    "| Net income attributable to common stockholders (GAAP) | 2,167 | 2,356 | 409 | 1,172 | 1,373 | -37% |",
    "| Free cash flow | 2,742 | 2,034 | 664 | 146 | 3,990 | 46% |",
  ].join("\n");
  const text = `Hanul Precision Q3 2026 update.\n\n${table}\n\nOperating income fell on lower average selling prices.`;
  const bytes = (value: string) => new TextEncoder().encode(value).length;
  const at = (start: number, length: number) => ({ start: bytes(text.slice(0, start)), end: bytes(text.slice(0, start + length)) });
  const title = "Hanul Precision Q3 2026 update";
  const saved = await bridge.knowledgeSourceRetain({ loupistId: "loupist-quality", title, originLocator: "Pasted text", coverage: "included_full", text });
  const read = bridge.knowledgeSourceRead.bind(bridge);
  const spanId = "preview-cited-table";
  bridge.knowledgeSourceRead = async request => request.recordId !== saved.recordId ? read(request) : {
    ...await read(request.spanId === spanId ? { ...request, spanId: undefined } : request), material: "provider_payload", selectedSpanId: spanId,
    spans: [{
      spanId, mediaType: "application/json", selector: "bytes:0-4096", contextNote: "", text: JSON.stringify({ content: text }),
      presentation: {
        text, sourcePointer: "/content", extractionMethod: "readability", contentOffset: 0, nextOffset: null, extractionTruncated: false,
        citedPassages: [{ ...at(text.indexOf(table), table.length), marks: [at(text.indexOf("| 718 |") + 2, 3)] }],
      },
    }],
  };
  await bridge.workspaceUiWrite({ version: 1, open: true, placement: "side", width: 480, height: 360, sessions: [{ loupist_id: "loupist-quality", thread_id: "preview-chat", active_tab_id: "saved-source", tabs: [{ id: "saved-source", kind: "memory", title, record_id: saved.recordId, location: { loupistId: "loupist-quality", surface: "source", recordId: saved.recordId, revisionId: saved.revisionId, selection: { kind: "source", sourceId: saved.sourceId, spanId } } }] }] });
}

export const panelScenarios: PreviewScenario[] = [
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
      const saved = await bridge.knowledgeSourceRetain({ loupistId: "loupist-quality", title, originLocator: "Pasted text", coverage: "included_full", text: previewKo() ? "매출은 11% 늘었지만 영업이익률은 14.8%로 낮아졌습니다. 새 생산라인의 인증 비용이 늘었습니다. 재고일수와 가동률을 함께 확인해야 합니다." : "Revenue grew 11% while operating margin fell to 14.8%. Qualification costs at the new production line increased. Compare inventory days with utilization." });
      catalog.sources.unshift({ ...catalog.sources[0], recordId: saved.recordId, revisionId: saved.revisionId, title, objects: [{ id: saved.sourceId, title }] });
      bridge.knowledgeCatalog = async ({ loupistId }) => structuredClone({ ...catalog, loupistId });
      await bridge.workspaceUiWrite({ version: 1, open: true, placement: "side", width: 480, height: 360, sessions: [{ loupist_id: "loupist-quality", thread_id: "preview-chat", active_tab_id: "saved-source", tabs: [{ id: "saved-source", kind: "memory", title, record_id: saved.recordId, location: { loupistId: "loupist-quality", surface: "source", recordId: saved.recordId, revisionId: saved.revisionId, selection: { kind: "source", sourceId: saved.sourceId, spanId: saved.spanId ?? undefined } } }] }] });
    },
  })),
  { ...base("panel-source-cited-table", "A cited figure in a wide retained table opens beside its row label", () => select(".workspace-panel .saved-passage-table table[data-row-labels] mark.evidence-match"), async () => {
    await until(() => select(".workspace-panel .saved-passage-table table[data-row-labels]"), "The cited table did not open");
  }), setup: citedWideTableFixture },
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
    ...base(id, "World for this chat", () => select(".chat-mini-map .map-empty") ?? (mapCanvas() ? select(".chat-mini-map .map-canvas") : null), async root => {
      if (id === "chat-mini-map") await click('.chat-receipt [data-receipt="names"] button', root);
      else {
        await selectPanel("materials");
        await click("#workspace-tab-chat-map", root);
      }
      await until(() => select(".chat-mini-map .map-empty") ?? (mapCanvas() ? select(".chat-mini-map .map-canvas") : null), "World for this chat did not load");
      if (hasLayout()) for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
    }),
    setup: async bridge => {
      await prepareFixture(bridge, id === "chat-mini-map" ? "answer" : "empty");
      const readMap = bridge.knowledgeMap.bind(bridge);
      bridge.knowledgeMap = async request => {
        const map = await readMap(request);
        map.nodes = ["Hanul Precision", "Morrow Components", "Capacity utilization", "Operating margin"].map((label, index) => ({ id: `chat-node-${index}`, kind: "entity", label, entityKind: index < 2 ? "Organization" : "Entity", degree: index === 0 ? 3 : 1, watched: false }));
        map.edges = [1, 2, 3].map(index => ({ id: `chat-edge-${index}`, kind: "claim", source: "chat-node-0", target: `chat-node-${index}`, relation: "related_to", status: "active" }));
        map.worldRevisionId = "chat-mini-map";
        return map;
      };
      bridge.knowledgeGrowthEvents = async () => ({ events: id === "chat-mini-map-empty" ? [] : [0, 1, 2].map(index => ({ eventId: `chat-growth-${index}`, loupistId: "loupist-quality", originChatId: "preview-chat", originMessageId: "preview-answer", atMs: Date.now(), kind: "entity_added", subjectId: `chat-node-${index}`, recordId: "preview-world", actor: "loupist", summary: "", beforeSummary: null, afterSummary: null })), nextCursor: null });
    },
  })),
  { ...base("chat-live-map", "Names a running step reads glow in this chat’s World", () => document.querySelector(".chat-progress-names") && (mapCanvas() ? select(".chat-mini-map .map-canvas") : null), async root => {
    await submitPrompt(root);
    await click(".message.assistant.streaming .chat-progress-toggle", root);
    await until(() => document.querySelector(".chat-progress-names"), "Step names did not appear");
    await selectPanel("materials");
    await click("#workspace-tab-chat-map", root);
    await until(() => mapCanvas() ? select(".chat-mini-map .map-canvas") : null, "World for this chat did not load");
    if (hasLayout()) for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
  }), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    // Earlier growth is settled, so only the running step's names glow.
    chatResearchMap(bridge, () => [0, 1, 2].map(index => chatGrowth(index, "preview-answer", Date.now() - 60_000)));
    bridge.chatSend = async (request, observer) => {
      const run_id = request.run_id;
      observer({ type: "started", run_id });
      observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: Date.now() - 9_000, items: [
        { id: "preview-map-read", kind: "tool", category: "memory", operation: "read", action: "Read World", status: "succeeded", entityIds: ["chat-node-0", "chat-node-2"], target: null, href: null, startedAtMs: 0, finishedAtMs: null },
        { id: "preview-page", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", running: "Reading hanul.example", target: "Q3 operating review · hanul.example", status: "running", href: null, startedAtMs: 0, finishedAtMs: null },
      ], finishedAtMs: null } });
      observer({ type: "phase", run_id, phase: "tool" });
      return { run_id };
    };
    asSession(bridge);
  } },
  // W15.29, W15.32, W15.33: the inspector's Relationships view, the panel's Work list, and charts docked below the chat.
  { ...base("map-relationships", "A company's suppliers, customers and peers in the inspector", () => select(".map-relationships"), async (root) => {
    const map = await enterMap(root);
    const input = await until(() => map.querySelector<HTMLInputElement>('[role="combobox"]'), "World search did not load");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "TSMC");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await click('[role="option"]', map);
    await until(() => select(".map-inspector"), "Inspector did not open");
    await click(".map-inspector-views button:last-child");
    // The dotted Lens line closes the view; bring it into the inspector's scroll.
    (await until(() => select(".map-relationship-lenses"), "The Lens line did not render")).scrollIntoView({ block: "nearest" });
  }), setup: async (bridge) => { await prepareFixture(bridge); localViewMap(bridge, { lens: true }); } },
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
  { ...base("panel-dock-menu", "The Dock control's menu with Always dock at side", () => select(".workspace-dock-menu") ?? (window.matchMedia(workspaceOverlayMedia).matches ? select(".work-list .work-row") : null), async (root) => {
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
  { ...base("panel-workbook", "A generated workbook opens in Files with its sheets and cached values", () => select(".reader-workbook .reader-data-table"), async root => {
    await click('button[role="link"][title*="outputs/revenue-comparison.xlsx"]', root);
  }), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    const path = "outputs/revenue-comparison.xlsx";
    const selectWorkspace = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await selectWorkspace(loupistId);
      const answer = workspace.threads[0]?.messages.at(-1);
      if (answer?.role === "assistant") {
        const at = Date.parse(answer.created_at);
        const step: ChatProgressItem = { id: "preview-workbook-write", kind: "tool", toolName: "run_python", category: "files", operation: "write", action: "Wrote a file", writtenPaths: [path], status: "succeeded", startedAtMs: at - 4000, finishedAtMs: at - 3000, target: null, href: null };
        answer.progress = { startedAtMs: at - 9000, finishedAtMs: at, ...answer.progress, items: [...(answer.progress?.items ?? []), step] };
        answer.content += `\n\n${previewKo() ? `[매출 비교 통합 문서](${path})를 저장했습니다.` : `I saved the [revenue comparison workbook](${path}).`}`;
      }
      return workspace;
    };
    const workspaceRead = bridge.workspaceRead.bind(bridge);
    bridge.workspaceRead = async request => request.path === path ? {
      path, size_bytes: 8919, content: null, content_sha256: null, binary: true, truncated: false,
      media_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    } : workspaceRead(request);
    bridge.workspaceDocumentHandler = async () => ({ status: "allowed", application_name: "Microsoft Excel" });
    const sheets = [
      { name: previewKo() ? "매출 비교" : "Comparison", hidden: false, worksheet: true },
      { name: previewKo() ? "원본" : "Sources", hidden: false, worksheet: true },
      { name: previewKo() ? "가정" : "Assumptions", hidden: true, worksheet: true },
    ];
    bridge.workspaceWorkbookRead = async ({ sheet_index }): Promise<WorkbookRead> => {
      const active = sheet_index ?? 0;
      if (active !== 0 && active !== 1) return { status: "unsupported_sheet" };
      const rows = active === 0 ? [
        [previewKo() ? "회사" : "Company", "FY 2023", "FY 2024"],
        ["Apple", "383,285", "391,035"], ["Microsoft", "211,915", "245,122"], ["Alphabet", "307,394", "350,018"],
      ] : [[previewKo() ? "출처" : "Source", "Form 10-K"]];
      return {
        status: "ready", sheets, selected_sheet: active, first_row: 1, first_column: 1,
        rows_total: rows.length, columns_total: rows[0]!.length, truncated: false, omitted: ["charts", "images", "visual_styles"],
        rows: rows.map((row, index) => row.map((display, column) => ({ kind: active === 0 && index > 0 && column > 0 ? "number" : "text", display, formula: false }))),
      };
    };
  } },
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
        const wrote = (id: string, filePath: string): ChatProgressItem => ({ id, kind: "tool", toolName: "write", category: "files", operation: "write", action: "Wrote a file", filePath, status: "succeeded", startedAtMs: at - 4_000, finishedAtMs: at - 3_000, target: null, href: null });
        answer.progress = { startedAtMs: at - 9_000, finishedAtMs: at, ...answer.progress, items: [...(answer.progress?.items ?? []), wrote("preview-write-memo", "notes/q3-margin-memo.md"), wrote("preview-write-table", "exports/hanul-margins-2024-2026.csv")] };
        answer.content = `${answer.content}\n\n${previewKo() ? "비교 메모는 [q3-margin-memo.md](notes/q3-margin-memo.md)에, 분기별 수치는 [hanul-margins-2024-2026.csv](exports/hanul-margins-2024-2026.csv)에 저장했습니다." : "I saved the comparison to [q3-margin-memo.md](notes/q3-margin-memo.md) and the quarterly figures to [hanul-margins-2024-2026.csv](exports/hanul-margins-2024-2026.csv)."}`;
      }
      return workspace;
    };
  } },
  { ...base("panel-opens-first-map-change", "A chat's first names open World beside it", () => {
    const names = select('article.message.assistant.complete .chat-receipt [data-receipt="names"]');
    if (!names) return null;
    // Below 1280 px, or as a sheet, the panel stays closed and the receipt carries the names.
    if (window.innerWidth < 1280) return select(".workspace-panel") ? null : names;
    return select('#workspace-tab-chat-map[aria-selected="true"]') && (mapCanvas() ? select(".chat-mini-map .map-canvas") : null);
  }, async root => {
    await submitPrompt(root);
    await until(() => select('article.message.assistant.complete .chat-receipt [data-receipt="names"]'), "The answer's names did not arrive", 20_000);
    if (hasLayout() && window.innerWidth >= 1280) {
      await until(() => mapCanvas() ? select(".chat-mini-map .map-canvas") : null, "World did not open beside the chat");
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
  // The page header's two splits: the one in use reads pressed (a filled button), the other does not.
  ...(["side", "bottom"] as const).map((placement): PreviewScenario => ({
    ...base(`panel-split-${placement}`, placement === "side" ? "Split left and right is pressed while the panel docks beside the chat" : "Split top and bottom is pressed while the panel docks below the chat",
      () => select(`[data-workspace-split="${placement}"][aria-pressed="true"]`) && select(`#workspace-panel[data-placement="${placement}"]`), async root => {
        await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
        // A pointer press, so focus stays on the header as it does after a click.
        (await until(() => select(`[data-workspace-split="${placement}"]`), "The split control did not render")).dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
        await until(() => select(`#workspace-panel[data-placement="${placement}"]`), "The split did not open the panel");
      }),
    setup: workListFixture,
  })),
];
