import type { UsageSummary, UsageSummaryRequest, UsageTotals } from "../../contracts/generated";

export function mockUsageSummary({ fromMs, toMs, timeZone }: UsageSummaryRequest, loupist?: { id: string; name: string }): UsageSummary {
  const chat: UsageTotals = { runs: 2, unknownRuns: 0, requests: 5, inputTokens: 3200, cachedInputTokens: 4800, outputTokens: 700, costEstimate: null };
  const world: UsageTotals = { runs: 1, unknownRuns: 0, requests: 2, inputTokens: 5600, cachedInputTokens: 0, outputTokens: 1100, costEstimate: { microusd: 18300, fromProvider: true } };
  const date = new Intl.DateTimeFormat("en-CA", { timeZone, year: "numeric", month: "2-digit", day: "2-digit" }).format(toMs - 1);
  const totals: UsageTotals = {
    runs: 3, unknownRuns: 0, requests: 7, inputTokens: 8800, cachedInputTokens: 4800,
    outputTokens: 1800, costEstimate: world.costEstimate,
  };
  return {
    fromMs, toMs, totals,
    accounts: [
      { provider: "openai", credentialKind: "plan", totals: chat },
      { provider: "openrouter", credentialKind: "api_key", totals: world },
    ],
    days: [{ date, you: chat.requests, background: 0, world: world.requests }],
    surfaces: [{ surface: "research", totals: chat }, { surface: "world", totals: world }],
    loupists: [{ loupistId: loupist?.id ?? null, name: loupist?.name ?? null, totals }],
    programs: [],
  };
}
