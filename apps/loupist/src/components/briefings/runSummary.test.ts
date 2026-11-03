import i18n from "@/i18n";
import { runSummary } from "./runSummary";

afterEach(async () => { await i18n.changeLanguage("en"); });

it("words the results task's structured run items in the reader's language", async () => {
  expect(runSummary([{ kind: "earnings_expected", name: "Morrow Components", period: "Q3 2026", count: 3 }]))
    .toBe("Captured what was expected for Morrow Components Q3 2026 (3 values).");
  expect(runSummary([{ kind: "earnings_reported", name: "Morrow Components", period: "Q3 2026", count: 1 }, { kind: "earnings_nothing" }]))
    .toBe("Recorded Morrow Components Q3 2026 results (1 value). Nothing to capture now.");
  await i18n.changeLanguage("ko");
  const names = (name: string) => (name === "Solsup Foods" ? "솔숲식품" : name);
  expect(runSummary([{ kind: "earnings_reported", name: "Solsup Foods", period: "Q3 2026", count: 2 }], names)).toBe("솔숲식품 Q3 2026 발표치 2개를 기록했어요.");
  expect(runSummary([{ kind: "earnings_expected", name: "Solsup Foods", period: "Q3 2026", count: 4 }], names)).toBe("솔숲식품 Q3 2026 예상치 4개를 기록했어요.");
  // A failed capture says which one failed; its reason is a diagnostic, never shown here.
  expect(runSummary([{ kind: "earnings_failed", name: "Solsup Foods", period: "Q3 2026" }], names)).toBe("솔숲식품 Q3 2026 실적을 기록하지 못했어요.");
  expect(runSummary([{ kind: "earnings_nothing" }])).toBe("지금은 기록할 것이 없어요.");
});
