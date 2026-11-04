import { render, screen } from "@testing-library/react";
import type { ChatThread } from "../../types";
import { LoupistChatItem } from "./LoupistChatItem";
import { SidebarProvider } from "../ui/sidebar";
import { TooltipProvider } from "../ui/tooltip";

it("marks unsent drafts and private chats in the sidebar", () => {
  const thread: ChatThread = {
    id: "chat-1", loupist_id: "loupist", title: "Hanul Q3 deep dive",
    updated_at: "2026-10-31T00:00:00Z", use_memory: true, update_memory: true, messages: [],
  };
  render(<TooltipProvider><SidebarProvider><ul>
    <LoupistChatItem thread={thread} active={false} running={false} draft onSelect={vi.fn()} onRename={vi.fn()} onDelete={vi.fn()} />
    <LoupistChatItem thread={{ ...thread, id: "chat-2", title: "Private notes", use_memory: false, update_memory: false }} active={false} running={false} onSelect={vi.fn()} onRename={vi.fn()} onDelete={vi.fn()} />
  </ul></SidebarProvider></TooltipProvider>);
  expect(screen.getByRole("img", { name: "Unsent draft" })).toBeInTheDocument();
  expect(screen.getAllByRole("img", { name: "Private chat" })).toHaveLength(1);
});
