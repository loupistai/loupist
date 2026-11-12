import { act, render, screen, waitFor, within } from "@testing-library/react";
import i18n from "../../i18n";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatMessage } from "../../types";
import { MessageMemoryRow } from "./MessageMemoryRow";
import { renderWithOpen } from "../../test/open";
import { libraryState } from "../map/libraryState";
import { FixMemoryProvider } from "../memory/fixMemory";
import { ChatSetAsideProvider, type ChatSetAside } from "../../chat/chatSetAside";

it("localizes the World name inside the saved-change action", async () => {
  const change = { title: "Saved relationships", recordId: "world-1", operation: "create" as const, lesson: "", basis: "", futureUse: "" };
  const message: ChatMessage = { id: "world-answer", role: "assistant", content: "Saved.", status: "complete", created_at: new Date().toISOString(),
    memory_update: { status: "applied", commitId: "commit", changes: [
      { ...change, kind: "World", knowledge: { kind: "world", recordId: "world-1", revisionId: "rev", objectId: "world-1" } },
      { ...change, kind: "Journal", recordId: "journal-1", knowledge: { kind: "journal", recordId: "journal-1", revisionId: "rev", objectId: "journal-1" } },
    ] } };
  renderWithOpen(<MessageMemoryRow loupistId="loupist-a" message={message} />);
  try {
    await act(async () => { await i18n.changeLanguage("en-XA"); });
    expect(screen.getByRole("button", { name: i18n.t("chat:messageMemoryRow.openIn", { value: i18n.t("chat:workspaceEvidence.kind.world") }) })).toBeVisible();
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});
it("shows the principle statement and supports View and Undo", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  const principle = await bridge.principleCreate({ loupistId, statement: "Check the reporting period before comparing figures.", origin: "settings" });
  const message: ChatMessage = { id: "answer", role: "assistant", content: "Saved.", status: "complete", created_at: new Date().toISOString(), memory_update: { status: "applied", commitId: "commit", changes: [{ kind: "principle", statement: principle.statement, title: "Short title", recordId: principle.recordId, operation: "create", lesson: "", basis: "", futureUse: "", knowledge: { kind: "lens", recordId: principle.recordId, revisionId: principle.revisionId, objectId: principle.recordId } }] } };
  const archive = vi.spyOn(bridge, "principleArchive");
  const { open } = renderWithOpen(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={message} />);
  expect(screen.queryByText("Saved to your principles")).toBeNull();
  expect(screen.getByRole("button", { name: "Saved 1 new principle" })).toBeVisible();
  expect(screen.getByText(principle.statement)).toBeVisible();
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "View" }));
  expect(open).toHaveBeenCalledWith({ kind: "page", location: { loupistId, surface: "lens", recordId: principle.recordId } });
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
  const { open } = renderWithOpen(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={message} />);
  await userEvent.click(screen.getByRole("button", { name: "Saved 1 journal entry" }));
  expect(libraryState(bridge, loupistId)).toMatchObject({ open: true, tab: "note" });
  expect(open).toHaveBeenCalledWith({ kind: "memory", location: expect.objectContaining({ loupistId, surface: "note" }), title: "Hold the quality call" });
});

it("says when nothing new was saved, and shows nothing for a private chat or a running answer", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  const unchanged: ChatMessage = { id: "answer", role: "assistant", content: "Done.", status: "complete", created_at: new Date().toISOString(), memory_update: { status: "no_change", commitId: null, changes: [] } };
  const { rerender } = render(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={unchanged} />);
  expect(screen.getByRole("navigation", { name: "What this answer used and saved" })).toHaveTextContent("Nothing new saved");
  rerender(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={{ ...unchanged, memory_update: undefined }} />);
  expect(screen.queryByRole("navigation")).toBeNull();
  rerender(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={{ ...unchanged, status: "streaming" }} />);
  expect(screen.queryByRole("navigation")).toBeNull();
});

describe("what the answer learned", () => {
  const filed: ChatMessage = { id: "mock-filed-answer", role: "assistant", content: "Noted.", status: "complete", created_at: new Date().toISOString(), memory_update: { status: "applied", commitId: "commit", changes: [] } };

  it("says Learned 3 · 1 idea for two filed pages and lists each section with Fix and Forget", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const loupistId = (await bridge.loupistList())[0].id;
    const fix = vi.fn();
    const user = userEvent.setup();
    const { open: openPage } = renderWithOpen(<FixMemoryProvider value={fix}><MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={filed} /></FixMemoryProvider>);
    const receipt = await screen.findByRole("navigation", { name: "What this answer used and saved" });
    await waitFor(() => expect(receipt).toHaveTextContent("Learned 3"));
    expect(receipt).toHaveTextContent("Learned 3·1 idea");
    await user.click(within(receipt).getByRole("button", { name: "Learned 3" }));
    const card = await screen.findByRole("group", { name: "Filed in this answer" });
    const pages = card.querySelectorAll("[data-learned-page]");
    expect([...pages].map(page => page.getAttribute("data-learned-page"))).toEqual(["lens-subject-tcp", "lens-idea-quic"]);
    const tcp = pages[0] as HTMLElement;
    await waitFor(() => expect(within(tcp).getByText("Overview")).toBeVisible());
    expect(within(tcp).getByText("Risks")).toBeVisible();
    await user.click(within(tcp).getByRole("button", { name: "Fix Overview" }));
    expect(fix).toHaveBeenCalledWith(expect.objectContaining({ kind: "lens", recordId: "lens-subject-tcp", label: "TCP · Overview" }));
    await user.click(within(tcp).getByRole("button", { name: "Forget Risks" }));
    expect(await within(tcp).findByText("Forgotten.")).toBeVisible();
    await user.click(within(tcp).getByRole("button", { name: "Undo" }));
    expect(await within(tcp).findByRole("button", { name: "Forget Risks" })).toBeVisible();
    await user.click(within(tcp).getByRole("button", { name: "TCP" }));
    // A learned page's title opens the Lens record the one way chat opens Lens: the Evidence reader.
    expect(openPage).toHaveBeenCalledWith({ kind: "lens", recordId: "lens-subject-tcp" });
  });

  it("counts a perspective the host delivered as a used page", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const loupistId = (await bridge.loupistList())[0].id;
    const message: ChatMessage = { ...filed, id: "answer-2", lens_receipts: [{ recordId: "lens-perspective", revisionId: "rev", kind: "perspective", title: "Evidence-led perspective", delivery: "context", revisionDigest: "digest" }] };
    render(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={message} />);
    const receipt = await screen.findByRole("navigation", { name: "What this answer used and saved" });
    expect(receipt).toHaveTextContent("Used 1 page");
    await userEvent.click(within(receipt).getByRole("button", { name: "Used 1 page" }));
    expect(await screen.findByText("Evidence-led perspective")).toBeVisible();
  });

  it("shows no Learned line in a private chat", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const loupistId = (await bridge.loupistList())[0].id;
    const read = vi.spyOn(bridge, "memoryLearned");
    const aside: ChatSetAside = { ids: new Set<string>(), saveOff: true, toggle: vi.fn() };
    render(<ChatSetAsideProvider value={aside}><MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={filed} /></ChatSetAsideProvider>);
    await new Promise(resolve => setTimeout(resolve, 20));
    expect(screen.queryByText(/Learned/)).toBeNull();
    expect(read).not.toHaveBeenCalled();
  });
});
