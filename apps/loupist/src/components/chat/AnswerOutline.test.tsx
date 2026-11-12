import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AnswerOutline } from "./AnswerOutline";

const markdown = "## Demand\n\nOne.\n\n## Risks\n\nTwo.\n\n## Demand\n\nThree.";

beforeEach(() => { Element.prototype.scrollIntoView = vi.fn(); });

it("jumps past a narration preamble to the chosen heading, telling apart headings of the same name", async () => {
  const user = userEvent.setup();
  render(<article data-message-id="answer">
    <div className="message-content message-narration"><p>Reading the filing first.</p><h2>Demand</h2></div>
    <div className="message-content"><h2>Demand</h2><p>One.</p><h2>Risks</h2></div>
    <div className="message-content"><p>Two.</p><h2>Demand</h2><p>Three.</p></div>
    <AnswerOutline messageId="answer" markdown={markdown} />
  </article>);
  const answerHeadings = [...document.querySelectorAll(".message-content:not(.message-narration) h2")];
  await user.click(screen.getByRole("button", { name: "Outline" }));
  await user.click((await screen.findAllByRole("button", { name: "Demand" }))[1]!);
  expect(answerHeadings[2]).toHaveFocus();
  await user.click(screen.getByRole("button", { name: "Outline" }));
  await user.click((await screen.findAllByRole("button", { name: "Demand" }))[0]!);
  expect(answerHeadings[0]).toHaveFocus();
});
