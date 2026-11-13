import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { MemoryWorkspace } from "../../components/knowledge/MemoryWorkspace";
import { PageReader } from "../../components/memory/PageReader";
import { ResearchReader } from "../../components/knowledge/ResearchReader";
import { initialKnowledgeNavigation, openKnowledgeLocation, patchKnowledgeLocation } from "../../knowledge/location";
import type { LoupistSummary, LoupistBridge, KnowledgeLocation, KnowledgePin } from "../../types";

const firstLoupist = async (bridge: MockLoupistBridge) => ({ ...(await bridge.loupistList())[0]!, record_count: 0 });
const journal = (journalId: string) => ({ loupistId: "loupist-quality", surface: "note" as const, selection: { kind: "journal" as const, journalId } });

function SourceHarness({ bridge, loupist, onAsk }: { bridge: LoupistBridge; loupist: LoupistSummary; onAsk: (pin: KnowledgePin) => void }) {
  const [navigation, setNavigation] = useState(() => patchKnowledgeLocation(initialKnowledgeNavigation(loupist.id), { loupistId: loupist.id, surface: "source" }));
  return <MemoryWorkspace bridge={bridge} loupist={loupist} navigation={navigation}
    onPatch={(next: KnowledgeLocation) => setNavigation((current) => patchKnowledgeLocation(current, next))}
    onOpen={(next) => setNavigation((current) => openKnowledgeLocation(current, next))} onBack={vi.fn()} onAsk={onAsk} />;
}

describe("Loupist · Memory reading", () => {
  it("retries a failed page read", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const read = bridge.knowledgeLensRead.bind(bridge);
    const calls = vi.spyOn(bridge, "knowledgeLensRead").mockRejectedValueOnce(new Error("Approach is temporarily unavailable")).mockImplementation(read);
    const loupist = await firstLoupist(bridge);
    const onAsk = vi.fn();
    render(<PageReader bridge={bridge} loupistId={loupist.id} recordId="lens-compare-accounts" onOpen={vi.fn()} onOpenPage={vi.fn()} onAsk={onAsk} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Approach is temporarily unavailable");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("heading", { name: "Compare independent accounts" })).toHaveFocus();
    expect(calls.mock.calls[1][0]).toEqual(calls.mock.calls[0][0]);
    await user.click(screen.getByRole("button", { name: "Ask Bot to revise" }));
    expect(onAsk).toHaveBeenCalledWith(expect.objectContaining({ kind: "lens", label: "Compare independent accounts" }));
  });

  it("finds a saved source by title and offers that passage to Chat", async () => {
    const user = userEvent.setup();
    const onAsk = vi.fn();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    render(<SourceHarness bridge={bridge} loupist={await firstLoupist(bridge)} onAsk={onAsk} />);
    const sources = await screen.findByRole("list", { name: "Sources" });
    await user.type(screen.getByRole("textbox", { name: "Search sources" }), "status line");
    expect(within(sources).queryByRole("button", { name: /RFC 9110 HTTP semantics/ })).toBeNull();
    await user.click(within(sources).getByRole("button", { name: /Tool excerpt of a status line/ }));
    expect(within(await screen.findByRole("article", { name: "Source reader" })).getByText("HTTP/1.1 200")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Add to question" }));
    expect(onAsk).toHaveBeenCalledWith(expect.objectContaining({ kind: "source", label: "Tool excerpt of a status line" }));
  });

  it("reads a Journal entry and opens the source saved with it", async () => {
    const user = userEvent.setup();
    render(<ResearchReader bridge={new MockLoupistBridge({ delay_ms: 0 })} location={journal("journal-http")} onOpen={vi.fn()} />);
    const note = await screen.findByRole("article", { name: "Journal entry" });
    expect(within(note).getByText("The retained excerpt states the dependency.")).toBeVisible();
    expect(note.textContent).not.toMatch(/receipt-http|rev-source-1|world-protocols/);
    const readSource = screen.getByRole("button", { name: "Read saved source" });
    await user.click(readSource);
    const saved = await screen.findByRole("article", { name: "Source saved with this entry" });
    expect(within(saved).getByText(/three-digit integer code/)).toBeVisible();
    await user.click(within(saved).getByRole("button", { name: "Close saved source" }));
    expect(readSource).toHaveFocus();
  });

  it("shows an exact saved-record failure with a retry", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "knowledgeJournalRead").mockRejectedValue(new Error("Saved record unavailable"));
    render(<ResearchReader bridge={bridge} location={journal("journal-http")} onOpen={vi.fn()} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Saved record unavailable");
    expect(screen.getByRole("button", { name: "Retry saved record" })).toBeVisible();
  });

  it("keeps a newly opened note visible when an earlier Journal source returns late", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const pinned = { receiptId: "receipt-a", recordId: "source-pack", revisionId: "rev-source-1", spanId: "span-1", coverage: "included_excerpt", attribution: "Filing" };
    vi.spyOn(bridge, "knowledgeJournalRead").mockResolvedValue({ loupistId: "loupist-quality", researchId: "research", entities: [], createdAtMs: 1, updatedAtMs: 1, archived: false, inputDigest: "mock", journalId: "journal-a", judgment: "First conclusion", reasoning: "", check: { claim: "Cash flow will stay positive.", metric: "Cash flow", direction: "above", threshold: 0, byDate: "2027-01-01", confidence: 70 }, inputs: { sources: [pinned], worlds: [], lenses: [], earlier: [] }, originChatId: null });
    let release!: (value: { mediaType: string; byteLength: number; text: string; pinned: typeof pinned }) => void;
    vi.spyOn(bridge, "knowledgeJournalSourceRead").mockReturnValue(new Promise((resolve) => { release = resolve; }));
    vi.spyOn(bridge, "knowledgeWorkingNoteRead").mockResolvedValue({ researchId: "note-b", chatId: "chat-b", title: "New working note", markdown: "Cash flow stayed positive in the annual filing.", changeSummary: "", recordedAtMs: 1, previousNoteIds: [] });
    const view = render(<ResearchReader bridge={bridge} location={journal("journal-a")} onOpen={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Read saved source" }));
    view.rerender(<ResearchReader bridge={bridge} location={{ loupistId: "loupist-quality", surface: "note", selection: { kind: "research", recordId: "note-b" } }} onOpen={vi.fn()} />);
    expect(await screen.findByRole("article", { name: "Saved research note" })).toHaveTextContent("Cash flow stayed positive in the annual filing.");
    await act(async () => release({ mediaType: "text/plain", byteLength: 13, text: "STALE PASSAGE", pinned }));
    expect(screen.queryByText("STALE PASSAGE")).not.toBeInTheDocument();
  });
});

describe("Loupist · Source catalog", () => {
  const renderSources = async (bridge: MockLoupistBridge) => {
    const loupist = await firstLoupist(bridge);
    await act(async () => { render(<MemoryWorkspace bridge={bridge} loupist={loupist}
      navigation={{ current: { loupistId: loupist.id, surface: "source" }, views: {}, worldSurface: "world" as const, past: [], generation: 0 }}
      onPatch={vi.fn()} onOpen={vi.fn()} onBack={vi.fn()} onAsk={vi.fn()} />); });
  };

  it("explains an empty Source collection and offers to add one", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "knowledgeCatalog").mockResolvedValue({ loupistId: "loupist-quality", worlds: [], lenses: [], sources: [] });
    await renderSources(bridge);
    expect(await screen.findByText("No sources yet")).toBeVisible();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add source" }));
    expect(screen.getByRole("button", { name: "Save source" })).toBeDisabled();
  });

  it("surfaces a catalog failure and reloads after retry", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const original = bridge.knowledgeCatalog.bind(bridge);
    vi.spyOn(bridge, "knowledgeCatalog").mockRejectedValueOnce(new Error("Knowledge catalog unavailable")).mockImplementation(original);
    await renderSources(bridge);
    expect(await screen.findByRole("alert")).toHaveTextContent("Knowledge catalog unavailable");
    expect(screen.queryByText("No sources yet")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("list", { name: "Sources" })).toBeVisible();
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("keeps a failed Source save and its entered values in the dialog until retry succeeds", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const original = bridge.knowledgeSourceRetain.bind(bridge);
    const create = vi.spyOn(bridge, "knowledgeSourceRetain").mockRejectedValueOnce(new Error("Storage is unavailable")).mockImplementation(original);
    await renderSources(bridge);
    await user.click(screen.getByRole("button", { name: "Add source" }));
    const dialog = screen.getByRole("dialog", { name: "Add source" });
    await user.type(within(dialog).getByRole("textbox", { name: "Source title" }), "Annual filing");
    await user.click(within(dialog).getByRole("tab", { name: "Link" }));
    await user.type(within(dialog).getByRole("textbox", { name: "Source origin" }), "Annual report");
    await user.click(within(dialog).getByRole("button", { name: "Save source" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("Storage is unavailable");
    expect(within(dialog).getByRole("textbox", { name: "Source title" })).toHaveValue("Annual filing");
    await user.click(within(dialog).getByRole("button", { name: "Save source" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Add source" })).toBeNull());
    expect(create.mock.calls[1][0]).toEqual(create.mock.calls[0][0]);
  });

  it.each([true, false])("refreshes the World head only when following current knowledge (%s)", async (followCurrent) => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const loupist = (await bridge.loupistList())[0]!;
    const catalog = await bridge.knowledgeCatalog({ loupistId: loupist.id });
    const world = catalog.worlds[0];
    const fetch = vi.spyOn(bridge, "knowledgeCatalog").mockResolvedValue(catalog);
    const onPatch = vi.fn();
    const navigation = { current: { loupistId: loupist.id, surface: "world" as const, recordId: world.recordId, revisionId: world.revisionId, selection: { kind: "node" as const, id: "node-http" }, view: { followCurrent, search: "HTTP" } }, views: {}, worldSurface: "world" as const, past: [], generation: 0 };
    const props = { bridge, loupist, navigation, onPatch, onOpen: vi.fn(), onBack: vi.fn(), onAsk: vi.fn() };
    const view = render(<MemoryWorkspace {...props} />);
    await screen.findByRole("heading", { name: "HTTP" });
    fetch.mockResolvedValue({ ...catalog, worlds: [{ ...world, revisionId: "world-updated" }] });
    view.rerender(<MemoryWorkspace {...props} refreshKey={1} />);
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
    if (followCurrent) await waitFor(() => expect(onPatch).toHaveBeenCalledWith(expect.objectContaining({ revisionId: "world-updated", selection: navigation.current.selection })));
    else expect(onPatch).not.toHaveBeenCalledWith(expect.objectContaining({ revisionId: "world-updated" }));
  });
});
