import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ChatMessage, ChatThread } from "../../types";
import { LoupistChatItem } from "./LoupistChatItem";
import { SidebarProvider } from "../ui/sidebar";
import { TooltipProvider } from "../ui/tooltip";
import "../../styles/tokens.css";

const thread: ChatThread = {
  id: "chat-1", loupist_id: "loupist", title: "Hanul Q3 deep dive",
  updated_at: "2026-10-31T00:00:00Z", use_memory: true, update_memory: true, messages: [],
};

const row = (props: Partial<Parameters<typeof LoupistChatItem>[0]> = {}) => (
  <LoupistChatItem thread={thread} active={false} running={false} onSelect={vi.fn()} onRename={vi.fn()} onDelete={vi.fn()} {...props} />
);
const inSidebar = (rows: React.ReactNode) => <TooltipProvider><SidebarProvider><ul>{rows}</ul></SidebarProvider></TooltipProvider>;

it("marks unsent drafts and private chats in the sidebar, and describes the row with them", () => {
  render(inSidebar(<>{row({ draft: true })}{row({ thread: { ...thread, id: "chat-2", title: "Private notes", use_memory: false, update_memory: false } })}</>));
  expect(screen.getByRole("img", { name: "Unsent draft" })).toBeInTheDocument();
  expect(screen.getAllByRole("img", { name: "Private chat" })).toHaveLength(1);
  expect(screen.getByRole("button", { name: "Hanul Q3 deep dive" })).toHaveAccessibleDescription("Unsent draft");
  expect(screen.getByRole("button", { name: "Private notes" })).toHaveAccessibleDescription("Private chat");
});

it("shows the app's spinner in the status slot while a reply runs, named once in the row's description", () => {
  const { rerender } = render(inSidebar(row({ running: true, finished: true, thread: { ...thread, use_memory: false, update_memory: false } })));
  const button = screen.getByRole("button", { name: "Hanul Q3 deep dive" });
  const spinner = button.querySelector("[data-slot=spinner]")!;
  // Running wins the one status slot over a finished dot and the private mark.
  expect(spinner).toHaveClass("thread-running", "animate-spin");
  expect(spinner).toHaveAccessibleName("Running");
  expect(button.querySelector(".loupist-status-dot, .loupist-private-mark")).toBeNull();
  // A fixed name, not a live region: nothing is announced as the ring turns.
  expect(spinner).toHaveAttribute("role", "img");
  expect(button).toHaveAccessibleDescription("Running");
  expect(button).not.toHaveAttribute("aria-busy");

  rerender(inSidebar(row({ running: false, finished: true })));
  expect(button.querySelector("[data-slot=spinner]")).toBeNull();
  expect(button).toHaveAccessibleDescription("Finished while you were away");
});

it("opens Bot from the collapsed rail when its icon is hovered", async () => {
  const user = userEvent.setup();
  const onSelect = vi.fn();
  const bot: ChatThread = { ...thread, id: "home-loupist", home: true, title: "Thread" };
  render(inSidebar(row({ thread: bot, iconOnly: true, onSelect, onSchedule: vi.fn() })));
  const icon = screen.getByRole("button", { name: "Bot" });
  await user.hover(icon);
  expect(screen.queryByRole("button", { name: "Options for Bot" })).toBeNull();
  await user.click(icon);
  expect(onSelect).toHaveBeenCalledOnce();
  expect(onSelect).toHaveBeenCalledWith(bot);
});

it("keeps Bot's scheduled-work options on the expanded row", () => {
  const bot: ChatThread = { ...thread, id: "home-loupist", home: true, title: "Thread" };
  render(inSidebar(row({ thread: bot, onSchedule: vi.fn() })));
  expect(screen.getByRole("button", { name: "Options for Bot" })).toBeInTheDocument();
});

it("keeps Bot's name and descriptor while it runs, in the rail too", () => {
  const bot: ChatThread = { ...thread, id: "home-loupist", home: true, title: "Thread" };
  const { rerender } = render(inSidebar(row({ thread: bot, running: true })));
  expect(screen.getByRole("button", { name: "Bot" })).toHaveAccessibleDescription("Chat and scheduled task results Running");
  rerender(inSidebar(row({ thread: bot, running: true, iconOnly: true })));
  expect(screen.getByRole("button", { name: "Bot" })).toHaveAccessibleDescription("Running");
});

it("stills the spinner with reduced motion", () => {
  render(inSidebar(row({ running: true })));
  const spinner = screen.getByRole("img", { name: "Running" });
  // The tokens' reduced-motion guard is what stops it: a rule there matches this element.
  const stills = [...document.styleSheets].flatMap((sheet) => [...sheet.cssRules])
    .filter((rule): rule is CSSMediaRule => rule instanceof CSSMediaRule && rule.conditionText.includes("prefers-reduced-motion: reduce"))
    .flatMap((media) => [...media.cssRules])
    .filter((rule): rule is CSSStyleRule => rule instanceof CSSStyleRule && spinner.matches(rule.selectorText) && /^none\b/.test(rule.style.getPropertyValue("animation")));
  expect(stills).not.toHaveLength(0);
});


it("describes only a continued session that began with a scheduled request", () => {
  const question: ChatMessage = { id: "question", role: "user", status: "complete", content: "Why?", created_at: thread.updated_at };
  const request: ChatMessage = { ...question, id: "scheduled", scheduled: { task_id: "task", run_id: "run", task_title: "Check", task_kind: "custom" } };
  const { rerender } = render(inSidebar(row({ thread: { ...thread, messages: [request, question] } })));
  const button = screen.getByRole("button", { name: thread.title });
  expect(button).toHaveAccessibleDescription("Started by a schedule");
  for (const messages of [[question, request], [request]]) {
    rerender(inSidebar(row({ thread: { ...thread, messages } })));
    expect(button).not.toHaveAccessibleDescription("Started by a schedule");
  }
});
