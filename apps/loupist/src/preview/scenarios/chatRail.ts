/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, previewKo } from "../fixtures";
import i18n from "../../i18n";
import { approvalRun, approvalSteps, base, centerOn, chatGrowth, chatResearchMap, click, hasLayout, messagesRendered, openScheduleRun, select, selectPanel, submitPrompt, until, visible, type PreviewScenario } from "./kit";

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

/** The "In this chat" rail, images, printing and attachments. */
export const chatRailScenarios: PreviewScenario[] = [
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
        answer.progress = { startedAtMs: at - 12_000, finishedAtMs: at, items: approvalSteps(at, "succeeded") };
        answer.lens_receipts = [...answer.lens_receipts ?? [], {
          recordId: "preview-rail-method", revisionId: "rev", kind: "method", delivery: "context",
          title: previewKo() ? "고급 메모리 수요와 출하량 비교 · Advanced-Memory Demand-to-Shipments Conversion" : "Advanced-Memory Demand-to-Shipments Conversion",
          revisionDigest: "digest",
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
      if (id === "chat-rail-needs-you") approvalRun(bridge);
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
  // A protected-path refusal explains itself in the step, with no override.
  { ...base("chat-step-paused", "A private app path stays closed", () => select(".chat-step-paused"), async root => {
    const summary = await until(() => root.querySelector("button[data-research-summary]"), "Source review did not render");
    if (summary.getAttribute("aria-expanded") !== "true") await click("button[data-research-summary]", root);
    await centerOn(".chat-step-paused")();
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const answer = workspace.threads[0]!.messages.find((message) => message.role === "assistant");
      const at = Date.parse("2026-10-28T00:00:00Z");
      if (answer) answer.progress = { startedAtMs: at, finishedAtMs: at + 12_000, items: [
        { id: "paused-search", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", target: "Hanul Precision third-quarter margin", status: "succeeded", startedAtMs: at + 1_000, finishedAtMs: at + 3_000, href: null },
        { id: "paused-page", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", target: "Q3 operating review · hanul.example", href: "https://hanul.example/quarterly-review", status: "succeeded", startedAtMs: at + 3_000, finishedAtMs: at + 6_000 },
        { id: "paused-file", kind: "tool", toolName: "read", category: "files", operation: "read", action: "Read a file", target: "credentials.json", status: "failed", refusal: "protected_path", startedAtMs: at + 7_000, finishedAtMs: at + 7_100, href: null },
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
];
