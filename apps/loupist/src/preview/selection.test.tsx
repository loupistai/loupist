import { act, fireEvent, render, screen } from "@testing-library/react";
import { QuoteAsk } from "../components/chat/QuoteAsk";
import { selectAnswerWords } from "./scenarios";

it("waits through an absent answer and its lazy fallback before selecting final Markdown", async () => {
  document.getSelection()?.removeAllRanges();
  const onQuote = vi.fn();
  const view = render(<><article className="message assistant" data-message-id="delayed"><div className="message-content" /></article><QuoteAsk onQuote={onQuote} /></>);
  const content = view.container.querySelector(".message-content")!;
  const selecting = selectAnswerWords(0, 4);
  content.innerHTML = '<p class="message-response-fallback">Temporary answer has enough words for selection.</p>';
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 100)); });
  expect(document.getSelection()?.toString()).toBe("");
  content.innerHTML = '<p>Final answer keeps this selection after rendering.</p>';
  await screen.findByRole("button", { name: "Ask about this" });
  await selecting;
  const range = document.getSelection()!.getRangeAt(0);
  expect(range.startContainer.isConnected).toBe(true);
  expect(range.toString()).toBe("Final answer keeps this");
  fireEvent.click(screen.getByRole("button", { name: "Ask about this" }));
  expect(onQuote).toHaveBeenCalledWith("delayed", "Final answer keeps this");
});
