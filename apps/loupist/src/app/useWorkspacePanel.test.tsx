import { act, renderHook, waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import { chatSessionKey } from "../chat/sessionRegistry";
import type { ChatArtifactRef, WorkspaceUiState } from "../types";
import { useWorkspacePanel } from "./useWorkspacePanel";

const artifact: ChatArtifactRef = {
  artifact_id: "document", revision: 1, kind: "markdown", title: "Research", digest: "digest",
};

describe("workspace session lifecycle", () => {
  it("closes the panel when its last tab is closed, including batched changes", async () => {
    const { result } = renderHook(() => useWorkspacePanel(new MockLoupistBridge({ delay_ms: 0 })));
    await waitFor(() => expect(result.current.hydrated).toBe(true));
    const key = chatSessionKey("loupist", "thread");
    act(() => result.current.openFiles("loupist", "thread", "report.md"));
    const tab = result.current.sessions[key].tabs[0];
    act(() => {
      result.current.openArtifact("loupist", "thread", artifact);
      result.current.closeTab(key, tab);
    });
    expect(result.current.open).toBe(true);
    act(() => result.current.closeTab(key, result.current.sessions[key].tabs[0]));
    expect(result.current.sessions[key].tabs).toEqual([]);
    expect(result.current.open).toBe(false);
  });

  it("keeps selected work when a draft becomes a persisted conversation", async () => {
    const { result } = renderHook(() => useWorkspacePanel(new MockLoupistBridge({ delay_ms: 0 })));
    await waitFor(() => expect(result.current.hydrated).toBe(true));
    const from = chatSessionKey("loupist", "draft");
    const to = chatSessionKey("loupist", "thread");
    act(() => {
      result.current.openArtifact("loupist", "draft", artifact);
      result.current.selectArtifactForChat(from, artifact);
      result.current.adoptSession(from, to);
    });
    expect(result.current.sessions[from]).toBeUndefined();
    expect(result.current.selectedArtifacts[from]).toBeUndefined();
    expect(result.current.sessions[to].tabs[0]).toMatchObject({ artifact });
    expect(result.current.selectedArtifacts[to]).toEqual(artifact);
  });

  it("drops a deleted session before waiting for native cleanup and preserves later work", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    let finishClose!: () => void;
    vi.spyOn(bridge, "browserTabClose").mockReturnValue(new Promise<void>((resolve) => { finishClose = resolve; }));
    const { result } = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(result.current.hydrated).toBe(true));
    const key = chatSessionKey("loupist", "thread");
    act(() => {
      result.current.openBrowser("loupist", "thread", "https://example.com");
      result.current.visibleRef.current = { loupistId: "loupist", threadId: "thread", tab: "chat" };
    });
    act(() => result.current.updateTab(key, result.current.sessions[key].tabs[0].id, (tab) => ({ ...tab, native_id: "native" })));
    let cleanup!: Promise<void>;
    const tabId = result.current.sessions[key].tabs[0].id;
    act(() => {
      cleanup = result.current.removeSession("loupist", "thread");
      result.current.updateTab(key, tabId, (tab) => tab);
      result.current.selectTab(key, tabId);
    });
    expect(result.current.sessions[key]).toBeUndefined();
    expect(result.current.open).toBe(false);
    act(() => result.current.openArtifact("loupist", "thread", artifact));
    await act(async () => { finishClose(); await cleanup; });
    expect(result.current.sessions[key].tabs[0]).toMatchObject({ artifact });
    expect(result.current.open).toBe(true);
  });
});

describe("dock by content kind", () => {
  it("docks charts at the bottom and reading at the side until the user chooses, then remembers the choice", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const first = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(first.result.current.hydrated).toBe(true));
    const key = chatSessionKey("loupist", "chat");
    act(() => first.result.current.applyDockKind("chart"));
    expect(first.result.current.placement).toBe("bottom");
    act(() => first.result.current.applyDockKind("reading"));
    expect(first.result.current.placement).toBe("side");
    // The user docks a chart at the side: charts now open there, reading stays at the side by default.
    act(() => { first.result.current.show(key); first.result.current.applyDockKind("chart"); });
    act(() => first.result.current.togglePlacement("side", key, "chart"));
    act(() => first.result.current.applyDockKind("reading"));
    act(() => first.result.current.applyDockKind("chart"));
    expect(first.result.current.placement).toBe("side");
    await waitFor(async () => expect((await bridge.workspaceUiRead())?.dock_by_kind).toEqual({ chart: "side" }));
    first.unmount();
    const second = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(second.result.current.hydrated).toBe(true));
    act(() => second.result.current.applyDockKind("chart"));
    expect(second.result.current.placement).toBe("side");
  });

  it("docks every kind where the panel is with Always dock here, remembers it, and returns to the defaults when turned off", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const first = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(first.result.current.hydrated).toBe(true));
    act(() => first.result.current.applyDockKind("chart"));
    expect(first.result.current.placement).toBe("bottom");
    expect(first.result.current.placementPinned).toBe(false);
    act(() => first.result.current.pinPlacement(true));
    expect(first.result.current.placementPinned).toBe(true);
    // Reading content now docks at the bottom too.
    act(() => first.result.current.applyDockKind("reading"));
    expect(first.result.current.placement).toBe("bottom");
    await waitFor(async () => expect((await bridge.workspaceUiRead())?.dock_by_kind).toEqual({ chart: "bottom", reading: "bottom" }));
    first.unmount();
    const second = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(second.result.current.hydrated).toBe(true));
    act(() => second.result.current.applyDockKind("reading"));
    expect(second.result.current.placement).toBe("bottom");
    expect(second.result.current.placementPinned).toBe(true);
    act(() => second.result.current.pinPlacement(false));
    expect(second.result.current.placementPinned).toBe(false);
    act(() => second.result.current.applyDockKind("chart"));
    act(() => second.result.current.applyDockKind("reading"));
    expect(second.result.current.placement).toBe("side");
    await waitFor(async () => expect((await bridge.workspaceUiRead())?.dock_by_kind).toBeUndefined());
  });
});

describe("panel opening rules", () => {
  it("marks a published result while the user is reading another tab, and switches to it otherwise", async () => {
    const { result } = renderHook(() => useWorkspacePanel(new MockLoupistBridge({ delay_ms: 0 })));
    await waitFor(() => expect(result.current.hydrated).toBe(true));
    const key = chatSessionKey("loupist", "chat");
    act(() => {
      result.current.visibleRef.current = { loupistId: "loupist", threadId: "chat", tab: "chat" };
      result.current.openFiles("loupist", "chat", "analysis.py");
      result.current.noteInteraction();
      result.current.publishArtifact("loupist", "chat", artifact);
    });
    expect(result.current.sessions[key].active_tab_id).toBe("files");
    expect(result.current.sessions[key].badges).toEqual(["work"]);
    act(() => result.current.selectTab(key, "work"));
    expect(result.current.sessions[key].badges).toBeUndefined();
    act(() => result.current.selectTab(key, "files"));
    const later = Date.now() + 11_000;
    vi.spyOn(Date, "now").mockReturnValue(later);
    act(() => result.current.publishArtifact("loupist", "chat", { ...artifact, artifact_id: "brief", revision: 2 }));
    expect(result.current.sessions[key].active_tab_id).toBe("work");
    expect(result.current.sessions[key].tabs.find((tab) => tab.kind === "work")).toMatchObject({ artifact: { artifact_id: "brief" }, artifactThreadId: "chat" });
    vi.restoreAllMocks();
  });

  it("keeps a published document in Work until the panel is opened explicitly", async () => {
    const { result } = renderHook(() => useWorkspacePanel(new MockLoupistBridge({ delay_ms: 0 })));
    await waitFor(() => expect(result.current.hydrated).toBe(true));
    const key = chatSessionKey("loupist", "chat");
    act(() => result.current.publishArtifact("loupist", "chat", artifact));
    expect(result.current.open).toBe(false);
    act(() => {
      result.current.visibleRef.current = { loupistId: "loupist", threadId: "chat", tab: "chat" };
      result.current.publishArtifact("loupist", "chat", { ...artifact, revision: 2 });
    });
    expect(result.current.open).toBe(false);
    expect(result.current.sessions[key].tabs.find(tab => tab.kind === "work")).toBeDefined();
    act(() => result.current.dismiss(key));
    expect(result.current).toMatchObject({ open: false });
    expect(result.current.sessions[key].dismissed).toBe(true);
    act(() => result.current.publishArtifact("loupist", "chat", { ...artifact, artifact_id: "next" }));
    expect(result.current.open).toBe(false);
    expect(result.current.sessions[key].tabs.find((tab) => tab.kind === "work")).toMatchObject({ artifact: { artifact_id: "next" } });
    // A programmatic close is not the user's choice for this chat.
    act(() => { result.current.show(key); result.current.close(); });
    expect(result.current.sessions[key].dismissed).toBeUndefined();
    act(() => result.current.publishArtifact("loupist", "chat", { ...artifact, artifact_id: "last" }));
    expect(result.current.open).toBe(false);
  });

  it("leaves a published chart in the answer: no panel, no tab, no badge", async () => {
    const { result } = renderHook(() => useWorkspacePanel(new MockLoupistBridge({ delay_ms: 0 })));
    await waitFor(() => expect(result.current.hydrated).toBe(true));
    act(() => {
      result.current.visibleRef.current = { loupistId: "loupist", threadId: "chat", tab: "chat" };
      result.current.publishArtifact("loupist", "chat", { ...artifact, artifact_id: "margin-chart", kind: "chart", title: "Margin" });
    });
    expect(result.current.open).toBe(false);
    expect(result.current.sessions[chatSessionKey("loupist", "chat")]).toBeUndefined();
  });

  it("opens the Map beside a wide, visible chat for its first names, and marks it while the user works in the panel", async () => {
    const width = vi.spyOn(window, "innerWidth", "get").mockReturnValue(1100);
    try {
      const { result } = renderHook(() => useWorkspacePanel(new MockLoupistBridge({ delay_ms: 0 })));
      await waitFor(() => expect(result.current.hydrated).toBe(true));
      const key = chatSessionKey("loupist", "chat");
      expect(result.current.revealMap("loupist", "chat")).toBe("none");
      result.current.visibleRef.current = { loupistId: "loupist", threadId: "chat", tab: "chat" };
      expect(result.current.revealMap("loupist", "chat")).toBe("none");
      width.mockReturnValue(1680);
      act(() => { expect(result.current.revealMap("loupist", "chat")).toBe("opened"); });
      expect(result.current.open).toBe(true);
      act(() => result.current.noteInteraction());
      expect(result.current.revealMap("loupist", "chat")).toBe("badged");
      act(() => result.current.dismiss(key));
      expect(result.current.revealMap("loupist", "chat")).toBe("none");
      expect(result.current.open).toBe(false);
    } finally { width.mockRestore(); }
  });

  it("never opens the panel for computation output or Memory receipts", async () => {
    const { evidenceFixture } = await import("../test/evidenceFixtures");
    const { result } = renderHook(() => useWorkspacePanel(new MockLoupistBridge({ delay_ms: 0 })));
    await waitFor(() => expect(result.current.hydrated).toBe(true));
    act(() => {
      result.current.openEvidence("loupist", "chat", evidenceFixture({ id: "calc", kind: "derived", tool_name: "bash" }));
      result.current.openEvidence("loupist", "chat", evidenceFixture({ id: "notes", kind: "saved_context", tool_name: "world_search" }));
    });
    expect(result.current.open).toBe(false);
    expect(result.current.sessions[chatSessionKey("loupist", "chat")]).toBeUndefined();
  });

  it("keeps one Evidence history per chat without duplicates, bounded at 50", async () => {
    const { evidenceFixture } = await import("../test/evidenceFixtures");
    const { result } = renderHook(() => useWorkspacePanel(new MockLoupistBridge({ delay_ms: 0 })));
    await waitFor(() => expect(result.current.hydrated).toBe(true));
    const key = chatSessionKey("loupist", "chat");
    const evidenceTab = () => result.current.sessions[key].tabs.find((tab) => tab.kind === "evidence")!;
    act(() => { for (let index = 0; index < 52; index++) result.current.openEvidence("loupist", "chat", evidenceFixture({ id: `source-${index}`, url: undefined })); });
    expect(result.current.sessions[key].tabs).toHaveLength(1);
    let tab = evidenceTab();
    expect(tab.kind === "evidence" && tab.entries.length).toBe(50);
    act(() => result.current.openEvidence("loupist", "chat", evidenceFixture({ id: "source-10", url: undefined })));
    tab = evidenceTab();
    expect(tab.kind === "evidence" && [tab.entries.length, tab.entries[tab.index]!.key]).toEqual([50, "evidence:source-10"]);
    act(() => result.current.moveEvidence(key, 1));
    tab = evidenceTab();
    expect(tab.kind === "evidence" && tab.entries[tab.index]!.key).toBe("evidence:source-11");
    act(() => result.current.openMemory("loupist", "chat", { loupistId: "loupist", surface: "lens", recordId: "method" }, "Lens"));
    tab = evidenceTab();
    // A new entry drops the forward history: source-2 … source-11, then the Lens.
    expect(tab.kind === "evidence" && [tab.entries.length, tab.entries.at(-1)!.kind]).toEqual([11, "knowledge"]);
  });
});

const savedLayout = (): WorkspaceUiState => ({
  version: 1, open: true, placement: "bottom", width: 610, height: 390,
  sessions: [{ loupist_id: "loupist", thread_id: "chat", tabs: [
    { id: "files", kind: "files", title: "Files", path: "reports", filePath: "reports/result.md" },
    { id: "work", kind: "work", artifact, artifact_thread_id: "chat" },
    { id: "source", kind: "memory", title: "Saved source", record_id: "source-id", location: { loupistId: "loupist", surface: "source", recordId: "source-id", revisionId: "rev-1", selection: { kind: "source", sourceId: "source-id", spanId: "span-1" } } },
  ], active_tab_id: "source" }],
});

describe("workspace restart persistence", () => {
  it("restores Files, the open Work item and Source with the selected reader and geometry after remount", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const write = vi.spyOn(bridge, "workspaceUiWrite");
    const first = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(first.result.current.hydrated).toBe(true));
    expect(write).not.toHaveBeenCalled();
    act(() => {
      first.result.current.openFiles("loupist", "chat", "reports/result.md");
      first.result.current.openArtifact("loupist", "chat", artifact);
      first.result.current.openMemory("loupist", "chat", { loupistId: "loupist", surface: "source", recordId: "source-id", revisionId: "rev-1", selection: { kind: "source", sourceId: "source-id", spanId: "span-1" } }, "Saved source");
      first.result.current.togglePlacement("bottom");
      first.result.current.setPanelWidth(610);
      first.result.current.setPanelHeight(390);
    });
    await waitFor(() => expect(write).toHaveBeenCalledTimes(1));
    const saved = await bridge.workspaceUiRead();
    expect(saved).toMatchObject({ open: true, placement: "bottom", width: 610, height: 390 });
    expect(saved!.sessions[0].tabs.map((tab) => tab.kind)).toEqual(["files", "work", "memory"]);
    expect(saved!.sessions[0].active_tab_id).toBe("evidence");
    first.unmount();
    const second = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(second.result.current.hydrated).toBe(true));
    expect(second.result.current).toMatchObject({ open: true, placement: "bottom", panelWidth: 610, panelHeight: 390 });
    const restored = second.result.current.sessions[chatSessionKey("loupist", "chat")];
    expect(restored.tabs.map((tab) => tab.kind)).toEqual(["files", "work", "evidence"]);
    expect(restored.active_tab_id).toBe("evidence");
    const evidence = restored.tabs[2];
    expect(evidence.kind === "evidence" && evidence.entries[evidence.index]).toMatchObject({ kind: "knowledge", title: "Saved source", location: { surface: "source", recordId: "source-id" } });
    expect(write).toHaveBeenCalledTimes(1);
  });

  it("remembers a chat the user closed the panel in, even without readers", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const first = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(first.result.current.hydrated).toBe(true));
    const key = chatSessionKey("loupist", "chat");
    act(() => { first.result.current.show(key); first.result.current.dismiss(key); });
    await waitFor(async () => expect((await bridge.workspaceUiRead())?.sessions).toEqual([{ loupist_id: "loupist", thread_id: "chat", tabs: [], dismissed: true }]));
    first.unmount();
    const second = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(second.result.current.hydrated).toBe(true));
    expect(second.result.current.sessions[key]).toMatchObject({ tabs: [], dismissed: true });
    act(() => second.result.current.togglePlacement("side", key));
    expect(second.result.current.open).toBe(true);
    expect(second.result.current.sessions[key].dismissed).toBeUndefined();
  });

  it("resets native children before restoring browser references and omits runtime state and Ask context", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    let finishReset!: () => void;
    vi.spyOn(bridge, "browserTabsReset").mockImplementationOnce(() => new Promise<void>((resolve) => { finishReset = resolve; }));
    const read = vi.spyOn(bridge, "workspaceUiRead");
    const write = vi.spyOn(bridge, "workspaceUiWrite");
    const mounted = renderHook(() => useWorkspacePanel(bridge));
    expect(read).not.toHaveBeenCalled();
    expect(write).not.toHaveBeenCalled();
    await act(async () => { finishReset(); });
    await waitFor(() => expect(mounted.result.current.hydrated).toBe(true));
    const key = chatSessionKey("loupist", "chat");
    act(() => {
      mounted.result.current.openArtifact("loupist", "chat", { ...artifact, digest: "b".repeat(64), selected_range: { start: "private-selection", end: "end" } });
      mounted.result.current.selectArtifactForChat(key, artifact);
      mounted.result.current.openMemory("loupist", "chat", { loupistId: "loupist", surface: "source", recordId: "source-id", view: { search: "private-reader-search" } }, "Source");
      mounted.result.current.openBrowser("loupist", "chat", "https://example.com/report");
    });
    const browserId = mounted.result.current.sessions[key].active_tab_id!;
    act(() => mounted.result.current.updateTab(key, browserId, (tab) => ({ ...tab, title: " \nReport\u0085Page\u0000 ", native_id: "expired-handle", loading: true, error: "private-error" })));
    await waitFor(() => expect(write).toHaveBeenCalled());
    const saved = await bridge.workspaceUiRead();
    expect(JSON.stringify(saved)).not.toMatch(/expired-handle|private-|selected_range|loading|view|native_id/);
    expect(saved!.sessions[0].tabs.at(-1)).toEqual({ id: browserId, kind: "browser", title: "Report Page", url: "https://example.com/report" });
    mounted.unmount();
    const restored = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(restored.result.current.hydrated).toBe(true));
    expect(restored.result.current.sessions[key].tabs.at(-1)).toEqual({ id: browserId, kind: "browser", title: "Report Page", url: "https://example.com/report", loading: false });
    expect(restored.result.current.selectedArtifacts).toEqual({});
  });

  it("keeps user changes and deletion choices made before a delayed hydration response", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    let finishRead!: (state: WorkspaceUiState) => void;
    const read = vi.spyOn(bridge, "workspaceUiRead").mockImplementationOnce(() => new Promise((resolve) => { finishRead = resolve; }));
    const write = vi.spyOn(bridge, "workspaceUiWrite");
    const { result } = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(read).toHaveBeenCalled());
    await act(async () => {
      await result.current.removeSession("loupist", "chat");
      await result.current.removeLoupistSessions("removed");
      result.current.openFiles("loupist", "chat", "new.md");
      result.current.setPanelWidth(730);
    });
    expect(write).not.toHaveBeenCalled();
    const saved = savedLayout();
    saved.sessions.push({ ...saved.sessions[0], loupist_id: "removed", thread_id: "other" });
    await act(async () => { finishRead(saved); });
    await waitFor(() => expect(result.current.hydrated).toBe(true));
    expect(result.current.sessions[chatSessionKey("removed", "other")]).toBeUndefined();
    expect(result.current.sessions[chatSessionKey("loupist", "chat")].tabs).toEqual([{ id: "files", kind: "files", title: "Files", path: "", filePath: "new.md" }]);
    expect(result.current).toMatchObject({ panelWidth: 730, panelHeight: 390, placement: "bottom", open: true });
    await waitFor(() => expect(write).toHaveBeenCalledTimes(1));
    expect(write.mock.calls[0][0].sessions).toHaveLength(1);
  });

  it("does not overwrite a layout when hydration fails", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "workspaceUiRead").mockRejectedValue(new Error("unavailable"));
    const write = vi.spyOn(bridge, "workspaceUiWrite");
    const { result } = renderHook(() => useWorkspacePanel(bridge));
    await act(async () => { await Promise.resolve(); });
    act(() => result.current.openFiles("loupist", "chat", "local.md"));
    expect(result.current.open).toBe(true);
    expect(result.current.hydrated).toBe(false);
    expect(write).not.toHaveBeenCalled();
  });

  it("persists bounded recent sessions and tabs while keeping the selected reader", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const write = vi.spyOn(bridge, "workspaceUiWrite");
    const { result } = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(result.current.hydrated).toBe(true));
    act(() => {
      for (let index = 0; index < 102; index++) result.current.openFiles("loupist", `chat-${index}`, "report.md");
      for (let index = 0; index < 25; index++) result.current.openBrowser("loupist", "chat-101", `https://example.com/${index}`);
      result.current.selectTab(chatSessionKey("loupist", "chat-101"), "files");
      result.current.openFiles("loupist", "__empty-chat__", "draft.md");
      result.current.setPanelWidth(Infinity);
      result.current.setPanelHeight(99999);
    });
    await waitFor(() => expect(write).toHaveBeenCalledTimes(1));
    const state = write.mock.calls[0][0];
    expect(state.sessions).toHaveLength(100);
    expect(state.sessions[0].thread_id).toBe("chat-2");
    expect(state.sessions.at(-1)).toMatchObject({ thread_id: "chat-101", active_tab_id: "files" });
    expect(state.sessions.at(-1)!.tabs).toHaveLength(20);
    expect(state.sessions.at(-1)!.tabs[0].id).toBe("files");
    expect(state).toMatchObject({ width: 460, height: 720 });
    expect(new TextEncoder().encode(JSON.stringify(state)).length).toBeLessThanOrEqual(512 * 1024);
  });
});

it("opens a web citation in the browser, keeps the saved passage in memory, and leaves excerpts out of the saved layout", async () => {
  const { evidenceFixture } = await import("../test/evidenceFixtures");
  const web = evidenceFixture({ url: "https://example.com/" });
  const filing = evidenceFixture({ id: "filing", tool_name: "finance_market_data", url: "https://finance.example/filing" });
  const quote = evidenceFixture({
    id: "quote", tool_name: "equity_price_quote", url: "https://finance.example/quotes",
    excerpt: JSON.stringify({ results: [{ symbol: "AAPL", name: "Apple", currency: "USD", last_price: 271.4 }] }),
  });
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const { result } = renderHook(() => useWorkspacePanel(bridge));
  await waitFor(() => expect(result.current.hydrated).toBe(true));
  const key = chatSessionKey("loupist", "chat");
  act(() => result.current.openBrowser("loupist", "chat", "https://example.com"));
  const id = result.current.sessions[key].tabs[0].id;
  act(() => result.current.updateTab(key, id, (tab) => ({ ...tab, native_id: "live-browser" })));
  act(() => {
    result.current.openEvidence("loupist", "chat", web);
    result.current.openBrowser("loupist", "another", "https://example.com/");
    result.current.openBrowser("loupist", "chat", "javascript:alert(1)");
  });
  expect(result.current.sessions[key].tabs).toHaveLength(2);
  expect(result.current.sessions[key].active_tab_id).toBe(id);
  expect(result.current.sessions[key].tabs[0]).toMatchObject({ id, native_id: "live-browser", evidence_key: `evidence:${web.id}` });
  const stashed = result.current.sessions[key].tabs[1];
  expect(stashed.kind === "evidence" && stashed.entries.map((entry) => entry.key)).toEqual([`evidence:${web.id}`]);
  act(() => {
    result.current.openEvidence("loupist", "chat", filing);
    result.current.openEvidence("loupist", "chat", filing);
    result.current.openEvidence("loupist", "chat", quote);
  });
  expect(result.current.sessions[key].active_tab_id).toBe("evidence");
  const evidence = result.current.sessions[key].tabs.find((tab) => tab.kind === "evidence");
  expect(evidence?.kind === "evidence" && evidence.entries.map((entry) => entry.key)).toEqual([`evidence:${web.id}`, "evidence:filing", "evidence:quote"]);
  expect(evidence?.kind === "evidence" && evidence.entries[evidence.index]?.key).toBe("evidence:quote");
  expect(result.current.sessions[key].tabs.filter((tab) => tab.kind === "browser")).toHaveLength(1);
  const other = result.current.sessions[chatSessionKey("loupist", "another")];
  expect(other.tabs[0].id).not.toBe(id);
  expect(other.tabs.some((tab) => tab.kind === "evidence")).toBe(false);
  const later = evidenceFixture({ id: "later", url: "https://example.com/", excerpt: "Later passage on the same page." });
  act(() => {
    result.current.openEvidence("loupist", "chat", later);
    result.current.openEvidence("loupist", "chat", web);
  });
  const reread = result.current.sessions[key].tabs.find((tab) => tab.kind === "evidence");
  expect(reread?.kind === "evidence" && reread.entries[reread.index]!.key).toBe(`evidence:${web.id}`);
  expect(result.current.sessions[key].active_tab_id).toBe(id);
  expect(result.current.sessions[key].tabs.filter((tab) => tab.kind === "browser")).toHaveLength(1);
  act(() => result.current.updateTab(key, id, (tab) => tab.kind === "browser" ? { ...tab, url: "https://example.com/landed" } : tab));
  const redirected = result.current.sessions[key].tabs.find((tab) => tab.id === id);
  expect(redirected?.kind === "browser" && redirected.evidence_key).toBe(`evidence:${web.id}`);
  const second = evidenceFixture({ id: "second", url: "https://issuer.example/q3", excerpt: "Second page passage." });
  act(() => result.current.openEvidence("loupist", "chat", second));
  const browsers = result.current.sessions[key].tabs.filter((tab) => tab.kind === "browser");
  expect(browsers.map((tab) => tab.kind === "browser" ? [tab.url, tab.evidence_key] : [])).toEqual([
    ["https://example.com/landed", `evidence:${web.id}`],
    ["https://issuer.example/q3", "evidence:second"],
  ]);
  await waitFor(async () => {
    const saved = JSON.stringify(await bridge.workspaceUiRead());
    expect(saved).toContain('"kind":"browser"');
    expect(saved).not.toContain(web.excerpt);
    expect(saved).not.toContain(quote.excerpt);
  });
});


describe("canonical panel kinds", () => {
  it("keeps material references when moving between Lens and World, and explicit readers return to materials", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const { result } = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(result.current.hydrated).toBe(true));
    const key = chatSessionKey("loupist", "thread");
    expect(result.current.open).toBe(false);
    expect(result.current.kind).toBe("materials");
    act(() => result.current.openArtifact("loupist", "thread", artifact));
    const session = result.current.sessions[key];
    act(() => result.current.selectKind("lens", key));
    expect(result.current.kind).toBe("lens");
    expect(result.current.sessions[key]).toBe(session);
    act(() => result.current.selectKind("world", key));
    expect(result.current.sessions[key]).toBe(session);
    act(() => result.current.openBrowser("loupist", "thread", "https://example.com/report"));
    expect(result.current.kind).toBe("materials");
    expect(result.current.sessions[key].tabs).toEqual(expect.arrayContaining([expect.objectContaining({ kind: "work", artifact }), expect.objectContaining({ kind: "browser", url: "https://example.com/report" })]));
  });

  it("keeps the header split for all panel content and toggles the active split closed", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const { result } = renderHook(() => useWorkspacePanel(bridge));
    await waitFor(() => expect(result.current.hydrated).toBe(true));
    const key = chatSessionKey("loupist", "thread");
    act(() => result.current.selectSplit("bottom", key));
    act(() => result.current.selectKind("world", key));
    act(() => result.current.applyDockKind("reading"));
    expect(result.current.placement).toBe("bottom");
    expect(result.current.open).toBe(true);
    act(() => result.current.selectSplit("bottom", key));
    expect(result.current.open).toBe(false);
    await waitFor(async () => expect((await bridge.workspaceUiRead())?.dock_by_kind).toEqual({ chart: "bottom", reading: "bottom" }));
  });
});
