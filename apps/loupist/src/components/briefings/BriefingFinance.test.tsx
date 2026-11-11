import { render, screen } from "@testing-library/react";
import type { BriefingContext } from "../../contracts/scheduled";
import { BriefingAheadRows, hasAheadRows } from "./BriefingFinance";

it("keeps the saved run's seven-day results and dividend window when read weeks later", () => {
  const startedAtMs = new Date(2026, 9, 28, 8).getTime();
  const context: BriefingContext = {
    symbols: [], principlesChecked: 0, newFacts: 0, disputedEntityIds: [],
    changed: { names: 0, relations: 0, conflicts: 0, rows: [] }, ahead: [], assets: [],
    earnings: [
      { assetId: "morrow", eventId: "results", name: "Morrow", symbol: "MORR", market: "NASDAQ", period: "Q3", reportDate: "2026-11-04", status: "upcoming", captured: false, measures: [] },
      { assetId: "late", eventId: "late", name: "Later", symbol: "LATE", market: "NASDAQ", period: "Q3", reportDate: "2026-11-05", status: "upcoming", captured: false, measures: [] },
    ],
    watchEvents: [{ eventId: "dividend", assetId: "morrow", name: "Morrow", symbol: "MORR", market: "NASDAQ", kind: "dividends", factDate: "2026-10-27", detectedAtMs: startedAtMs, facts: { recordDate: "2026-10-28", amount: 1, currency: "USD" }, evidence: [] }],
  };
  vi.useFakeTimers();
  try {
    vi.setSystemTime(new Date(2026, 10, 18));
    expect(hasAheadRows(context, startedAtMs)).toBe(true);
    render(<ul><BriefingAheadRows context={context} startedAtMs={startedAtMs} loupistId="test" /></ul>);
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByText(/Morrow.*Nov 4/)).toBeVisible();
    expect(screen.getByText(/Morrow.*Oct 28/)).toBeVisible();
    expect(screen.queryByText(/Later/)).toBeNull();
    expect(hasAheadRows(context, new Date(2026, 10, 18).getTime())).toBe(false);
  } finally { vi.useRealTimers(); }
});
