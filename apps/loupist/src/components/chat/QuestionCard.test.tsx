import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatQuestion } from "../../contracts/chat";
import { QuestionCards } from "./QuestionCard";

const waiting = (extra: Partial<ChatQuestion> = {}): ChatQuestion => ({
  requestId: "q-1", question: "Compare on which period?", state: "waiting", requestedAtMs: Date.now() - 12_000, recommended: 0,
  options: [{ label: "Last 4 quarters", description: "Trailing twelve months" }, { label: "Last 3 years" }], ...extra,
});

const setup = () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const answer = vi.spyOn(bridge, "chatQuestionAnswer").mockResolvedValue();
  const view = (question: ChatQuestion) => <QuestionCards questions={[question]} bridge={bridge} loupistId="loupist-one" />;
  return { answer, view, ...render(view(waiting())) };
};

it("answers with an option, marks the recommended one and shows the wait", async () => {
  const user = userEvent.setup();
  const { answer } = setup();
  const card = screen.getByRole("region", { name: "Question: Compare on which period?" });
  expect(within(card).getByText("Compare on which period?")).toBeVisible();
  expect(within(card).getByText(/^waiting 0:1\d$/)).toBeVisible();
  const options = within(within(card).getByRole("group", { name: "Options" })).getAllByRole("button");
  expect(options.map((option) => option.textContent)).toEqual(["Last 4 quartersRecommendedTrailing twelve months", "Last 3 years"]);
  expect(options[0]).toHaveAttribute("data-variant", "default");
  expect(options[1]).toHaveAttribute("data-variant", "outline");
  await user.click(options[1]);
  expect(answer).toHaveBeenCalledWith({ loupist_id: "loupist-one", request_id: "q-1", choice: 1 });
  expect(answer.mock.calls[0][0]).not.toHaveProperty("text");
});

it("answers in the user's own words with Enter or Send and never sends an empty answer", async () => {
  const user = userEvent.setup();
  const { answer } = setup();
  const send = screen.getByRole("button", { name: "Send" });
  const field = screen.getByRole("textbox", { name: "Or answer in your own words" });
  expect(send).toBeDisabled();
  await user.type(field, "   ");
  expect(send).toBeDisabled();
  await user.type(field, "Since the 2023 listing{Enter}");
  expect(answer).toHaveBeenCalledWith({ loupist_id: "loupist-one", request_id: "q-1", text: "Since the 2023 listing" });
  expect(answer.mock.calls[0][0]).not.toHaveProperty("choice");
});

it("settles into one line once the host answers or the wait ends", () => {
  const { view, rerender } = setup();
  rerender(view(waiting({ state: "answered", answer: { choice: 1 } })));
  expect(screen.queryByRole("region")).not.toBeInTheDocument();
  expect(screen.getByText("You answered: Last 3 years")).toBeVisible();
  rerender(view(waiting({ state: "answered", answer: { text: "Since the 2023 listing" } })));
  expect(screen.getByText("You answered: Since the 2023 listing")).toBeVisible();
  rerender(view(waiting({ state: "expired" })));
  expect(screen.getByText("No answer · continued with Last 4 quarters")).toBeVisible();
  rerender(view(waiting({ state: "expired", recommended: undefined })));
  expect(screen.getByText("No answer · continued with its own assumption")).toBeVisible();
});

it("says when the question is no longer waiting and lets the user try again", async () => {
  const user = userEvent.setup();
  const { answer } = setup();
  answer.mockRejectedValueOnce(new Error("This question is no longer waiting"));
  await user.click(screen.getByRole("button", { name: /^Last 3 years/ }));
  expect(await screen.findByRole("alert")).toHaveTextContent("This question is no longer waiting.");
  expect(screen.getByRole("button", { name: /^Last 3 years/ })).toBeEnabled();
});
