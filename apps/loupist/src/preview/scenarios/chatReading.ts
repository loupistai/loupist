/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, previewKo, type PreviewBridge } from "../fixtures";
import { CHART_SCHEMA } from "../../contracts/chat";
import type { ChartDataset, ChartDocument, ChatArtifactView } from "../../types";
import { asSession, base, centerOn, click, hasLayout, inlineChartFixture, select, selectAnswerWords, submitPrompt, until, type PreviewScenario } from "./kit";

const REVENUE_CHART = "chart-revenue-by-year";

async function seedRevenueChart(bridge: PreviewBridge, chart: Pick<ChartDataset, "columns" | "rows"> & {
  title: string;
  prompt?: string;
  content: string;
  view: ChartDocument["view"];
}) {
  await prepareFixture(bridge, "answer");
  const { title, prompt, content, columns, rows } = chart;
  const dataset: ChartDataset = {
    id: "dataset-revenue", digest: "e".repeat(64), lineage: { kind: "agent_authored", upstream_receipts: [] },
    columns, rows,
  };
  const view: ChatArtifactView = {
    artifact: { id: REVENUE_CHART, chat_session_id: "preview-chat", kind: "chart", title, current_revision: 1, created_at_ms: 0, updated_at_ms: 0 },
    revision: { artifact_id: REVENUE_CHART, revision: 1, digest: "e".repeat(64), source_message_id: "preview-answer", created_at_ms: 0, payload: { kind: "chart", schema: CHART_SCHEMA, chart: {
      dataset_id: dataset.id, dataset_digest: dataset.digest, studies: [], drawings: [], custom_series: [], note: null,
      view: chart.view,
    } } },
    chart_dataset: dataset,
  };
  const original = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async loupistId => {
    const workspace = await original(loupistId);
    const messages = workspace.threads.find(thread => thread.id === "preview-chat")?.messages;
    const question = messages?.find(message => message.role === "user");
    if (prompt && question) question.content = prompt;
    const answer = messages?.find(message => message.id === "preview-answer");
    if (answer) Object.assign(answer, {
      content,
      ...(prompt ? { evidence_refs: [] } : {}),
      artifact_refs: [{ artifact_id: REVENUE_CHART, revision: 1, kind: "chart", title, digest: "e".repeat(64) }],
    });
    return workspace;
  };
  const read = bridge.chatArtifactRead.bind(bridge);
  bridge.chatArtifactRead = async (loupistId, chatId, artifactId, revisionId) => artifactId === REVENUE_CHART ? structuredClone(view) : read(loupistId, chatId, artifactId, revisionId);
}

/** The saved answer carries a bar chart of revenue in won by year, read back from the store. */
function revenueChartFixture(bridge: PreviewBridge) {
  const ko = previewKo();
  return seedRevenueChart(bridge, {
    title: ko ? "연도별 매출" : "Revenue by year",
    content: ko ? "매출은 2023년에 줄었다가 2025년에 다시 늘었습니다." : "Revenue dipped in 2023 and grew again by 2025.",
    columns: [{ id: "year", label: ko ? "연도" : "Year", kind: "number" }, { id: "revenue_krw", label: ko ? "매출" : "Revenue", kind: "number" }],
    rows: [[2021, 279_604_799_000_000], [2022, 302_231_360_000_000], [2023, 258_935_494_000_000], [2024, 300_870_903_000_000], [2025, 333_605_938_000_000]],
    view: { kind: "bar", x: "year", y: ["revenue_krw"], color: null, semantic_types: {}, title: null, subtitle: null },
  });
}

function groupedRevenueFixture(bridge: PreviewBridge) {
  const ko = previewKo();
  return seedRevenueChart(bridge, {
    title: ko ? "기업별 매출 비교" : "Revenue by company",
    prompt: ko ? "세 기업의 최근 두 회계연도 매출을 비교해 줘." : "Compare the three companies' revenue across both fiscal years.",
    content: ko ? "세 기업의 2023년과 2024년 회계연도 매출입니다." : "Reported revenue for three companies in fiscal 2023 and 2024.",
    columns: [
      { id: "company", label: ko ? "기업" : "Company", kind: "string" },
      { id: "year", label: ko ? "회계연도" : "Fiscal year", kind: "string" },
      { id: "revenue_usd_million", label: ko ? "매출(백만 달러)" : "Revenue (USD millions)", kind: "number" },
    ],
    rows: [
      ["Apple", "FY2023", 383_285], ["Apple", "FY2024", 391_035],
      ["Microsoft", "FY2023", 211_915], ["Microsoft", "FY2024", 245_122],
      ["Alphabet", "FY2023", 307_394], ["Alphabet", "FY2024", 350_018],
    ],
    view: { kind: "bar", x: "company", y: ["revenue_usd_million"], color: "year", semantic_types: {}, title: null, subtitle: null },
  });
}

function stackedRevenueFixture(bridge: PreviewBridge) {
  const ko = previewKo();
  return seedRevenueChart(bridge, {
    title: ko ? "사업부문별 매출 구성" : "Revenue by segment",
    prompt: ko ? "연도별 전체 매출에서 각 사업부문이 차지하는 금액을 보여 줘." : "Show how much each segment contributed to annual revenue.",
    content: ko ? "하드웨어와 서비스 매출을 더하면 각 연도의 전체 매출이 됩니다." : "Hardware and services make up the company's annual revenue.",
    columns: [
      { id: "year", label: ko ? "연도" : "Year", kind: "number" },
      { id: "hardware", label: ko ? "하드웨어(백만 달러)" : "Hardware (USD millions)", kind: "number" },
      { id: "services", label: ko ? "서비스(백만 달러)" : "Services (USD millions)", kind: "number" },
    ],
    rows: [[2023, 86, 35], [2024, 91, 46], [2025, 95, 53]],
    view: { kind: "bar", x: "year", y: ["hardware", "services"], color: null, stack: true, semantic_types: {}, title: null, subtitle: null },
  });
}

/** A year of daily closes in won, as a price-history chart publishes it: a date and a price per trading day. */
function dailyPriceFixture(bridge: PreviewBridge) {
  const ko = previewKo();
  const rows: Array<[string, number]> = [];
  for (let day = Date.UTC(2025, 10, 24); day <= Date.UTC(2026, 10, 23); day += 86_400_000) {
    if ([0, 6].includes(new Date(day).getUTCDay())) continue;
    rows.push([new Date(day).toISOString().slice(0, 10), Math.round(520_000 + rows.length * 4_200 + Math.sin(rows.length / 9) * 90_000)]);
  }
  return seedRevenueChart(bridge, {
    title: ko ? "한울정밀 최근 1년 일별 종가" : "Hanul Precision daily close, last 12 months",
    prompt: ko ? "한울정밀 최근 1년 주가를 차트로 보여 줘." : "Chart Hanul Precision's share price over the last year.",
    content: ko ? "최근 1년 일별 종가입니다." : "Daily closing prices for the last twelve months.",
    columns: [{ id: "date", label: ko ? "날짜" : "Date", kind: "date" }, { id: "close_krw", label: ko ? "종가(원)" : "Close (KRW)", kind: "number" }],
    rows,
    view: { kind: "line", x: "date", y: ["close_krw"], color: null, semantic_types: { date: "Date", close_krw: "Price" }, title: null, subtitle: null },
  });
}

const revenueChartReady = () => {
  const card = select('.chat-artifact-block[data-state="ready"]');
  return card && (!hasLayout() || card.querySelector(".chart-analytic-canvas svg [aria-roledescription='axis']")) ? card : null;
};

/** The same report read in a research session, a scheduled run and a Lens section. */
async function readingColumnFixture(bridge: PreviewBridge) {
  await revenueChartFixture(bridge);
  asSession(bridge);
  const ko = previewKo();
  const quarter = ko ? "보고 분기" : "Quarter";
  const sales = ko ? "매출" : "Sales";
  const orders = ko ? "해외 신규 수주액 (백만 원)" : "New overseas manufacturing orders (millions)";
  const firstId = `ev_${"a".repeat(32)}`, secondId = `ev_${"b".repeat(32)}`;
  const report = [
    ko ? "## 생산 설비와 매출" : "## Capacity and revenue",
    (ko ? "신규 수주와 설비 가동률을 함께 보면 매출 증가의 배경을 이해할 수 있습니다. 두 보고서 모두 같은 방향을 가리킵니다."
      : "New orders and capacity utilization help explain why sales rose. Both quarterly reports point in the same direction.") +
      ` [13](#evidence-${firstId}) [14](#evidence-${secondId})`,
    `| ${quarter} | ${sales} | % |\n| --- | ---: | ---: |\n| Q1 | 250 | 3 |\n| Q2 | 280 | 6 |`,
    ko ? "- 신규 수주가 늘었습니다.\n- 가동률이 올랐습니다." : "- New orders increased.\n- Utilization rose.",
    `| ${quarter} | ${Array.from({ length: 7 }, (_, n) => `${orders} FY${2025 + Math.floor(n / 4)} Q${n % 4 + 1}`).join(" | ")} |\n| --- | ${Array(7).fill("---:").join(" | ")} |\n| Q1 | 210 | 240 | 270 | 310 | 345 | 380 | 420 |`,
  ].join("\n\n");
  const selectWorkspace = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async id => {
    const workspace = await selectWorkspace(id);
    const answer = workspace.threads[0]?.messages.find(message => message.id === "preview-answer");
    if (answer?.evidence_refs?.[0]) {
      answer.content = report;
      answer.evidence_refs.push({ ...answer.evidence_refs[0], id: secondId, title: ko ? "설비 가동률 보고서" : "Capacity utilization report" });
    }
    return workspace;
  };
  const readPage = bridge.knowledgeLensRead.bind(bridge);
  bridge.knowledgeLensRead = async request => {
    const page = await readPage(request);
    if (page.body.kind === "subject" && page.body.sections[0]) page.body.sections[0].markdown = report;
    return page;
  };
}

/** Reading long threads: dividers, outlines, tables, charts, quotes and chips. */
export const chatReadingScenarios: PreviewScenario[] = [
  { ...base("answer-reading-column", "A research answer with two tables, a chart and consecutive citations",
    () => select('.message.assistant .message-content table') && select('.chat-artifact-block[data-state="ready"]')), setup: readingColumnFixture },
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
    asSession(bridge);
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
  { ...base("chat-outline", "A long answer's outline opens from its first line", () => select(".answer-outline [data-level]"), async () => {
    await centerOn("[data-answer-outline]", "start")();
    const trigger = await until(() => select("[data-answer-outline]"), "Outline trigger did not render") as HTMLElement;
    trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
    trigger.click();
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const ko = previewKo();
    const sections = ko
      ? ["## 매출", "매출은 11% 늘어 1,842억 원입니다.", "## 영업이익률", "영업이익률은 16.2%에서 14.8%로 낮아졌습니다.", "### 인증 비용", "하락의 대부분은 새 생산라인의 인증 비용입니다.", "## 다음에 볼 것", "가동률과 재고일수를 비교해 보십시오."]
      : ["## Revenue", "Revenue grew 11% to ₩184.2 billion.", "## Operating margin", "The operating margin fell from 16.2% to 14.8%.", "### Qualification costs", "Most of the decline came from qualification costs at the new line.", "## What to watch next", "Compare utilization against inventory days."];
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const answer = workspace.threads.find(thread => thread.id === "preview-chat")?.messages.find(message => message.id === "preview-answer");
      if (answer) answer.content = sections.join("\n\n");
      return workspace;
    };
    asSession(bridge);
  } },
  { ...base("chat-table", "A wide answer table keeps its figures whole", () => select('.message-content [data-streamdown="table"]'), async () => {
    await centerOn('.message-content [data-streamdown="table"]', "center")();
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const table = [
      "Segment results, ₩ billions.", "",
      "| Segment | Q3 2025 | Q3 2026 | Change | Operating margin | Inventory days | Note |",
      "| :--- | ---: | ---: | ---: | ---: | ---: | :--- |",
      "| Precision parts | 1,204.5 | 1,336.8 | +11.0% | 14.8% | 54 | New production line is still in qualification, so costs run ahead of volume. |",
      "| Tooling | 312.0 | 298.4 | -4.4% | 9.1% | 71 | Two customers delayed orders into next year. |",
      "| Services | 88.2 | 101.7 | +15.3% | 22.6% | 12 | Recurring contracts. |",
    ].join("\n");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const answer = workspace.threads.find(thread => thread.id === "preview-chat")?.messages.find(message => message.id === "preview-answer");
      if (answer) answer.content = table;
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
  // A company's revenue by year as a model publishes it: years as numbers, no semantic types, won in the column id.
  { ...base("chat-chart-revenue", "A revenue chart counts in 조 or tn and keeps its years",
    revenueChartReady, centerOn('.chat-artifact-block[data-state="ready"]')), setup: revenueChartFixture },
  { ...base("chat-chart-grouped", "Company revenue compares two fiscal years side by side",
    revenueChartReady, centerOn('.chat-artifact-block[data-state="ready"]')), setup: groupedRevenueFixture },
  { ...base("chat-chart-stacked", "Revenue segments stack to each year's total",
    revenueChartReady, centerOn('.chat-artifact-block[data-state="ready"]')), setup: stackedRevenueFixture },
  // W16b-003: a year of daily prices gets as many date labels as the chart's width holds, none touching.
  { ...base("chat-chart-daily", "A year of daily prices in the answer column, one date label a month",
    revenueChartReady, centerOn('.chat-artifact-block[data-state="ready"]')), setup: dailyPriceFixture },
  { ...base("chat-chart-daily-panel", "The same chart in the side panel, one date label a quarter", () => {
    const panel = select(".workspace-panel");
    return panel?.querySelector(hasLayout() ? ".chart-analytic-canvas svg [aria-roledescription='axis']" : ".artifact-chart-shell") ? panel : null;
  }, async () => {
    await until(revenueChartReady, "The chart card did not draw");
    await click('.chat-artifact-block[data-state="ready"] .open-in-panel');
  }), setup: dailyPriceFixture },
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
  // The footer's Sources (n) open: numbered rows with each site's icon, kind and date.
  { ...base("chat-sources-open", "The answer's numbered sources, open under it", () => select(".message-sources[open] ol li"), async root => {
    await until(() => root.querySelector("article.message.assistant.complete"), "Answer did not load");
    await click(".message-sources > summary", root);
    await until(() => select(".message-sources[open] ol li"), "The sources did not open");
    if (hasLayout()) select(".message-sources")?.scrollIntoView({ block: "center" });
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const answer = workspace.threads.find(thread => thread.id === "preview-chat")?.messages.find(message => message.id === "preview-answer");
      const [cited] = answer?.evidence_refs ?? [];
      if (answer && cited) {
        const survey = { ...cited, id: `ev_${"d".repeat(32)}`, title: previewKo() ? "모로 컴포넌츠 공급 능력 조사" : "Morrow Components supplier capacity survey", url: "https://morrow.example/capacity-survey", provider: "Morrow Components IR", published_at: "2026-10-20", content_sha256: "d".repeat(64) };
        answer.evidence_refs = [cited, survey];
        answer.content += ` [2](#evidence-${survey.id})`;
      }
      return workspace;
    };
    asSession(bridge);
  } },
];
