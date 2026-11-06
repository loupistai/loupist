import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ChatMessage, HomeHandoff } from "../../types";
import { ThreadHandoff } from "./ThreadHandoff";

const card = (handoff: Partial<HomeHandoff>, content = "HBM4 supply widens in 2027."): ChatMessage => ({
  id: "handoff-card", role: "assistant", content, status: "complete", created_at: "2026-11-06T09:00:00Z", host_composed: true,
  handoff: { handoffId: "handoff-1", chatId: "chat-session", title: "Hynix HBM supply", status: "done", ...handoff },
});

it("shows the finished session's title, status, first paragraph and Open", async () => {
  const onOpen = vi.fn();
  render(<ThreadHandoff message={card({})} loupistName="Quality" onOpenRunChat={onOpen} />);
  const article = screen.getByRole("article", { name: "Research: Hynix HBM supply" });
  expect(within(article).getByText("Researched: Hynix HBM supply")).toBeVisible();
  expect(within(article).getByText("Done")).toBeVisible();
  expect(within(article).getByText("HBM4 supply widens in 2027.")).toBeVisible();
  expect(within(article).queryByText(/Learned/)).toBeNull();
  await userEvent.setup().click(within(article).getByRole("button", { name: "Open" }));
  expect(onOpen).toHaveBeenCalledExactlyOnceWith("chat-session");
});

it("shows a running session without an answer and counts what a finished one learned", () => {
  const { rerender } = render(<ThreadHandoff message={card({ status: "running" }, "Researching: Hynix HBM supply.")} loupistName="Quality" />);
  expect(screen.getByText("Researching: Hynix HBM supply")).toBeVisible();
  expect(screen.getByText("Running")).toBeVisible();
  expect(screen.queryByText("Researching: Hynix HBM supply.")).toBeNull();
  expect(screen.queryByRole("button", { name: "Open" })).toBeNull();

  const learned = { messageId: "answer-1", pages: [
    { recordId: "rec_a", title: "HBM", kind: "subject", created: true, outcomes: [] },
    { recordId: "rec_b", title: "Supply checks", kind: "method", created: false, outcomes: [] },
  ] };
  rerender(<ThreadHandoff message={card({ learned })} loupistName="Quality" />);
  expect(screen.getByText("Learned 2")).toBeVisible();

  rerender(<ThreadHandoff message={card({ status: "failed" }, "Research on Hynix HBM supply could not finish.")} loupistName="Quality" />);
  expect(screen.getByText("Couldn't finish")).toBeVisible();
  expect(screen.getByRole("article")).toHaveAttribute("data-status", "failed");
});
