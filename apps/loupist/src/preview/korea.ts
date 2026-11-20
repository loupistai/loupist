/**
 * A fictional reverse-DCF answer for the design preview (finance-korea-retail-disagreement.md §8):
 * Solsup Foods (990221:KRX), whose answer links its grid result in a paragraph of its own.
 * Every value has a dated source or a calculation, like Rust's.
 */
import type { ChatEvidenceRef, ChatMessage, NumberCheck } from "../contracts/chat";
import { previewKo } from "./fixtures";

const ko = () => previewKo();

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
/** What the turn retained of the reverse-DCF result: the grid as `calculate` wrote it (calculations/dcf.rs). */
const IMPLIED_EXCERPT = JSON.stringify({ operation: "reverse_dcf", grid: {
  measure: "implied annual growth for 5 years", forecast_years: 5, amount_unit: "KRW billions", unit: "%",
  rows: "wacc_pct", row_values: [8, 9, 10, 11, 12], columns: "terminal_growth_pct", column_values: [1.5, 2, 2.5, 3, 3.5],
  values: IMPLIED, reference: 8, above_reference: IMPLIED.map(line => line.map(value => value > 8)),
} }, null, 2);
export const SENSITIVITY_EVIDENCE = {
  statements: evidence("ev_solsup_fy2025", "Solsup Foods FY2025 consolidated statements", "dart_financials", { provider: "OpenDART", published_at: "2026-03-18" }),
  close: evidence("ev_solsup_close", "KRX daily close · 990221", "krx_daily_quote", { provider: "KRX", published_at: "2026-04-16" }),
  cagr: evidence("ev_solsup_cagr", "Compound annual growth", "calculate", { upstream_evidence_ids: ["ev_solsup_fy2025"] }),
  grid: evidence("ev_solsup_reverse", "Reverse DCF grid", "calculate", { calc_operation: "reverse_dcf", excerpt: IMPLIED_EXCERPT, upstream_evidence_ids: ["ev_solsup_fy2025", "ev_solsup_close"] }),
};

/**
 * An answer whose middle paragraph is only the link to its reverse-DCF result, followed by a
 * paragraph with a checked number. Rust counted the links as written: the grid link is link 0,
 * the checked 8.0% link 1. The link stays a citation in its own paragraph.
 */
export function previewGridLinkAnswer(): ChatMessage {
  const { statements, close, cagr, grid } = SENSITIVITY_EVIDENCE;
  const subject = ko() ? "솔숲식품" : "Solsup Foods";
  // The prose quotes the grid's base cell (discount 10%, long-run growth 2.5%) and its five-year reference.
  const base = `${IMPLIED[2]![2]!.toFixed(1)}%`;
  const content = ko()
    ? `현재 주가는 앞으로도 현금흐름이 꽤 늘어난다고 가정하고 있습니다. 할인율 8~12%, 장기 성장률 1.5~3.5%로 계산한 범위입니다.\n\n[${subject}](#evidence-${grid.id})\n\n가운데 경우에는 연 ${base} 성장이 필요합니다. 지난 5년 동안은 연 [8.0%](#evidence-${cagr.id}) 성장했습니다 [1](#evidence-${statements.id}).`
    : `Solsup Foods’ price already assumes cash flow keeps growing. The range runs the discount rate from 8% to 12% and long-run growth from 1.5% to 3.5%.\n\n[${subject}](#evidence-${grid.id})\n\nThe middle case needs about ${base} a year; the last five years delivered [8.0%](#evidence-${cagr.id}) a year [1](#evidence-${statements.id}).`;
  const checks: NumberCheck[] = [{
    link_index: 1, text: "8.0%", evidence_id: cagr.id, kind: "calculated", status: "matched",
    calc: { title: "Compound annual growth", formulas: ["(free cash flow FY2025 ÷ free cash flow FY2020)^(1/5) − 1 = (24.5 ÷ 16.7)^(1/5) − 1 = 7.97%"], inputs: [
      { name: "start", label: "free cash flow FY2020", unit: { kind: "amount", currency: "KRW", scale: 9 }, display: "16.7", evidence_id: statements.id, status: "rounded" },
      { name: "end", label: "free cash flow FY2025", unit: { kind: "amount", currency: "KRW", scale: 9 }, display: "24.5", evidence_id: statements.id, status: "rounded" },
      { name: "years", label: "years", unit: { kind: "number" }, display: "5" },
    ] },
  }];
  const at = Date.parse("2026-04-17T06:00:00Z");
  const step = (id: string, category: "finance" | "compute", operation: "read" | "calculate", action: string, resultIds: string[], target?: string) =>
    ({ id, kind: "tool" as const, category, operation, action, status: "succeeded" as const, resultIds, target: target ?? null, href: null, startedAtMs: at, finishedAtMs: at + 900 });
  return {
    id: "solsup-grid-link-answer", role: "assistant", status: "complete", created_at: "2026-04-17T06:01:00Z", content,
    evidence_refs: [statements, close, cagr, grid], number_checks: checks,
    progress: { startedAtMs: at, finishedAtMs: at + 38_000, items: [
      step("step-statements", "finance", "read", "Read financial statements", [statements.id], `${subject} · FY2025`),
      step("step-close", "finance", "read", "Read market data", [close.id], "990221"),
      step("step-cagr", "compute", "calculate", "Calculated", [cagr.id]),
      step("step-reverse", "compute", "calculate", "Calculated", [grid.id]),
    ] },
    execution_model: { profile_id: "openai/gpt-6.1-sol", name: "GPT-6.1 Sol", provider: "openai", model: "gpt-6.1-sol", thinking_level: "medium", run_options: {} },
  };
}

export const previewGridLinkQuestion = (): ChatMessage => ({
  id: "solsup-grid-link-question", role: "user", status: "complete", created_at: "2026-04-17T06:00:00Z",
  content: ko() ? "솔숲식품 주가에는 어떤 성장이 반영돼 있어?" : "What growth does Solsup’s price imply?",
});
