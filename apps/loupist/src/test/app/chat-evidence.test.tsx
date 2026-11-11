import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatEvidenceRef, ChatMessage, ChatStreamEvent } from "../../types";
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
    await user.click(within(alert).getByRole("button", { name: "Open in browser" }));
    expect(external).toHaveBeenCalledExactlyOnceWith(evidence.url);
    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(open).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    const panel = screen.getByLabelText("Chat workspace panel");
    expect(within(panel).queryByRole("button", { name: "View saved excerpt" })).toBeNull();
    await user.click(within(panel).getByRole("button", { name: "View saved passage" }));
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
    expect(screen.queryByLabelText("Chat workspace panel")).toBeNull();
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
      expect(screen.queryByLabelText("Chat workspace panel")).toBeNull();
    }
    expect(browser).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Source 1: Market source" }));
    expect(within(await screen.findByLabelText("Chat workspace panel")).getByRole("article", { name: "Research source: Market source" })).toBeVisible();
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
    const panel = await screen.findByLabelText("Chat workspace panel");
    expect(await within(panel).findByRole("textbox", { name: "Web address" })).toHaveValue(evidence.url);
    await waitFor(() => expect(browser).toHaveBeenCalledTimes(1));
    await user.click(citation);
    await waitFor(() => expect(browser).toHaveBeenCalledTimes(1));
    await user.click(within(panel).getByRole("tab", { name: "Evidence" }));
    expect(within(panel).getByRole("article", { name: `Research source: ${evidence.title}` })).toHaveTextContent(evidence.excerpt);
    // Opening the same citation twice keeps one saved passage.
    expect(within(panel).getByRole("button", { name: "Back" })).toBeDisabled();
    expect(screen.getByText("Unknown source", { exact: true }).closest("a, button")).toBeNull();
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
    const restored = await screen.findByLabelText("Chat workspace panel");
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
    const panel = await screen.findByLabelText("Chat workspace panel");
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
    await user.click(screen.getByRole("button", { name: "Close workspace" }));
    await waitFor(() => expect(other).not.toHaveAttribute("aria-current"));
  });
});
