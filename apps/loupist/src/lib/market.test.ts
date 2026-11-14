import { describe, expect, it } from "vitest";
import { shownMarket, tickerLabel } from "./market";

describe("market labels", () => {
  it("never shows the UNKNOWN storage key", () => {
    expect(tickerLabel("NVDA", "UNKNOWN")).toBe("NVDA");
    expect(tickerLabel("NVDA", "unknown")).toBe("NVDA");
    expect(tickerLabel("NVDA", " ")).toBe("NVDA");
    expect(tickerLabel("NVDA", null)).toBe("NVDA");
    expect(shownMarket("UNKNOWN")).toBeNull();
  });

  it("shows a known market beside the ticker", () => {
    expect(tickerLabel("005930", "KRX")).toBe("005930 · KRX");
    expect(shownMarket(" NASDAQ ")).toBe("NASDAQ");
  });
});
