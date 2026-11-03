import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatMessage } from "../../types";
import { MessageMemoryRow } from "./MessageMemoryRow";
import { libraryState } from "../map/libraryState";
it("shows the principle statement and supports View and Undo", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  const principle = await bridge.principleCreate({ loupistId, statement: "Check the reporting period before comparing figures.", origin: "settings" });
  const message: ChatMessage = { id: "answer", role: "assistant", content: "Saved.", status: "complete", created_at: new Date().toISOString(), memory_update: { status: "applied", commitId: "commit", changes: [{ kind: "principle", statement: principle.statement, title: "Short title", recordId: principle.recordId, operation: "create", lesson: "", basis: "", futureUse: "", knowledge: { kind: "lens", recordId: principle.recordId, revisionId: principle.revisionId, objectId: principle.recordId } }] } };
  const open = vi.fn(), archive = vi.spyOn(bridge, "principleArchive");
  render(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={message} onOpenMemory={vi.fn()} onOpenMemoryPage={open} />);
  expect(screen.queryByText("Saved to your principles")).toBeNull();
  expect(screen.getByRole("button", { name: "Saved 1 new rule" })).toBeVisible();
  expect(screen.getByText(principle.statement)).toBeVisible();
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "View" }));
  expect(open).toHaveBeenCalledWith({ loupistId, surface: "lens", recordId: principle.recordId });
  archive.mockRejectedValueOnce(new Error("Offline"));
  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not archive this principle.");
  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(await screen.findByText("Principle archived")).toBeVisible();
  expect(archive).toHaveBeenLastCalledWith({ loupistId, recordId: principle.recordId, archived: true });
});

it("opens a saved journal entry on the notes tab", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  const message: ChatMessage = { id: "answer", role: "assistant", content: "Saved.", status: "complete", created_at: new Date().toISOString(), memory_update: { status: "applied", commitId: "commit", changes: [{ kind: "Journal", title: "Hold the quality call", recordId: "journal-1", operation: "create", lesson: "", basis: "", futureUse: "", knowledge: { kind: "journal", recordId: "journal-1", revisionId: "rev", objectId: "journal-1" } }] } };
  const open = vi.fn();
  render(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={message} onOpenMemory={open} onOpenMemoryPage={vi.fn()} />);
  await userEvent.click(screen.getByRole("button", { name: "Saved 1 journal entry" }));
  expect(libraryState(bridge, loupistId)).toMatchObject({ open: true, tab: "note" });
  expect(open).toHaveBeenCalledWith(expect.objectContaining({ loupistId, surface: "note" }), "Hold the quality call");
});

it("lists a proposal to review and says when nothing new was saved", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  const proposal: ChatMessage = { id: "answer", role: "assistant", content: "Proposed.", status: "complete", created_at: new Date().toISOString(), memory_update: { status: "applied", commitId: "commit", changes: [{ kind: "Lens", title: "Margin method", recordId: "lens-1", operation: "proposed", lesson: "Check cash.", basis: "", futureUse: "", knowledge: { kind: "lens", recordId: "lens-1", revisionId: "rev", objectId: "lens-1" } }] } };
  const view = render(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={proposal} onOpenMemory={vi.fn()} onOpenMemoryPage={vi.fn()} onDismissLensProposal={vi.fn()} />);
  await userEvent.click(screen.getByRole("button", { name: "1 proposal to review" }));
  expect(screen.getByRole("list", { name: "Saved in this answer" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Review Margin method" })).toBeVisible();
  view.unmount();
  const unchanged: ChatMessage = { ...proposal, memory_update: { status: "no_change", commitId: null, changes: [] } };
  const { rerender } = render(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={unchanged} onOpenMemory={vi.fn()} onOpenMemoryPage={vi.fn()} />);
  expect(screen.getByRole("navigation", { name: "What this answer used and saved" })).toHaveTextContent("Nothing new saved");
  // A private chat and a running answer show nothing.
  rerender(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={{ ...unchanged, memory_update: undefined }} onOpenMemory={vi.fn()} onOpenMemoryPage={vi.fn()} />);
  expect(screen.queryByRole("navigation")).toBeNull();
  rerender(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={{ ...unchanged, status: "streaming" }} onOpenMemory={vi.fn()} onOpenMemoryPage={vi.fn()} />);
  expect(screen.queryByRole("navigation")).toBeNull();
});
