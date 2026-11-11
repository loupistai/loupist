import { act, fireEvent, render, screen } from "@testing-library/react";
import { QuoteAsk } from "./QuoteAsk";
import { composeSubmission, quoteChip } from "../../chat/composerChips";

function selectText(node: Node, start: number, end: number) {
  const range = document.createRange();
  range.setStart(node, start);
  range.setEnd(node, end);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
}

it("offers to ask about a phrase selected in an answer and hands over its message and text", async () => {
  vi.useFakeTimers();
  const onQuote = vi.fn();
  render(<>
    <article className="message assistant" data-message-id="answer-1"><div className="message-content"><p>Inventory is 54 days, so demand has not reached cash.</p></div></article>
    <article className="message user" data-message-id="question-1"><div className="message-content"><p>Why did the margin fall so much?</p></div></article>
    <QuoteAsk onQuote={onQuote} />
  </>);
  const answer = screen.getByText(/Inventory is 54 days/).firstChild!;
  selectText(answer, 0, 22);
  fireEvent.mouseUp(document);
  await act(async () => { vi.runAllTimers(); });
  fireEvent.click(screen.getByRole("button", { name: "Ask about this" }));
  expect(onQuote).toHaveBeenCalledWith("answer-1", "Inventory is 54 days,");

  // The user's own message and one-word selections are left alone.
  selectText(screen.getByText(/Why did the margin/).firstChild!, 0, 20);
  fireEvent.mouseUp(document);
  await act(async () => { vi.runAllTimers(); });
  expect(screen.queryByRole("button", { name: "Ask about this" })).not.toBeInTheDocument();
  selectText(answer, 0, 9);
  fireEvent.mouseUp(document);
  await act(async () => { vi.runAllTimers(); });
  expect(screen.queryByRole("button", { name: "Ask about this" })).not.toBeInTheDocument();
  vi.useRealTimers();
});

it("sends a quote as a blockquote before the question, or asks to explain it alone", () => {
  const chip = quoteChip("answer-1", "Inventory is 54 days.");
  expect(chip.label).toBe("“Inventory is 54 days.”");
  expect(composeSubmission("Is that high?", [chip])).toBe("> Inventory is 54 days.\n\nIs that high?");
  expect(composeSubmission("", [chip])).toBe("> Inventory is 54 days.\n\nExplain this.");
});

it("turns a selection in a panel reader into a question chip, but not text outside readers", async () => {
  vi.useFakeTimers();
  const onQuote = vi.fn();
  render(<>
    <section className="workspace-panel" role="dialog" aria-label="Panel" data-quote-source="evidence:e1"><pre>Revenue grew 11% while margin fell to 14.8%.</pre></section>
    <p>Somewhere else in the window with plenty of words.</p>
    <QuoteAsk scope="reader" onQuote={onQuote} />
  </>);
  selectText(screen.getByText(/Revenue grew/).firstChild!, 0, 18);
  fireEvent.mouseUp(document);
  await act(async () => { vi.runAllTimers(); });
  const action = screen.getByRole("button", { name: "Add to question" });
  expect(screen.getByRole("dialog", { name: "Panel" })).toContainElement(action);
  fireEvent.click(action);
  expect(onQuote).toHaveBeenCalledWith("evidence:e1", "Revenue grew 11% w");
  selectText(screen.getByText(/Somewhere else/).firstChild!, 0, 25);
  fireEvent.mouseUp(document);
  await act(async () => { vi.runAllTimers(); });
  expect(screen.queryByRole("button", { name: "Add to question" })).not.toBeInTheDocument();
  vi.useRealTimers();
});

it("retains exact whitespace and UTF-16 offsets for a host-resolved Source selection and offers retry", async () => {
  const source = { loupistId: "a", recordId: "record", revisionId: "rev", sourceId: "source", spanId: "span", purpose: "renderer_inspection" };
  const onQuote = vi.fn().mockRejectedValueOnce(new Error("read failed")).mockResolvedValueOnce(undefined);
  render(<><blockquote data-quote-source="source:s" data-quote-retained-source={JSON.stringify(source)}>{"😀 Before. Revenue grew 11%\n  while margin fell."}</blockquote><QuoteAsk scope="reader" onQuote={onQuote} /></>);
  const passage = document.querySelector("blockquote")!;
  const node = passage.firstChild!;
  const start = 11, end = passage.textContent!.length;
  selectText(node, start, end);
  fireEvent.mouseUp(document);
  fireEvent.click(await screen.findByRole("button", { name: "Add to question" }));
  expect(onQuote).toHaveBeenCalledWith("source:s", "Revenue grew 11% while margin fell.", { source, selection: { startUtf16: start, endUtf16: end, text: passage.textContent!.slice(start, end) } });
  fireEvent.click(await screen.findByRole("button", { name: "Try adding again" }));
  await act(async () => {});
  expect(onQuote).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole("button", { name: "Try adding again" })).not.toBeInTheDocument();
});
