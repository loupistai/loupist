import { useState } from "react";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ChatMessage } from "../../types";
import { ChatSetAsideProvider } from "../../chat/chatSetAside";
import { MessageMemoryRow } from "./MessageMemoryRow";
import i18n from "@/i18n";

const answer = (overrides: Partial<ChatMessage> = {}): ChatMessage => ({
  id: "answer-1", role: "assistant", status: "complete", created_at: "2026-10-28T00:00:00Z",
  content: "Margins fell while cash conversion held, so I checked [Check cash flow before profit](lens:principle-cash) first.",
  lens_receipts: [{ recordId: "principle-cash", revisionId: "rev", kind: "principle", title: "Check cash flow before profit", delivery: "context" }],
  memory_update: { status: "no_change", commitId: null, changes: [] }, ...overrides,
});

function Harness({ messages, toggle }: { messages: ChatMessage[]; toggle: (recordId: string, value: boolean) => Promise<void> }) {
  const [ids, setIds] = useState<ReadonlySet<string>>(new Set());
  const firstDeclinedMessageId = messages.find(message => message.tool_refusals?.some(refusal => refusal.reason === "sensitive_memory"))?.id;
  const value = { ids, firstDeclinedMessageId, toggle: async (recordId: string, setAside: boolean) => {
    await toggle(recordId, setAside);
    setIds(current => { const next = new Set(current); if (setAside) next.add(recordId); else next.delete(recordId); return next; });
  } };
  return <ChatSetAsideProvider value={value}>
    {messages.map(message => <MessageMemoryRow key={message.id} loupistId="loupist-quality" chatId="chat-a" message={message} />)}
  </ChatSetAsideProvider>;
}

it("quotes a used principle and sets it aside for this chat until Undo", async () => {
  const user = userEvent.setup();
  const toggle = vi.fn(async () => {});
  render(<Harness messages={[answer()]} toggle={toggle} />);
  await user.click(screen.getByRole("button", { name: "Used 1 page" }));
  const panel = screen.getByText("Used · Principle").closest("li")!;
  expect(within(panel).getByText("Check cash flow before profit")).toBeVisible();
  await user.click(within(panel).getByRole("button", { name: "Not this time" }));
  expect(toggle).toHaveBeenCalledWith("principle-cash", true);
  expect(within(panel).getByRole("status")).toHaveTextContent("Set aside for this chat");
  await user.click(within(panel).getByRole("button", { name: "Undo" }));
  expect(toggle).toHaveBeenLastCalledWith("principle-cash", false);
  expect(within(panel).getByRole("button", { name: "Not this time" })).toBeVisible();
});

it("says once per chat that a secret was not saved to memory", () => {
  const refusal = { connection_id: "memory", connection_name: "Memory", tool_name: "note_save", reason: "sensitive_memory" as const };
  render(<Harness toggle={vi.fn()} messages={[
    answer({ id: "first", content: "Noted.", tool_refusals: [refusal] }),
    answer({ id: "second", content: "Still noted.", tool_refusals: [refusal] }),
  ]} />);
  expect(screen.getAllByRole("note")).toHaveLength(1);
  expect(screen.getByRole("note")).toHaveTextContent(/Skipped saving one item/);
});

it("labels a deleted receipt target without offering edit or archive actions", async () => {
  const { MockLoupistBridge } = await import("../../bridge/mockBridge");
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "knowledgeCatalog").mockResolvedValue({ loupistId: "loupist-quality", lenses: [], sources: [], worlds: [] });
  render(<MessageMemoryRow bridge={bridge} loupistId="loupist-quality" message={answer({ lens_receipts: [{ recordId: "deleted-method", revisionId: "rev", kind: "method", title: "Deleted method", delivery: "exact_read" }] })} />);
  await userEvent.click(screen.getByRole("button", { name: "Used 1 page" }));
  expect(await screen.findByText(/Removed/)).toBeVisible();
  expect(screen.getByText("Deleted method").tagName).toBe("S");
  expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Archive" })).toBeNull();
});

it("shows the default perspective under its interface-language name", async () => {
  await i18n.changeLanguage("ko");
  try {
    render(<Harness toggle={vi.fn()} messages={[answer({ lens_receipts: [{ recordId: "perspective-default", revisionId: "rev", kind: "perspective", title: "Outlook", delivery: "context" }] })]} />);
    await userEvent.click(screen.getByRole("button", { name: "페이지 1개 사용" }));
    const panel = screen.getByText("사용함 · 관점").closest("li")!;
    expect(within(panel).getByRole("blockquote")).toHaveTextContent(/^관점$/);
  } finally {
    await i18n.changeLanguage("en");
  }
});
