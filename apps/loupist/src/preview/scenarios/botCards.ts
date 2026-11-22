/**
 * Bot answers in the form the agent wrote them, from the market-data reads it chose: a
 * quick answer, a small table, and a long reply with headings, a list and a table. For
 * looking at how a Bot bubble lays out Markdown; nothing here is an assertion about the form.
 */
import type { ChatEvidenceRef, ChatMessage, NumberCheck } from "../../contracts/chat";
import { FIXTURE_TIME, prepareFixture, previewKo } from "../fixtures";
import { asSession, base, select, type PreviewScenario } from "./kit";

/** One cited figure: the text the answer prints, the read it came from, and the field it matched. */
type Figure = { text: string; read: number; field?: string; value: string };

/** NVIDIA's quote, valuation, results and headlines, one bundled read each, as the answer cites them. */
function reads(): ChatEvidenceRef[] {
  const at = new Date(FIXTURE_TIME).toISOString();
  const ref = (id: string, title: string, tool: string, provider: string, excerpt: string): ChatEvidenceRef => ({
    id, title, kind: "original", provider, tool_name: tool, retrieved_at: at, excerpt, excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [],
  });
  return [
    ref("ev_read_price", "NVDA · Quote", "mcp__openbb__equity_price_quote", "yfinance", "NVDA last 162.85 prev_close 166.19 year_high 255.50 year_low 148.80 ma_50d 214.65 ma_200d 204.19"),
    ref("ev_read_valuation", "NVDA · Valuation", "mcp__openbb__equity_fundamental_metrics", "fmp", "market_cap 3930000000000 forward_pe 20.6"),
    ref("ev_read_results", "NVDA · Results", "mcp__openbb__equity_fundamental_income", "sec", "Q3 FY2026 revenue 96200000000"),
    ref("ev_read_news", "NVDA · Headlines", "mcp__openbb__news_company", "Benzinga", "NVIDIA reaffirms shipping schedule for next accelerators"),
  ];
}

/** An answer whose `{n}` placeholders are the figures, each a link to its read and a matched number check. */
function answer(template: string, figures: Figure[]): Pick<ChatMessage, "content" | "evidence_refs" | "number_checks"> {
  const evidence = reads();
  const content = template.replace(/\{(\d+)\}/gu, (_, index: string) => {
    const figure = figures[Number(index)]!;
    return `[${figure.text}](#evidence-${evidence[figure.read]!.id})`;
  });
  // Checks follow the links' order in the text, which is the template's placeholder order.
  const order = [...template.matchAll(/\{(\d+)\}/gu)].map(match => Number(match[1]));
  const number_checks: NumberCheck[] = order.map((index, link_index) => {
    const figure = figures[index]!;
    const read = evidence[figure.read]!;
    return { link_index, text: figure.text, evidence_id: read.id, kind: "data", status: "matched",
      fields: [{ key: figure.field ?? "next_results", value: figure.value, matched: true }] };
  });
  return { content, evidence_refs: evidence, number_checks };
}

const PRICE = 0, VALUATION = 1, RESULTS = 2;

/** A few sentences on how a followed stock stands. */
function quickAnswer() {
  const ko = previewKo();
  return answer(ko
    ? "엔비디아는 금요일 {0}에 마감해 하루 {1} 내렸습니다. 200일 평균보다 {2}, 52주 최고가보다 {3} 낮은 수준입니다. 하락을 설명하는 회사 뉴스는 확인되지 않았고, 다음 실적 발표는 11월 19일입니다."
    : "NVIDIA closed at {0} on Friday, down {1} on the day. That leaves it {2} below its 200-day average and {3} under its 52-week high. No company news was found that explains the drop; results are due on Nov 19.", [
    { text: ko ? "162.85달러" : "$162.85", read: PRICE, field: "last", value: "162.85" },
    { text: "2.01%", read: PRICE, field: "change_pct", value: "-2.01" },
    { text: "20.25%", read: PRICE, field: "below_ma200_pct", value: "20.25" },
    { text: "36.26%", read: PRICE, field: "below_52w_high_pct", value: "36.26" },
  ]);
}

/** Price, a multiple and the next results date, as a small table. */
function tableAnswer() {
  const ko = previewKo();
  return answer(ko
    ? "금요일 종가 기준입니다.\n\n| 항목 | 엔비디아 |\n| --- | ---: |\n| 종가 | {0} |\n| 선행 PER | {1} |\n| 다음 실적 발표 | {2} |"
    : "As of Friday's close:\n\n| Figure | NVIDIA |\n| --- | ---: |\n| Price | {0} |\n| Forward P/E | {1} |\n| Next results | {2} |", [
    { text: ko ? "162.85달러" : "$162.85", read: PRICE, field: "last", value: "162.85" },
    { text: ko ? "20.6배" : "20.6x", read: VALUATION, field: "forward_pe_multiple", value: "20.6" },
    { text: ko ? "11월 19일" : "Nov 19", read: RESULTS, value: "2026-11-19" },
  ]);
}

/** One reply with headings, a list and a wide table. */
function longAnswer() {
  const ko = previewKo();
  return answer(ko
    ? [
      "### 가격",
      "엔비디아는 금요일 {0}에 마감해 하루 {1} 내렸습니다. 주요 평균과 52주 범위 모두에서 아래쪽에 있습니다.",
      "",
      "| 기준 | 가격 | 종가가 낮은 폭 | 근거 |",
      "| --- | ---: | ---: | --- |",
      "| 50일 평균 | {2} | {3} | 일별 종가 |",
      "| 200일 평균 | {4} | {5} | 일별 종가 |",
      "| 52주 최고가 | {6} | {7} | 장중 범위 |",
      "| 52주 최저가 | {8} | — | 장중 범위 |",
      "",
      "### 가치 평가와 실적",
      "- 시가총액: {9}",
      "- 선행 PER: {10}",
      "- 직전 분기(2026 회계연도 3분기) 매출: {11}, 전년 대비 {12} 증가",
      "",
      "### 지켜볼 점",
      "1. 11월 19일 실적 발표에서 데이터센터 매출이 전망을 지키는지",
      "2. 주가가 52주 최저가 위에서 버티는지",
    ].join("\n")
    : [
      "### Price",
      "NVIDIA closed at {0} on Friday, down {1} on the day. It sits below its main averages and in the lower part of its 52-week range.",
      "",
      "| Level | Price | Last close below it | Basis |",
      "| --- | ---: | ---: | --- |",
      "| 50-day average | {2} | {3} | Daily closes |",
      "| 200-day average | {4} | {5} | Daily closes |",
      "| 52-week high | {6} | {7} | Intraday range |",
      "| 52-week low | {8} | — | Intraday range |",
      "",
      "### Valuation and results",
      "- Market value: {9}",
      "- Forward P/E: {10}",
      "- Last quarter (Q3 FY2026) revenue: {11}, up {12} year over year",
      "",
      "### What to watch",
      "1. Whether data-center revenue holds up in the Nov 19 results",
      "2. Whether the price holds above its 52-week low",
    ].join("\n"), [
    { text: ko ? "162.85달러" : "$162.85", read: PRICE, field: "last", value: "162.85" },
    { text: "2.01%", read: PRICE, field: "change_pct", value: "-2.01" },
    { text: ko ? "214.65달러" : "$214.65", read: PRICE, field: "ma50", value: "214.65" },
    { text: "24.13%", read: PRICE, field: "below_ma50_pct", value: "24.13" },
    { text: ko ? "204.19달러" : "$204.19", read: PRICE, field: "ma200", value: "204.19" },
    { text: "20.25%", read: PRICE, field: "below_ma200_pct", value: "20.25" },
    { text: ko ? "255.50달러" : "$255.50", read: PRICE, field: "high_52w", value: "255.5" },
    { text: "36.26%", read: PRICE, field: "below_52w_high_pct", value: "36.26" },
    { text: ko ? "148.80달러" : "$148.80", read: PRICE, field: "low_52w", value: "148.8" },
    { text: ko ? "3.93조 달러" : "$3.93T", read: VALUATION, field: "market_cap", value: "3930000000000" },
    { text: ko ? "20.6배" : "20.6x", read: VALUATION, field: "forward_pe_multiple", value: "20.6" },
    { text: ko ? "962억 달러" : "$96.2B", read: RESULTS, field: "revenue", value: "96200000000" },
    { text: "17.9%", read: RESULTS, field: "revenue_growth_yoy_pct", value: "17.9" },
  ]);
}

/** The home Thread with one question about NVIDIA and the answer as the agent wrote it. */
const withAnswer = (question: () => string, written: () => Pick<ChatMessage, "content" | "evidence_refs" | "number_checks">): PreviewScenario["setup"] => async (bridge) => {
  await prepareFixture(bridge, "answer");
  const select = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async (loupistId) => {
    const workspace = await select(loupistId);
    const asked = new Date(FIXTURE_TIME - 5 * 60_000);
    const answered = new Date(asked.getTime() + 9_000);
    workspace.threads[0].messages = [
      { id: "preview-user", role: "user", status: "complete", created_at: asked.toISOString(), content: question() },
      { id: "preview-answer", role: "assistant", status: "complete", created_at: answered.toISOString(), ...written(),
        progress: { startedAtMs: asked.getTime(), finishedAtMs: answered.getTime(), items: [
          { id: "preview-step-quote", kind: "tool", toolName: "mcp__openbb__equity_price_quote", category: "finance", operation: "read", action: "Read the quote", target: "NVDA",
            status: "succeeded", href: null, startedAtMs: asked.getTime() + 500, finishedAtMs: asked.getTime() + 2_400, resultIds: ["ev_read_price", "ev_read_valuation", "ev_read_results"] },
        ] } },
    ];
    return workspace;
  };
};

const trace = () => select('[data-bot-message] .bot-trace-open');

export const botCardScenarios: PreviewScenario[] = [
  { ...base("bot-quick-answer", "Bot: a quick answer on a followed stock, figures as chips, and the trace line", trace),
    setup: withAnswer(() => previewKo() ? "엔비디아 현황을 가격, 실적, 뉴스 중심으로 간단히 알려 줘." : "Where does NVIDIA stand? Give me the price, results and news in brief.", quickAnswer) },
  { ...base("bot-answer-table", "Bot: an answer that is a small table, each number a chip", () => trace() && select('[data-bot-message] table')),
    setup: withAnswer(() => previewKo() ? "엔비디아 종가, PER, 다음 실적일을 표로" : "NVIDIA's price, P/E and next results date in a table", tableAnswer) },
  { ...base("bot-answer-long", "Bot: one reply with headings, a list and a wide table", () => trace() && select('[data-bot-message] h3')),
    setup: withAnswer(() => previewKo() ? "엔비디아 현황을 제목을 붙여 자세히" : "Explain where NVIDIA stands properly, with headings", longAnswer) },
  // The same reply in a research session, which shares the Markdown and number-chip styles with the Bot.
  { ...base("session-answer-long", "Research session: the headed reply with a list and a wide table of number chips", () => select('.message-content [data-streamdown="table"] [data-testid="number-chip"]')),
    setup: async (bridge) => {
      await withAnswer(() => previewKo() ? "엔비디아 현황을 제목을 붙여 자세히" : "Explain where NVIDIA stands properly, with headings", longAnswer)(bridge);
      asSession(bridge);
    } },
];
