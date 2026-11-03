import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ChatThread } from "../../types";
import { ChatTitle } from "./ChatTitle";
import { LoupistChatItem } from "../app/LoupistChatItem";
import { SidebarProvider } from "../ui/sidebar";
import { TooltipProvider } from "../ui/tooltip";

const thread = (extra: Partial<ChatThread> = {}): ChatThread => ({ id: "chat-1", loupist_id: "loupist", title: "Hanul Q3 deep dive", updated_at: "2026-10-31T00:00:00Z", use_memory: true, update_memory: true, messages: [], ...extra });

it("renames the chat in place: Enter saves, Escape cancels", async () => {
  const user = userEvent.setup();
  const onRename = vi.fn(async () => true);
  render(<ChatTitle thread={thread()} title="Hanul Q3 deep dive" privateChat={false} onRename={onRename} />);
  await user.click(screen.getByRole("button", { name: "Rename chat: Hanul Q3 deep dive" }));
  const field = screen.getByRole("textbox", { name: "Chat title" });
  expect(field).toHaveFocus();
  await user.keyboard("{Escape}");
  expect(onRename).not.toHaveBeenCalled();
  expect(screen.queryByRole("textbox", { name: "Chat title" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Rename chat: Hanul Q3 deep dive" }));
  await user.clear(screen.getByRole("textbox", { name: "Chat title" }));
  await user.type(screen.getByRole("textbox", { name: "Chat title" }), "Margin review{Enter}");
  expect(onRename).toHaveBeenCalledWith(expect.objectContaining({ id: "chat-1" }), "Margin review");
});

it("keeps the home Thread's name fixed and marks a private chat", () => {
  render(<ChatTitle thread={thread({ home: true, title: "Thread" })} title="Thread" privateChat onRename={vi.fn()} />);
  expect(screen.queryByRole("button", { name: /Rename chat/ })).not.toBeInTheDocument();
  expect(screen.getByRole("img", { name: "Private chat" })).toBeInTheDocument();
});

it("marks unsent drafts and private chats in the sidebar", () => {
  render(<TooltipProvider><SidebarProvider><ul>
    <LoupistChatItem thread={thread()} active={false} running={false} draft onSelect={vi.fn()} onRename={vi.fn()} onDelete={vi.fn()} />
    <LoupistChatItem thread={thread({ id: "chat-2", title: "Private notes", use_memory: false, update_memory: false })} active={false} running={false} onSelect={vi.fn()} onRename={vi.fn()} onDelete={vi.fn()} />
  </ul></SidebarProvider></TooltipProvider>);
  expect(screen.getByRole("img", { name: "Unsent draft" })).toBeInTheDocument();
  expect(screen.getAllByRole("img", { name: "Private chat" })).toHaveLength(1);
});
