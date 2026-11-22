import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatEvidenceRef, ChatMessage, ChatStreamEvent, KnowledgeReference } from "../../types";
import type { SourceSpanView } from "../../contracts/knowledge";
import { evidenceFixture } from "../evidenceFixtures";
import { openApp, openChat } from "../renderApp";
import { completedRun } from "../streamFixtures";

const withEvidence = (bridge: MockLoupistBridge, refs: ChatEvidenceRef[], content = refs.map((ref) => `[${ref.title}](#evidence-${ref.id})`).join("\n\n")) => {
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async (id) => {
    const workspace = await select(id);
    const message = workspace.threads[0]!.messages.at(-1)!;
    message.content = content;
    message.evidence_refs = refs;
    return workspace;
  });
};
const completed = (run_id: string, final_text: string): Extract<ChatStreamEvent, { type: "completed" }> => ({
  type: "completed", ...completedRun, run_id, message_id: "assistant-evidence", final_text, created_at: "2026-10-11T03:01:00Z",
  execution_model: { profile_id: "model-test", name: "GPT-5.6 Luna", provider: "openai-codex", model: "gpt-5.6-luna", thinking_level: "medium", run_options: {} },
  agent_harness: { schema: "loupist-agent-harness/1", mode: "chat", skill_ids: [], capability_ids: [], digest: "d".repeat(64) },
});

describe("Loupist · Chat evidence", () => {
  it("uses replayed checks for the number mark and the receipt when opening a saved conversation", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const source = evidenceFixture({ excerpt: "Revenue was 383,285 million." });
    const storedChecks: NonNullable<ChatMessage["number_checks"]> = ["383,285", "400,000"].map((text, link_index) => ({
      text, link_index, evidence_id: source.id, kind: "passage", status: "not_found",
    }));
    const select = bridge.loupistSelect.bind(bridge);
    vi.spyOn(bridge, "loupistSelect").mockImplementation(async (id) => {
      const workspace = await select(id);
      const message = workspace.threads[0]!.messages.at(-1)!;
      message.content = `Revenue was [383,285](#evidence-${source.id}). Target was [400,000](#evidence-${source.id}).`;
      message.evidence_refs = [source];
      message.number_checks = [{ ...storedChecks[0]!, status: "matched", unstated_scale: "answer" }, storedChecks[1]!];
      return workspace;
    });
    render(<App bridge={bridge} />);
    await openChat();
    const mark = await screen.findByRole("button", { name: /^383,285,/u });
    expect(mark).toHaveAttribute("data-status", "matched");
    expect(mark).toHaveAccessibleDescription("Matches the source’s number; the answer does not state a unit.");
    const receipt = within(mark.closest("article")!).getByTestId("memory-row");
    await userEvent.click(within(receipt).getByRole("button", { name: "1 number to check" }));
    const list = await screen.findByRole("dialog");
    expect(list).toHaveTextContent("400,000");
    expect(list).not.toHaveTextContent("383,285");
  });

  it("opens each filing figure at its own passage and opens the receipt at the first", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
    const source = catalog.sources[0]!;
    const { selectedSpanId: spanId } = await bridge.knowledgeSourceRead({
      loupistId: catalog.loupistId, recordId: source.recordId, revisionId: source.revisionId, purpose: "renderer_inspection",
    });
    if (!spanId) throw new Error("The filing fixture has no saved span");
    const reference: KnowledgeReference = { kind: "source", recordId: source.recordId, revisionId: source.revisionId, objectId: spanId, spanId };
    const sales = "| 서비스 매출 | 96,169 | 85,200 |";
    const margin = "| 서비스 매출총이익률 | 73.9% | 70.8% |";
    const text = [sales, "다른 회계 정보 🌏".repeat(60), margin].join("\n\n");
    const bytes = (value: string) => new TextEncoder().encode(value).length;
    const range = (value: string) => {
      const start = bytes(text.slice(0, text.indexOf(value)));
      return { start, end: start + bytes(value) };
    };
    const amounts = ["96,169", "73.9%", "74.0%"];
    const marks = ["96,169", "73.9%", "73.9%"].map(range);
    const presentation: NonNullable<SourceSpanView["presentation"]> = {
      text, sourcePointer: "/content", extractionMethod: "readability", contentOffset: 0, nextOffset: null, extractionTruncated: false,
      citedPassages: [
        { ...range(sales), marks: [marks[0]!, range("85,200")] },
        { ...range(margin), marks: [marks[1]!, range("70.8%")] },
      ],
    };
    const evidence = evidenceFixture({ kind: "saved_context", title: source.title, tool_name: "world_read", read_ref: "filing-read", url: undefined, excerpt: text });
    const select = bridge.loupistSelect.bind(bridge);
    vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
      const workspace = await select(id);
      const message = workspace.threads[0]!.messages.at(-1)!;
      message.content = amounts.map(amount => `[${amount}](#evidence-${evidence.id})`).join(" · ");
      message.evidence_refs = [evidence];
      message.memory_refs = [{ kind: "source", read_ref: evidence.read_ref!, title: source.title, excerpt: text, access: "exact_read", as_of: null, section: null, knowledge: reference }];
      message.memory_update = { status: "applied", commitId: "filing", changes: [{ kind: "Source", recordId: source.recordId, operation: "create", title: source.title, knowledge: reference, lesson: "", basis: "", futureUse: "" }] };
      message.number_checks = amounts.map((value, link_index) => ({
        kind: "passage", evidence_id: evidence.id, text: value, link_index, span: marks[link_index],
        ...(link_index === 2 ? { status: "not_found", comparedValue: "73.9%", comparisonReason: "digits_differ" } : { status: "matched" }),
      }));
      return workspace;
    });
    const originalRead = bridge.knowledgeSourceRead.bind(bridge);
    const read = vi.spyOn(bridge, "knowledgeSourceRead").mockImplementation(async request => ({
      ...await originalRead(request),
      selectedSpanId: spanId,
      spans: [{ spanId, mediaType: "text/plain", selector: "", contextNote: "", text, presentation: {
        ...presentation, ...(request.figure ? { focusedMark: marks[request.figure.linkIndex] } : {}),
      } }],
    }));
    render(<App bridge={bridge} />);
    await openChat();
    const expectPassage = async (position: string, value?: string) => {
      await waitFor(() => {
        const panel = within(screen.getByLabelText("Side panel"));
        expect(panel.getByText(position)).toBeVisible();
        const reader = panel.getByRole("article", { name: "Source reader" });
        const focus = [...reader.querySelectorAll('mark[aria-current="true"]')].map(mark => mark.textContent);
        expect(focus).toEqual(value ? [value] : []);
        expect(reader.querySelectorAll("mark")).toHaveLength(4);
      });
    };
    const openFigure = async (button: RegExp) => {
      await user.click(await screen.findByRole("button", { name: button }));
      await user.click(within(await screen.findByRole("dialog", { name: source.title })).getByRole("button", { name: "Open in Evidence" }));
    };

    await openFigure(/^96,169/u);
    await expectPassage("1 of 2", "96,169");
    const first = read.mock.lastCall?.[0].figure;
    expect(first).toEqual({ threadId: "thread-margin", messageId: expect.any(String), evidenceId: evidence.id, linkIndex: 0 });
    await openFigure(/^73\.9 percent/u);
    await expectPassage("2 of 2", "73.9%");
    expect(read.mock.lastCall?.[0].figure).toEqual({ ...first, linkIndex: 1 });

    const panel = within(screen.getByLabelText("Side panel"));
    await user.click(panel.getByRole("button", { name: "Previous cited passage" }));
    expect(panel.getByText("1 of 2")).toBeVisible();
    await openFigure(/^73\.9 percent/u);
    await expectPassage("2 of 2", "73.9%");

    await user.click(screen.getByRole("button", { name: "1 number to check" }));
    await user.click(await screen.findByRole("button", { name: "Open the source" }));
    await expectPassage("2 of 2", "73.9%");
    expect(read.mock.lastCall?.[0].figure).toEqual({ ...first, linkIndex: 2 });

    await user.click(screen.getByRole("button", { name: "Activity" }));
    const activity = await screen.findByRole("complementary", { name: "Activity" });
    const checkRow = within(activity).getAllByRole("listitem").find(item => item.textContent?.includes("96,169"))!;
    await user.click(within(checkRow).getByRole("button", { name: source.title }));
    await expectPassage("1 of 2", "96,169");
    expect(read.mock.lastCall?.[0].figure).toEqual(first);

    const receipt = screen.getByRole("navigation", { name: "What this answer used and saved" });
    await user.click(within(receipt).getByRole("button", { name: "Saved 1 source" }));
    await expectPassage("1 of 2");
    expect(read.mock.lastCall?.[0].figure).toBeUndefined();
  });

  it("opens a web citation in the browser first and keeps the saved passage when that open fails", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const evidence = evidenceFixture();
    withEvidence(bridge, [evidence]);
    const open = vi.spyOn(bridge, "browserTabOpen").mockRejectedValueOnce(new Error("Browser unavailable"));
    const external = vi.spyOn(bridge, "openExternalUrl").mockResolvedValue(undefined);
    render(<App bridge={bridge} />);
    await openChat();
    const citation = await screen.findByRole("button", { name: `Source 1: ${evidence.title}` });
    await user.click(citation);
    const alert = await screen.findByRole("alert");
    expect(open).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("article", { name: `Research source: ${evidence.title}` })).toBeNull();
    expect(alert).toHaveTextContent(i18n.t("chat:useNativeBrowserTabs.couldNotOpenThisWebPage"));
    expect(alert).not.toHaveTextContent("Browser unavailable");
    expect(within(alert).getByRole("button", { name: "View saved passage" })).toBeVisible();
    await user.click(within(alert).getByRole("button", { name: "Open in system browser" }));
    expect(external).toHaveBeenCalledExactlyOnceWith(evidence.url);
    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(open).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    const panel = screen.getByLabelText("Side panel");
    expect(within(panel).queryByRole("button", { name: "View saved excerpt" })).toBeNull();
    await user.click(within(panel).getByRole("button", { name: "Saved passage" }));
    expect(await screen.findByRole("article", { name: `Research source: ${evidence.title}` })).toHaveTextContent(evidence.excerpt);
    await user.click(citation);
    await waitFor(() => expect(open).toHaveBeenCalledTimes(2));
  });

  it.each([undefined, "javascript:alert(1)"])("shows the retained excerpt instead of loading an unusable URL (%s)", async (url) => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const evidence = evidenceFixture({ url });
    withEvidence(bridge, [evidence]);
    const browser = vi.spyOn(bridge, "browserTabOpen");
    render(<App bridge={bridge} />);
    await openChat();
    await userEvent.setup().click(await screen.findByRole("button", { name: `Source 1: ${evidence.title}` }));
    expect(await screen.findByRole("article", { name: `Research source: ${evidence.title}` })).toHaveTextContent(evidence.excerpt);
    expect(browser).not.toHaveBeenCalled();
  });

  it("opens a citation's page in the system browser on Ctrl-click and leaves the panel closed", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const evidence = evidenceFixture();
    withEvidence(bridge, [evidence]);
    const external = vi.spyOn(bridge, "openExternalUrl").mockResolvedValue(undefined);
    const browser = vi.spyOn(bridge, "browserTabOpen");
    render(<App bridge={bridge} />);
    await openChat();
    fireEvent.click(await screen.findByRole("button", { name: `Source 1: ${evidence.title}` }), { ctrlKey: true });
    await waitFor(() => expect(external).toHaveBeenCalledExactlyOnceWith(evidence.url));
    expect(browser).not.toHaveBeenCalled();
    expect(screen.queryByLabelText("Side panel")).toBeNull();
  });

  it("announces a panel tab it opened in one polite live region, without moving focus", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const original = evidenceFixture({ tool_name: "finance_market_data", title: "Market source", url: "https://finance.example/filing", excerpt: "Retained market source" });
    withEvidence(bridge, [original]);
    render(<App bridge={bridge} />);
    await openChat();
    const citation = await screen.findByRole("button", { name: "Source 1: Market source" });
    await user.click(citation);
    const regions = document.querySelectorAll("[data-panel-announcement]");
    expect(regions).toHaveLength(1);
    expect(regions[0]).toHaveAttribute("aria-live", "polite");
    await waitFor(() => expect(regions[0]).toHaveTextContent("Opened Market source in the panel"));
  });

  it("opens a numbered asset page read on its asset page, never a dead citation", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const read = evidenceFixture({ kind: "saved_context", tool_name: "asset_page_read", url: undefined, title: "Asset page: Apple", excerpt: '{\n  "assetId": "NASDAQ:AAPL"\n}' });
    withEvidence(bridge, [read]);
    const page = vi.spyOn(bridge, "assetPageGet");
    render(<App bridge={bridge} />);
    await openChat();
    await user.click(await screen.findByRole("button", { name: "Source 1: Asset page: Apple" }));
    await waitFor(() => expect(page).toHaveBeenCalledWith(expect.objectContaining({ assetId: "NASDAQ:AAPL" })));
    // The page opens by the id the read kept, and nothing else.
    const request = page.mock.calls[0]![0];
    expect(request).toStrictEqual({ loupistId: request.loupistId, assetId: "NASDAQ:AAPL" });
  });

  it("routes original sources to readers and tool results to work progress", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const original = evidenceFixture({ id: `ev_${"0".repeat(32)}`, tool_name: "finance_market_data", title: "Market source", url: "https://finance.example/filing", excerpt: "Retained market source" });
    const search = evidenceFixture({ id: `ev_${"1".repeat(32)}`, kind: "discovery", tool_name: "web_search", title: "Search preview", excerpt: "Retained search preview" });
    const derived = evidenceFixture({ id: `ev_${"2".repeat(32)}`, kind: "derived", tool_name: "dataset_import", url: undefined, title: "Margin calculation", excerpt: "Value: 391" });
    withEvidence(bridge, [original, search, derived]);
    const browser = vi.spyOn(bridge, "browserTabOpen");
    render(<App bridge={bridge} />);
    await openChat();
    const sources = await screen.findByLabelText("Research sources");
    expect(sources).toHaveTextContent("Sources (1)");
    for (const result of [search, derived]) {
      await user.click(screen.getByRole("button", { name: `Result: ${result.title}` }));
      expect(within(screen.getByLabelText("Work progress")).getByText(result.excerpt)).toBeVisible();
      expect(screen.queryByLabelText("Side panel")).toBeNull();
    }
    expect(browser).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Source 1: Market source" }));
    expect(within(await screen.findByLabelText("Side panel")).getByRole("article", { name: "Research source: Market source" })).toBeVisible();
  });

  it("hides internal tool identifiers in work progress and opens web steps in the app", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const external = vi.spyOn(bridge, "openExternalUrl");
    const browser = vi.spyOn(bridge, "browserTabOpen");
    let finishRun!: () => void;
    const gate = new Promise<void>((resolve) => { finishRun = resolve; });
    vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
      observer({ type: "started", run_id: request.run_id });
      observer({ type: "progress", run_id: request.run_id, sequence: 1, progress: { startedAtMs: Date.now(), items: [
        { id: "web-1", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", target: "Rate outlook · example.com", href: "https://example.com/rates", source: { kind: "web", name: "web_search" }, status: "succeeded", startedAtMs: 0, finishedAtMs: null },
        { id: "tool-1", kind: "tool", category: "capability", operation: "read", action: "Opened a tool", target: "private-tool-id", status: "succeeded", href: null, startedAtMs: 0, finishedAtMs: null },
        { id: "hash-1", kind: "tool", category: "web", operation: "read", action: "Checked a page", target: "web:f6495eccf2c0cca04e4c6a03", status: "succeeded", href: null, startedAtMs: 0, finishedAtMs: null },
      ], finishedAtMs: null } });
      await gate;
      observer(completed(request.run_id, "Source checked."));
      return { run_id: request.run_id };
    });
    await openApp(bridge);
    await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Check this web source");
    await user.click(screen.getByRole("button", { name: "Send" }));
    const digest = await screen.findByLabelText("Work progress");
    const read = within(digest).getByRole("button");
    const article = read.closest("article")!;
    expect(within(article).getAllByLabelText("Work progress")).toHaveLength(1);
    expect(read).toHaveAttribute("aria-expanded", "false");
    expect(within(digest).queryByRole("button", { name: "Rate outlook · example.com" })).toBeNull();
    for (const internal of ["web:f6495eccf2c0cca04e4c6a03", "private-tool-id", "web_search"]) expect(article).not.toHaveTextContent(internal);
    await user.click(read);
    expect(digest.querySelectorAll(".chat-progress-row")).toHaveLength(3);
    for (const internal of ["web:f6495eccf2c0cca04e4c6a03", "private-tool-id", "web_search"]) expect(article).not.toHaveTextContent(internal);
    await user.click(within(article).getByRole("button", { name: "Rate outlook · example.com" }));
    await waitFor(() => expect(browser).toHaveBeenCalledWith(expect.objectContaining({ url: "https://example.com/rates" }), expect.any(Function)));
    expect(external).not.toHaveBeenCalled();
    finishRun();
    const message = (await screen.findByText("Source checked.")).closest("article")!;
    expect(message).not.toHaveTextContent("private-tool-id");
  });

  it("binds streamed citations to host-issued sources and restores them after reload", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const originalSelect = bridge.loupistSelect.bind(bridge);
    const savedWorkspace = await originalSelect("loupist-quality");
    let saved = false;
    vi.spyOn(bridge, "loupistSelect").mockImplementation((loupistId) =>
      saved && loupistId === "loupist-quality" ? Promise.resolve(structuredClone(savedWorkspace)) : originalSelect(loupistId));
    const browser = vi.spyOn(bridge, "browserTabOpen");
    const external = vi.spyOn(bridge, "openExternalUrl");
    const chatSend = vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
      observer({ type: "started", run_id: request.run_id });
      return { run_id: request.run_id };
    });
    const evidence = evidenceFixture();
    const answer = `Net sales were USD 416,161 million. [Annual report](#evidence-${evidence.id}) Unissued: [Unknown source](#evidence-ev_${"c".repeat(32)}).`;
    const first = render(<App bridge={bridge} />);
    await openChat();
    await screen.findByRole("heading", { name: "How should we read the margin decline?" });
    fireEvent.change(screen.getByRole("textbox", { name: "Message Loupist" }), { target: { value: "Check Apple's FY2025 net sales." } });
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(1));
    const [request, observer] = chatSend.mock.calls[0]!;
    const emit = async (event: ChatStreamEvent) => { await act(async () => observer(event)); };

    await emit({ type: "evidence", run_id: request.run_id, evidence });
    await emit({ type: "delta", run_id: request.run_id, text: answer });
    const citation = await screen.findByRole("button", { name: `Source 1: ${evidence.title}` });
    await user.click(citation);
    const panel = await screen.findByLabelText("Side panel");
    expect(await within(panel).findByRole("textbox", { name: "Web address" })).toHaveValue(evidence.url);
    await waitFor(() => expect(browser).toHaveBeenCalledTimes(1));
    await user.click(citation);
    await waitFor(() => expect(browser).toHaveBeenCalledTimes(1));
    await user.click(within(panel).getByRole("tab", { name: "Evidence" }));
    expect(within(panel).getByRole("article", { name: `Research source: ${evidence.title}` })).toHaveTextContent(evidence.excerpt);
    // Opening the same citation twice keeps one saved passage.
    expect(within(panel).getByRole("button", { name: "Back" })).toBeDisabled();
    // Revealed words are wrapped one by one while the reply streams; the unissued citation is still not a control.
    await waitFor(() => {
      const unknown = [...document.querySelectorAll(".message.assistant .message-content *")]
        .find((element) => element.textContent === "Unknown source" && ![...element.children].some((child) => child.textContent === "Unknown source"));
      expect(unknown).toBeDefined();
      expect(unknown!.closest("a, button")).toBeNull();
    });
    await emit({ type: "evidence", run_id: request.run_id, evidence });

    const persisted: ChatMessage = { id: "assistant-evidence", role: "assistant", status: "complete", content: answer, created_at: "2026-10-11T03:01:00Z", evidence_refs: [structuredClone(evidence)] };
    savedWorkspace.threads.find((thread) => thread.id === request.thread_id)!.messages.push(
      { id: "user-evidence", role: "user", status: "complete", content: request.prompt, created_at: "2026-10-11T03:00:00Z" }, persisted);
    saved = true;
    await emit(completed(request.run_id, answer));
    // The sources list arrives with the settled answer and names a source once, however often it was issued.
    const sources = await screen.findByLabelText("Research sources");
    expect(sources).toHaveTextContent("Sources (1)");
    expect(within(sources).getAllByRole("button", { name: new RegExp(evidence.title) })).toHaveLength(1);
    first.unmount();

    render(<App bridge={bridge} />);
    await openChat();
    await user.click(await screen.findByRole("button", { name: `Source 1: ${evidence.title}` }));
    const restored = await screen.findByLabelText("Side panel");
    expect(within(restored).getByRole("textbox", { name: "Web address" })).toHaveValue(evidence.url);
    await user.click(within(restored).getByRole("tab", { name: "Evidence" }));
    expect(within(restored).getByRole("article", { name: `Research source: ${evidence.title}` })).toHaveTextContent(evidence.excerpt);
    expect(screen.getByText("Unknown source", { exact: true }).closest("a, button")).toBeNull();
    expect(external).not.toHaveBeenCalled();
  });
});

describe("Loupist · Chat evidence marking", () => {
  it("marks the citation whose passage Evidence shows, and only while it shows it", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const first = evidenceFixture();
    const second = evidenceFixture({ id: "evidence-second", title: "Second filing", url: "https://second.example/report" });
    withEvidence(bridge, [first, second]);
    vi.spyOn(bridge, "browserTabOpen").mockResolvedValue(undefined as never);
    render(<App bridge={bridge} />);
    await openChat();
    const chip = await screen.findByRole("button", { name: `Source 1: ${first.title}` });
    const other = screen.getByRole("button", { name: `Source 2: ${second.title}` });
    expect(chip).not.toHaveAttribute("aria-current");
    await user.click(chip);
    const panel = await screen.findByLabelText("Side panel");
    expect(chip).not.toHaveAttribute("aria-current");
    await user.click(within(panel).getByRole("tab", { name: "Evidence" }));
    await screen.findByRole("article", { name: `Research source: ${first.title}` });
    expect(chip).toHaveAttribute("aria-current", "true");
    expect(other).not.toHaveAttribute("aria-current");
    await user.click(other);
    expect(chip).not.toHaveAttribute("aria-current");
    expect(other).not.toHaveAttribute("aria-current");
    await user.click(within(panel).getByRole("tab", { name: "Evidence" }));
    await screen.findByRole("article", { name: `Research source: ${second.title}` });
    expect(other).toHaveAttribute("aria-current", "true");
    expect(chip).not.toHaveAttribute("aria-current");
    // Closing the panel leaves nothing marked.
    await user.click(screen.getByRole("button", { name: "Close panel" }));
    await waitFor(() => expect(other).not.toHaveAttribute("aria-current"));
  });
});
