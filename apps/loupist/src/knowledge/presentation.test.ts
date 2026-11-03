import i18n from "../i18n";
import { dateValue, spanSelectorLabel, timePointValue } from "./presentation";

afterEach(async () => { await i18n.changeLanguage("en"); });

it("formats saved date precision without adding a day or shifting a date-only value", async () => {
  expect(dateValue("2026")).toBe("2026");
  expect(dateValue("2026-10")).toBe("Oct 2026");
  expect(dateValue("2026-10-01")).toBe("Oct 1, 2026");
  expect(dateValue("source date not specified")).toBe("source date not specified");
  await i18n.changeLanguage("ko");
  expect(dateValue("2026-10-01")).toBe("2026년 10월 1일");
  expect(timePointValue({ precision: "date", value: "2026-10-01", approximate: true })).toContain("2026년 10월 1일");
});

it("localizes selector counts and preserves unsupported source selectors without inventing labels", async () => {
  expect(spanSelectorLabel('{"kind":"multi","documents":[{}]}')).toBe("1 document");
  expect(spanSelectorLabel('{"kind":"multi","documents":[{},{}]}')).toBe("2 documents");
  expect(spanSelectorLabel('{"kind":"utf8_line_range","startLine":1000,"endLine":1001}')).toBe("Lines 1,000–1,001");
  expect(spanSelectorLabel('{"kind":"byte_range","start":0,"end":20}')).toBe("");
});
