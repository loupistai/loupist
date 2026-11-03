import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatMessage } from "../../types";

const delivered: ChatMessage = {
  id: "delivery-1", role: "assistant", status: "complete", host_composed: true, created_at: new Date().toISOString(),
  content: "Hanul Precision's inventory days rose to 61.",
  delivery: { run_id: "run-1", task_id: "task-1", task_title: "Inventory check", task_kind: "custom", schedule: { kind: "weekly", days: [1], hour: 8, minute: 0, timezone: "Asia/Seoul" }, run_thread_id: "thread-margin", status: "succeeded", finished_at_ms: Date.now() },
};

it("lists the home Thread first, lands on it, and opens a delivered result's own Chat", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await select(id);
    const home = workspace.threads.find(thread => thread.home);
    if (home && id === "loupist-quality") home.messages = [delivered];
    return workspace;
  });
  const markRead = vi.spyOn(bridge, "inboxMarkRead").mockResolvedValue();
  render(<App bridge={bridge} />);

  expect(await screen.findByRole("heading", { name: "Bot" })).toBeVisible();
  const chats = document.querySelector<HTMLElement>(".loupist-navigation")!;
  expect(within(chats).getAllByRole("button").map(button => button.getAttribute("aria-label")).filter(Boolean)[0]).toBe("Bot");
  // The home Thread has no rename or delete menu.
  expect(screen.queryByRole("button", { name: "Options for Thread" })).toBeNull();

  const result = await screen.findByRole("article", { name: "Result from Inventory check" });
  expect(within(result).getByText("Hanul Precision's inventory days rose to 61.")).toBeVisible();
  expect(within(result).getByText(/^From: Inventory check · /)).toBeVisible();
  await vi.waitFor(() => expect(markRead).toHaveBeenCalledWith({ loupistId: "loupist-quality", runIds: ["run-1"] }), { timeout: 2000 });

  await user.click(within(result).getByRole("button", { name: "Open" }));
  expect(await screen.findByRole("heading", { name: "How should we read the margin decline?" })).toBeVisible();
});

it("lists and selects a new topic chat under the Thread, and its title says which chat it is", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  render(<App bridge={bridge} />);
  expect(await screen.findByRole("heading", { name: "Bot", level: 1 })).toBeVisible();
  const chats = document.querySelector<HTMLElement>(".loupist-navigation")!;
  await user.click(screen.getByRole("button", { name: "New chat with Quality Compounder" }));
  expect(await screen.findByRole("heading", { name: "New chat", level: 1 })).toBeVisible();
  const labels = [...chats.querySelectorAll(".loupist-chat-button")].map(button => button.getAttribute("aria-label"));
  expect(labels.slice(0, 2)).toEqual(["Bot", "New chat"]);
  expect(within(chats).getByRole("button", { name: "New chat" })).toHaveAttribute("aria-current", "page");
  // An unsent chat has nothing to rename or delete yet.
  expect(within(chats).queryByRole("button", { name: "Options for New chat" })).toBeNull();
});
