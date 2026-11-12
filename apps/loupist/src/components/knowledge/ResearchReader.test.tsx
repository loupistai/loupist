import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { JournalRecord, JournalSourceRead, KnowledgeLocation, WorkingNote } from "../../types";
import { ResearchReader } from "./ResearchReader";

const note: WorkingNote = { researchId: "old-note", chatId: "deleted-chat", title: "Earlier research", markdown: "The original assumption was conditional.", changeSummary: "Saved before new evidence", recordedAtMs: 1, previousNoteIds: ["preceding-note"] };
const noteLocation: KnowledgeLocation = { loupistId: "loupist-quality", surface: "note", selection: { kind: "research", recordId: "old-note" } };
const journalLocation: KnowledgeLocation = { loupistId: "loupist-quality", surface: "note", selection: { kind: "journal", journalId: "journal-http" } };
const check: JournalRecord["check"] = { claim: "Evidence coverage will exceed one reference.", metric: "References", direction: "above", threshold: 1, byDate: "2027-01-01", confidence: 70 };
const source: JournalSourceRead["pinned"] = { receiptId: "receipt-1", recordId: "source-1", revisionId: "source-rev-1", spanId: "span-1", coverage: "included_excerpt", attribution: "Original statement" };
const deferred = <T,>() => { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => { resolve = done; }); return { promise, resolve }; };
const setup = () => ({ bridge: new MockLoupistBridge({ delay_ms: 0 }), location: noteLocation, onOpen: vi.fn(), onAsk: vi.fn() });

it("reads the saved note and continues its original chat", async () => {
  const props = setup();
  const read = vi.spyOn(props.bridge, "knowledgeWorkingNoteRead").mockResolvedValue(note);
  render(<ResearchReader {...props} />);
  expect(await screen.findByRole("article", { name: "Saved research note" })).toHaveTextContent(note.markdown);
  expect(read).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-quality", noteId: "old-note" });
  fireEvent.click(screen.getByRole("button", { name: "Continue chat" }));
  expect(props.onAsk).toHaveBeenCalledWith(expect.objectContaining({ kind: "research", objectId: "old-note" }), "deleted-chat");
  fireEvent.click(screen.getByRole("button", { name: "Earlier note 1" }));
  expect(props.onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ surface: "note", selection: { kind: "research", recordId: "preceding-note", sectionId: undefined } }));
});

it("opens an exact journal inline with source, World, and Lens revision links", async () => {
  const props = setup();
  const read = vi.spyOn(props.bridge, "knowledgeJournalRead");
  render(<ResearchReader {...props} location={journalLocation} />);
  const journal = await screen.findByRole("article", { name: "Journal entry" });
  expect(read).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-quality", journalId: "journal-http" });
  expect(within(journal).getByRole("region", { name: "Why" })).toHaveTextContent("The retained excerpt states the dependency.");
  fireEvent.click(screen.getByRole("button", { name: "Open source 1" }));
  expect(props.onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ surface: "source", recordId: "source-pack", revisionId: "rev-source-1", selection: expect.objectContaining({ spanId: "span-status-codes" }) }));
  fireEvent.click(screen.getByRole("button", { name: "Open map version 1" }));
  expect(props.onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ surface: "world", recordId: "world-protocols", revisionId: expect.any(String) }));
  fireEvent.click(screen.getByRole("button", { name: "Open Lens 1" }));
  expect(props.onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ surface: "lens", recordId: "lens-compare-accounts", revisionId: "rev-lens-1" }));
  fireEvent.click(screen.getByRole("button", { name: "Open originating Chat" }));
  expect(props.onAsk).toHaveBeenCalledWith(expect.objectContaining({ kind: "journal", loupistId: "loupist-quality", objectId: "journal-http" }), "chat-quality-1");
});

it("retries the same note identity and focuses its restored reader", async () => {
  const props = setup();
  const read = vi.spyOn(props.bridge, "knowledgeWorkingNoteRead").mockRejectedValueOnce(new Error("Read unavailable")).mockResolvedValue(note);
  render(<ResearchReader {...props} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Read unavailable");
  fireEvent.click(screen.getByRole("button", { name: "Retry saved record" }));
  // Focus moves in an effect after the reader mounts, so wait for it rather than for the element alone.
  await waitFor(() => expect(screen.getByRole("article", { name: "Saved research note" })).toHaveFocus());
  expect(read).toHaveBeenCalledTimes(2);
  expect(read.mock.calls[1]).toEqual(read.mock.calls[0]);
});

it("does not show a late exact-note read in another Loupist", async () => {
  const props = setup();
  const pending = deferred<WorkingNote>();
  vi.spyOn(props.bridge, "knowledgeWorkingNoteRead").mockReturnValue(pending.promise);
  const view = render(<ResearchReader {...props} />);
  view.rerender(<ResearchReader {...props} location={{ loupistId: "loupist-value", surface: "note" }} />);
  await act(async () => pending.resolve(note));
  expect(screen.queryByText(note.markdown)).not.toBeInTheDocument();
  expect(screen.getByText("Open a research note or journal entry from Chat.")).toBeVisible();
});

it("rejects mismatched exact record and source receipt identities", async () => {
  const props = setup();
  vi.spyOn(props.bridge, "knowledgeWorkingNoteRead").mockResolvedValue({ ...note, researchId: "another-note" });
  const view = render(<ResearchReader {...props} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("The requested research note was not returned.");
  vi.spyOn(props.bridge, "knowledgeJournalRead").mockResolvedValue({ loupistId: "loupist-quality", researchId: "research", entities: [], createdAtMs: 1, updatedAtMs: 1, archived: false, inputDigest: "mock", journalId: "journal-http", check, judgment: "Saved conclusion", reasoning: "", inputs: { sources: [source], worlds: [], lenses: [], earlier: [] }, originChatId: null });
  vi.spyOn(props.bridge, "knowledgeJournalSourceRead").mockResolvedValue({ mediaType: "text/plain", byteLength: 3, text: "wrong receipt", pinned: { ...source, revisionId: "other-revision" } });
  view.rerender(<ResearchReader {...props} location={journalLocation} />);
  fireEvent.click(await screen.findByRole("button", { name: "Read saved source" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("The requested saved source was not returned.");
  expect(screen.queryByText("wrong receipt")).not.toBeInTheDocument();
});

it("retries receipt reads without reloading or replacing the saved journal", async () => {
  const props = setup();
  const readJournal = vi.spyOn(props.bridge, "knowledgeJournalRead");
  const original = props.bridge.knowledgeJournalSourceRead.bind(props.bridge);
  const readReceipt = vi.spyOn(props.bridge, "knowledgeJournalSourceRead").mockRejectedValueOnce(new Error("Receipt unavailable")).mockImplementation(original);
  render(<ResearchReader {...props} location={journalLocation} />);
  const trigger = await screen.findByRole("button", { name: "Read saved source" });
  trigger.focus();
  fireEvent.click(trigger);
  expect(await screen.findByRole("alert")).toHaveTextContent("Receipt unavailable");
  fireEvent.click(screen.getByRole("button", { name: "Retry saved source" }));
  expect(await screen.findByRole("article", { name: "Source saved with this entry" })).toHaveTextContent("three-digit integer code");
  expect(readJournal).toHaveBeenCalledTimes(1);
  expect(readReceipt).toHaveBeenCalledTimes(2);
  expect(readReceipt.mock.calls[1]).toEqual(readReceipt.mock.calls[0]);
  fireEvent.click(screen.getByRole("button", { name: "Close saved source" }));
  expect(trigger).toHaveFocus();
});
