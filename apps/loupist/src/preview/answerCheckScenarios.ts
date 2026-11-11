/**
 * Number chips (finance-show-the-math.md §8) and as-of answers with the Journal
 * look-back (finance-point-in-time.md §8). Fictional company: Solsup Foods
 * (990221). Every scenario reaches its screen through the app's own controls;
 * Korean previews use 억/조 amounts.
 */
import i18n from "../i18n";
import type { AsOfReport, CalcInputUnit, ChatEvidenceRef, ChatMessage, ChatProgress, JournalRecord, NumberCheck } from "../types";
import { prepareFixture, previewKo, type PreviewBridge } from "./fixtures";
import type { PreviewScenario } from "./scenarios";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string, root?: ParentNode) => Element | null;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
  click: (selector: string, root?: ParentNode) => Promise<void>;
  clickMapAction: (selector: string, root?: ParentNode) => Promise<void>;
  hasLayout: () => boolean;
  submitPrompt: (root: HTMLElement, prompt?: string) => Promise<void>;
  navigate: (root: HTMLElement, tab: "map" | "schedule" | "settings") => Promise<void>;
};

const RETRIEVED = "2026-03-18T06:00:00Z";
const ko = () => previewKo();
/** Bundled Skills for everyday questions (finance-korea-retail-disagreement.md §3.2). */
const RETAIL_SKILLS = ["dividend-income", "fund-compare", "why-it-moved", "goal-math"] as const;

/** The `/` menu's rows for the retail Skills, found by their blurbs, in list order. */
function retailRows(assist: Element | null) {
  const blurbs = RETAIL_SKILLS.map(id => i18n.t(`chat:skillBlurb.${id}`));
  return [...assist?.querySelectorAll<HTMLElement>('[role="option"]') ?? []].filter(row => blurbs.includes(row.querySelector("small")?.textContent ?? ""));
}

/** A row the list shows whole, above its fading edge; without layout (tests), being listed is enough. */
function shownInList(row: HTMLElement, assist: Element) {
  const scroll = assist.querySelector(".composer-assist-scroll");
  if (!scroll || document.documentElement.clientWidth === 0) return true;
  const box = scroll.getBoundingClientRect();
  const fade = scroll.hasAttribute("data-more-below") ? 24 : 0;
  const rect = row.getBoundingClientRect();
  return rect.height > 0 && rect.top >= box.top - 1 && rect.bottom <= box.bottom - fade + 1;
}

/** UTF-8 byte offsets of `needle` in `text`, as Rust stores a passage match. */
function spanOf(text: string, needle: string) {
  const at = text.indexOf(needle);
  const encoder = new TextEncoder();
  const start = encoder.encode(text.slice(0, at)).length;
  return { start, end: start + encoder.encode(needle).length };
}

const DART: ChatEvidenceRef = {
  id: "ev_solsup_statements", title: "Solsup Foods FY2025 consolidated statements", kind: "original", provider: "OpenDART",
  tool_name: "dart_financials", retrieved_at: RETRIEVED, published_at: "2026-03-18",
  excerpt: JSON.stringify({ facts: [
    { corp_name: "Solsup Foods", canonical_concept: "revenue", period: "FY2025", fs_div: "CFS", value: "452312000000", unit: "KRW", scale: "1" },
    { corp_name: "Solsup Foods", canonical_concept: "operating_income", period: "FY2025", fs_div: "CFS", value: "38104000000", unit: "KRW", scale: "1" },
  ] }, null, 2),
  excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [],
  dataset_ref: { dataset_id: "ds_solsup_fy2025", revision: 1, digest: "d".repeat(64) },
};
const guidanceText = () => ko()
  ? "2월 실적 설명회에서 경영진은 FY2025 영업이익률을 9.1%로, 매출은 4,400억 원 안팎으로 제시했어요."
  : "On the February call, management guided to an operating margin of 9.1% for FY2025, on revenue near ₩440bn.";
const guidance = (): ChatEvidenceRef => ({
  id: "ev_solsup_guidance", title: ko() ? "2월 실적 설명회" : "February guidance call", kind: "original", url: "https://solsup.example/ir/guidance-2026-02",
  tool_name: "web_fetch", retrieved_at: RETRIEVED, published_at: "2026-02-12", excerpt: guidanceText(),
  excerpt_truncated: false, content_sha256: "e".repeat(64), warnings: [],
});
const calcEvidence = (id: string, title: string): ChatEvidenceRef => ({
  id, title, kind: "derived", tool_name: "calculate", retrieved_at: RETRIEVED, excerpt: "", excerpt_truncated: false,
  content_sha256: "f".repeat(64), warnings: [], upstream_evidence_ids: [DART.id],
});
const MARGIN = calcEvidence("ev_solsup_margin", "Margin");
const GROWTH = calcEvidence("ev_solsup_growth", "Percent change");
const REVERSE = calcEvidence("ev_solsup_reverse", "Reverse DCF grid");

/** The unit Rust ships with an input stated in won (the request's `unit` "KRW", or "KRW billions" for scale 9). */
const KRW = (scale = 0): CalcInputUnit => ({ kind: "amount", currency: "KRW", ...(scale ? { scale } : {}) });
const revenueText = () => (ko() ? "4,523억 원" : "₩452.3bn");
const incomeText = () => (ko() ? "381억 원" : "₩38.1bn");

const marginCheck = (text: string, status: NumberCheck["status"] = "matched", unlinked = false): NumberCheck => ({
  link_index: 0, text, evidence_id: MARGIN.id, kind: "calculated", status, ...(status === "differs" ? { nearest: "8.42%" } : {}),
  calc: { title: "Margin", formulas: ["Operating income / Revenue = 38.1 / 452.3 = 8.42%"], inputs: [
    { name: "profit", label: "Operating income", unit: KRW(9), display: "38.1", evidence_id: DART.id, status: "rounded" },
    unlinked ? { name: "revenue", label: "Revenue", unit: KRW(9), display: "452.3" }
      : { name: "revenue", label: "Revenue", unit: KRW(9), display: "452.3", evidence_id: DART.id, status: "rounded" },
  ] },
});
const dataCheck = (text: string, concept: string, value: string, row: number): NumberCheck => ({
  link_index: 1, text, evidence_id: DART.id, kind: "data", status: "rounded", exact: value, pointer: `/facts/${row}/value`,
  fields: [{ key: "corp_name", value: "Solsup Foods" }, { key: "canonical_concept", value: concept }, { key: "period", value: "FY2025" }, { key: "value", value, matched: true }],
});

/** Four finished steps: the statements, the guidance page and two calculations. */
function steps(extra: ChatProgress["items"] = []): ChatProgress {
  const at = Date.parse("2026-03-18T06:00:00Z");
  const step = (id: string, category: "finance" | "web" | "compute", operation: "read" | "calculate", action: string, resultIds: string[], target?: string) =>
    ({ id, kind: "tool" as const, category, operation, action, status: "succeeded" as const, resultIds, target: target ?? null, href: null, startedAtMs: at, finishedAtMs: at + 900 });
  return { startedAtMs: at, finishedAtMs: at + 41_000, items: [
    // Rust names the asset the Loupist has a page for, not the tool's symbol argument.
    step("step-statements", "finance", "read", "Read financial statements", [DART.id], `${ko() ? "솔숲식품" : "Solsup Foods"} · FY2025`),
    step("step-guidance", "web", "read", "Read a web source", ["ev_solsup_guidance"], "solsup.example"),
    step("step-margin", "compute", "calculate", "Calculated", [MARGIN.id]),
    step("step-growth", "compute", "calculate", "Calculated", [GROWTH.id]),
    ...extra,
  ] };
}

type Variant = "chips" | "differs" | "unlinked" | "grid";

function numberAnswer(variant: Variant): ChatMessage {
  const guide = guidance();
  const margin = variant === "differs" ? "8.5%" : "8.4%";
  const link = (text: string, evidence: ChatEvidenceRef) => `[${text}](#evidence-${evidence.id})`;
  const content = variant === "grid"
    ? ko()
      ? `현재 주가는 앞으로 5년간 연 ${link("4.1%", REVERSE)}~${link("7.8%", REVERSE)} 성장을 가정한 값이에요. WACC와 영구성장률에 따라 달라지는 범위예요.`
      : `Today’s price implies growth of ${link("4.1%", REVERSE)} to ${link("7.8%", REVERSE)} a year for five years, depending on the discount rate and terminal growth.`
    : ko()
      ? `솔숲식품의 FY2025 영업이익률은 ${link(margin, MARGIN)}였고, 매출은 ${link(revenueText(), DART)}, 영업이익은 ${link(incomeText(), DART)}이었어요. 매출은 전년보다 ${link("6.2%", GROWTH)} 늘었어요.\n\n2월에 경영진이 제시한 ${link("9.1%", guide)}보다는 낮았어요 [1](#evidence-${guide.id}).`
      : `Solsup Foods’ FY2025 operating margin was ${link(margin, MARGIN)}, on revenue of ${link(revenueText(), DART)} and operating income of ${link(incomeText(), DART)}. Revenue grew ${link("6.2%", GROWTH)} from FY2024.\n\nMargins were thinner than the ${link("9.1%", guide)} management guided in February [1](#evidence-${guide.id}).`;
  const checks: NumberCheck[] = variant === "grid" ? ["4.1%", "7.8%"].map((text, index) => ({
    link_index: index, text, evidence_id: REVERSE.id, kind: "calculated", status: "matched",
    calc: { title: "Reverse DCF grid", formulas: [], grid: [3, 3], inputs: [
      { name: "cash_flow", label: "cash flow", unit: KRW(), display: "24,500,000,000", evidence_id: DART.id, status: "matched" },
      { name: "price", label: "price", unit: { kind: "price", currency: "KRW" }, display: "38,200" },
      { name: "shares", label: "shares", unit: { kind: "number" }, display: "34,560,000", evidence_id: DART.id, status: "matched" },
    ] },
  })) : [
    marginCheck(margin, variant === "differs" ? "differs" : "matched", variant === "unlinked"),
    dataCheck(revenueText(), "revenue", "452312000000", 0),
    dataCheck(incomeText(), "operating_income", "38104000000", 1),
    { link_index: 3, text: "6.2%", evidence_id: GROWTH.id, kind: "calculated", status: "matched",
      calc: { title: "Percent change", formulas: ["revenue FY2025 − revenue FY2024 = 452,312,000,000 − 425,900,000,000 = 26,412,000,000", "(revenue FY2025 − revenue FY2024) / revenue FY2024 = 26,412,000,000 / 425,900,000,000 = 6.20%"],
        inputs: [{ name: "from", label: "revenue FY2024", unit: KRW(), display: "425,900,000,000", evidence_id: DART.id, status: "matched" },
          { name: "to", label: "revenue FY2025", unit: KRW(), display: "452,312,000,000", evidence_id: DART.id, status: "matched" }] } },
    { link_index: 4, text: "9.1%", evidence_id: guide.id, kind: "passage", status: "matched", span: spanOf(guide.excerpt, "9.1%") },
  ];
  return {
    id: "solsup-answer", role: "assistant", status: "complete", created_at: "2026-03-18T06:01:00Z", content,
    evidence_refs: variant === "grid" ? [DART, REVERSE] : [DART, guide, MARGIN, GROWTH],
    number_checks: checks,
    progress: variant === "grid" ? { ...steps(), items: [steps().items[0]!, { id: "step-reverse", kind: "tool", toolName: "calculate", category: "compute", operation: "calculate", action: "Calculated", status: "succeeded", resultIds: [REVERSE.id], startedAtMs: Date.parse(RETRIEVED), finishedAtMs: Date.parse(RETRIEVED) + 900, target: null, href: null }] } : steps(),
    execution_model: { profile_id: "openai/gpt-6.1-sol", name: "GPT-6.1 Sol", provider: "openai", model: "gpt-6.1-sol", thinking_level: "medium", run_options: {} },
  };
}

const question = (text: string, asOf?: string): ChatMessage => ({ id: "solsup-question", role: "user", status: "complete", created_at: "2026-03-18T06:00:00Z", content: text, ...(asOf ? { as_of: asOf } : {}) });

/** The home Thread holds one question and its answer. */
function installThread(bridge: PreviewBridge, messages: () => ChatMessage[]) {
  const select = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async loupistId => {
    const workspace = await select(loupistId);
    const home = workspace.threads.find(thread => thread.home) ?? workspace.threads[0];
    if (home) {
      home.messages = messages();
      home.title = ko() ? "솔숲식품 FY2025 이익률" : "Solsup Foods’ FY2025 margin";
    }
    return workspace;
  };
}

const AS_OF = "2026-03-03";

function asOfAnswer(nothing: boolean): ChatMessage {
  const guide = guidance();
  const report: AsOfReport = nothing
    ? { cutoff: AS_OF, refused: [{ tool: "kis_read" }, { tool: "dart_company_lookup" }], empty: [{ tool: "krx_daily_quote", target: "990221" }, { tool: "dart_financials", target: "990221" }], left_out: { facts: 0, sources: 0, notes: 0, methods: 0 } }
    : { cutoff: AS_OF, refused: [{ tool: "kis_read" }], empty: [{ tool: "krx_daily_quote", target: "990221" }], left_out: { facts: 12, sources: 2, notes: 0, methods: 1 } };
  const at = Date.parse("2026-03-18T06:00:00Z");
  const content = nothing
    ? ko()
      ? "2026년 3월 3일 기준으로는 솔숲식품의 FY2025 매출을 알 수 없었어요. 실적 공시는 아직 나오지 않았고, 현재 시세만 알려 주는 자료는 그날 기준으로 볼 수 없어요."
      : "As of March 3, 2026, nothing yet showed Solsup Foods’ FY2025 revenue: the results had not been filed, and the sources that report only current values cannot show that day."
    : ko()
      ? `3월 3일까지 솔숲식품은 FY2025 실적을 공시하지 않았어요. 2월 실적 설명회는 매출을 [4,400억 원](#evidence-${guide.id}) 안팎으로 제시했어요 [1](#evidence-${guide.id}).`
      : `Solsup had not filed FY2025 results by Mar 3. The February guidance pointed to [₩440bn](#evidence-${guide.id}) of revenue [1](#evidence-${guide.id}).`;
  const guideAmount = ko() ? "4,400억 원" : "₩440bn";
  return {
    id: "solsup-asof-answer", role: "assistant", status: "complete", created_at: "2026-03-18T06:01:00Z", content, as_of: AS_OF, as_of_report: report,
    evidence_refs: nothing ? [] : [guide],
    number_checks: nothing ? [] : [{ link_index: 0, text: guideAmount, evidence_id: guide.id, kind: "passage", status: "matched", span: spanOf(guide.excerpt, ko() ? "4,400억 원" : "₩440bn") }],
    progress: { startedAtMs: at, finishedAtMs: at + 41_000, items: [
      { id: "step-quote", kind: "tool", toolName: "kis_read", category: "finance", operation: "read", action: "Read market data", target: "990221", status: "skipped", startedAtMs: at, finishedAtMs: at + 400, href: null },
      ...(nothing ? [{ id: "step-company", kind: "tool" as const, toolName: "dart_company_lookup", category: "finance" as const, operation: "read" as const, action: "Looked up a company", target: "990221", href: null, status: "skipped" as const, startedAtMs: at, finishedAtMs: at + 400 }] : [
        { id: "step-guidance", kind: "tool" as const, toolName: "web_fetch", category: "web" as const, operation: "read" as const, action: "Read a web source", target: "solsup.example", href: null, status: "succeeded" as const, resultIds: [guide.id], startedAtMs: at, finishedAtMs: at + 900 },
      ]),
    ] },
    execution_model: { profile_id: "openai/gpt-6.1-sol", name: "GPT-6.1 Sol", provider: "openai", model: "gpt-6.1-sol", thinking_level: "medium", run_options: {} },
  };
}

/** Journal entries for Then · Now: one that came true (with a saved source), one overdue. */
function journalEntries(): JournalRecord[] {
  const created = Date.parse("2026-03-05T09:00:00+09:00");
  const base = { loupistId: "loupist-quality", researchId: "", originChatId: "preview-chat", entities: [], createdAtMs: created, archived: false, inputDigest: "preview", reasoning: "" };
  const sources = [
    { receiptId: "receipt-preview", recordId: "src-preview", revisionId: "rev-preview", spanId: "spn-preview", coverage: "included_full", attribution: ko() ? "4분기 미리보기 · solsup.example" : "Q4 preview · solsup.example", publicationTime: { precision: "date", value: "2026-02-27", approximate: false } },
    { receiptId: "receipt-call", recordId: "src-call", revisionId: "rev-call", spanId: "spn-call", coverage: "included_full", attribution: ko() ? "2월 실적 설명회" : "February guidance call", publicationTime: { precision: "date", value: "2026-02-12", approximate: false } },
  ];
  return [
    { ...base, journalId: "journal-solsup-beat", updatedAtMs: Date.parse("2026-04-02T09:00:00+09:00"),
      judgment: ko() ? "솔숲식품의 FY2025 매출은 4,450억 원을 넘을 거예요." : "Solsup's FY2025 revenue will beat ₩445bn.",
      check: { claim: ko() ? "매출 4,450억 원 초과" : "Revenue above ₩445bn", metric: ko() ? "매출" : "revenue", direction: "above", threshold: 445_000_000_000, unit: "KRW", byDate: "2026-03-31" },
      outcome: { observed: 452_312_000_000, observedAt: "2026-03-18", result: "came_true" },
      inputs: { sources, worlds: [], lenses: [], earlier: [] } },
    { ...base, journalId: "journal-solsup-margin", updatedAtMs: created,
      judgment: ko() ? "솔숲식품의 영업이익률은 9%를 넘을 거예요." : "Solsup's operating margin will stay above 9%.",
      check: { claim: ko() ? "영업이익률 9% 초과" : "Operating margin above 9%", metric: ko() ? "영업이익률" : "operating margin", direction: "above", threshold: 9, unit: "%", byDate: "2026-09-30" },
      inputs: { sources: [], worlds: [], lenses: [], earlier: [] } },
  ] as JournalRecord[];
}

async function prepareJournal(bridge: PreviewBridge) {
  await prepareFixture(bridge);
  const entries = journalEntries();
  bridge.journalList = async () => structuredClone(entries);
  const read = bridge.knowledgeJournalRead.bind(bridge);
  bridge.knowledgeJournalRead = async request => structuredClone(entries.find(entry => entry.journalId === request.journalId) ?? await read(request));
}

export function answerCheckScenarios({ base, select, until, click, clickMapAction, hasLayout, submitPrompt, navigate }: Helpers): PreviewScenario[] {
  const answerLoaded = (root: HTMLElement) => until(() => root.querySelector("article.message.assistant.complete .number-chip, article.message.assistant.complete .as-of-tag"), "Answer did not load");
  const reveal = (element: Element | null) => { if (element && hasLayout()) element.scrollIntoView({ block: "center" }); };
  /** Pins the popover of the chip whose text is `text`. */
  const pin = async (root: HTMLElement, text: () => string) => {
    await answerLoaded(root);
    const chip = await until(() => [...root.querySelectorAll<HTMLButtonElement>("button.number-chip")].find(button => button.textContent?.trim() === text()), `Number chip ${text()} did not render`);
    reveal(chip);
    chip.click();
  };
  const chatScenario = (id: string, title: string, landmark: string, variant: Variant, enter: (root: HTMLElement) => Promise<void>): PreviewScenario => ({
    ...base(id, title, () => select(landmark), enter),
    setup: async bridge => {
      await prepareFixture(bridge, "answer");
      installThread(bridge, () => [question(ko() ? "솔숲식품(990221)의 FY2025 연결 영업이익률은?" : "What was Solsup Foods (990221) FY2025 consolidated operating margin?"), numberAnswer(variant)]);
    },
  });
  /** Types into the composer the way a person does, firing the change React reads. */
  const type = async (root: HTMLElement, text: string) => {
    const composer = await until(() => root.querySelector<HTMLTextAreaElement>("textarea[data-chat-composer]"), "Composer did not appear");
    composer.focus();
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, text);
    composer.setSelectionRange(text.length, text.length);
    composer.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const openJournal = async (root: HTMLElement, journalId: string) => {
    await navigate(root, "map");
    await clickMapAction("[data-map-library]", root);
    await click('[role="tab"][id$="-trigger-note"]', root);
    await until(() => root.querySelectorAll(".journal-library-row").length === 2, "Journal entries did not load");
    await click(`[data-research-id="${journalId}"]`, root);
    const panel = await until(() => select(".journal-then-now"), "Then · Now did not appear");
    reveal(panel);
  };
  return [
    chatScenario("chat-number-chips", "Numbers in an answer link to their source or calculation", 'article.message.assistant .number-chip[data-kind="calculated"]', "chips", async root => {
      reveal(await answerLoaded(root));
    }),
    chatScenario("chat-number-calc-popover", "A calculated number shows its formula and inputs", '.number-chip-popover[data-kind="calculated"][role="dialog"]', "chips", root => pin(root, () => "8.4%")),
    chatScenario("chat-number-passage-popover", "A sourced number shows its passage", '.number-chip-popover[data-kind="passage"] mark', "chips", root => pin(root, () => "9.1%")),
    chatScenario("chat-number-data-popover", "A data number shows its matched row", '.number-chip-popover[data-kind="data"] mark', "chips", root => pin(root, revenueText)),
    chatScenario("chat-number-differs", "A calculation that differs from the answer is marked", '.number-chip-popover .number-chip-status[data-status="differs"]', "differs", async root => {
      await answerLoaded(root);
      reveal(await until(() => select('.chat-receipt [data-receipt="numbers"]'), "The receipt did not count the number to check"));
      await pin(root, () => "8.5%");
    }),
    chatScenario("chat-number-unlinked-input", "An input without a source says so", ".number-chip-popover .number-chip-unlinked", "unlinked", root => pin(root, () => "8.4%")),
    chatScenario("chat-number-grid", "A reverse DCF number is a grid, not one value", ".number-chip-popover .number-chip-grid", "grid", root => pin(root, () => "4.1%")),
    { ...base("chat-number-pending", "Numbers stay pending while the answer streams", () => select('.number-chip[data-status="pending"]'), async root => {
      await submitPrompt(root, ko() ? "솔숲식품 FY2025 매출은?" : "What was Solsup Foods’ FY2025 revenue?");
      reveal(await until(() => select('.number-chip[data-status="pending"]'), "No pending number"));
    }), setup: async bridge => {
      await prepareFixture(bridge, "streaming");
      bridge.chatSend = async (request, observer) => {
        const run_id = request.run_id;
        observer({ type: "started", run_id });
        observer({ type: "evidence", run_id, evidence: DART });
        observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: Date.now() - 6_000, items: [{ id: "step-statements", kind: "tool", toolName: "dart_financials", category: "finance", operation: "read", action: "Read financial statements", target: `${ko() ? "솔숲식품" : "Solsup Foods"} · FY2025`, status: "succeeded", resultIds: [DART.id], href: null, startedAtMs: 0, finishedAtMs: null }], finishedAtMs: null } });
        observer({ type: "phase", run_id, phase: "writing" });
        observer({ type: "delta", run_id, text: ko()
          ? `솔숲식품의 FY2025 연결 매출은 [${revenueText()}](#evidence-${DART.id})였고, 영업이익은 [${incomeText()}](#evidence-${DART.id})예요. 이익률을 계산하고 있어요…`
          : `Solsup Foods’ FY2025 consolidated revenue was [${revenueText()}](#evidence-${DART.id}), with operating income of [${incomeText()}](#evidence-${DART.id}). I’m calculating the margin…` });
        return { run_id };
      };
    } },

    { ...base("composer-asof-menu", "/asof in the composer's command menu", () => select('.composer-assist [role="option"][aria-label="/asof"]'), async root => {
      await type(root, "/as");
    }), setup: bridge => prepareFixture(bridge, "answer") },
    { ...base("composer-asof-date-panel", "The as-of date panel refuses a future date", () => select(".composer-asof-panel .composer-asof-error"), async root => {
      await type(root, "/asof 2999-01-01 ");
    }), setup: bridge => prepareFixture(bridge, "answer") },
    { ...base("composer-asof-chip", "The as-of mode stays below the question", () => select(".composer-chip-tray .composer-asof-chip"), async root => {
      await type(root, `/asof ${AS_OF} `);
    }), setup: bridge => prepareFixture(bridge, "answer") },
    // finance-korea-retail-disagreement.md §2.4: the retail Skills, each with its blurb, under `/`,
    // scrolled into the list's view as a reader would scroll to them.
    { ...base("composer-slash-retail-skills", "The retail Skills with their blurbs under /", () => {
      const assist = select(".composer-assist");
      const rows = retailRows(assist);
      return assist && rows.length === RETAIL_SKILLS.length && rows.every(row => shownInList(row, assist)) ? assist : null;
    }, async root => {
      await type(root, "/");
      const assist = await until(() => select(".composer-assist"), "The / menu did not open");
      const [first] = await until(() => { const rows = retailRows(assist); return rows.length === RETAIL_SKILLS.length && rows; }, "The retail Skills are not under /");
      const scroll = assist.querySelector<HTMLElement>(".composer-assist-scroll")!;
      if (hasLayout()) scroll.scrollTop += first!.getBoundingClientRect().top - scroll.getBoundingClientRect().top - 4;
    }), setup: bridge => prepareFixture(bridge, "answer") },
    { ...base("chat-asof-answer", "An as-of answer: tags, a skipped step, the note and Known as of", () => select('.number-chip-popover .number-chip-status[data-status="as_of"]') && select(".as-of-note"), async root => {
      await answerLoaded(root);
      await click("article.message.assistant .chat-progress-toggle", root);
      await until(() => select('.chat-progress-row.skipped'), "The refused step did not appear");
      await pin(root, () => (ko() ? "4,400억 원" : "₩440bn"));
    }), setup: async bridge => {
      await prepareFixture(bridge, "answer");
      installThread(bridge, () => [question(ko() ? "3월 3일 기준으로 솔숲식품 FY2025 매출에 대해 무엇을 알 수 있었어?" : "As of Mar 3, what did we know about Solsup's FY2025 revenue?", AS_OF), asOfAnswer(false)]);
    } },
    { ...base("chat-asof-nothing-known", "Nothing could be known on the as-of date", () => select(".as-of-note"), async () => {
      reveal(await until(() => select(".as-of-note"), "The as-of note did not appear"));
    }), setup: async bridge => {
      await prepareFixture(bridge, "answer");
      installThread(bridge, () => [question(ko() ? "3월 3일 기준 솔숲식품 FY2025 매출은?" : "As of Mar 3, what was Solsup's FY2025 revenue?", AS_OF), asOfAnswer(true)]);
    } },
    { ...base("journal-then-now", "Then · Now for a judgment that came true", () => select(".journal-then-now .journal-outcome-card"), root => openJournal(root, "journal-solsup-beat")), setup: prepareJournal },
    { ...base("journal-then-now-due", "Then · Now for an overdue check", () => select(".journal-then-now .journal-then-now-late"), root => openJournal(root, "journal-solsup-margin")), setup: async bridge => {
      await prepareJournal(bridge);
      // The check date has passed with no review yet.
      const list = bridge.journalList.bind(bridge);
      const overdue = (entry: JournalRecord) => entry.journalId === "journal-solsup-margin" ? { ...entry, check: entry.check ? { ...entry.check, byDate: "2026-10-01" } : entry.check } : entry;
      bridge.journalList = async request => (await list(request)).map(overdue);
      const read = bridge.knowledgeJournalRead.bind(bridge);
      bridge.knowledgeJournalRead = async request => overdue(await read(request));
    } },
    { ...base("journal-lookback-draft", "Look back opens a new chat with the date, the entry and the question", () => {
      const mode = select(".composer-chip-tray .composer-asof-chip");
      return mode && document.querySelector('.composer-prompt-highlight [data-inline-reference="journal"]') ? select(".composer-shell") : null;
    }, async root => {
      await openJournal(root, "journal-solsup-beat");
      await click(".journal-then-now button.journal-look-back, .journal-then-now section > button", root);
      await until(() => select(".composer-chip-tray .composer-asof-chip"), "Look back did not fill the composer");
    }), setup: prepareJournal },
  ];
}
