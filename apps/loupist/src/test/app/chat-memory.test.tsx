import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatMessage, LoupistWorkspace } from "../../types";
import { evidenceFixture } from "../evidenceFixtures";
import { openApp, privateChatToggle } from "../renderApp";

const withFirstMessage = async (edit: (message: ChatMessage, workspace: LoupistWorkspace) => void) => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const workspace = await bridge.loupistSelect("loupist-quality");
  edit(workspace.threads[0].messages[0], workspace);
  vi.spyOn(bridge, "loupistSelect").mockResolvedValue(workspace);
  return bridge;
};
const savedRecordLink = (message: ChatMessage, toolName: string, readRef: string) => {
  message.content += "\n\n[Saved record](#evidence-ev_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)";
  message.evidence_refs = [evidenceFixture({ id: "ev_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", kind: "saved_context", tool_name: toolName, read_ref: readRef })];
};

describe("Loupist · Chat memory", () => {
  it("shows outstanding source work in the header chip until the last job settles, including a job with no additions", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const width = vi.spyOn(Element.prototype, "clientWidth", "get").mockReturnValue(900);
    try {
      const bridge = await withFirstMessage(message => {
        message.memory_update = { status: "applied", commitId: "save", changes: ["first", "second"].map(id => ({
          recordId: id, kind: "Source", operation: "create", title: id, lesson: "", basis: "", futureUse: "",
          knowledge: { kind: "source", recordId: id, revisionId: "rev", objectId: id }, worldJobId: id, worldStatus: "queued",
        })) };
      });
      let statuses: Record<string, string> = { first: "running", second: "queued" };
      const jobs = vi.spyOn(bridge, "knowledgeGraphJob").mockImplementation(async ({ loupistId, jobId }) => ({
        jobId, loupistId, status: statuses[jobId], worldRecordId: "world", worldRevisionId: "rev", payloadDigest: "", createdAtMs: 0, hostMayRedrive: false,
      }));
      vi.spyOn(bridge, "knowledgeGrowthEvents").mockResolvedValue({ events: [], nextCursor: null });
      await openApp(bridge);
      const processing = () => screen.queryByText("Reviewing sources…");
      await waitFor(() => expect(processing()).toBeVisible());
      const rail = screen.getByRole("complementary", { name: "In this chat" });
      expect(within(rail).getByRole("status")).toHaveTextContent("Reviewing sources…");
      expect(within(rail).queryByRole("button", { name: /Show on the Map/ })).toBeNull();
      statuses = { first: "completed", second: "running" };
      await act(() => vi.advanceTimersByTimeAsync(5000));
      expect(processing()).toBeVisible();
      statuses.second = "paused_budget";
      jobs.mockClear();
      await act(() => vi.advanceTimersByTimeAsync(5000));
      expect(processing()).toBeNull();
      expect(jobs.mock.calls.map(([request]) => request.jobId)).toEqual(["second"]);
      await act(() => vi.advanceTimersByTimeAsync(5000));
      expect(jobs).toHaveBeenCalledTimes(1);
    } finally {
      width.mockRestore();
      vi.useRealTimers();
    }
  });

  it("sends a private request with Memory off and keeps that choice per Chat", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    await user.click(privateChatToggle());
    await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Answer without a new framework");
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(chatSend).toHaveBeenCalled());
    expect(chatSend.mock.calls[0][0]).toMatchObject({ use_memory: false, update_memory: false });
    expect(await screen.findByText(/using only this conversation/)).toBeVisible();
    expect(screen.queryByLabelText("Loupist memory activity")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /^Capital allocation checklist$/ }));
    expect(privateChatToggle()).toHaveAttribute("aria-pressed", "false");
    await user.click(screen.getByRole("button", { name: /^How should we read the margin decline\?$/ }));
    expect(privateChatToggle()).toHaveAttribute("aria-pressed", "true");
  });

  it("keeps Memory provenance out of the answer until the work trace is opened", async () => {
    const bridge = await withFirstMessage((message) => {
      message.memory_refs = [{
        read_ref: "lens-compare-accounts", kind: "lens", title: "INTERNAL_MEMORY_TITLE", excerpt: "INTERNAL_MEMORY_EXCERPT",
        as_of: "2026-08-06T00:00:00Z", access: "exact_read",
        knowledge: { kind: "lens", recordId: "lens-compare-accounts", revisionId: "rev-lens-1", objectId: "lens-compare-accounts" },
      }];
    });
    await openApp(bridge);
    expect(screen.queryByText("INTERNAL_MEMORY_EXCERPT")).not.toBeInTheDocument();
    await userEvent.click(within(screen.getByLabelText("Work progress")).getByRole("button"));
    await userEvent.click(screen.getByText("Consulted 1 saved note"));
    expect(within(screen.getByLabelText("Used in this answer")).getByText("INTERNAL_MEMORY_TITLE")).toBeVisible();
  });

  it("shows what an answer saved and opens its method in Lens alongside the chat", async () => {
    const user = userEvent.setup();
    await openApp();
    fireEvent.change(screen.getByRole("textbox", { name: "Message Loupist" }), { target: { value: "Learn from this mistake" } });
    await user.click(screen.getByRole("button", { name: "Send" }));
    const receipt = await screen.findByRole("button", { name: "Learned 1" }, { timeout: 40_000 });
    await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Check the next quarter too");
    await user.click(receipt);
    const card = await screen.findByRole("group", { name: "Filed in this answer" });
    expect(await within(card).findByText("Review lesson")).toBeVisible();
    await user.click(within(card).getByRole("button", { name: "Compare independent accounts" }));

    expect(screen.queryByRole("button", { name: "Revert" })).toBeNull();
    const page = await screen.findByRole("region", { name: /What .* knows/ });
    expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveValue("Check the next quarter too");
    expect(await within(page).findByRole("complementary", { name: "Compare independent accounts" })).toBeVisible();
    await user.click(within(page).getByRole("button", { name: "Back" }));
    expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveValue("Check the next quarter too");
    // A streamed reply with a Memory update; a loaded suite needs room.
  }, 60_000);

  it("opens the Bot composer with the page pinned and Correct this when Fix is chosen, without sending", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const send = vi.spyOn(bridge, "chatSend");
    fireEvent.change(screen.getByRole("textbox", { name: "Message Loupist" }), { target: { value: "Learn from this mistake" } });
    await user.click(screen.getByRole("button", { name: "Send" }));
    await user.click(await screen.findByRole("button", { name: "Learned 1" }, { timeout: 40_000 }));
    const card = await screen.findByRole("group", { name: "Filed in this answer" });
    send.mockClear();
    await user.click(await within(card).findByRole("button", { name: "Fix Review lesson" }));
    await waitFor(() => expect((screen.getByRole("textbox", { name: "Message Loupist" }) as HTMLTextAreaElement).value).toContain("Correct this:"));
    expect(document.querySelector('.composer-prompt-highlight [data-inline-reference="lens"]')).toHaveTextContent("Compare independent accounts");
    expect(send).not.toHaveBeenCalled();
  }, 60_000);

  it("opens a World citation at its saved revision", async () => {
    const user = userEvent.setup();
    const bridge = await withFirstMessage((message) => {
      message.memory_refs = [{ read_ref: "world-read-1", kind: "world", title: "Protocol world", excerpt: "", access: "exact_read",
        knowledge: { kind: "world_claim", recordId: "world-protocols", revisionId: "saved-revision", objectId: "claim-1" } }];
      savedRecordLink(message, "world_read", "world-read-1");
    });
    const endpoints = [{ nodeId: "node-1", role: "subject" }];
    const query = vi.spyOn(bridge, "knowledgeWorldQuery").mockResolvedValue({
      recordId: "world-protocols", revisionId: "saved-revision", revisionDigest: "digest",
      nodes: [{ id: "node-1", name: "Protocol", aliases: [], description: "", kind: "Concept", sourceRole: "owned", properties: {} }],
      edges: [{ id: "edge-1", relation: "DESCRIBES", endpoints, status: "active", claim: {
        id: "claim-1", proposition: "The protocol preserves the sender's original statement.", attribution: "Retained protocol notes",
        relation: "DESCRIBES", endpoints, qualifiers: [], evidence: [], status: "active",
      } }], coverage: "sealed_snapshot", indexingState: "ready", truncated: false,
    });
    await openApp(bridge);
    await user.click(await screen.findByRole("button", { name: "Saved record · Memory: Protocol world" }));
    const world = await screen.findByRole("region", { name: "Map" });
    expect(await within(world).findByText("The protocol preserves the sender's original statement.")).toBeVisible();
    expect(query).toHaveBeenCalledWith(expect.objectContaining({ recordId: "world-protocols", revisionId: "saved-revision" }));
  });

  it("reads private Journal content only after an explicit reference click", async () => {
    const bridge = await withFirstMessage((message) => {
      message.memory_refs = [{ read_ref: "journal-private", kind: "journal", title: "Private capacity review", excerpt: "", access: "exact_read",
        knowledge: { kind: "journal", recordId: "", revisionId: "", objectId: "journal-private", receiptId: "readref-unrelated" } }];
      savedRecordLink(message, "conversation_memory", "journal-private");
    });
    const read = vi.spyOn(bridge, "knowledgeJournalRead").mockResolvedValue({
      loupistId: "loupist-quality", researchId: "research", entities: [], createdAtMs: 1, updatedAtMs: 1, archived: false, inputDigest: "mock",
      journalId: "journal-private", judgment: "Original private thesis", reasoning: "Original basis",
      check: { claim: "Capacity will exceed the baseline.", metric: "Capacity", direction: "above", threshold: 1, byDate: "2027-01-01", confidence: 70 },
      inputs: { sources: [], worlds: [], lenses: [] },
    });
    await openApp(bridge);
    const reference = await screen.findByRole("button", { name: "Saved record · Memory: Private capacity review" });
    expect(read).not.toHaveBeenCalled();
    await userEvent.setup().click(reference);
    expect(await within(await screen.findByLabelText("Journal entry")).findByText("Original private thesis")).toBeVisible();
    expect(read).toHaveBeenCalledWith({ loupistId: "loupist-quality", journalId: "journal-private" });
  });

  it("opens the exact saved Source and Journal from Research updates", async () => {
    const bridge = await withFirstMessage((message) => {
      message.memory_update = { status: "applied", commitId: "receipt", changes: [
        { recordId: "source-check", kind: "Source", operation: "create", title: "Source check", lesson: "", basis: "", futureUse: "",
          knowledge: { kind: "source", recordId: "source-collection", revisionId: "source-revision", objectId: "source-check", spanId: "retained-span" } },
        { recordId: "journal:quality-call", kind: "Journal", operation: "create", title: "Hold the quality call", lesson: "", basis: "", futureUse: "",
          knowledge: { kind: "journal", recordId: "", revisionId: "", objectId: "journal-quality" } },
      ] };
    });
    const read = vi.spyOn(bridge, "knowledgeSourceRead").mockResolvedValue({
      sourceId: "source-check", recordId: "source-collection", revisionId: "source-revision", title: "Source check",
      sourceClass: "user_statement", originLabel: "Original source", coverage: "included_full", material: "original",
      verification: "verified_bytes", selectedSpanId: "retained-span", relatedClaims: [],
      spans: [{ spanId: "retained-span", selector: "utf8_line_range:1-1", contextNote: "Exact original", mediaType: "text/plain", text: "Exact retained statement" }],
    });
    await openApp(bridge);
    expect(screen.getByText("Saved to your journal: “Hold the quality call”")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Saved 1 source" }));
    await userEvent.click(screen.getByRole("button", { name: "Source check" }));
    expect(await within(await screen.findByLabelText("Source reader")).findByText("Exact retained statement")).toBeVisible();
    expect(read).toHaveBeenCalledWith(expect.objectContaining({ recordId: "source-collection", revisionId: "source-revision", sourceId: "source-check", spanId: "retained-span", purpose: "renderer_inspection" }));
  });
});
