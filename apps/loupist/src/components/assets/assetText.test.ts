import i18n from "@/i18n";
import type { AssetEvidence } from "../../contracts/assets";
import { citationNumbers, moveSentence } from "./assetText";
import { localAssetName } from "./assetName";

afterEach(async () => { await i18n.changeLanguage("en"); });

it("words a thesis move, a crossed line mid-sentence", async () => {
  const line = { journalId: "j", claim: "One customer above 25% of revenue", metric: "Largest customer", direction: "above" as const, threshold: 25, unit: "%", observed: 27.4, crossed: true, setAtMs: 0 };
  expect(moveSentence({ from: "weakening", to: "broken", reason: "Crossed your line", line, factDate: "2026-10-30" }))
    .toBe("Weakening → Broken · crossed your line: One customer above 25% of revenue (27.4%, Oct 30)");
  expect(moveSentence({ from: "holding", to: "weakening", reason: "The 10-Q shows a lower margin.", factDate: "2026-10-30" }))
    .toBe("Holding up → Weakening · the 10-Q shows a lower margin.");
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
  // A stored price cites the turn result its saved read came from.
  expect(numbers({ tool: "equity_quote", title: "Yahoo Finance", citationUrl: "#evidence-a" })).toBe(5);
  expect(numbers({ tool: "equity_quote", title: "yfinance", citationUrl: "#evidence-a" })).toBe(5);
});

it("uses the Korean name for a Korean listing in Korean, and the followed name otherwise", () => {
  const nodes = [{ id: "e1", kind: "entity" as const, label: "Hanul Precision", localNames: ["한울정밀"] }, { id: "e2", kind: "entity" as const, label: "Morrow Components", localNames: ["모로우 부품"] }];
  const pages = [{ assetId: "a1", symbol: "HNP", market: "KS", entityId: "e1" }, { assetId: "a2", symbol: "MORR", market: "US", entityId: "e2" }];
  expect(localAssetName({ name: "Hanul Precision", assetId: "a1" }, { nodes, pages }, "ko")).toBe("한울정밀");
  expect(localAssetName({ name: "Morrow Components", assetId: "a2" }, { nodes, pages }, "ko")).toBe("Morrow Components");
  expect(localAssetName({ name: "Hanul Precision", assetId: "a1" }, { nodes, pages }, "en")).toBe("Hanul Precision");
  // Matched by name when the page has no entity yet.
  expect(localAssetName({ name: "Hanul Precision", symbol: "HNP", market: "KS" }, { nodes }, "ko")).toBe("한울정밀");
});
