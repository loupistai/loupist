import i18n from "@/i18n";
import type { AssetEvidence } from "../../contracts/assets";
import { citationNumbers, dartReport, datedSentence, glanceLabel, glanceValue, rowSentence } from "./assetText";
import type { AssetGlanceRow } from "../../contracts/assets";
import { localAssetName } from "./assetName";

afterEach(async () => { await i18n.changeLanguage("en"); });

const filing = (facts: Record<string, unknown>) => rowSentence({ kind: "filing", factDate: "2026-10-30", facts });

it("names DART reports in English and the form only once", async () => {
  expect(filing({ form: "분기보고서", title: "분기보고서 (2026.09)" })).toBe("Quarterly report (2026.09) filed");
  expect(filing({ form: "사업보고서", title: "사업보고서" })).toBe("Annual report filed");
  expect(filing({ form: "분기보고서", title: "[기재정정]분기보고서 (2026.09)" })).toBe("Quarterly report, amended (2026.09) filed");
  expect(filing({ form: null, title: "임원ㆍ주요주주특정증권등소유상황보고서" })).toBe("Insider ownership report filed");
  expect(filing({ form: "주요사항보고서", title: "주요사항보고서(유상증자결정)" })).toBe("Major event report (유상증자결정) filed");
  // SEC forms keep their own names; an unknown Korean title stays as filed.
  expect(filing({ form: "10-Q", title: "Quarterly report" })).toBe("10-Q filed: Quarterly report");
  expect(filing({ form: "10-K", title: "10-K" })).toBe("10-K filed");
  expect(dartReport("기업설명회(IR)개최")).toBeUndefined();
  await i18n.changeLanguage("ko");
  expect(filing({ form: "분기보고서", title: "분기보고서 (2026.09)" })).toBe("분기보고서 (2026.09) 제출");
  expect(filing({ form: "분기보고서", title: "[기재정정]분기보고서 (2026.09)" })).toBe("[기재정정]분기보고서 (2026.09) 제출");
});

it("dates change rows shown outside the page", () => {
  expect(datedSentence({ kind: "source", factDate: "2026-10-26", facts: { title: "Series B announcement" } })).toBe("Oct 26 · Series B announcement");
});

it("words a crossed line mid-sentence and keeps a price range together", () => {
  const line = { journalId: "j", claim: "One customer above 25% of revenue", metric: "Largest customer", direction: "above" as const, threshold: 25, unit: "%", observed: 27.4, crossed: true, setAtMs: 0 };
  expect(rowSentence({ kind: "thesis", factDate: "2026-10-30", facts: { from: "weakening", to: "broken", reason: "Crossed your line", line } }))
    .toBe("Weakening → Broken · crossed your line: One customer above 25% of revenue (27.4%, Oct 30)");
  expect(rowSentence({ kind: "thesis", factDate: "2026-10-30", facts: { from: "holding", to: "weakening", reason: "The 10-Q shows a lower margin." } }))
    .toBe("Holding up → Weakening · the 10-Q shows a lower margin.");
  const price = rowSentence({ kind: "price", factDate: "2026-10-30", facts: { change: -3.1, from: 41200, to: 39920, fromDate: "2026-10-27", currency: "KRW" } });
  expect(price).toMatch(/^−3\.1% since Oct 27 \(₩41,200 → ₩39,920\)$/u);
});

it("gives each source one number, in reading order, across rows", () => {
  const filed: AssetEvidence = { tool: "dart_filing_search", title: "분기보고서 (2026.09)", url: "https://dart.fss.or.kr/dsaf001/main.do?rcpNo=1" };
  const close = (date: string): AssetEvidence => ({ tool: "krx_daily_quote", title: "KRX", dataset: { dataset_id: "krx", revision: 1, digest: "e" }, pointer: "/data/bars/0", date });
  const numbers = citationNumbers([[filed], [{ ...filed, date: "2026-10-30" }], [close("2026-10-27"), close("2026-10-30")]]);
  expect(numbers(filed)).toBe(1);
  expect(numbers({ ...filed, title: "OpenDART" })).toBe(1);
  expect(numbers(close("2026-10-27"))).toBe(2);
  expect(numbers(close("2026-10-30"))).toBe(3);
  expect(numbers({ tool: "world", title: "Morrow supplies Hanul", worldId: "claim-1" })).toBe(4);
});

it("uses the Korean name for a Korean listing in Korean, and the followed name otherwise", () => {
  const nodes = [{ id: "e1", kind: "entity" as const, label: "Hanul Precision", localNames: ["한울정밀"] }, { id: "e2", kind: "entity" as const, label: "Morrow Components", localNames: ["모로우 부품"] }];
  const pages = [{ assetId: "a1", symbol: "HANUL", market: "KRX", entityId: "e1" }, { assetId: "a2", symbol: "MORR", market: "NASDAQ", entityId: "e2" }];
  expect(localAssetName({ name: "Hanul Precision", assetId: "a1" }, { nodes, pages }, "ko")).toBe("한울정밀");
  expect(localAssetName({ name: "Morrow Components", assetId: "a2" }, { nodes, pages }, "ko")).toBe("Morrow Components");
  expect(localAssetName({ name: "Hanul Precision", assetId: "a1" }, { nodes, pages }, "en")).toBe("Hanul Precision");
  // Matched by name when the page has no entity yet.
  expect(localAssetName({ name: "Hanul Precision", symbol: "HANUL", market: "KRX" }, { nodes }, "ko")).toBe("한울정밀");
});

it("labels a rates level with its named series and keeps the generic label for FX", async () => {
  const level = (facts: Record<string, unknown>) => ({ key: "level", facts, date: "2026-10-30", evidence: [] }) as unknown as AssetGlanceRow;
  expect(glanceLabel(level({ value: 4.12, unit: "%", name: "10-year Treasury yield" }))).toBe("10-year Treasury yield");
  expect(glanceLabel(level({ value: 1385.2 }))).toBe("Level");
  expect(glanceValue(level({ value: 4.12, unit: "%", name: "10-year Treasury yield" }))).toBe("4.12%");
  await i18n.changeLanguage("ko");
  expect(glanceLabel(level({ value: 4.12, unit: "%", name: "10-year Treasury yield" }))).toBe("미국채 10년물 금리");
  expect(glanceLabel(level({ value: 1385.2 }))).toBe("수준");
});

it("shows an expense ratio as reported, never with a guessed unit", () => {
  const row = { key: "expenseRatio", facts: { ratio: 0.0009 }, date: "2026-10-30", evidence: [] } as unknown as AssetGlanceRow;
  expect(glanceValue(row)).toBe("0.0009 (as reported)");
});
