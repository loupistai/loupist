import { act } from "@testing-library/react";
import i18n from "@/i18n";
import type { WatchEvent } from "../../contracts/watch";
import { watchKindLabel, watchSentence } from "./watchText";

const alert = (kind: WatchEvent["kind"], facts: Record<string, unknown>): WatchEvent => ({
  eventId: `watch-${kind}`, assetId: "ast_0123456789abcdef", name: "Asset", symbol: "X", market: "CRYPTO", kind,
  factDate: "2026-10-30", detectedAtMs: 0, facts, evidence: [],
});

afterEach(async () => { await act(async () => { await i18n.changeLanguage("en"); }); });

it("words each class alert with its number and date, in English and Korean", async () => {
  const day = alert("cryptoMove", { window: "day", change: -0.123, to: "2026-10-30" });
  const peg = alert("depeg", { close: 0.9731, to: "2026-10-30" });
  const locked = alert("tvl", { change: -0.226, target: "Ethereum", to: "2026-10-29" });
  const crude = alert("oilInventory", { to: "2026-10-23", stocks: 419_200, change: -2_100 });
  const report = alert("oilInventory", { to: "2026-10-23" });
  expect(watchSentence(day, "Ether")).toBe("Ether fell 12.3% on Oct 30.");
  expect(watchSentence(peg, "USD Coin")).toBe("USD Coin closed at $0.9731, off its $1 peg (Oct 30).");
  expect(watchSentence(locked, "Ether")).toBe("Ether: value locked on Ethereum fell 22.6% over 7 days (to Oct 29).");
  expect(watchSentence(crude, "WTI crude")).toBe("WTI crude: U.S. commercial crude stocks fell 2.1M barrels in the week to Oct 23 (EIA).");
  expect(watchSentence(report, "WTI crude")).toBe("WTI crude: new EIA weekly petroleum report (week to Oct 23).");
  expect(watchKindLabel("positioning")).toBe("Positioning flips");
  await act(async () => { await i18n.changeLanguage("ko"); });
  expect(watchSentence(day, "이더")).toMatch(/^이더: 10월 30일 하루 동안 12\.3% 내렸어요\.$/);
  expect(watchSentence(crude, "WTI 원유")).toMatch(/^WTI 원유: 10월 23일까지 한 주 동안 미국 상업용 원유 재고가 210만 배럴 줄었어요 \(EIA\)\.$/);
  expect(watchSentence(alert("positioning", { direction: "long", to: "2026-10-27" }), "금")).toMatch(/^금: 매니지드 머니가 순매수 포지션으로 돌아섰어요 \(CFTC, 10월 27일 기준\)\.$/);
  expect(watchKindLabel("tvl")).toBe("예치 자산(TVL) 감소");
});
