/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, previewKo } from "../fixtures";
import i18n from "../../i18n";
import { approvalRun, approvalSteps, asSession, base, centerOn, chatGrowth, chatResearchMap, click, hasLayout, messagesRendered, openScheduleRun, select, selectPanel, submitPrompt, until, visible, type PreviewScenario } from "./kit";
import type { PreviewBridge } from "../fixtures";
import type { ChatProgressItem } from "../../types";
import { liveGridArtifact } from "../gridScenarios";

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

/** A finished answer's steps as the activity sheet lists them: repeats, a failure, one step with no end time. */
function sheetSteps(at: number): ChatProgressItem[] {
  const read = (id: string, toolName: string, target: string, start: number, ms: number | null, extra: Partial<ChatProgressItem> = {}) => ({
    id, kind: "tool" as const, toolName, category: "finance" as const, operation: "read" as const, action: "Read", target, href: null,
    status: "succeeded" as const, startedAtMs: at + start, finishedAtMs: ms === null ? null : at + start + ms, ...extra,
  }) as ChatProgressItem;
  const company = previewKo() ? "한울정밀" : "Hanul Precision";
  return [
    read("sheet-tools", "mcp__openbb__activate_tools", "equity_estimates_forward_eps", 0, 300, { category: "capability", operation: "generic" } as Partial<ChatProgressItem>),
    read("sheet-quote", "mcp__openbb__equity_price_quote", company, 400, 1_200),
    read("sheet-income", "mcp__openbb__equity_fundamental_income", company, 1_700, 1_600),
    read("sheet-search", "web_search", previewKo() ? "한울정밀 3분기 영업이익률" : "Hanul Precision third-quarter margin", 3_400, 2_100, { category: "web", operation: "search" } as Partial<ChatProgressItem>),
    read("sheet-page", "web_fetch", "Q3 operating review · hanul.example", 5_600, 2_400, { category: "web", href: "https://hanul.example/quarterly-review", resultIds: [`ev_${"a".repeat(32)}`] } as Partial<ChatProgressItem>),
    read("sheet-page-2", "web_fetch", "Morrow supply note · morrow.example", 5_700, null, { category: "web", href: "https://morrow.example/supply" } as Partial<ChatProgressItem>),
    read("sheet-estimates", "mcp__openbb__equity_estimates_forward_eps", company, 8_100, 2_000, { status: "failed" } as Partial<ChatProgressItem>),
    ...[previewKo() ? "영업이익률" : "Operating margin", previewKo() ? "매출 증가율" : "Revenue growth", previewKo() ? "재고일수" : "Inventory days"].map((target, index) =>
      read(`sheet-calc-${index}`, "calculate", target, 10_200 + index * 200, 100, { category: "compute", operation: "calculate" } as Partial<ChatProgressItem>)),
  ];
}

/** The fixture's answer with the steps above, three checked figures, and a sent image and a written file. */
function sheetAnswer(bridge: PreviewBridge, { files = false } = {}) {
  const swatch = (color: string) => `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect width="96" height="96" fill="${color}"/><path d="M12 72 L36 44 L52 58 L84 22" stroke="white" stroke-width="6" fill="none"/></svg>`)}`;
  const original = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async loupistId => {
    const workspace = await original(loupistId);
    const [question, answer] = workspace.threads[0]!.messages;
    if (answer?.role === "assistant") {
      const at = Date.parse(answer.created_at) - 12_000;
      const evidenceId = answer.evidence_refs?.[0]?.id ?? "";
      answer.progress = { startedAtMs: at, finishedAtMs: at + 11_400, items: sheetSteps(at) };
      answer.number_checks = [
        { link_index: 0, text: "14.8%", evidence_id: evidenceId, kind: "passage", status: "matched" },
        { link_index: 1, text: previewKo() ? "1,842억 원" : "₩184.2 billion", evidence_id: evidenceId, kind: "passage", status: "matched" },
        { link_index: 2, text: previewKo() ? "54일" : "54 days", evidence_id: evidenceId, kind: "passage", status: "matched" },
      ];
      if (files) answer.content = `${answer.content}\n\n${previewKo() ? "비교 내용은 [q3-memo.md](notes/q3-memo.md)에 적어 두었습니다." : "I wrote the comparison to [q3-memo.md](notes/q3-memo.md)."}`;
    }
    if (files && question?.role === "user") {
      question.attachments = [
        { id: "sheet-1", filename: "utilization.png", media_type: "image/svg+xml", size_bytes: 48_200, url: swatch("steelblue") },
        { id: "sheet-2", filename: "holdings.csv", media_type: "text/csv", size_bytes: 14_300 },
      ];
    }
    return workspace;
  };
}

/**
 * The fixture's answer as a reply that published a table and then failed to
 * revise it: the card keeps the published table, the sheet lists the failed step.
 */
function revisedTableAnswer(bridge: PreviewBridge) {
  const table = liveGridArtifact();
  const original = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async loupistId => {
    const workspace = await original(loupistId);
    const answer = workspace.threads[0]!.messages.find(message => message.role === "assistant");
    if (answer) {
      const at = Date.parse(answer.created_at) - 20_000;
      const publish = (id: string, start: number, extra: Partial<ChatProgressItem> = {}) => ({
        id, kind: "tool" as const, toolName: "grid_publish", category: "artifact" as const, operation: "publish" as const, action: "Built a table", target: table.title, href: null,
        status: "succeeded" as const, startedAtMs: at + start, finishedAtMs: at + start + 800, ...extra,
      }) as ChatProgressItem;
      answer.evidence_refs = [...answer.evidence_refs ?? [], ...table.evidence];
      answer.artifact_refs = [{ artifact_id: table.id, revision: 1, kind: "grid", title: table.title, digest: table.view.revision.digest }];
      answer.progress = { startedAtMs: at, finishedAtMs: at + 19_000, items: [
        ...sheetSteps(at).slice(1, 3),
        publish("sheet-table", 6_000),
        publish("sheet-table-revise", 12_000, { status: "failed" }),
      ] };
    }
    return workspace;
  };
  const read = bridge.chatArtifactRead.bind(bridge);
  bridge.chatArtifactRead = async (loupistId, threadId, artifactId, revision) =>
    artifactId === table.id ? structuredClone(table.view) : read(loupistId, threadId, artifactId, revision);
}

/** Opens the sheet the way a person does: the page header's activity button, then a tab. */
async function openSheet(tab: "activity" | "learned" | "using" = "activity") {
  await click("[data-header-activity]");
  await until(() => select(".activity-sheet"), "The activity sheet did not open");
  if (tab !== "activity") await click(`[data-activity-tab="${tab}"]`);
}

/** The activity sheet, images, printing and attachments. */
export const activitySheetScenarios: PreviewScenario[] = [
  // Step 5 of the redesign: how a reply was made lives in one sheet behind the Loupist's face.
  { ...base("activity-sheet", "The activity sheet beside a finished answer: steps by kind, a failure with its reason, sources and checks",
    () => select('.activity-sheet [data-activity-section="checks"]'), () => openSheet()), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    sheetAnswer(bridge);
  } },
  { ...base("activity-sheet-revision-failed", "A table revision that failed: the sheet says the earlier version is still shown",
    () => [...document.querySelectorAll(".activity-sheet .activity-step-reason")].find(visible) ?? null, () => openSheet()), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    revisedTableAnswer(bridge);
  } },
  { ...base("activity-sheet-steps-open", "A collapsed kind of step opened to each of its runs",
    () => select(".activity-step-members"), async () => {
      await openSheet();
      const expand = await until(() => [...document.querySelectorAll<HTMLButtonElement>("[data-activity-expand]")].find(visible) ?? null, "No collapsed step");
      expand.click();
    }), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    sheetAnswer(bridge);
  } },
  { ...base("activity-sheet-learning", "A finished reply's sources are still being reviewed: a quiet line in the sheet", () => select(".activity-quiet"), () => openSheet("learned")), setup: async bridge => {
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
  { ...base("activity-sheet-learned", "Learned names in World and what the answer saved, including a long label", () => select(".activity-chip"), () => openSheet("learned")), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    const at = Date.now();
    chatResearchMap(bridge, () => [0, 1, 2, 3].map(index => chatGrowth(index, "preview-answer", at)),
      ["Hanul Precision", "Morrow Components", "Quarterly capacity utilization at the new packaging line", "HBM4"]);
  } },
  { ...base("activity-sheet-using", "Connections, memory and files in use, and the way to the chat's materials", () => select('.activity-sheet [data-activity-section="files"] .activity-source'), () => openSheet("using")), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    sheetAnswer(bridge, { files: true });
    const read = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async id => {
      const workspace = await read(id);
      const answer = workspace.threads[0]?.messages.find(message => message.id === "preview-answer");
      if (answer) {
        const at = Date.parse(answer.created_at);
        answer.progress = { startedAtMs: at - 30_000, finishedAtMs: at, items: [...approvalSteps(at, "succeeded"), ...answer.progress?.items ?? []] };
        answer.lens_receipts = [...answer.lens_receipts ?? [], {
          recordId: "preview-rail-method", revisionId: "rev", kind: "method", delivery: "context",
          title: previewKo() ? "고급 메모리 수요와 출하량 비교 · Advanced-Memory Demand-to-Shipments Conversion" : "Advanced-Memory Demand-to-Shipments Conversion",
          revisionDigest: "digest",
        }];
      }
      return workspace;
    };
  } },
  { ...base("activity-sheet-needs-you", "A connection waiting for the user's OK, named in the sheet", () => select('.activity-sheet [data-activity-section="needs"]'), async root => {
    await submitPrompt(root, previewKo() ? "한울정밀의 3분기 공시를 읽고 무엇이 바뀌었는지 알려 줘." : "Read Hanul Precision’s Q3 filings and tell me what changed.");
    await until(() => select(".approval-card"), "The approval did not appear");
    await openSheet("using");
  }), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    approvalRun(bridge);
  } },
  // Beside an open panel the chat is too narrow to dock, so the sheet slides over it from the right.
  { ...base("activity-sheet-panel", "The activity sheet over a chat that shares the window with its materials", () => select('.activity-sheet[data-placement]'), async () => {
    if (hasLayout() && document.documentElement.clientWidth > 1060) await selectPanel("materials");
    await openSheet();
  }), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    sheetAnswer(bridge);
  } },
  { ...base("chat-header-resuming", "A resumed response keeps its status above the chat body",
    () => !hasLayout() ? select(".message.assistant") : select(".thread-statuses .memory-state")), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    bridge.runActivityList = async () => [{
      run_id: "preview-resumed-run", loupist_id: "loupist-quality", kind: "chat",
      target: "preview-chat", started_at_ms: Date.now() - 30_000,
    }];
  } },
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
            { ...answer, id: "lightbox-follow-up-answer", content: previewKo() ? "재고일수는 54일로, 같은 업종 평균보다 깁니다." : "Inventory sits at 54 days, above the peer average.", evidence_refs: [], memory_update: undefined, created_at: "2026-10-28T00:05:12Z" },
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
    asSession(bridge);
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
  { ...base("composer-attach-menu", "The + menu attaches files, a pasted image, World or a saved source", () => select(".composer-add-menu"), async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    const trigger = await until(() => select(".composer-add-trigger"), "The + control did not render");
    trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
  // A file over the limit is refused with the limit named; the one within it stays in the tray.
  { ...base("composer-attach-too-large", "A file over 25 MB is refused with the limit named, beside the file that was added", () => select(".composer-error") && select(".composer-attachments .composer-file-chip"), async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    const column = await until(() => select(".chat-workspace"), "The chat column did not render");
    const drop = new Event("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", { value: { types: ["Files"], files: [
      new File([new Uint8Array(26 * 1024 * 1024)], "annual-report-scan.pdf", { type: "application/pdf" }),
      new File(["symbol,shares\n".repeat(1_000)], "holdings.csv", { type: "text/csv" }),
    ] } });
    column.dispatchEvent(drop);
    await until(() => select(".composer-error"), "The size limit was not said");
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
];
