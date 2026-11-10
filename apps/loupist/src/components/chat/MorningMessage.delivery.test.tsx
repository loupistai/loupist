import { render, screen } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { InboxItem } from "../../types";
import { MorningMessage, wordTrimmed } from "./MorningMessage";

const briefing: InboxItem = { runId: "run-1", taskId: "briefing", kind: "briefing", title: "Briefings", status: "succeeded", summary: "Hanul Precision filed its Q3 report.", startedAtMs: Date.now() - 3_600_000, finishedAtMs: Date.now() - 3_590_000, readAtMs: null, threadId: "t", messageId: "m", skippedTools: [], briefingContext: null, error: null, summaryItems: [] };

function renderMessage(deliveredRunIds?: string[]) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "inboxList").mockResolvedValue({ items: [briefing], nextCursor: null });
  const onSettled = vi.fn();
  render(<MorningMessage bridge={bridge} loupistId="loupist-quality" loupistName="Quality Compounder" onAsk={vi.fn()} deliveredRunIds={deliveredRunIds} onSettled={onSettled} />);
  return onSettled;
}

it("does not repeat a briefing the Thread already shows as a delivered result", async () => {
  const settled = renderMessage(["run-1"]);
  await vi.waitFor(() => expect(settled).toHaveBeenCalled());
  expect(screen.queryByText(/Hanul Precision filed its Q3 report/)).toBeNull();
});

it("keeps the briefing when it was not delivered into the Thread", async () => {
  renderMessage([]);
  expect(await screen.findByText(/Hanul Precision filed its Q3 report/)).toBeVisible();
});

it("cuts a long source title at a word, never mid-word", () => {
  expect(wordTrimmed("Hanul Precision 2026 third-quarter results presentation", 40)).toBe("Hanul Precision 2026 third-quarter…");
  expect(wordTrimmed("Q3 operating review", 40)).toBe("Q3 operating review");
});
