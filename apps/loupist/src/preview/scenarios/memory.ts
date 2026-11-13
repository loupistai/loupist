/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, PreviewBridge, previewKo } from "../fixtures";
import { base, click, hasLayout, openLoupistSettings, select, selectPanel, until, type PreviewScenario } from "./kit";

// Preview fixtures may already hold a principle with the same words; reuse it.
const ensurePrinciple = async (bridge: PreviewBridge, request: Parameters<PreviewBridge["principleCreate"]>[0]) =>
  (await bridge.principleList({ loupistId: request.loupistId })).find(item => item.statement === request.statement) ?? await bridge.principleCreate(request);

/** A principle saved from Chat. */
export const principleScenarios: PreviewScenario[] = [
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
];

/** "What {Loupist} knows" and corrections under an answer. */
export const memoryScenarios: PreviewScenario[] = [
  // W15.15–W15.17: "What {Loupist} knows", the inline correction under an answer, and the once-per-chat decline.
  ...(["memory-page", "memory-page-empty", "memory-paused", "memory-reset-dialog"] as const).map((id): PreviewScenario => ({
    ...base(id, id === "memory-page" ? "What the Loupist knows, with a principle open" : id === "memory-paused" ? "Learning is paused" : "A new Loupist has no pages yet",
      () => select(id === "memory-reset-dialog" ? ".memory-reset-dialog" : id === "memory-page" ? ".memory-reader:not(.is-empty)" : id === "memory-paused" ? ".memory-page-paused" : ".memory-empty"), async root => {
        await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
        if (id === "memory-reset-dialog") {
          await openLoupistSettings();
          await click('[data-loupist-section="learning"] > summary');
          await click("[data-memory-reset]");
        } else {
          await selectPanel("lens");
          if (id === "memory-page") await click(".memory-page-list .memory-row");
          if (id === "memory-page-empty") await until(() => select(".memory-empty"), "Empty Lens did not load");
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
          answer.lens_receipts = [{ recordId: principle.recordId, revisionId: principle.revisionId, kind: "principle", title: principle.statement, delivery: "context", revisionDigest: "digest" }];
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
];

/** What an answer used and learned. */
export const receiptScenarios: PreviewScenario[] = [
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
        message.memory_refs = [{ read_ref: "preview-method-read", kind: "lens", title: "Separate durable margins from temporary capacity costs", excerpt: "", access: "exact_read", knowledge: { kind: "lens", recordId: "preview-method", revisionId: "preview-method-rev", objectId: "preview-method" }, as_of: null, section: null }];
        // The receipt's used principles and methods come from what the host delivered.
        message.lens_receipts = [
          { recordId: principle.recordId, revisionId: principle.revisionId, kind: "principle", title: principle.statement, delivery: "context", revisionDigest: "digest" },
          { recordId: "preview-method", revisionId: "preview-method-rev", kind: "method", title: "Separate durable margins from temporary capacity costs", delivery: "exact_read", revisionDigest: "digest" },
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
];
