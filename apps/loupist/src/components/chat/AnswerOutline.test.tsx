import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AnswerOutline } from "./AnswerOutline";

// The thread's scroll actions: revealing something in the answer keeps the page there.
const scroll = vi.hoisted(() => ({ holdScrollPosition: vi.fn(), jumpToLatest: vi.fn(), blockAppended: vi.fn() }));
vi.mock("../ai-elements/conversation", async (load) => ({ ...(await load<typeof import("../ai-elements/conversation")>()), useConversationScroll: () => scroll }));

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
  // The trigger sits after the answer's time: the list opens from it toward the answer.
  expect(await screen.findByRole("dialog", { name: "Outline" })).toHaveAttribute("data-align", "start");
  await user.click((await screen.findAllByRole("button", { name: "Demand" }))[1]!);
  expect(answerHeadings[2]).toHaveFocus();
  // A chat opened at its end stays at the heading when it grows later.
  expect(scroll.holdScrollPosition).toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Outline" }));
  await user.click((await screen.findAllByRole("button", { name: "Demand" }))[0]!);
  expect(answerHeadings[0]).toHaveFocus();
});
