import i18n from "@/i18n";
import type { EarningsEventSummary } from "../../contracts/earnings";
import { solsupSnapshot } from "../../preview/finance";
import { withNextEarnings } from "./AssetPage";
import { glanceValue } from "./assetText";
import { amountText, basisLabel, chainForm } from "./earningsText";

afterEach(async () => { await i18n.changeLanguage("en"); });

it("prints results amounts the way grids do, a range's scale once", async () => {
  expect(amountText({ low: 1.2e9, high: 1.25e9, unit: "currency", currency: "USD" })).toBe("$1.2–1.25bn");
  expect(amountText({ point: 1.27e9, unit: "currency", currency: "USD" })).toBe("$1.27bn");
  expect(amountText({ point: 115e9, unit: "currency", currency: "KRW" })).toBe("₩115bn");
  expect(amountText({ point: 16, unit: "percent" })).toBe("16.0%");
  await i18n.changeLanguage("ko");
  expect(amountText({ point: 115e9, unit: "currency", currency: "KRW" })).toBe("1,150억 원");
  expect(amountText({ low: 1.2e9, high: 1.25e9, unit: "currency", currency: "USD" })).toBe("12억–12.5억 달러");
});

it("words a measure's basis and a filing's form in the reader's language", async () => {
  expect(basisLabel("consolidated")).toBe("Consolidated");
  expect(chainForm("공정공시", "preliminary")).toBe("fair disclosure");
  expect(chainForm("8-K", "release")).toBe("8-K");
  // A Korean title that only repeats the step is left out.
  expect(chainForm("분기보고서 (2026.09)", "quarterly")).toBe("");
  await i18n.changeLanguage("ko");
  expect(basisLabel("consolidated")).toBe("연결");
  expect(basisLabel("separate")).toBe("별도");
  expect(chainForm("공정공시", "preliminary")).toBe("공정공시");
});

describe("Next earnings", () => {
  const now = Date.parse("2026-10-28T09:00:00");
  const upcoming: EarningsEventSummary = { eventId: "e2", period: "Q4 2026", reportDate: "2026-11-02", dateSource: "calendar", status: "upcoming" };
  const value = (rows: ReturnType<typeof withNextEarnings>) => {
    const row = rows.find(item => item.key === "nextEarnings")!;
    return row.facts.notAnnounced ? "Not announced" : glanceValue(row);
  };

  it("reads the next results event and says whether its date is expected, confirmed or a filing deadline", async () => {
    expect(value(withNextEarnings([], [upcoming], null, now))).toBe("Nov 2 (expected)");
    expect(value(withNextEarnings([], [{ ...upcoming, dateSource: "ir_notice" }], null, now))).toBe("Nov 2 (confirmed)");
    // A Korean card whose quarterly report is still due: the deadline Rust computed, never "Not announced".
    expect(value(withNextEarnings([], [], { ...solsupSnapshot(), chain: [{ stage: "quarterly", filed: false, dueBy: "2026-11-14", evidence: [] }] }, now))).toBe("Due by Nov 14 (filing deadline)");
    expect(value(withNextEarnings([], [{ ...upcoming, reportDate: "2026-10-01", status: "final" }], null, now))).toBe("Not announced");
    // Rust's own row keeps its date and gains whether it is confirmed.
    expect(value(withNextEarnings([{ key: "nextEarnings", facts: { date: "2026-11-02" }, evidence: [] }], [{ ...upcoming, dateSource: "ir_notice" }], null, now))).toBe("Nov 2 (confirmed)");
    await i18n.changeLanguage("ko");
    expect(value(withNextEarnings([{ key: "nextEarnings", facts: { date: "2026-11-14", dueBy: true }, evidence: [] }], [], null, now))).toBe("11월 14일까지 공시 (법정 기한)");
  });
});
