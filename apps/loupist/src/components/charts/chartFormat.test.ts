import { afterEach, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import { axisTick, columnUnit, dataLabel, dateLabel, exactValue, unitText } from "./chartFormat";

afterEach(async () => { await i18n.changeLanguage("en"); });

const ticks = (values: number[], lo: number, hi: number, count = 8, abbreviate = true) => values.map((value) => axisTick(value, lo, hi, count, abbreviate));

describe("a value axis", () => {
  it("counts large amounts in 조 and 억 in Korean, with the precision its step needs", async () => {
    await i18n.changeLanguage("ko");
    expect(ticks([0, 5e13, 1e14, 3e14, 3.5e14], 0, 3.02e14)).toEqual(["0", "50조", "100조", "300조", "350조"]);
    expect(ticks([0, 2.5e11, 5e11, 7.5e11], 0, 7.2e11)).toEqual(["0", "2,500억", "5,000억", "7,500억"]);
    // Adjacent ticks read apart: 1.25조 and 1.5조, never 1조 and 2조.
    expect(ticks([1e12, 1.25e12, 1.5e12], 0, 1.5e12, 12)).toEqual(["1조", "1.25조", "1.5조"]);
    expect(ticks([-2e13, -1e13, 0, 1e13], -1.5e13, 1.2e13)).toEqual(["-20조", "-10조", "0", "10조"]);
    // Prices stay in plain digits below 1,000만, as answers print them.
    expect(ticks([0, 20_000, 80_000], 0, 71_000)).toEqual(["0", "20,000", "80,000"]);
  });

  it("counts large amounts in tn, bn and m in English", () => {
    expect(ticks([0, 5e13, 3e14], 0, 3.02e14)).toEqual(["0", "50tn", "300tn"]);
    expect(ticks([0, 1e11, 4e11], 0, 3.91e11)).toEqual(["0", "100bn", "400bn"]);
    expect(ticks([-5e8, 0, 1.5e9], -4e8, 1.2e9)).toEqual(["-0.5bn", "0", "1.5bn"]);
    expect(ticks([0, 250_000, 500_000], 0, 480_000)).toEqual(["0", "250,000", "500,000"]);
  });

  it("never abbreviates a ratio or a percent and keeps its decimals", async () => {
    expect(ticks([-0.05, 0, 0.05, 0.15], -0.05, 0.2)).toEqual(["-0.05", "0", "0.05", "0.15"]);
    expect(ticks([0.0000001, 0.00000015], 0, 0.0000002)).toEqual(["0.0000001", "0.00000015"]);
    expect(ticks([5e12, 1e13], 0, 1.2e13, 8, false)).toEqual(["5,000,000,000,000", "10,000,000,000,000"]);
    await i18n.changeLanguage("ko");
    expect(ticks([0.2, 0.4, 2.4], 0, 2.4)).toEqual(["0.2", "0.4", "2.4"]);
  });
});

describe("printed and exact values", () => {
  it("prints a mark's value as the answer prints amounts, keeping a price's decimals", async () => {
    expect(dataLabel(3.02e14, true)).toBe("302tn");
    expect(dataLabel(71_000.5, true)).toBe("71,000.5");
    expect(dataLabel(0.1234, false)).toBe("0.123");
    expect(dataLabel(-1_200, true)).toBe("-1,200");
    await i18n.changeLanguage("ko");
    expect(dataLabel(3.02e14, true)).toBe("302조");
    expect(dataLabel(3.8329e11, true)).toBe("3,832.9억");
  });

  it("gives the tooltip every digit, in the dataset's currency", async () => {
    expect(exactValue(302_123_456_789_012, "KRW")).toBe("₩302,123,456,789,012");
    expect(exactValue(383_290_000_000, "USD")).toBe("$383,290,000,000");
    expect(exactValue(0.000123, "")).toBe("0.000123");
    expect(exactValue(-1_500_000, "")).toBe("-1,500,000");
    await i18n.changeLanguage("ko");
    expect(exactValue(302_123_400_000_000, "KRW")).toBe("302조 1,234억 원");
  });
});

describe("periods and units", () => {
  it("prints years as given and dates in the app's format", async () => {
    expect(dateLabel(2023, { grain: "year", utc: true })).toBe("2023");
    expect(dateLabel(Date.UTC(2023, 0, 1), { grain: "year", utc: true })).toBe("2023");
    expect(dateLabel(Date.UTC(2024, 0, 31), { grain: "day", utc: true })).toBe("Jan 31");
    expect(dateLabel(Date.UTC(2024, 2, 1), { grain: "month", utc: true })).toBe("Mar 2024");
    await i18n.changeLanguage("ko");
    expect(dateLabel(Date.UTC(2024, 0, 31), { grain: "day", utc: true })).toBe("1월 31일");
    expect(dateLabel(Date.UTC(2024, 2, 1), { grain: "month", utc: true })).toBe("2024년 3월");
    expect(dateLabel(2023, { grain: "year", utc: true })).toBe("2023");
  });

  it("names a value column's currency once, from its id or label", async () => {
    expect(columnUnit("revenue_krw", "Revenue")).toEqual({ currency: "KRW", scale: undefined, named: false });
    expect(columnUnit("revenue", "매출(원)")).toEqual({ currency: "KRW", scale: undefined, named: true });
    expect(columnUnit("sales_usd_bn", "Sales")).toEqual({ currency: "USD", scale: "bn", named: false });
    expect(columnUnit("margin", "Operating margin")).toEqual({ currency: undefined, scale: undefined, named: false });
    expect(unitText(columnUnit("sales_usd_bn", "Sales"))).toBe("USD bn");
    expect(unitText(columnUnit("margin", "Margin"))).toBe("");
    await i18n.changeLanguage("ko");
    expect(unitText(columnUnit("revenue_krw_trillion", "매출"))).toBe("조 원");
    expect(unitText(columnUnit("revenue_krw", "매출"))).toBe("원");
  });
});
