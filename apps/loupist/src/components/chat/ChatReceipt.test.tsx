import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import type { ChatEvidenceRef, MemoryUpdateChange } from "../../types";
import { chatReceipt, type ChatReceiptData } from "../../chat/receipt";
import { ChatReceipt } from "./ChatReceipt";

const source = (id: string): ChatEvidenceRef => ({ id, title: `Report ${id}`, kind: "original", tool_name: "web_fetch", url: `https://hanul.example/${id}`, retrieved_at: "2026-10-28T00:00:00Z", excerpt: "", excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [] });
const saved = (kind: "Source" | "principle", recordId: string) => ({ kind, recordId, operation: "create", title: recordId, lesson: "", basis: "", futureUse: "", knowledge: { kind: kind === "principle" ? "lens" : "source", recordId, revisionId: "r", objectId: recordId } }) as MemoryUpdateChange;
const empty: ChatReceiptData = { readSources: [], names: 0, relationships: 0, thesisMoves: [], unsavedTheses: [], skippedKinds: [], setup: [], mapHighlightIds: new Set(), usedPrinciples: [], usedMethods: [], usedPages: [], learned: null, saved: [], numbersToCheck: 0, empty: true };
const panels = { learnedListId: "learned", learnedExpanded: false, onToggleLearned: vi.fn() };

function renderReceipt(receipt: Partial<ChatReceiptData>, showNothingNew = false) {
  const handlers = { onOpenSource: vi.fn(), onShowOnMap: vi.fn(), onToggleUsed: vi.fn(), usedListId: "used", usedExpanded: false, onSaved: vi.fn(), ...panels, onToggleLearned: vi.fn() };
  render(<ChatReceipt receipt={{ ...empty, ...receipt, empty: false }} showNothingNew={showNothingNew} listId="list" listExpanded={false} {...handlers} />);
  return handlers;
}

it("lists what the answer learned, put on the map and used in a fixed order, each segment a link", async () => {
  const highlight = new Set(["a", "b"]);
  const learned = { sections: 3, ideas: 1, pages: [] };
  const handlers = renderReceipt({
    readSources: [source("q3")], names: 3, relationships: 1, mapHighlightIds: highlight, learned,
    usedPages: [{ recordId: "p", title: "Cash before profit", kind: "principle" }, { recordId: "o", title: "Evidence before conviction", kind: "perspective" }],
    saved: [{ kind: "Source", items: [saved("Source", "s")] }],
  });
  const receipt = screen.getByRole("navigation", { name: "What this answer used and saved" });
  expect(receipt).toHaveTextContent("Read 1 source·Learned 3·Map +3 names·1 relationship updated·1 idea·Used 2 pages·Saved 1 source");
  const user = userEvent.setup();
  await user.click(within(receipt).getByRole("button", { name: "Read 1 source" }));
  expect(handlers.onOpenSource).toHaveBeenCalledWith(expect.objectContaining({ id: "q3" }), expect.any(HTMLElement));
  await user.click(within(receipt).getByRole("button", { name: /Show 3 new on the map/ }));
  await user.click(within(receipt).getByRole("button", { name: "1 relationship updated on the map" }));
  expect(handlers.onShowOnMap).toHaveBeenNthCalledWith(2, highlight);
  const used = within(receipt).getByRole("button", { name: "Used 2 pages" });
  expect(used).toHaveAttribute("aria-expanded", "false");
  await user.click(used);
  expect(handlers.onToggleUsed).toHaveBeenCalled();
  await user.click(within(receipt).getByRole("button", { name: "Learned 3" }));
  await user.click(within(receipt).getByRole("button", { name: "1 idea" }));
  expect(handlers.onToggleLearned).toHaveBeenCalledTimes(2);
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
  const { rerender } = render(<ChatReceipt receipt={empty} showNothingNew listId="list" listExpanded={false} usedListId="used" usedExpanded={false} onToggleUsed={vi.fn()} onSaved={vi.fn()} {...panels} />);
  expect(screen.getByRole("navigation", { name: "What this answer used and saved" })).toHaveTextContent("Nothing new saved");
  rerender(<ChatReceipt receipt={empty} showNothingNew={false} listId="list" listExpanded={false} usedListId="used" usedExpanded={false} onToggleUsed={vi.fn()} onSaved={vi.fn()} {...panels} />);
  expect(screen.queryByRole("navigation")).toBeNull();
});

it("keeps sources and map growth as plain text where no reader is available", () => {
  render(<ChatReceipt receipt={{ ...empty, readSources: [source("a")], names: 2, empty: false }} showNothingNew={false} listId="list" listExpanded={false} usedListId="used" usedExpanded={false} onToggleUsed={vi.fn()} onSaved={vi.fn()} {...panels} />);
  expect(screen.queryByRole("button")).toBeNull();
  expect(screen.getByRole("navigation")).toHaveTextContent("·Read 1 source·Map +2 names");
});

it("says a read the answer changed was not saved when Rust could not keep it", () => {
  // The message as Rust returns it: the commit recorded the update as not saved.
  const message = { id: "answer", role: "assistant", content: "Moved Hanul Precision to Weakening.", created_at: "2026-10-28T00:00:00Z", status: "complete",
    unsaved_theses: [{ assetId: "ast_0123456789abcdef", name: "Hanul Precision", reason: "thesis_after_undo" }] } as unknown as import("../../types").ChatMessage;
  const receipt = chatReceipt(message);
  expect(receipt.empty).toBe(false);
  render(<ChatReceipt receipt={receipt} showNothingNew={false} listId="list" listExpanded={false} usedListId="used" usedExpanded={false} onToggleUsed={vi.fn()} onSaved={vi.fn()} {...panels} />);
  expect(screen.getByRole("navigation", { name: "What this answer used and saved" })).toHaveTextContent("Hanul Precision: read not changed (the page changed first)");
});

it("says a kind the answer asked for was not added when the map gained that name first", () => {
  const message = { id: "answer", role: "assistant", content: "Your map now also tracks Process step.", created_at: "2026-10-28T00:00:00Z", status: "complete",
    memory_update: { status: "no_change", commitId: null, changes: [], skippedKinds: [{ name: "ProcessStep", label: "Process step", labelKo: "공정 단계" }] } } as unknown as import("../../types").ChatMessage;
  const receipt = chatReceipt(message);
  expect(receipt.empty).toBe(false);
  render(<ChatReceipt receipt={receipt} showNothingNew listId="list" listExpanded={false} usedListId="used" usedExpanded={false} onToggleUsed={vi.fn()} onSaved={vi.fn()} {...panels} />);
  expect(screen.getByRole("navigation", { name: "What this answer used and saved" })).toHaveTextContent("Process step: not added to the map");
});

it("says what a setup reply now follows, its new focus and what the map now also tracks, in the reader's language", async () => {
  // The memory update as Rust commits it for a Bot reply that called loupist_setup.
  const message = { id: "answer", role: "assistant", content: "Done.", created_at: "2026-11-07T00:00:00Z", status: "complete",
    memory_update: { status: "applied", commitId: "answer", changes: [], setup: [
      { change: "followed", symbol: "NVDA", market: "NASDAQ", name: "Nvidia" },
      { change: "followed", symbol: "000660" },
      { change: "unfollowed", symbol: "AAPL", market: "NASDAQ" },
      { change: "focus", focus: "Semiconductor supply chain" },
      { change: "tracks", kinds: [{ name: "ProcessNode", label: "Process node", labelKo: "공정 노드" }, { name: "Fab", label: "Fab" }] },
    ] } } as unknown as import("../../types").ChatMessage;
  const receipt = chatReceipt(message);
  expect(receipt.empty).toBe(false);
  expect(chatReceipt(message, undefined, { saveOff: true }).setup).toEqual([]);
  render(<ChatReceipt receipt={receipt} showNothingNew listId="list" listExpanded={false} usedListId="used" usedExpanded={false} onToggleUsed={vi.fn()} onSaved={vi.fn()} {...panels} />);
  const line = screen.getByRole("navigation", { name: "What this answer used and saved" });
  expect(line).toHaveTextContent("·Following Nvidia·Following 000660·Stopped following AAPL·Focus updated·Map now also tracks Process node, Fab");
  expect(line).not.toHaveTextContent("Nothing new saved");
  await act(async () => { await i18n.changeLanguage("ko"); });
  try {
    expect(line).toHaveTextContent("·Nvidia 관심 자산에 추가·000660 관심 자산에 추가·AAPL 관심 자산에서 뺌·리서치 주제 바꿈·이제 지도가 공정 노드, Fab도 추적해요");
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});

it("names a moved read after the relationships and opens the asset page at its history", async () => {
  const onOpenThesis = vi.fn();
  const move = { assetId: "ast_0123456789abcdef", name: "Hanul Precision", verdict: "weakening" as const };
  render(<ChatReceipt receipt={{ ...empty, relationships: 1, thesisMoves: [move], empty: false }} showNothingNew={false} listId="list" listExpanded={false}
    usedListId="used" usedExpanded={false} onToggleUsed={vi.fn()} onSaved={vi.fn()} {...panels} onOpenThesis={onOpenThesis} />);
  const receipt = screen.getByRole("navigation", { name: "What this answer used and saved" });
  expect(receipt).toHaveTextContent("1 relationship updated·Hanul Precision: Weakening");
  await userEvent.setup().click(within(receipt).getByRole("button", { name: "Hanul Precision: Weakening" }));
  expect(onOpenThesis).toHaveBeenCalledWith(move);
});
