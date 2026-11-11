/**
 * Grid answers (finance-grid-answers.md §8) and the conflict card with the
 * Map's "Explain the disagreement" (finance-korea-retail-disagreement.md §8).
 * Fictional companies: Solsup Foods (990221), Iseul Logistics (990552) and
 * Hanbit Chem (990777). Korean previews use 억 amounts.
 */
import type { AssetsGrid, CalcInputUnit, ChatArtifactView, ChatEvidenceRef, ChatMessage, ConflictGrid, GridCell, GridDocument, LoupistArtifactItem, NumberCheck } from "../types";
import { GRID_SCHEMA } from "../contracts/chat";
import { prepareFixture, previewKo, type PreviewBridge } from "./fixtures";
import { prepareConflictsFixture } from "./fixtures/conflicts";
import type { PreviewScenario } from "./scenarios";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string, root?: ParentNode) => Element | null;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
  click: (selector: string, root?: ParentNode) => Promise<void>;
  selectPanel: (kind: "lens" | "world" | "materials") => Promise<void>;
  hasLayout: () => boolean;
  submitPrompt: (root: HTMLElement, prompt?: string) => Promise<void>;
  navigate: (root: HTMLElement, tab: "map" | "schedule" | "settings") => Promise<void>;
};

const ko = () => previewKo();
const RETRIEVED = "2026-03-20T06:00:00Z";
const GRID_ID = "grid-trio";
const CONFLICT_ID = "conflict-solsup";

type Company = { id: string; name: () => string; symbol: string; filed: string; revenue: number; income: number; net: number; basis: "CFS" | "OFS" };
const COMPANIES: Company[] = [
  { id: "solsup", name: () => (ko() ? "솔숲식품" : "Solsup Foods"), symbol: "990221", filed: "2026-03-18", revenue: 452_300_000_000, income: 38_100_000_000, net: 21_400_000_000, basis: "CFS" },
  { id: "iseul", name: () => (ko() ? "이슬물류" : "Iseul Logistics"), symbol: "990552", filed: "2026-03-20", revenue: 128_400_000_000, income: 7_600_000_000, net: 4_900_000_000, basis: "OFS" },
  { id: "hanbit", name: () => (ko() ? "한빛화학" : "Hanbit Chem"), symbol: "990777", filed: "2026-03-19", revenue: 286_500_000_000, income: 31_400_000_000, net: 19_800_000_000, basis: "CFS" },
];

const statements = (company: Company): ChatEvidenceRef => ({
  id: `ev_${company.id}_statements`, title: ko() ? `${company.name()} FY2025 재무제표` : `${company.name()} FY2025 statements`, kind: "original", provider: "OpenDART",
  tool_name: "dart_financials", retrieved_at: RETRIEVED, published_at: company.filed,
  excerpt: JSON.stringify({ facts: [
    { corp_name: company.name(), canonical_concept: "revenue", period: "FY2025", fs_div: company.basis, value: String(company.revenue), unit: "KRW" },
    { corp_name: company.name(), canonical_concept: "operating_income", period: "FY2025", fs_div: company.basis, value: String(company.income), unit: "KRW" },
    { corp_name: company.name(), canonical_concept: "net_income", period: "FY2025", fs_div: company.basis, value: String(company.net), unit: "KRW" },
  ] }, null, 2),
  excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [],
  dataset_ref: { dataset_id: `ds_${company.id}`, revision: 1, digest: "d".repeat(64) },
});
const calcEvidence = (company: Company, measure: string): ChatEvidenceRef => ({
  id: `ev_${company.id}_${measure}`, title: "Margin", kind: "derived", tool_name: "calculate", retrieved_at: RETRIEVED, excerpt: "",
  excerpt_truncated: false, content_sha256: "f".repeat(64), warnings: [], upstream_evidence_ids: [`ev_${company.id}_statements`],
});

/** "₩452.3bn" or "4,523억 원". */
const amount = (value: number) => ko()
  ? `${Math.round(value / 1e8).toLocaleString("en-US")}억 원`
  : `₩${(value / 1e9).toFixed(1)}bn`;
const billions = (value: number) => (value / 1e9).toFixed(1);
/** What Rust ships with an input of a calculation whose `unit` was "KRW billions". */
const KRW_BILLIONS: CalcInputUnit = { kind: "amount", currency: "KRW", scale: 9 };

function dataCheck(company: Company, text: string, concept: string, value: number, row: number, index: number): NumberCheck {
  return {
    link_index: index, text, evidence_id: `ev_${company.id}_statements`, kind: "data", status: "rounded", exact: String(value), pointer: `/facts/${row}/value`,
    fields: [{ key: "corp_name", value: company.name() }, { key: "canonical_concept", value: concept }, { key: "period", value: "FY2025" }, { key: "value", value: String(value), matched: true }],
  };
}

function marginCheck(company: Company, measure: "operating" | "net", text: string, index: number, differs = false): NumberCheck {
  const profit = measure === "operating" ? company.income : company.net;
  const exact = (profit / company.revenue) * 100;
  return {
    link_index: index, text, evidence_id: `ev_${company.id}_${measure}`, kind: "calculated", status: differs ? "differs" : "matched",
    ...(differs ? { nearest: `${exact.toFixed(2)}%` } : {}),
    calc: { title: "Margin", formulas: [`${measure === "operating" ? "Operating income" : "Net income"} / Revenue = ${billions(profit)} / ${billions(company.revenue)} = ${exact.toFixed(2)}%`], inputs: [
      { name: "profit", label: measure === "operating" ? "Operating income" : "Net income", unit: KRW_BILLIONS, display: billions(profit), evidence_id: `ev_${company.id}_statements`, status: "rounded" },
      { name: "revenue", label: "Revenue", unit: KRW_BILLIONS, display: billions(company.revenue), evidence_id: `ev_${company.id}_statements`, status: "rounded" },
    ] },
  };
}

type Variant = "card" | "empty" | "wide" | "differs" | "asof";
const AS_OF = "2026-04-01";

function gridFixture(variant: Variant): { title: string; grid: AssetsGrid; evidence: ChatEvidenceRef[] } {
  const evidence: ChatEvidenceRef[] = [];
  const cells: GridCell[][] = COMPANIES.map((company, row) => {
    evidence.push(statements(company), calcEvidence(company, "operating"), calcEvidence(company, "net"));
    const index = (column: number) => row * 8 + column;
    const revenue = amount(company.revenue);
    const operating = `${((company.income / company.revenue) * 100).toFixed(2)}%`;
    const net = `${((company.net / company.revenue) * 100).toFixed(2)}%`;
    const shown = variant === "differs" && company.id === "hanbit" ? "11.30%" : operating;
    const value = (text: string, check: NumberCheck, sort: number): GridCell => ({ value: text, evidence_id: check.evidence_id, check, sort });
    const empty = (reason: "not_reported" | "unavailable" | "not_applicable" | "not_checked", note?: string): GridCell => ({ empty: reason, ...(note ? { note } : {}) });
    const multiple = company.id === "solsup" ? 1.2 : 0.4;
    const multipleText = ko() ? `${multiple}배` : `${multiple}×`;
    const debtCheck: NumberCheck = { ...dataCheck(company, multipleText, "net_debt_to_ebitda", multiple, 3, index(3)), status: "matched" };
    delete debtCheck.exact;
    const debt: GridCell = company.id === "iseul"
      ? empty("not_reported", ko() ? "별도 재무제표만 있어 순부채를 계산하지 않았어요." : "Separate statements only; no consolidated net debt.")
      : value(multipleText, debtCheck, multiple);
    const base: GridCell[] = [
      value(revenue, dataCheck(company, revenue, "revenue", company.revenue, 0, index(0)), company.revenue),
      value(shown, marginCheck(company, "operating", shown, index(1), shown !== operating), (company.income / company.revenue) * 100),
      variant === "empty" && company.id === "hanbit"
        ? empty("unavailable", ko() ? "한빛화학의 순이익을 가져오지 못했어요." : "Hanbit Chem's net income could not be read.")
        : value(net, marginCheck(company, "net", net, index(2)), (company.net / company.revenue) * 100),
      debt,
    ];
    if (variant !== "wide") return base;
    // Eight measures: the table scrolls sideways inside the card.
    return [...base,
      empty("not_checked"),
      value(revenue, dataCheck(company, revenue, "revenue", company.revenue, 0, index(5)), company.revenue),
      empty("not_applicable", ko() ? "배당을 하지 않아요." : "Pays no dividend."),
      value(operating, marginCheck(company, "operating", operating, index(7)), (company.income / company.revenue) * 100),
    ];
  });
  const columns: AssetsGrid["columns"] = [
    { id: "revenue", label: ko() ? "매출" : "Revenue", period: "FY2025", unit: "amount" },
    { id: "operating_margin", label: ko() ? "영업이익률" : "Operating margin", period: "FY2025", unit: "percent" },
    { id: "net_margin", label: ko() ? "순이익률" : "Net margin", period: "FY2025", unit: "percent" },
    { id: "net_debt_ebitda", label: ko() ? "순부채/EBITDA" : "Net debt/EBITDA", period: "FY2025", unit: "multiple" },
  ];
  if (variant === "wide") {
    columns.push(
      { id: "payout", label: ko() ? "배당성향" : "Payout (earnings)", period: "FY2025", unit: "percent" },
      { id: "revenue_prior", label: ko() ? "매출 (재확인)" : "Revenue (checked)", period: "FY2025", unit: "amount" },
      { id: "dividend_yield", label: ko() ? "배당수익률" : "Dividend yield", period: ko() ? "10월 31일 기준" : "as of Oct 31", unit: "percent" },
      { id: "operating_margin_again", label: ko() ? "영업이익률 (재확인)" : "Operating margin (checked)", period: "FY2025", unit: "percent" },
    );
  }
  return {
    title: ko() ? "솔숲식품·이슬물류·한빛화학 FY2025 비교" : "Solsup, Iseul and Hanbit: FY2025 margins",
    grid: {
      layout: "assets",
      subtitle: ko() ? "별도 표시가 없으면 연결 기준 · 각 회사 통화" : "Consolidated unless noted · values in each company's own currency",
      notes: [ko() ? "이슬물류는 별도 재무제표만 공시해요." : "Iseul Logistics files separate statements only."],
      rows: COMPANIES.map((company) => ({ symbol: company.symbol, market: "KRX", label: company.name(), asset_id: `ast_${company.id.padEnd(16, "0").slice(0, 16)}` })),
      columns,
      cells,
      ...(variant === "asof" ? { as_of: AS_OF } : {}),
    },
    evidence,
  };
}

function conflictFixture(mapLink: boolean): { title: string; grid: ConflictGrid; evidence: ChatEvidenceRef[] } {
  const solsup = COMPANIES[0]!;
  const filed = statements(solsup);
  const news: ChatEvidenceRef = {
    id: "ev_solsup_news", title: ko() ? "한바다일보 잠정 실적 기사" : "News report, Hanbada Daily", kind: "original", url: "https://fictional.example/markets/solsup-foods-preliminary-fy2025",
    tool_name: "web_fetch", retrieved_at: RETRIEVED, published_at: "2026-02-05",
    excerpt: ko() ? "솔숲식품은 FY2025 잠정 매출을 4,550억 원으로 발표했다." : "Solsup Foods reported preliminary FY2025 revenue of ₩455.0bn.",
    excerpt_truncated: false, content_sha256: "e".repeat(64), warnings: [],
  };
  const broker: ChatEvidenceRef = {
    id: "ev_solsup_broker", title: ko() ? "한 증권사의 추정치" : "One broker's estimate", kind: "original", url: "https://fictional.example/research/solsup-estimate",
    tool_name: "web_fetch", retrieved_at: RETRIEVED, published_at: "2026-01-20",
    excerpt: ko() ? "FY2025 매출 추정치 4,490억 원." : "FY2025 revenue estimate: ₩449.0bn.",
    excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [],
  };
  const says = (value: number) => amount(value);
  const span = (excerpt: string, needle: string) => {
    const at = excerpt.indexOf(needle);
    const encoder = new TextEncoder();
    const start = encoder.encode(excerpt.slice(0, at)).length;
    return { start, end: start + encoder.encode(needle).length };
  };
  const passage = (evidence: ChatEvidenceRef, text: string, index: number): NumberCheck => ({ link_index: index, text, evidence_id: evidence.id, kind: "passage", status: "matched", span: span(evidence.excerpt, text) });
  const filedText = says(solsup.revenue);
  const newsText = says(455_000_000_000);
  const brokerText = says(449_000_000_000);
  return {
    title: ko() ? "솔숲식품 FY2025 매출" : "Solsup Foods FY2025 revenue",
    grid: {
      layout: "conflict",
      measure: { label: ko() ? "매출" : "revenue", entity_label: solsup.name(), period: "FY2025" },
      reports: [
        { evidence_id: filed.id, says: filedText, basis: ko() ? "연결, 확정" : "Consolidated, final", dated: solsup.filed, check: dataCheck(solsup, filedText, "revenue", solsup.revenue, 0, 0) },
        { evidence_id: news.id, says: newsText, basis: ko() ? "잠정 실적" : "Preliminary", dated: "2026-02-05", check: passage(news, newsText, 1) },
        { evidence_id: broker.id, says: brokerText, basis: ko() ? "실적 발표 전 추정" : "Estimate before results", dated: "2026-01-20", check: passage(broker, brokerText, 2) },
      ],
      reason: "preliminary_vs_final",
      explanation: ko()
        ? "기사는 3월 공시 전에 나온 잠정 수치를 전했고, 공시는 일회성 반품 충당금을 반영한 확정 수치예요."
        : "The news report carried the preliminary figure from February; the March filing is final and books a one-off returns allowance.",
      used: 0,
      used_reason: ko() ? "Quality Compounder는 공시된 확정 수치인 FY2025 재무제표 값을 썼어요." : "Quality Compounder uses the FY2025 statements because they are the filed figure.",
      ...(mapLink ? { world_entity_id: "hanul", map_link: true } : {}),
    },
    evidence: [filed, news, broker],
  };
}

const view = (id: string, title: string, grid: GridDocument, evidence: ChatEvidenceRef[]): ChatArtifactView => ({
  artifact: { id, chat_session_id: "preview-chat", kind: "grid", title, current_revision: 1, created_at_ms: Date.parse(RETRIEVED), updated_at_ms: Date.parse(RETRIEVED) },
  revision: { artifact_id: id, revision: 1, payload: { kind: "grid", schema: GRID_SCHEMA, grid }, digest: "b".repeat(64), source_message_id: "grid-answer", created_at_ms: Date.parse(RETRIEVED) },
  evidence_refs: evidence,
  chart_dataset: null,
});

function answer(id: string, title: string, content: string, asOf?: string): ChatMessage {
  const at = Date.parse(RETRIEVED);
  return {
    id: "grid-answer", role: "assistant", status: "complete", created_at: "2026-03-20T06:01:00Z", content, ...(asOf ? { as_of: asOf } : {}),
    artifact_refs: [{ artifact_id: id, revision: 1, kind: "grid", title, digest: "b".repeat(64) }],
    progress: { startedAtMs: at, finishedAtMs: at + 38_000, items: [
      { id: "step-statements", kind: "tool", toolName: "dart_financials", category: "finance", operation: "read", action: "Read financial statements", target: COMPANIES.map((company) => company.name()).join(" · "), status: "succeeded", startedAtMs: at, finishedAtMs: at + 900, href: null },
      { id: "step-grid", kind: "tool", toolName: "grid_publish", category: "artifact", operation: "publish", action: "Built a table", running: "Building a table", target: title, status: "succeeded", startedAtMs: at + 30_000, finishedAtMs: at + 31_000, href: null },
    ] },
    execution_model: { profile_id: "openai/gpt-6.1-sol", name: "GPT-6.1 Sol", provider: "openai", model: "gpt-6.1-sol", thinking_level: "medium", run_options: {} },
  };
}

const question = (text: string, asOf?: string): ChatMessage => ({ id: "grid-question", role: "user", status: "complete", created_at: "2026-03-20T06:00:00Z", content: text, ...(asOf ? { as_of: asOf } : {}) });

/** The home Thread holds one question and its answer; the artifact read returns the stored grid. */
function install(bridge: PreviewBridge, messages: () => ChatMessage[], artifacts: () => ChatArtifactView[]) {
  const select = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async (loupistId) => {
    const workspace = await select(loupistId);
    const home = workspace.threads.find((thread) => thread.home) ?? workspace.threads[0];
    if (home) home.messages = messages();
    return workspace;
  };
  const read = bridge.chatArtifactRead.bind(bridge);
  bridge.chatArtifactRead = async (loupistId, threadId, artifactId, revisionId) => structuredClone(artifacts().find((item) => item.artifact.id === artifactId) ?? await read(loupistId, threadId, artifactId, revisionId));
  const list = bridge.loupistArtifactList.bind(bridge);
  bridge.loupistArtifactList = async (request) => [
    ...artifacts().map((item): LoupistArtifactItem => ({ artifact: item.artifact, chat_title: ko() ? "배당주 비교" : "Comparing dividend payers" })),
    ...await list(request),
  ];
}

const gridAnswerText = () => ko()
  ? "세 회사를 FY2025 기준으로 나란히 놓았어요. 한빛화학의 이익률이 가장 높고, 이슬물류는 별도 재무제표만 있어 순부채는 비워 뒀어요."
  : "Here are the three side by side for FY2025. Hanbit Chem has the widest margins; Iseul Logistics files separate statements only, so its net debt stays empty.";
const gridQuestion = () => (ko() ? "솔숲식품, 이슬물류, 한빛화학의 FY2025 매출과 이익률을 표로 비교해 줘." : "Compare Solsup Foods, Iseul Logistics and Hanbit Chem on FY2025 revenue and margins in a table.");

async function prepareGrid(bridge: PreviewBridge, variant: Variant) {
  await prepareFixture(bridge, "answer");
  const fixture = () => gridFixture(variant);
  const asOf = variant === "asof" ? AS_OF : undefined;
  install(bridge, () => [question(gridQuestion(), asOf), answer(GRID_ID, fixture().title, gridAnswerText(), asOf)], () => {
    const { title, grid, evidence } = fixture();
    return [view(GRID_ID, title, grid, evidence)];
  });
}

async function prepareConflict(bridge: PreviewBridge, mapLink: boolean) {
  await prepareFixture(bridge, "answer");
  const fixture = () => conflictFixture(mapLink);
  const text = () => ko()
    ? "출처마다 솔숲식품의 FY2025 매출이 달라요. 공시된 확정 수치인 4,523억 원을 기준으로 답했어요."
    : "Sources give different FY2025 revenue for Solsup Foods. This answer uses the filed figure.";
  install(bridge, () => [question(ko() ? "솔숲식품 FY2025 매출이 기사마다 다르던데 뭐가 맞아?" : "Why do I see different FY2025 revenue for Solsup Foods?"), answer(CONFLICT_ID, fixture().title, text())], () => {
    const { title, grid, evidence } = fixture();
    return [view(CONFLICT_ID, title, grid, evidence)];
  });
}

export function gridScenarios({ base, select, until, click, selectPanel, hasLayout, submitPrompt, navigate }: Helpers): PreviewScenario[] {
  const reveal = (element: Element | null) => { if (element && hasLayout()) element.scrollIntoView({ block: "center" }); };
  const cardLoaded = async (root: HTMLElement, selector = '.grid-card[data-layout="assets"]') => {
    const card = await until(() => select(selector, root), "The grid card did not load");
    reveal(card);
    return card;
  };
  const gridScenario = (id: string, title: string, landmark: string, variant: Variant, enter: (root: HTMLElement) => Promise<void> = async (root) => { await cardLoaded(root); }): PreviewScenario => ({
    ...base(id, title, () => select(landmark), enter),
    setup: (bridge) => prepareGrid(bridge, variant),
  });
  /** Radix menus open on pointer down, as a mouse press does. */
  const press = async (selector: string, root: ParentNode) => {
    const trigger = await until(() => select(selector, root), `Control not visible: ${selector}`);
    // jsdom has no PointerEvent; a mouse event of the same type reaches React's pointer handler.
    const init = { bubbles: true, cancelable: true, button: 0 };
    trigger.dispatchEvent(typeof PointerEvent === "function" ? new PointerEvent("pointerdown", { ...init, pointerType: "mouse" }) : new MouseEvent("pointerdown", init));
  };
  return [
    gridScenario("chat-grid-card", "A grid answer: every value links its source or formula", '.grid-card[data-layout="assets"] .number-chip', "card"),
    { ...base("chat-grid-building", "A table reserves its card while it is built", () => select('.grid-card-reserved[data-state="building"]'), async (root) => {
      await submitPrompt(root, gridQuestion());
      reveal(await until(() => select('.grid-card-reserved[data-state="building"]'), "The reserved table did not appear"));
    }), setup: async (bridge) => {
      await prepareFixture(bridge, "streaming");
      bridge.chatSend = async (request, observer) => {
        const run_id = request.run_id;
        observer({ type: "started", run_id });
        const title = gridFixture("card").title;
        observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: Date.now() - 20_000, items: [
          { id: "step-statements", kind: "tool", toolName: "dart_financials", category: "finance", operation: "read", action: "Read financial statements", target: COMPANIES.map((company) => company.name()).join(" · "), status: "succeeded", href: null, startedAtMs: 0, finishedAtMs: null },
          { id: "step-grid", kind: "tool", toolName: "grid_publish", category: "artifact", operation: "publish", action: "Built a table", running: "Building a table", target: title, status: "running", href: null, startedAtMs: 0, finishedAtMs: null },
        ], finishedAtMs: null } });
        return { run_id };
      };
    } },
    gridScenario("chat-grid-empty-cells", "Empty cells say why, with the run's note", ".grid-card-note", "empty", async (root) => {
      const card = await cardLoaded(root);
      const cell = await until(() => card.querySelector<HTMLButtonElement>('button.grid-card-empty[data-empty="unavailable"]'), "The empty cell did not render");
      cell.focus();
    }),
    gridScenario("chat-grid-wide-scroll", "Eight measures scroll inside the card", ".grid-card-table thead th:nth-child(9)", "wide"),
    gridScenario("chat-grid-differs", "A cell that differs from its calculation is marked", '.number-chip-popover .number-chip-status[data-status="differs"]', "differs", async (root) => {
      await cardLoaded(root);
      await click(".grid-card .grid-card-check", root);
    }),
    gridScenario("chat-grid-export-menu", "Export: CSV with sources or a Markdown table", ".grid-card-menu[role='menu']", "card", async (root) => {
      const card = await cardLoaded(root);
      // At 700 px the card's actions fold into ⋯, where Export lives.
      await press(select("[data-grid-export]", card) ? "[data-grid-export]" : "[data-grid-more]", card);
    }),
    gridScenario("chat-grid-asof", "An as-of grid: Known as of in the caption and each popover", '.number-chip-popover .number-chip-status[data-status="as_of"]', "asof", async (root) => {
      const card = await cardLoaded(root);
      const chip = await until(() => card.querySelector<HTMLButtonElement>("button.number-chip"), "No number chip in the grid");
      chip.click();
    }),
    { ...base("panel-work-grid", "A grid opened from Work shows the same card", () => select(".workspace-grid-view .grid-card"), async (root) => {
      await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
      await selectPanel("materials");
      await click("#workspace-tab-work");
      await click(".work-list button.work-row");
    }), setup: (bridge) => prepareGrid(bridge, "card") },
    { ...base("chat-conflict-card", "Where sources disagree, linked to the Map", () => select(".conflict-card [data-conflict-map]"), async (root) => {
      await cardLoaded(root, ".conflict-card");
    }), setup: (bridge) => prepareConflict(bridge, true) },
    { ...base("chat-conflict-no-map", "A conflict the Map has not recorded", () => {
      const card = select(".conflict-card");
      return card && !card.querySelector("[data-conflict-map]") && card.querySelector(".conflict-card-used") ? card : null;
    }, async (root) => {
      await cardLoaded(root, ".conflict-card");
    }), setup: (bridge) => prepareConflict(bridge, false) },
    { ...base("map-inspector-explain-disagreement", "The inspector offers to explain a disagreement", () => select(".map-conflicts [data-explain-disagreement]"), async (root) => {
      await navigate(root, "map");
      const input = await until(() => select('.map-view [role="combobox"]'), "Map search missing") as HTMLInputElement;
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "Hanul");
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await click('[role="option"]');
      reveal(await until(() => select(".map-conflicts [data-explain-disagreement]"), "Explain the disagreement did not appear"));
    }), setup: prepareConflictsFixture },
  ];
}
