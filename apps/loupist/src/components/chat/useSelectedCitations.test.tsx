import { act, render, screen } from "@testing-library/react";
import { useSelectedCitations } from "./useSelectedCitations";

function Answer({ active = true }: { active?: boolean }) {
  useSelectedCitations(active);
  return <article className="message assistant" data-message-id="a1"><div className="message-content">
    <p>Revenue grew 11% <button type="button" className="loupist-citation">1</button> while margin fell to 14.8% <button type="button" className="loupist-citation">2</button>.</p>
  </div></article>;
}

function select(start: Node, startOffset: number, end: Node, endOffset: number) {
  const range = document.createRange();
  range.setStart(start, startOffset);
  range.setEnd(end, endOffset);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  act(() => { document.dispatchEvent(new Event("selectionchange")); });
}

it("marks the citation numbers a text selection runs over, and clears them when it goes", () => {
  const view = render(<Answer />);
  const [first, second] = screen.getAllByRole("button");
  const paragraph = first!.parentElement!;
  // From "Revenue" to "while margin": over the first number only.
  select(paragraph.firstChild!, 0, first!.nextSibling!, 13);
  expect(first).toHaveAttribute("data-in-selection");
  expect(second).not.toHaveAttribute("data-in-selection");
  // A selection that stops just before a number leaves it as it is.
  select(paragraph.firstChild!, 0, paragraph, 1);
  expect(first).not.toHaveAttribute("data-in-selection");
  select(paragraph.firstChild!, 0, paragraph, paragraph.childNodes.length);
  expect(first).toHaveAttribute("data-in-selection");
  expect(second).toHaveAttribute("data-in-selection");
  act(() => { document.getSelection()!.collapseToEnd(); document.dispatchEvent(new Event("selectionchange")); });
  expect(document.querySelector("[data-in-selection]")).toBeNull();
  // A hidden conversation leaves no marks behind.
  select(paragraph.firstChild!, 0, paragraph, paragraph.childNodes.length);
  view.rerender(<Answer active={false} />);
  expect(document.querySelector("[data-in-selection]")).toBeNull();
});
