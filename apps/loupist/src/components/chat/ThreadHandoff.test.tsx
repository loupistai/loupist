import { act, render, screen, within } from "@testing-library/react";
import i18n from "../../i18n";
import userEvent from "@testing-library/user-event";
import type { ChatMessage, HomeHandoff } from "../../types";
import { ThreadHandoff } from "./ThreadHandoff";
import { renderWithOpen } from "../../test/open";

const card = (handoff: Partial<HomeHandoff>, content = "HBM4 supply widens in 2027."): ChatMessage => ({
  id: "handoff-card", role: "assistant", content, status: "complete", created_at: "2026-11-06T09:00:00Z", host_composed: true,
  handoff: { handoffId: "handoff-1", chatId: "chat-session", title: "Hynix HBM supply", status: "done", ...handoff },
});

it("posts a finished session's answer lead under its title, with Open and no status word", async () => {
  const { open: onOpen } = renderWithOpen(<ThreadHandoff message={card({})} loupistName="Quality" />);
  const article = screen.getByRole("article", { name: "Research: Hynix HBM supply" });
  expect(article).toHaveAttribute("data-status", "done");
  expect(within(article).getByText("Session ended: Hynix HBM supply")).toBeVisible();
  expect(within(article).getByText("HBM4 supply widens in 2027.")).toBeVisible();
  // The heading already says it finished; nothing in the post runs or stops.
  expect(within(article).queryByText("Done")).toBeNull();
  expect(within(article).queryByText(/Learned/)).toBeNull();
  expect(within(article).queryByRole("button", { name: /^Stop/ })).toBeNull();
  await userEvent.setup().click(within(article).getByRole("button", { name: "Open" }));
  expect(onOpen).toHaveBeenCalledExactlyOnceWith({ kind: "chat", threadId: "chat-session" });
});

it("lists the items a finished hand-off could not keep", () => {
  render(<ThreadHandoff message={card({ memorySaveSkip: "failed", discardedMemoryItems: [{ kind: "lens_page", title: "Supply forecast" }] }, "I saved the supply forecast.")} loupistName="Quality" />);
  const article = within(screen.getByRole("article", { name: "Research: Hynix HBM supply" }));
  expect(article.getByText("I saved the supply forecast.")).toBeVisible();
  expect(article.getByText("Not saved to Memory: saving failed.")).toBeVisible();
  expect(article.getByText("Anything this answer says it saved or filed was not kept.")).toBeVisible();
  expect(article.getByRole("list", { name: "Not kept" })).toHaveTextContent("Supply forecast");
});

it("counts what a finished session learned in the words of its answer's receipt", async () => {
  // The answer's receipt counts changed sections, not pages; the post says the same.
  const learned = { messageId: "answer-1", pages: [
    { recordId: "rec_a", title: "HBM", kind: "subject" as const, created: true, outcomes: [
      { opId: "op-1", outcome: "added", key: "supply" }, { opId: "op-2", outcome: "added", key: "pricing" }, { opId: "op-3", outcome: "kept", key: "intro" },
    ] },
    { recordId: "rec_b", title: "Supply checks", kind: "method" as const, created: false, outcomes: [{ opId: "op-4", outcome: "replaced", key: "steps" }] },
  ] };
  const { rerender } = render(<ThreadHandoff message={card({ learned })} loupistName="Quality" />);
  expect(screen.getByText("Learned 3 things")).toBeVisible();
  await act(async () => { await i18n.changeLanguage("ko"); });
  rerender(<ThreadHandoff message={card({ learned })} loupistName="Quality" />);
  expect(screen.getByText("새로 배운 것 3개")).toBeVisible();
  await act(async () => { await i18n.changeLanguage("en"); });
});

it("posts a failed session as a short status with Open and no preview", async () => {
  const { open: onOpen } = renderWithOpen(<ThreadHandoff message={card({ status: "failed" }, "Research on Hynix HBM supply could not finish.")} loupistName="Quality" />);
  const article = screen.getByRole("article", { name: "Research: Hynix HBM supply" });
  expect(article).toHaveAttribute("data-status", "failed");
  expect(within(article).getByText("Research: Hynix HBM supply")).toBeVisible();
  expect(within(article).getByText("Couldn't finish")).toBeVisible();
  expect(within(article).queryByText("Research on Hynix HBM supply could not finish.")).toBeNull();
  expect(within(article).queryByText(/Ask again/)).toBeNull();
  await userEvent.setup().click(within(article).getByRole("button", { name: "Open" }));
  expect(onOpen).toHaveBeenCalledExactlyOnceWith({ kind: "chat", threadId: "chat-session" });
});

it("posts a session stopped in its own view as Stopped, with no reason line", () => {
  renderWithOpen(<ThreadHandoff message={card({ status: "stopped" }, "Research on Hynix HBM supply could not finish.")} loupistName="Quality" />);
  const article = screen.getByRole("article", { name: "Research: Hynix HBM supply" });
  expect(article).toHaveAttribute("data-status", "stopped");
  expect(within(article).getByText("Stopped")).toBeVisible();
  expect(article.querySelector("[data-stop-reason]")).toBeNull();
  expect(within(article).getByRole("button", { name: "Open" })).toBeVisible();
});

it("says why a hand-off the host stopped before running did not run, and still opens it", async () => {
  const stopped = card({ status: "stopped", stopReason: "before_update" }, "Research on Hynix HBM supply stopped.");
  const { open: onOpen, rerender } = renderWithOpen(<ThreadHandoff message={stopped} loupistName="Quality" />);
  const article = screen.getByRole("article", { name: "Research: Hynix HBM supply" });
  expect(article).toHaveAttribute("data-status", "stopped");
  expect(within(article).getByText("Opened before an update. Ask again to run it.")).toBeVisible();
  expect(within(article).getByText("Stopped")).toBeVisible();
  await userEvent.setup().click(within(article).getByRole("button", { name: "Open" }));
  expect(onOpen).toHaveBeenCalledExactlyOnceWith({ kind: "chat", threadId: "chat-session" });

  rerender(<ThreadHandoff message={card({ status: "stopped", stopReason: "origin_unavailable" })} loupistName="Quality" />);
  expect(screen.getByText("The request it came from is no longer available. Ask again to run it.")).toBeVisible();
  rerender(<ThreadHandoff message={card({ status: "stopped", stopReason: "invalid_authority" })} loupistName="Quality" />);
  expect(screen.getByText("It couldn't start with the permissions it was given. Ask again to run it.")).toBeVisible();
  await act(async () => { await i18n.changeLanguage("ko"); });
  rerender(<ThreadHandoff message={stopped} loupistName="Quality" />);
  expect(screen.getByText("업데이트 전에 요청된 조사입니다. 실행하려면 다시 요청하십시오.")).toBeVisible();
  await act(async () => { await i18n.changeLanguage("en"); });
});

it("shows no preview for a finished session with no prose, rather than the host's English caption", async () => {
  const { rerender } = render(<ThreadHandoff message={card({}, "Researched: Hynix HBM supply.")} loupistName="Quality" />);
  const article = screen.getByRole("article", { name: "Research: Hynix HBM supply" });
  expect(within(article).getByText("Session ended: Hynix HBM supply")).toBeVisible();
  expect(within(article).queryByText("Researched: Hynix HBM supply.")).toBeNull();
  rerender(<ThreadHandoff message={card({}, "Session ended: Hynix HBM supply.")} loupistName="Quality" />);
  expect(within(article).queryByText("Session ended: Hynix HBM supply.")).toBeNull();
  // A link's hand-off keeps the host's caption under the action's name.
  rerender(<ThreadHandoff message={card({ connectionId: "con_1" }, "Researched: Look into it.")} loupistName="Quality" />);
  expect(screen.queryByText("Researched: Look into it.")).toBeNull();
  await act(async () => { await i18n.changeLanguage("ko"); });
  rerender(<ThreadHandoff message={card({}, "Researched: Hynix HBM supply.")} loupistName="Quality" />);
  expect(screen.getByText("세션 종료: Hynix HBM supply")).toBeVisible();
  expect(screen.queryByText(/Researched/)).toBeNull();
  await act(async () => { await i18n.changeLanguage("en"); });
});

it("wraps its status line only between items: each separator leads the item it belongs to", () => {
  renderWithOpen(<ThreadHandoff message={card({ learned: { messageId: "answer-1", pages: [
    { recordId: "rec_a", title: "HBM", kind: "subject", created: true, outcomes: [{ opId: "op-1", outcome: "added", key: "supply" }] },
  ] } })} loupistName="Quality" />);
  // Each item starts with its own silent "·"; the row clips the one that begins a line (bot-cards.css).
  const items = [...screen.getByRole("article").querySelectorAll(".thread-delivery-source .bot-meta-item")];
  expect(items.map((item) => item.textContent)).toEqual(["·Session ended: Hynix HBM supply", "·Learned 1 thing", "·Open"]);
  for (const item of items) expect(item.firstElementChild).toHaveAttribute("aria-hidden", "true");
});

it("says plainly that the app quit while a session ran, and still opens it", async () => {
  const interrupted = card({ status: "stopped", stopReason: "interrupted" }, "Research on Hynix HBM supply stopped.");
  const { open: onOpen, rerender } = renderWithOpen(<ThreadHandoff message={interrupted} loupistName="Quality" />);
  const article = screen.getByRole("article", { name: "Research: Hynix HBM supply" });
  expect(within(article).getByText("The app quit before this session finished.")).toBeVisible();
  expect(within(article).getByText("Interrupted")).toBeVisible();
  expect(within(article).queryByText("Stopped")).toBeNull();
  expect(within(article).queryByText(/Ask again/)).toBeNull();
  await userEvent.setup().click(within(article).getByRole("button", { name: "Open" }));
  expect(onOpen).toHaveBeenCalledExactlyOnceWith({ kind: "chat", threadId: "chat-session" });
  await act(async () => { await i18n.changeLanguage("ko"); });
  rerender(<ThreadHandoff message={interrupted} loupistName="Quality" />);
  expect(screen.getByText("세션이 끝나기 전에 앱이 종료되었습니다.")).toBeVisible();
  expect(screen.getByText("중단됨")).toBeVisible();
  await act(async () => { await i18n.changeLanguage("en"); });
});
