import { act, render, screen, within } from "@testing-library/react";
import i18n from "../../i18n";
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

it("shows a running session without an answer and counts what a finished one learned", async () => {
  const { rerender } = render(<ThreadHandoff message={card({ status: "running" }, "Researching: Hynix HBM supply.")} loupistName="Quality" />);
  expect(screen.getByText("Researching: Hynix HBM supply")).toBeVisible();
  expect(screen.getByText("Running")).toBeVisible();
  expect(screen.queryByText("Researching: Hynix HBM supply.")).toBeNull();
  expect(screen.queryByRole("button", { name: "Open" })).toBeNull();

  // The answer's receipt counts changed sections, not pages; the card says the same.
  const learned = { messageId: "answer-1", pages: [
    { recordId: "rec_a", title: "HBM", kind: "subject" as const, created: true, outcomes: [
      { opId: "op-1", outcome: "added", key: "supply" }, { opId: "op-2", outcome: "added", key: "pricing" }, { opId: "op-3", outcome: "kept", key: "intro" },
    ] },
    { recordId: "rec_b", title: "Supply checks", kind: "method" as const, created: false, outcomes: [{ opId: "op-4", outcome: "replaced", key: "steps" }] },
  ] };
  rerender(<ThreadHandoff message={card({ learned })} loupistName="Quality" />);
  expect(screen.getByText("Learned 3")).toBeVisible();
  await act(async () => { await i18n.changeLanguage("ko"); });
  rerender(<ThreadHandoff message={card({ learned })} loupistName="Quality" />);
  expect(screen.getByText("새로 배운 것 3개")).toBeVisible();
  await act(async () => { await i18n.changeLanguage("en"); });

  rerender(<ThreadHandoff message={card({ status: "failed" }, "Research on Hynix HBM supply could not finish.")} loupistName="Quality" />);
  expect(screen.getByText("Couldn't finish")).toBeVisible();
  expect(screen.getByRole("article")).toHaveAttribute("data-status", "failed");
});
