/**
 * Fictional Korean depth and sensitivity-card fixtures for the design preview
 * (finance-korea-retail-disagreement.md §8): Solsup Foods (990221:KRX) with its
 * investor flows and dividend record date, Byeolmuri Systems (990415:KRX) with
 * capital decisions, and reverse-DCF grids as Rust copies them into answers.
 * Every value has a dated source or a calculation, like Rust's.
 */
import type { AssetChangeRow, AssetEvidence, AssetFlows, AssetPage } from "../contracts/assets";
import type { ChatEvidenceRef, ChatMessage, NumberCheck, SensitivityCard, SensitivityGrid } from "../contracts/chat";
import type { EarningsCalc, EarningsSnapshot } from "../contracts/earnings";
import { mockAssetId, mockChecks } from "../bridge/mock/assets";
import type { AssetFixture } from "./assets";
import { morrowSnapshot, SOLSUP } from "./finance";
import { previewKo } from "./fixtures";

const DAY = 86_400_000;
/** YYYY-MM-DD `daysAgo` days before today (negative: ahead). */
const day = (daysAgo: number) => new Date(Date.now() - daysAgo * DAY).toISOString().slice(0, 10);
const ko = () => previewKo();

export const BYEOLMURI = { symbol: "990415", market: "KRX", name: "Byeolmuri Systems" } as const;

const DART = (receipt: string, title: string, date: string): AssetEvidence => ({ tool: "dart_major_report", provider: "opendart", title, url: `https://dart.fss.or.kr/dsaf001/main.do?rcpNo=${receipt}`, date });
const KIS = (title: string, date: string, id: string): AssetEvidence => ({ tool: "kis_read", provider: "kis", title, dataset: { dataset_id: id, revision: 1, digest: "b".repeat(64) }, pointer: "/data/rows/0", date });

const sum = (label: string, display: string, value: number): EarningsCalc => ({
  title: "Sum", display, value, unit: "KRW",
  formulas: [`sum(${label}, 10 trading days) = ${display}`],
  inputs: [{ name: "series", display: `${label} · 10 rows` }],
});

/** Ten trading days of investor flows to the last close, refreshed after 18:30 in Seoul. */
export function previewFlows(): AssetFlows {
  const to = day(1);
  const lendingTo = day(3);
  const flows = (title: string) => [KIS(title, to, "ds-solsup-flows")];
  return {
    days: 10, to,
    rows: [
      { key: "foreign", value: 12.3e9, unit: "KRW", calc: sum("foreign net buying", "12,300,000,000", 12.3e9), evidence: flows("Investor trading by stock · Korea Investment") },
      { key: "institutions", value: -4.1e9, unit: "KRW", calc: sum("institution net buying", "−4,100,000,000", -4.1e9), evidence: flows("Investor trading by stock · Korea Investment") },
      { key: "individuals", value: -8.2e9, unit: "KRW", calc: sum("individual net buying", "−8,200,000,000", -8.2e9), evidence: flows("Investor trading by stock · Korea Investment") },
      { key: "shortSelling", value: 4.2, unit: "%", calc: { title: "Ratio", display: "4.2%", value: 4.2, unit: "%", formulas: ["short volume ÷ volume = 412,300 ÷ 9,816,700 = 4.2%"], inputs: [{ name: "numerator", display: "412,300" }, { name: "denominator", display: "9,816,700" }] }, evidence: [KIS("Daily short sales · Korea Investment", to, "ds-solsup-short")] },
      { key: "lending", value: -3.1, unit: "%", to: lendingTo, calc: { title: "Percent change", display: "−3.1%", value: -3.1, unit: "%", formulas: ["(1,512,000 − 1,560,400) ÷ 1,560,400 = −3.1%"], inputs: [{ name: "from", display: "1,560,400" }, { name: "to", display: "1,512,000" }] }, evidence: [KIS("Daily lending balance · Korea Investment", lendingTo, "ds-solsup-loan")] },
    ],
  };
}

const row = (id: string, kind: AssetChangeRow["kind"], factDate: string, facts: Record<string, unknown>, evidence: AssetEvidence[]): AssetChangeRow =>
  ({ id, kind, factDate, recordedAtMs: Date.now() - DAY, facts, evidence, new: true, material: true });

const ratio = (display: string, value: number, shares: string, before: string): EarningsCalc => ({
  title: "Ratio", display, value, unit: "%",
  formulas: [`new shares ÷ shares before = ${shares} ÷ ${before} = ${display}`],
  inputs: [{ name: "numerator", display: shares }, { name: "denominator", display: before }],
});

/**
 * A convertible bond that can be reset down to a floor: the most shares it can
 * become at the floor, as Rust calculates it from the filing (₩30bn ÷ ₩6,370 =
 * 4,709,576 shares over the 54,044,311 shares its filed 6.10% implies).
 */
const convertibleRow = (id: string, daysAgo: number): AssetChangeRow => row(id, "capital", day(daysAgo), {
  decision: "convertible_bonds", filingId: "20261030000552", currency: "KRW", amount: 30e9, conversionPrice: 9100, floor: 6370,
  dilution: { value: 8.71, display: "8.71%", basis: "calculated", at: "floor", calc: {
    title: "Most new shares at the floor", display: "8.71%", value: 8.71, unit: "%",
    formulas: [
      "Face amount / Floor price = 30,000,000,000 / 6,370 = 4,709,576 new shares at most",
      "Shares outstanding = Shares at the conversion price / Their filed share = 3,296,703 / 6.1% = 54,044,311",
      "New shares at the floor / Shares outstanding = 4,709,576 / 54,044,311 = 8.71%",
    ],
    inputs: [
      { name: "Face amount", display: "30,000,000,000" }, { name: "Floor price", display: "6,370" },
      { name: "Shares at the conversion price", display: "3,296,703" }, { name: "Their filed share of all shares", display: "6.1%" },
    ],
  } },
}, [DART("20261030000552", "주요사항보고서(전환사채권발행결정)", day(daysAgo))]);

/** Solsup Foods with Korea Investment on: dividend record date, results due by the legal deadline, flows. */
export function koreaDepthFixture(): AssetFixture {
  const assetId = mockAssetId(SOLSUP.symbol, SOLSUP.market);
  const filing = (receipt: string, title: string, date: string): AssetEvidence => ({ tool: "dart_filing_search", provider: "opendart", title, url: `https://dart.fss.or.kr/dsaf001/main.do?rcpNo=${receipt}`, date });
  const page: AssetPage = {
    assetId, symbol: SOLSUP.symbol, market: SOLSUP.market, name: SOLSUP.name, class: "equity", classSource: "market", watch: "watching",
    reasons: [], lines: [], history: [],
    glance: [
      { key: "lastResults", facts: { period: "Q3 2026" }, date: day(4), evidence: [filing("20261028000231", "연결재무제표기준영업(잠정)실적(공정공시)", day(4))] },
      { key: "nextEarnings", facts: { date: day(-14), dueBy: true }, date: day(4), evidence: [] },
      { key: "dividend", facts: { amount: 300, currency: "KRW", fiscalYear: "FY2026", recordDate: day(-61) }, date: day(3), evidence: [{ tool: "dart_dividends", provider: "opendart", title: "현금ㆍ현물배당결정", url: "https://dart.fss.or.kr/dsaf001/main.do?rcpNo=20261029000077", date: day(3) }] },
    ],
    checks: { ...mockChecks("equity", "KRX"), kinds: ["filings", "results", "price", "dividends", "flows"], connections: ["OpenDART", "KRX", "Korea Investment"] },
    flows: previewFlows(),
    createdAtMs: Date.now() - 40 * DAY, refreshedAtMs: Date.now() - 3_600_000, revision: 3,
  };
  return { page, delta: { moved: false, crossed: false, rows: [
    convertibleRow("preview-solsup-cb", 2),
    row("preview-solsup-filing", "filing", day(4), { title: "연결재무제표기준영업(잠정)실적(공정공시)" }, [filing("20261028000231", "OpenDART", day(4))]),
  ] } };
}

/** The same page with Korea Investment off: flows say how to turn it on. */
export function koreaNeedsKeyFixture(): AssetFixture {
  const fixture = koreaDepthFixture();
  delete fixture.page.flows;
  fixture.page.glance = fixture.page.glance.map(item => item.key === "dividend" ? { ...item, facts: { amount: 300, currency: "KRW", fiscalYear: "FY2026", recordInNotice: true } } : item);
  fixture.page.checks = { ...fixture.page.checks, kinds: ["filings", "results", "price", "dividends"], connections: ["OpenDART", "KRX"], failures: [{ kind: "flows", connection: "Korea Investment", reason: "needs_connection" }] };
  return fixture;
}

/** Byeolmuri Systems: four capital decisions, the dilution filed or calculated. */
export function capitalRowsFixture(): AssetFixture {
  const assetId = mockAssetId(BYEOLMURI.symbol, BYEOLMURI.market);
  return { page: {
    assetId, symbol: BYEOLMURI.symbol, market: BYEOLMURI.market, name: BYEOLMURI.name, class: "equity", classSource: "market", watch: "watching",
    reasons: [], lines: [], history: [], glance: [], checks: mockChecks("equity", "KRX"), createdAtMs: Date.now() - 30 * DAY, refreshedAtMs: Date.now() - 3_600_000, revision: 2,
  }, delta: { moved: false, crossed: false, rows: [
    convertibleRow("preview-byeolmuri-cb", 1),
    row("preview-byeolmuri-rights", "capital", day(3), {
      decision: "rights_offering", filingId: "20261028000310", currency: "KRW", shares: 4_200_000, method: "third_party", price: null, paymentDate: null,
      // OpenDART's rights-offering decision states no ratio: Rust calculates new shares ÷ shares before.
      dilution: { value: 7.8, display: "7.8%", basis: "calculated", calc: ratio("7.8%", 7.8, "4,200,000", "53,846,000") },
    }, [DART("20261028000310", "주요사항보고서(유상증자결정)", day(3))]),
    row("preview-byeolmuri-bw", "capital", day(6), {
      decision: "bonds_with_warrants", filingId: "20261025000144", currency: "KRW", amount: 15e9, conversionPrice: 8800, floor: null,
      dilution: { value: 3.2, display: "3.2%", basis: "filing", at: "price" },
    }, [DART("20261025000144", "주요사항보고서(신주인수권부사채권발행결정)", day(6))]),
    row("preview-byeolmuri-buyback", "capital", day(9), {
      decision: "buyback", filingId: "20261022000098", currency: "KRW", amount: 5e9, by: day(-80),
    }, [DART("20261022000098", "주요사항보고서(자기주식취득결정)", day(9))]),
  ] } };
}

/** Rust's 5 × 5 reverse-DCF grid (discount 8–12%, long-run growth 1.5–3.5%) for Solsup Foods at ₩38,200. */
const IMPLIED: number[][] = [
  [4.1, 3.4, 2.6, 1.7, 0.6],
  [6.3, 5.7, 5.0, 4.2, 3.3],
  [8.4, 7.9, 7.3, 6.6, 5.8],
  [10.4, 9.9, 9.4, 8.8, 8.1],
  [12.3, 11.9, 11.4, 10.9, 10.3],
];

const evidence = (id: string, title: string, tool: string, extra: Partial<ChatEvidenceRef> = {}): ChatEvidenceRef => ({
  id, title, kind: tool === "calculate" ? "derived" : "original", tool_name: tool, retrieved_at: "2026-04-17T06:00:00Z",
  excerpt: "", excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [], ...extra,
});
export const SENSITIVITY_EVIDENCE = {
  statements: evidence("ev_solsup_fy2025", "Solsup Foods FY2025 consolidated statements", "dart_financials", { provider: "OpenDART", published_at: "2026-03-18" }),
  close: evidence("ev_solsup_close", "KRX daily close · 990221", "krx_daily_quote", { provider: "KRX", published_at: "2026-04-16" }),
  cagr: evidence("ev_solsup_cagr", "Compound annual growth", "calculate", { upstream_evidence_ids: ["ev_solsup_fy2025"] }),
  grid: evidence("ev_solsup_reverse", "Reverse DCF grid", "calculate", { upstream_evidence_ids: ["ev_solsup_fy2025", "ev_solsup_close"] }),
};

export type SensitivityVariant = "reference" | "none" | "nulls";

/** The grid as Rust copies it from `calculate` (reverse_dcf), with or without a five-year reference. */
export function previewSensitivityGrid(variant: SensitivityVariant = "reference"): SensitivityGrid {
  const { statements, close, cagr } = SENSITIVITY_EVIDENCE;
  const values: (number | null)[][] = IMPLIED.map(line => [...line]);
  if (variant === "nulls") { values[0]![3] = null; values[0]![4] = null; }
  const reference = variant === "reference";
  return {
    operation: "reverse_dcf", row_values: [8, 9, 10, 11, 12], column_values: [1.5, 2, 2.5, 3, 3.5], values, unit: "%", forecast_years: 5,
    ...(reference ? {
      reference: { value_pct: 8, label: "FY2020–FY2025", years: 5, evidence_id: cagr.id, status: "matched" as const },
      above_reference: values.map(line => line.map(value => value !== null && value > 8)),
    } : {}),
    price: { value: 38200, display: "38,200", currency: "KRW", date: "2026-04-16", evidence_id: close.id, status: "matched" },
    amount_unit: "KRW billions",
    inputs: [
      { name: "cash_flow", label: "free cash flow FY2025", display: "24.5", evidence_id: statements.id, status: "rounded" },
      { name: "shares", display: "34560000", evidence_id: statements.id, status: "matched" },
      { name: "net_debt", display: "61.2", evidence_id: statements.id, status: "rounded" },
    ],
    formulas: ["Solve g: Σ cash_flow × (1 + g)^t ÷ (1 + discount)^t + long-run value = price × shares + net_debt", "38,200 × 34,560,000 + 61.2bn = 1,381.4bn"],
  };
}

/** An answer whose middle paragraph is only the link to the grid, as the valuation Skill writes it. */
export function previewSensitivityAnswer(variant: SensitivityVariant = "reference"): ChatMessage {
  const { statements, close, cagr, grid } = SENSITIVITY_EVIDENCE;
  const subject = ko() ? "솔숲식품" : "Solsup Foods";
  const card: SensitivityCard = { ...previewSensitivityGrid(variant), link_index: 0, evidence_id: grid.id, subject };
  // The prose quotes the card's base cell (discount 10%, long-run growth 2.5%), and cites
  // the five-year reference only when the card shows it.
  const reference = variant === "reference";
  const base = `${IMPLIED[2]![2]!.toFixed(1)}%`;
  const intro = ko()
    ? `현재 주가는 앞으로도 현금흐름이 꽤 늘어난다고 가정하고 있어요. 할인율 8~12%, 장기 성장률 1.5~3.5%로 계산한 범위예요.\n\n[${subject}](#evidence-${grid.id})\n\n`
    : `Solsup Foods’ price already assumes cash flow keeps growing. The range runs the discount rate from 8% to 12% and long-run growth from 1.5% to 3.5%.\n\n[${subject}](#evidence-${grid.id})\n\n`;
  const middle = ko()
    ? reference ? `가운데 경우에는 연 ${base} 성장이 필요해요. 지난 5년 동안은 연 [8.0%](#evidence-${cagr.id}) 성장했어요 [1](#evidence-${statements.id}).` : `가운데 경우에는 연 ${base} 성장이 필요해요.`
    : reference ? `The middle case needs about ${base} a year; the last five years delivered [8.0%](#evidence-${cagr.id}) a year [1](#evidence-${statements.id}).` : `The middle case needs about ${base} a year.`;
  const content = intro + middle;
  const checks: NumberCheck[] = reference ? [{
    link_index: 1, text: "8.0%", evidence_id: cagr.id, kind: "calculated", status: "matched",
    calc: { title: "Compound annual growth", formulas: ["(free cash flow FY2025 ÷ free cash flow FY2020)^(1/5) − 1 = (24.5 ÷ 16.7)^(1/5) − 1 = 7.97%"], inputs: [
      { name: "start", label: "free cash flow FY2020", unit: { kind: "amount", currency: "KRW", scale: 9 }, display: "16.7", evidence_id: statements.id, status: "rounded" },
      { name: "end", label: "free cash flow FY2025", unit: { kind: "amount", currency: "KRW", scale: 9 }, display: "24.5", evidence_id: statements.id, status: "rounded" },
      { name: "years", label: "years", unit: { kind: "number" }, display: "5" },
    ] },
  }] : [];
  const at = Date.parse("2026-04-17T06:00:00Z");
  const step = (id: string, category: "finance" | "compute", operation: "read" | "calculate", action: string, resultIds: string[], target?: string) =>
    ({ id, kind: "tool" as const, category, operation, action, status: "succeeded" as const, resultIds, target: target ?? null, href: null, startedAtMs: at, finishedAtMs: at + 900 });
  return {
    id: "solsup-sensitivity-answer", role: "assistant", status: "complete", created_at: "2026-04-17T06:01:00Z", content,
    evidence_refs: reference ? [statements, close, cagr, grid] : [statements, close, grid], number_checks: checks, sensitivity_cards: [card],
    progress: { startedAtMs: at, finishedAtMs: at + 38_000, items: [
      step("step-statements", "finance", "read", "Read financial statements", [statements.id], `${ko() ? "솔숲식품" : "Solsup Foods"} · FY2025`),
      step("step-close", "finance", "read", "Read market data", [close.id], "990221"),
      ...(reference ? [step("step-cagr", "compute", "calculate", "Calculated", [cagr.id])] : []),
      step("step-reverse", "compute", "calculate", "Calculated", [grid.id]),
    ] },
    execution_model: { profile_id: "openai/gpt-6.1-sol", name: "GPT-6.1 Sol", provider: "openai", model: "gpt-6.1-sol", thinking_level: "medium", run_options: {} },
  };
}

export const previewSensitivityQuestion = (): ChatMessage => ({
  id: "solsup-sensitivity-question", role: "user", status: "complete", created_at: "2026-04-17T06:00:00Z",
  content: ko() ? "솔숲식품 주가에는 어떤 성장이 반영돼 있어?" : "What growth does Solsup’s price imply?",
});

/** Morrow Components' results card with the whole reverse-DCF grid behind PRICE IMPLIED. */
export function morrowWithGrid(): EarningsSnapshot {
  const snapshot = morrowSnapshot("after");
  const implied = snapshot.implied!;
  snapshot.implied = { ...implied, grid: {
    operation: "reverse_dcf", row_values: [9, 10, 11], column_values: [2, 2.5, 3], unit: "%", forecast_years: implied.years,
    values: [[6.1, 6.8, 7.6], [7.3, 7.9, 8.6], [8.5, 9.1, 9.8]],
    price: { value: implied.price, display: "41.20", currency: implied.currency, date: implied.closeDate },
    amount_unit: "USD millions",
    inputs: [{ name: "cash_flow", label: "free cash flow FY2025", display: "402" }, { name: "shares", display: "182400000" }, { name: "net_debt", display: "310" }],
    formulas: implied.calc.formulas,
  } };
  return snapshot;
}
