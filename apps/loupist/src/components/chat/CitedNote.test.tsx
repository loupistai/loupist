import { render } from "@testing-library/react";
import type { ChatEvidenceRef } from "../../types";
import { citedParts } from "../../chat/citedText";
import { CitedNote } from "./CitedNote";

const evidence = (id: string, provider: string): ChatEvidenceRef => ({
  id, title: `${provider} annual statements`, kind: "original", provider, tool_name: "equity_fundamental_income",
  retrieved_at: "2026-11-13T00:00:00Z", excerpt: "", excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [],
});
const APPLE = evidence("ev_183823be95c03204cf5dc12764336f79", "yfinance");
const MSFT = evidence("ev_e5844f3b99926dde551b0a65875a282e", "SEC");

it("splits a caption into text and the citations it names, bare or linked", () => {
  expect(citedParts(`Apple: #evidence-${APPLE.id}; Microsoft: [10-K](#evidence-${MSFT.id}).`)).toEqual([
    { text: "Apple: " }, { evidenceId: APPLE.id }, { text: "; Microsoft: " }, { evidenceId: MSFT.id, label: "10-K" }, { text: "." },
  ]);
  expect(citedParts("No sources here.")).toEqual([{ text: "No sources here." }]);
});

it("draws a chart caption's citation targets as numbered citations, never as raw ids", () => {
  const { container } = render(<p><CitedNote text={`Apple: #evidence-${APPLE.id}; Microsoft: #evidence-${MSFT.id}; old: #evidence-ev_gone.`} evidenceRefs={[APPLE, MSFT]} /></p>);
  expect(container).toHaveTextContent("Apple: 1; Microsoft: 2; old: source.");
  expect(container.textContent).not.toContain("evidence");
  expect(container.textContent).not.toContain("ev_");
});

it("keeps a caption without citations as it is", () => {
  const { container } = render(<p><CitedNote text="Daily prices and volume." /></p>);
  expect(container.innerHTML).toBe("<p>Daily prices and volume.</p>");
});
