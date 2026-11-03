import { afterEach, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import { formatAmount, formatAmountRange, formatMeasure, formatShareCount } from "./format";

describe("amounts", () => {
  afterEach(async () => { await i18n.changeLanguage("en"); });

  it("prints money the way answers do in English", () => {
    expect(formatAmount(452_312_000_000, "KRW")).toBe("₩452.3bn");
    expect(formatAmount(38_104_000_000, "KRW")).toBe("₩38.1bn");
    expect(formatAmount(1_250_000_000_000, "USD")).toBe("$1.3tn");
    expect(formatAmount(12_500_000, "USD")).toBe("$12.5m");
    expect(formatAmount(38_200, "KRW")).toBe("₩38,200");
    expect(formatAmount(-24_500_000_000, "KRW")).toBe("-₩24.5bn");
    expect(formatAmount(452_312_000_000, "KRW", { exact: true })).toBe("₩452,312,000,000");
    expect(formatAmount(452_312_000_000)).toBe("452.3bn");
  });

  it("prints money in 조, 억 and 만 in Korean", async () => {
    await i18n.changeLanguage("ko");
    expect(formatAmount(452_312_000_000, "KRW")).toBe("4,523억 원");
    expect(formatAmount(38_104_000_000, "KRW")).toBe("381억 원");
    expect(formatAmount(1_230_000_000, "KRW")).toBe("12.3억 원");
    expect(formatAmount(1_234_500_000_000, "KRW")).toBe("1조 2,345억 원");
    expect(formatAmount(999_960_000_000, "KRW")).toBe("1조 원");
    expect(formatAmount(38_200, "KRW")).toBe("38,200원");
    expect(formatAmount(12_500_000, "KRW")).toBe("1,250만 원");
    expect(formatAmount(9_500, "KRW", { exact: true })).toBe("9,500원");
    expect(formatAmount(452_312_000_000, "KRW", { exact: true })).toBe("4,523억 1,200만 원");
    expect(formatAmount(100_001_234, "KRW", { exact: true })).toBe("1억 1,234원");
    expect(formatAmount(4_500_000_000, "USD")).toBe("45억 달러");
    // Under 10억 Korean keeps what "$402m" keeps.
    expect(formatAmount(402_000_000, "USD")).toBe("4억 200만 달러");
    expect(formatAmount(310_000_000, "USD")).toBe("3억 1,000만 달러");
    expect(formatAmount(100_000_000, "KRW")).toBe("1억 원");
    expect(formatAmount(999_996_000, "KRW")).toBe("10억 원");
  });

  it("counts shares at the same scale", async () => {
    expect(formatShareCount(34_560_000)).toBe("34.6m shares");
    expect(formatShareCount(9_800)).toBe("9,800 shares");
    await i18n.changeLanguage("ko");
    expect(formatShareCount(34_560_000)).toBe("3,456만 주");
    expect(formatShareCount(182_400_000)).toBe("1억 8,240만 주");
    expect(formatShareCount(9_800)).toBe("9,800주");
  });

  it("keeps a second decimal when asked and writes a range's currency and scale once", async () => {
    expect(formatAmount(1_250_000_000, "USD")).toBe("$1.3bn");
    expect(formatAmount(1_250_000_000, "USD", { digits: 2 })).toBe("$1.25bn");
    expect(formatAmountRange(1_200_000_000, 1_250_000_000, "USD", { digits: 2 })).toBe("$1.2–1.25bn");
    expect(formatAmountRange(900_000_000, 1_200_000_000, "USD", { digits: 2 })).toBe("$900m–$1.2bn");
    await i18n.changeLanguage("ko");
    expect(formatAmount(115_000_000_000, "KRW", { digits: 2 })).toBe("1,150억 원");
    expect(formatAmountRange(1_200_000_000, 1_250_000_000, "USD", { digits: 2 })).toBe("12억–12.5억 달러");
    expect(formatAmountRange(110_000_000_000, 120_000_000_000, "KRW")).toBe("1,100억–1,200억 원");
    expect(formatAmountRange(37_000, 38_200, "KRW")).toBe("37,000–38,200원");
  });

  it("prints a measure by its unit", async () => {
    expect(formatMeasure(445_000_000_000, "KRW")).toBe("₩445bn");
    expect(formatMeasure(9, "%")).toBe("9%");
    expect(formatMeasure(12.5, "days")).toBe("12.5 days");
    await i18n.changeLanguage("ko");
    expect(formatMeasure(445_000_000_000, "KRW")).toBe("4,450억 원");
  });
});
