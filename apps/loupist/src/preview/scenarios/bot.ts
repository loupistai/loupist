/**
 * Bot as a conversation (owner redesign, area D): the greeting, the one-line working
 * state and a short answer, with the prototype's followed assets. Figures are
 * illustrative; each tile is the stored price a saved read last replayed
 * (`watchlist_quotes`), with the date and provider the result stated.
 */
import { MIN_VISIT_MS, recordVisit } from "../../app/loupistVisit";
import { mockAssetId, mockSavedRead } from "../../bridge/mock/assets";
import type { ChatEvidenceRef, ChatMessage } from "../../contracts/chat";
import { FIXTURE_TIME, prepareFixture, PreviewBridge, previewKo } from "../fixtures";
import { base, select, submitPrompt, until, type PreviewScenario } from "./kit";

const LOUPIST = "loupist-quality";

/**
 * The prototype's four followed assets, named in the interface language. Three have a stored
 * price as its provider stated it: a day, a time, and one without a currency; Tesla has none yet.
 */
function followPrototypeAssets(bridge: PreviewBridge) {
  const ko = previewKo();
  const day = new Date(FIXTURE_TIME - 86_400_000).toISOString().slice(0, 10);
  const quoted = new Date(FIXTURE_TIME - 25 * 60_000).toISOString();
  for (const [ticker, exchange, local, english, price] of [
    ["NVDA", "US", "엔비디아", "NVIDIA", { value: 162.85, date: day, currency: "USD", provider: "yfinance" }],
    ["005930", "KS", "삼성전자", "Samsung Electronics", { value: 112_400, date: quoted, currency: "KRW", provider: "krx" }],
    ["000660", "KS", "SK하이닉스", "SK hynix", { value: 618_000, date: day, currency: null, provider: "kis" }],
    ["TSLA", "US", "테슬라", "Tesla", null],
  ] as const) {
    const assetId = mockAssetId(english);
    bridge.assets.installAsset({ assetId, name: ko ? local : english, ticker, exchange });
    if (price) bridge.assets.installPrice(LOUPIST, assetId, { price: { ...price, sourceUrl: null, dataset: { dataset_id: ticker, revision: 1, digest: "preview" } }, savedRead: mockSavedRead("ready") });
  }
}

async function prepareGreeting(bridge: PreviewBridge, visited = true) {
  await prepareFixture(bridge);
  followPrototypeAssets(bridge);
  // Last here two days ago. The answer scenario has no earlier visit,
  // so no "since you were last here" news follows the answer.
  const lastSeenAtMs = FIXTURE_TIME - 2 * 86_400_000;
  if (visited) recordVisit(LOUPIST, lastSeenAtMs - MIN_VISIT_MS, lastSeenAtMs);
  await bridge.briefingSettingsSet({ loupistId: LOUPIST, enabled: true });
}

const QUOTE_ID = `ev_${"c".repeat(32)}`;

/** The price read the answer cites, as Rust keeps it. */
function quoteEvidence(): ChatEvidenceRef {
  return {
    id: QUOTE_ID, title: previewKo() ? "엔비디아 일별 종가 · yfinance" : "NVIDIA daily closes · yfinance", kind: "original", url: "https://finance.yahoo.com/quote/NVDA/history",
    provider: "yfinance", tool_name: "equity_price_historical", retrieved_at: new Date(FIXTURE_TIME).toISOString(), published_at: new Date(FIXTURE_TIME).toISOString().slice(0, 10),
    excerpt: "Close 162.85; previous close 166.19; change -2.01%; close Nov 4 200.13.", excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [],
  };
}

/** A quick answer: three formal sentences, each figure linked to the read it came from. */
function shortAnswer(): ChatMessage[] {
  const ko = previewKo();
  const link = (text: string) => `[${text}](#evidence-${QUOTE_ID})`;
  const content = ko
    ? `엔비디아는 직전 거래일 ${link("162.85달러")}에 마감했습니다. 하루 ${link("2.01%")} 하락했고, 11월 4일 이후로는 ${link("18.6%")} 내렸습니다. 하락 원인으로 확인된 회사 뉴스는 없습니다.`
    : `NVIDIA closed at ${link("$162.85")} in the last session. It fell ${link("2.01%")} on the day and is down ${link("18.6%")} since Nov 4. No company news was found that explains the drop.`;
  const texts = ko ? ["162.85달러", "2.01%", "18.6%"] : ["$162.85", "2.01%", "18.6%"];
  const asked = new Date(FIXTURE_TIME - 5 * 60_000);
  return [
    { id: "bot-question", role: "user", status: "complete", created_at: asked.toISOString(), content: ko ? "엔비디아 현황을 가격, 실적, 뉴스 중심으로 간단히 알려 줘." : "Where does NVIDIA stand? Give me the price, results and news in brief." },
    { id: "bot-answer", role: "assistant", status: "complete", created_at: new Date(asked.getTime() + 9_000).toISOString(), content,
      evidence_refs: [quoteEvidence()],
      // The change since Nov 4 is not in the read as written: it draws as a number to check, never as sourced.
      number_checks: texts.map((text, index) => ({ link_index: index, text, evidence_id: QUOTE_ID, kind: "passage" as const, status: index === 2 ? "not_found" as const : "matched" as const })),
      progress: { startedAtMs: asked.getTime(), finishedAtMs: asked.getTime() + 9_000, items: [
        { id: "bot-step-price", kind: "tool", toolName: "mcp__openbb__equity_price_historical", category: "finance", operation: "read", action: "Read price history", target: "NVDA", status: "succeeded", href: null, startedAtMs: asked.getTime() + 500, finishedAtMs: asked.getTime() + 2_400, resultIds: [QUOTE_ID] },
      ] } },
  ];
}

/** A reply in flight on its first read, with no text yet. */
function workingRun(bridge: PreviewBridge) {
  bridge.chatSend = async (request, observer) => {
    const run_id = request.run_id;
    observer({ type: "started", run_id });
    observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: Date.now() - 4_000, items: [
      { id: "bot-step-price", kind: "tool", toolName: "mcp__openbb__equity_price_historical", category: "finance", operation: "read", action: "Read price history", target: "NVDA", status: "running", href: null, startedAtMs: Date.now() - 2_000, finishedAtMs: null },
    ], finishedAtMs: null } });
    observer({ type: "phase", run_id, phase: "tool" });
    return { run_id };
  };
}

export const botScenarios: PreviewScenario[] = [
  { ...base("bot-greeting", "Bot greets: questions to ask, and each followed asset's stored price with its stated date and provider", () => select('.bot-greeting[aria-busy="false"] .bot-tiles li:nth-child(3) .bot-tile-foot')),
    setup: prepareGreeting },
  { ...base("bot-working", "Bot works: one quiet line by the face, which opens the activity", () => select('[data-bot-message] .bot-working[data-kind="step"]'), async root => {
    await submitPrompt(root, previewKo() ? "엔비디아 현황을 가격, 실적, 뉴스 중심으로 간단히 알려 줘." : "Where does NVIDIA stand? Give me the price, results and news in brief.");
    await until(() => select('[data-bot-message] .bot-working[data-kind="step"]'), "The working line did not appear");
  }), setup: async bridge => { await prepareGreeting(bridge); workingRun(bridge); } },
  { ...base("bot-answer", "Bot answers in three sentences; figures checked and sources open the activity", () => select('[data-bot-message] .bot-trace-open')),
    setup: async bridge => {
      await prepareGreeting(bridge, false);
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const home = workspace.threads.find(thread => thread.home);
        if (home) home.messages = shortAnswer();
        return workspace;
      };
    } },
];
