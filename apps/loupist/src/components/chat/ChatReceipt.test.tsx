import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ChatEvidenceRef, MemoryUpdateChange } from "../../types";
import { chatReceipt, type ChatReceiptData } from "../../chat/receipt";
import { ChatReceipt } from "./ChatReceipt";

const source = (id: string): ChatEvidenceRef => ({ id, title: `Report ${id}`, kind: "original", tool_name: "web_fetch", url: `https://hanul.example/${id}`, retrieved_at: "2026-10-28T00:00:00Z", excerpt: "", excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [] });
const saved = (kind: "Source" | "Lens", recordId: string) => ({ kind, recordId, operation: "create", title: recordId, lesson: "", basis: "", futureUse: "", knowledge: { kind: kind === "Lens" ? "lens" : "source", recordId, revisionId: "r", objectId: recordId } }) as MemoryUpdateChange;
const empty: ChatReceiptData = { readSources: [], names: 0, relationships: 0, thesisMoves: [], unsavedTheses: [], mapHighlightIds: new Set(), usedPrinciples: [], usedMethods: [], saved: [], proposals: [], numbersToCheck: 0, empty: true };

function renderReceipt(receipt: Partial<ChatReceiptData>, showNothingNew = false) {
  const handlers = { onOpenSource: vi.fn(), onShowOnMap: vi.fn(), onToggleUsed: vi.fn(), usedListId: "used", usedExpanded: false, onSaved: vi.fn(), onToggleList: vi.fn() };
  render(<ChatReceipt receipt={{ ...empty, ...receipt, empty: false }} showNothingNew={showNothingNew} listId="list" listExpanded={false} {...handlers} />);
  return handlers;
}

it("lists what the answer used and saved in a fixed order, each segment a link", async () => {
  const highlight = new Set(["a", "b"]);
  const handlers = renderReceipt({
    readSources: [source("q3")], names: 3, relationships: 1, mapHighlightIds: highlight,
    usedPrinciples: [{ recordId: "p", title: "Cash before profit" }],
    saved: [{ kind: "Source", items: [saved("Source", "s")] }, { kind: "Lens", items: [saved("Lens", "m")] }],
  });
  const receipt = screen.getByRole("navigation", { name: "What this answer used and saved" });
  expect(receipt).toHaveTextContent("Read 1 source·+3 on the map·1 relationship updated·Used 1 of your rules·Saved 1 source·Learned 1 new way of working");
  const user = userEvent.setup();
  await user.click(within(receipt).getByRole("button", { name: "Read 1 source" }));
  expect(handlers.onOpenSource).toHaveBeenCalledWith(expect.objectContaining({ id: "q3" }), expect.any(HTMLElement));
  await user.click(within(receipt).getByRole("button", { name: /Show 3 new on the map/ }));
  await user.click(within(receipt).getByRole("button", { name: "1 relationship updated on the map" }));
  expect(handlers.onShowOnMap).toHaveBeenNthCalledWith(2, highlight);
  // "Used 1 principle" expands the correction panel under the answer.
  const used = within(receipt).getByRole("button", { name: "Used 1 of your rules" });
  expect(used).toHaveAttribute("aria-expanded", "false");
  await user.click(used);
  expect(handlers.onToggleUsed).toHaveBeenCalled();
  await user.click(within(receipt).getByRole("button", { name: "Learned 1 new way of working" }));
  expect(handlers.onSaved).toHaveBeenCalledWith(expect.objectContaining({ kind: "Lens" }));
});

it("opens a short list when one segment stands for several sources", async () => {
  const handlers = renderReceipt({ readSources: [source("a"), source("b")] });
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "Read 2 sources" }));
  const list = await screen.findByRole("dialog", { name: "Sources read in this answer" });
  await user.click(within(list).getByRole("button", { name: /Report b/ }));
  expect(handlers.onOpenSource).toHaveBeenCalledWith(expect.objectContaining({ id: "b" }), expect.any(HTMLElement));
});

it("says nothing new was saved only when the turn could save", () => {
  const { rerender } = render(<ChatReceipt receipt={empty} showNothingNew listId="list" listExpanded={false} usedListId="used" usedExpanded={false} onToggleUsed={vi.fn()} onSaved={vi.fn()} onToggleList={vi.fn()} />);
  expect(screen.getByRole("navigation", { name: "What this answer used and saved" })).toHaveTextContent("Nothing new saved");
  rerender(<ChatReceipt receipt={empty} showNothingNew={false} listId="list" listExpanded={false} usedListId="used" usedExpanded={false} onToggleUsed={vi.fn()} onSaved={vi.fn()} onToggleList={vi.fn()} />);
  expect(screen.queryByRole("navigation")).toBeNull();
});

it("keeps sources and map growth as plain text where no reader is available", () => {
  render(<ChatReceipt receipt={{ ...empty, readSources: [source("a")], names: 2, empty: false }} showNothingNew={false} listId="list" listExpanded={false} usedListId="used" usedExpanded={false} onToggleUsed={vi.fn()} onSaved={vi.fn()} onToggleList={vi.fn()} />);
  expect(screen.queryByRole("button")).toBeNull();
  expect(screen.getByRole("navigation")).toHaveTextContent("Read 1 source·+2 on the map");
});

it("says a read the answer changed was not saved when Rust could not keep it", () => {
  // The message as Rust returns it: the commit recorded the update as not saved.
  const message = { id: "answer", role: "assistant", content: "Moved Hanul Precision to Weakening.", created_at: "2026-10-28T00:00:00Z", status: "complete",
    unsaved_theses: [{ assetId: "ast_0123456789abcdef", name: "Hanul Precision", reason: "thesis_after_undo" }] } as unknown as import("../../types").ChatMessage;
  const receipt = chatReceipt(message);
  expect(receipt.empty).toBe(false);
  render(<ChatReceipt receipt={receipt} showNothingNew={false} listId="list" listExpanded={false} usedListId="used" usedExpanded={false} onToggleUsed={vi.fn()} onSaved={vi.fn()} onToggleList={vi.fn()} />);
  expect(screen.getByRole("navigation", { name: "What this answer used and saved" })).toHaveTextContent("Hanul Precision: read not changed (the page changed first)");
});

it("names a moved read after the relationships and opens the asset page at its history", async () => {
  const onOpenThesis = vi.fn();
  const move = { assetId: "ast_0123456789abcdef", name: "Hanul Precision", verdict: "weakening" as const };
  render(<ChatReceipt receipt={{ ...empty, relationships: 1, thesisMoves: [move], empty: false }} showNothingNew={false} listId="list" listExpanded={false}
    usedListId="used" usedExpanded={false} onToggleUsed={vi.fn()} onSaved={vi.fn()} onToggleList={vi.fn()} onOpenThesis={onOpenThesis} />);
  const receipt = screen.getByRole("navigation", { name: "What this answer used and saved" });
  expect(receipt).toHaveTextContent("1 relationship updated·Hanul Precision: Weakening");
  await userEvent.setup().click(within(receipt).getByRole("button", { name: "Hanul Precision: Weakening" }));
  expect(onOpenThesis).toHaveBeenCalledWith(move);
});
