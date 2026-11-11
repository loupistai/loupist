import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { renderWithOpen, destinations } from "../../test/open";
import { ChatMapProvider } from "./ChatMapContext";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatArtifactView, LoupistArtifactItem } from "../../types";
import { ChatWorkspacePanel } from "./ChatWorkspacePanel";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { EVIDENCE_TAB_ID, WORK_TAB_ID, evidenceEntry, pushEvidenceEntry, workWorkspaceTab } from "../../chat/workspace";

const view: ChatArtifactView = {
  artifact: { id: "note", chat_session_id: "chat", kind: "markdown", title: "Saved note", current_revision: 1, created_at_ms: 1_700_000_000_000, updated_at_ms: 1_700_000_000_000 },
  revision: { artifact_id: "note", revision: 1, payload: { kind: "markdown", schema: "loupist-markdown/1", markdown: "# Saved note\n\nAvailable after retry." }, digest: "a".repeat(64), source_message_id: "message", created_at_ms: 1_700_000_000_000 },
  chart_dataset: null,
};

const panelProps = (bridge: MockLoupistBridge) => ({
  bridge, loupistId: "loupist", threadId: "chat", open: true,
  session: { tabs: [workWorkspaceTab({ artifact_id: "note", revision: 1, kind: "markdown" as const, title: "Saved note", digest: "a".repeat(64) }, "chat")], active_tab_id: WORK_TAB_ID },
  theme: "light" as const, width: 620, height: 420, placement: "side" as const,
  onWidthChange: vi.fn(), onHeightChange: vi.fn(), onPlacementToggle: vi.fn(),
  onSelectTab: vi.fn(), onUpdateTab: vi.fn(), onCloseTab: vi.fn(), onOpenArtifact: vi.fn(), onAskArtifact: vi.fn(),
  onOpenKnowledge: vi.fn(), onOpenKnowledgePage: vi.fn(), onAskKnowledge: vi.fn(),
  onNewBrowser: vi.fn(), onOpenLink: vi.fn(), onClose: vi.fn(),
});

const deferred = <T,>() => { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => { resolve = done; }); return { promise, resolve }; };

it.each(["side", "bottom"] as const)("fits the %s panel without replacing the remembered size", async placement => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const props = { ...panelProps(bridge), placement, session: { tabs: [], active_tab_id: EVIDENCE_TAB_ID } };
  let stage = { width: 1400, height: 900 };
  const callbacks = new Set<() => void>();
  const originalRect = HTMLElement.prototype.getBoundingClientRect;
  const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return this.classList.contains("app-stage") ? DOMRect.fromRect(stage) : originalRect.call(this);
  });
  vi.stubGlobal("ResizeObserver", class {
    constructor(readonly callback: () => void) {}
    observe() { callbacks.add(this.callback); }
    disconnect() { callbacks.delete(this.callback); }
  });
  try {
    const mounted = render(<div className="app-stage"><div className="app-stage-main" /><ChatWorkspacePanel {...props} /></div>);
    const panel = screen.getByLabelText("Chat workspace panel");
    const sizeProperty = placement === "side" ? "--artifact-panel-width" : "--artifact-panel-height";
    expect(panel.style.getPropertyValue(sizeProperty)).toBe(placement === "side" ? "620px" : "420px");
    act(() => { stage = { width: 900, height: 600 }; callbacks.forEach(callback => callback()); });
    expect(panel.style.getPropertyValue(sizeProperty)).toBe(placement === "side" ? "480px" : "360px");
    expect(props.onWidthChange).not.toHaveBeenCalled();
    expect(props.onHeightChange).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Panel menu" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Expand" }));
    await userEvent.click(screen.getByRole("button", { name: "Back to chat" }));
    expect(props.onWidthChange).not.toHaveBeenCalled();
    expect(props.onHeightChange).not.toHaveBeenCalled();
    const handle = screen.getByRole("separator");
    fireEvent.keyDown(handle, { key: placement === "side" ? "ArrowRight" : "ArrowDown" });
    expect(placement === "side" ? props.onWidthChange : props.onHeightChange).toHaveBeenCalledWith(placement === "side" ? 464 : 344);
    act(() => { stage = { width: 1400, height: 900 }; callbacks.forEach(callback => callback()); });
    expect(panel.style.getPropertyValue(sizeProperty)).toBe(placement === "side" ? "620px" : "420px");
    mounted.unmount();
  } finally { rect.mockRestore(); vi.unstubAllGlobals(); }
});

describe("ChatWorkspacePanel Evidence reader", () => {
  it("reads one retained source at a time and moves through its history", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const first = evidenceFixture({ id: "first", url: "https://hanul.example/q3", title: "Q3 review", excerpt: "Margin 14.8%" });
    const second = evidenceFixture({ id: "second", url: "https://morrow.example/survey", title: "Supplier survey", excerpt: "Lead times fell" });
    const evidence = pushEvidenceEntry(pushEvidenceEntry(undefined, evidenceEntry(first)), evidenceEntry(second));
    const onMoveEvidence = vi.fn();
    const { open: onOpenLink } = renderWithOpen(<ChatWorkspacePanel {...panelProps(bridge)} loupistName="Quality Compounder" onMoveEvidence={onMoveEvidence}
      session={{ tabs: [evidence], active_tab_id: EVIDENCE_TAB_ID }} />);
    expect(screen.getAllByRole("tab", { name: /Evidence/ })).toHaveLength(1);
    expect(screen.getByText("Lead times fell")).toBeVisible();
    expect(screen.queryByText("Margin 14.8%")).toBeNull();
    expect(screen.getByText(/^Source · morrow\.example · /)).toBeVisible();
    expect(screen.getByRole("button", { name: "Forward" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(onMoveEvidence).toHaveBeenLastCalledWith(-1);
    screen.getByRole("button", { name: "Back" }).focus();
    await user.keyboard("{Alt>}{ArrowLeft}{/Alt}");
    expect(onMoveEvidence).toHaveBeenCalledTimes(2);
    await user.click(screen.getByRole("button", { name: "Open live page" }));
    expect(destinations(onOpenLink)).toEqual([{ kind: "link", url: "https://morrow.example/survey" }]);
    expect(screen.queryByRole("button", { name: "View saved excerpt" })).toBeNull();
  });

  it("shows the Evidence empty state instead of a generic panel when nothing is open", () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    render(<ChatWorkspacePanel {...panelProps(bridge)} session={{ tabs: [], active_tab_id: undefined }} />);
    expect(screen.getByRole("tab", { name: "Evidence" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("Click a citation or a name in the answer to read it here.")).toBeVisible();
    expect(screen.queryByText("Open alongside the conversation")).toBeNull();
  });

  it("reports pointer and key activity so new content becomes a badge", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const onInteract = vi.fn();
    render(<ChatWorkspacePanel {...panelProps(bridge)} onInteract={onInteract}
      session={{ ...panelProps(bridge).session, tabs: [...panelProps(bridge).session.tabs, { id: "browser:a", kind: "browser" as const, url: "", title: "Notes page", loading: false }], badges: ["browser:a"] }} />);
    expect(screen.getByRole("tab", { name: /Notes page/ })).toHaveTextContent("New in Notes page");
    await userEvent.setup().click(screen.getByRole("tab", { name: "Evidence" }));
    expect(onInteract).toHaveBeenCalled();
  });
  it("retries a failed artifact read within the existing tab", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const read = vi.spyOn(bridge, "chatArtifactRead").mockRejectedValueOnce(new Error("Temporary read failure")).mockResolvedValue(view);
    render(<ChatWorkspacePanel {...panelProps(bridge)} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Temporary read failure");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("heading", { name: "Saved note" })).toBeVisible();
    expect(read).toHaveBeenCalledTimes(2);
    expect(screen.getAllByRole("tab", { name: "Work" })).toHaveLength(1);
  });
});

const workItem = (id: string, chat: string, title: string, kind: "chart" | "markdown" = "markdown", updated = 1_700_000_000_000): LoupistArtifactItem => ({
  artifact: { id, chat_session_id: chat, kind, title, current_revision: 2, created_at_ms: updated, updated_at_ms: updated },
  chat_title: chat === "chat" ? "This chat" : "Earlier chat",
});

describe("ChatWorkspacePanel Work tab", () => {
  it("lists the Loupist's work across chats with charts still building, and opens an item from its own chat", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "loupistArtifactList").mockResolvedValue([workItem("note", "chat", "Saved note"), workItem("older", "chat-1", "Supplier map", "chart")]);
    const read = vi.spyOn(bridge, "chatArtifactRead").mockResolvedValue({ ...view, artifact: { ...view.artifact, id: "older", chat_session_id: "chat-1" } });
    const props = { ...panelProps(bridge), session: { tabs: [], active_tab_id: WORK_TAB_ID } };
    render(<ChatWorkspacePanel {...props} buildingWork={[{ key: "building:step", title: "Margin vs utilization" }]} />);
    const list = await screen.findByRole("list", { name: "Charts and documents" });
    expect(list).toHaveTextContent("Margin vs utilization");
    expect(list).toHaveTextContent("Building…");
    expect(screen.getByRole("button", { name: /Supplier map/ })).toHaveTextContent("Chart · Version 2");
    expect(screen.getByRole("button", { name: /Supplier map/ })).toHaveTextContent("Earlier chat");
    await user.click(screen.getByRole("button", { name: /Supplier map/ }));
    expect(read).toHaveBeenCalledWith("loupist", "chat-1", "older");
    expect(props.onOpenArtifact).toHaveBeenCalledWith(expect.objectContaining({ artifact_id: "older", digest: "a".repeat(64) }), "chat-1");
  });

  it("shows an open item in place and returns to the list", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "chatArtifactRead").mockResolvedValue(view);
    const onUpdateTab = vi.fn();
    render(<ChatWorkspacePanel {...panelProps(bridge)} onUpdateTab={onUpdateTab} />);
    expect(await screen.findByRole("heading", { name: "Saved note" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "All work" }));
    expect(onUpdateTab).toHaveBeenCalledWith(WORK_TAB_ID, expect.any(Function));
    expect(onUpdateTab.mock.calls[0]![1]({ id: WORK_TAB_ID, kind: "work" })).toEqual({ id: WORK_TAB_ID, kind: "work" });
  });

  it("does not open an item whose read finishes after the chat changed", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const pending = deferred<ChatArtifactView>();
    vi.spyOn(bridge, "loupistArtifactList").mockResolvedValue([workItem("note", "chat", "Saved note")]);
    vi.spyOn(bridge, "chatArtifactRead").mockReturnValue(pending.promise);
    const props = { ...panelProps(bridge), session: { tabs: [], active_tab_id: WORK_TAB_ID } };
    const mounted = render(<ChatWorkspacePanel {...props} />);
    await user.click(await screen.findByRole("button", { name: /Saved note/ }));
    mounted.rerender(<ChatWorkspacePanel {...props} threadId="new-chat" />);
    await act(async () => pending.resolve(view));
    expect(props.onOpenArtifact).not.toHaveBeenCalled();
  });

  it("docks charts and reading content by kind and reports the kind with the Dock control", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "chatArtifactRead").mockResolvedValue(view);
    const onDockKind = vi.fn();
    const onPlacementToggle = vi.fn();
    const chart = { tabs: [workWorkspaceTab({ artifact_id: "chart", revision: 1, kind: "chart" as const, title: "Margin", digest: "b".repeat(64) }, "chat")], active_tab_id: WORK_TAB_ID };
    const reading = { tabs: [], active_tab_id: EVIDENCE_TAB_ID };
    const mounted = render(<ChatWorkspacePanel {...panelProps(bridge)} session={reading} onDockKind={onDockKind} onPlacementToggle={onPlacementToggle} />);
    // The placement restored at start-up stands until the kind on screen changes.
    expect(onDockKind).not.toHaveBeenCalled();
    mounted.rerender(<ChatWorkspacePanel {...panelProps(bridge)} session={chart} onDockKind={onDockKind} onPlacementToggle={onPlacementToggle} />);
    expect(onDockKind).toHaveBeenLastCalledWith("chart");
    await user.click(screen.getByRole("button", { name: "Panel menu" }));
    await user.click(await screen.findByRole("menuitem", { name: "Dock at bottom" }));
    expect(onPlacementToggle).toHaveBeenCalledWith("bottom", "chart");
    mounted.rerender(<ChatWorkspacePanel {...panelProps(bridge)} session={reading} onDockKind={onDockKind} onPlacementToggle={onPlacementToggle} />);
    expect(onDockKind).toHaveBeenLastCalledWith("reading");
  });

  it("pins the placement for every kind from the Dock control's menu and shows when it is on", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "chatArtifactRead").mockResolvedValue(view);
    const onPinPlacement = vi.fn();
    const mounted = render(<ChatWorkspacePanel {...panelProps(bridge)} onPinPlacement={onPinPlacement} />);
    await user.click(screen.getByRole("button", { name: "Panel menu" }));
    const pin = await screen.findByRole("menuitemcheckbox", { name: "Always dock here" });
    expect(pin).toHaveAttribute("aria-checked", "false");
    await user.click(pin);
    expect(onPinPlacement).toHaveBeenLastCalledWith(true);
    mounted.rerender(<ChatWorkspacePanel {...panelProps(bridge)} placementPinned onPinPlacement={onPinPlacement} />);
    await user.click(screen.getByRole("button", { name: "Panel menu" }));
    const pinned = await screen.findByRole("menuitemcheckbox", { name: "Always dock here" });
    expect(pinned).toHaveAttribute("aria-checked", "true");
    await user.click(pinned);
    expect(onPinPlacement).toHaveBeenLastCalledWith(false);
  });
});

describe("ChatWorkspacePanel versions", () => {
  const at = (day: number) => Date.UTC(2026, 9, day, 3, 0);
  const revisionView = (revision: number, current: number, text: string, day: number): ChatArtifactView => ({
    artifact: { ...view.artifact, current_revision: current, updated_at_ms: at(day) },
    revision: { ...view.revision, revision, payload: { kind: "markdown", schema: "loupist-markdown/1", markdown: `# Saved note\n\n${text}` }, digest: String(revision).repeat(64), created_at_ms: at(day) },
    chart_dataset: null,
  });

  it("lists the kept versions, shows an earlier one read-only and restores it as the newest only after a confirmation", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const first = revisionView(1, 2, "First draft.", 27);
    let current = revisionView(2, 2, "Second draft.", 28);
    const restoredView = { ...revisionView(3, 3, "First draft.", 29) };
    vi.spyOn(bridge, "chatArtifactRead").mockImplementation(async (_loupist, _chat, _artifact, revision) => revision === 1 ? first : current);
    vi.spyOn(bridge, "chatArtifactVersions").mockImplementation(async () => current.revision.revision === 3
      ? [{ revision_id: 3, title: "Saved note", digest: "3", source_message_id: "m3", created_at_ms: at(29), restored_from_revision_id: 1 }, { revision_id: 2, title: "Saved note", digest: "2", source_message_id: "m2", created_at_ms: at(28) }, { revision_id: 1, title: "Saved note", digest: "1", source_message_id: "m1", created_at_ms: at(27) }]
      : [{ revision_id: 2, title: "Saved note", digest: "2", source_message_id: "m2", created_at_ms: at(28) }, { revision_id: 1, title: "Saved note", digest: "1", source_message_id: "m1", created_at_ms: at(27) }]);
    const restore = vi.spyOn(bridge, "chatArtifactRestore").mockImplementation(async () => { current = restoredView; return restoredView; });
    const onUpdateTab = vi.fn();
    render(<ChatWorkspacePanel {...panelProps(bridge)} onUpdateTab={onUpdateTab} />);
    expect(await screen.findByText("Second draft.")).toBeVisible();

    // The menu lists what Rust keeps, newest first, with dates; the current one is checked.
    await user.click(screen.getByRole("button", { name: "Version 2" }));
    const items = await screen.findAllByRole("menuitemradio");
    expect(items.map((item) => item.textContent)).toEqual([expect.stringMatching(/^Version 2 · CurrentOct 28/), expect.stringMatching(/^Version 1Oct 27/)]);
    expect(items[0]).toHaveAttribute("aria-checked", "true");
    await user.click(items[1]!);
    expect(await screen.findByText("First draft.")).toBeVisible();
    expect(screen.getByText(/^You're viewing version 1 from Oct 27/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Version 1" })).toBeVisible();

    // Restore asks first; Cancel leaves everything as it was. From the keyboard, focus follows the question and back.
    screen.getByRole("button", { name: "Restore this version" }).focus();
    await user.keyboard("{Enter}");
    let question = screen.getByRole("group", { name: /^Make version 1 current again\?/ });
    expect(within(question).getByRole("button", { name: "Restore" })).toHaveFocus();
    await user.keyboard("{Tab}{Enter}");
    expect(screen.queryByRole("group", { name: /^Make version 1/ })).toBeNull();
    expect(screen.getByRole("button", { name: "Restore this version" })).toHaveFocus();
    expect(restore).not.toHaveBeenCalled();

    // After a press, focus stays put rather than drawing a ring on the moved control.
    await user.click(screen.getByRole("button", { name: "Restore this version" }));
    question = screen.getByRole("group", { name: /^Make version 1 current again\?/ });
    expect(within(question).getByRole("button", { name: "Restore" })).not.toHaveFocus();
    await user.click(within(question).getByRole("button", { name: "Restore" }));
    expect(restore).toHaveBeenCalledExactlyOnceWith({ loupist_id: "loupist", thread_id: "chat", artifact_id: "note", revision_id: 1, expected_current_revision: 2 });
    // The panel shows the new current revision, says what happened and follows it.
    expect(await screen.findByText("Version 1 is back as version 3.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Version 3" })).toBeVisible();
    expect(screen.getByText("First draft.")).toBeVisible();
    expect(screen.queryByText(/^You're viewing/)).toBeNull();
    const follow = onUpdateTab.mock.calls.at(-1)!;
    expect(follow[0]).toBe(WORK_TAB_ID);
    expect(follow[1](panelProps(bridge).session.tabs[0])).toMatchObject({ artifact: { artifact_id: "note", revision: 3 }, artifactThreadId: "chat" });
    await user.click(screen.getByRole("button", { name: "Version 3" }));
    expect((await screen.findAllByRole("menuitemradio"))[0]).toHaveTextContent(/^Version 3 · Current.*Restored from version 1$/);
  });

  it("keeps the earlier version when Rust refuses the restore and says why", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const first = revisionView(1, 2, "First draft.", 27);
    vi.spyOn(bridge, "chatArtifactRead").mockImplementation(async (_loupist, _chat, _artifact, revision) => revision === 1 ? first : revisionView(2, 2, "Second draft.", 28));
    vi.spyOn(bridge, "chatArtifactVersions").mockResolvedValue([{ revision_id: 2, title: "Saved note", digest: "2", source_message_id: "m2", created_at_ms: at(28) }, { revision_id: 1, title: "Saved note", digest: "1", source_message_id: "m1", created_at_ms: at(27) }]);
    vi.spyOn(bridge, "chatArtifactRestore").mockRejectedValue(new Error("Artifact changed before restore."));
    render(<ChatWorkspacePanel {...panelProps(bridge)} />);
    await user.click(await screen.findByRole("button", { name: "Version 2" }));
    await user.click((await screen.findAllByRole("menuitemradio"))[1]!);
    await user.click(await screen.findByRole("button", { name: "Restore this version" }));
    await user.click(within(screen.getByRole("group", { name: /^Make version 1/ })).getByRole("button", { name: "Restore" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't restore this version.");
    expect(screen.getByText("First draft.")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Back to current" }));
    expect(await screen.findByText("Second draft.")).toBeVisible();
  });
});

describe("ChatWorkspacePanel Expand", () => {
  it("shows the open tab as a reading column with Back to chat, and Esc returns before it closes", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "chatArtifactRead").mockResolvedValue(view);
    const onClose = vi.fn();
    const { container } = render(<div className="app-stage"><div className="app-stage-main"><textarea aria-label="Message" /></div><ChatWorkspacePanel {...panelProps(bridge)} onClose={onClose} /></div>);
    const panel = screen.getByRole("complementary", { name: "Chat workspace panel" });
    expect(await screen.findByRole("heading", { name: "Saved note" })).toBeVisible();
    // From the keyboard, focus moves into the column and back to the panel menu.
    const actions = screen.getByRole("button", { name: "Panel menu" });
    actions.focus();
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("menuitem", { name: "Expand" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(panel).toHaveAttribute("data-expanded", "true");
    expect(screen.getByRole("button", { name: "Back to chat" })).toHaveFocus();
    // The reader stays mounted; the chat under the column leaves the focus order.
    expect(screen.getByRole("heading", { name: "Saved note" })).toBeVisible();
    expect(container.querySelector<HTMLElement>(".app-stage-main")!.inert).toBe(true);
    await user.keyboard("{Escape}");
    expect(panel).not.toHaveAttribute("data-expanded");
    expect(onClose).not.toHaveBeenCalled();
    expect(actions).toHaveFocus();
    expect(container.querySelector<HTMLElement>(".app-stage-main")!.inert).toBe(false);
    await user.click(actions);
    await user.click(await screen.findByRole("menuitem", { name: "Expand" }));
    await user.click(screen.getByRole("button", { name: "Back to chat" }));
    expect(panel).not.toHaveAttribute("data-expanded");
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("opens the full Map instead when the Map tab is shown", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const onClose = vi.fn();
    const onOpenKnowledgePage = vi.fn();
    render(<ChatMapProvider bridge={bridge} loupistId="loupist-quality" chatId="chat" messages={[]} onOpen={vi.fn()}>
      <ChatWorkspacePanel {...panelProps(bridge)} loupistId="loupist-quality" session={{ tabs: [], active_tab_id: undefined }} onClose={onClose} onOpenKnowledgePage={onOpenKnowledgePage} />
    </ChatMapProvider>);
    expect(await screen.findByRole("button", { name: "Open full map →" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Panel menu" }));
    await user.click(await screen.findByRole("menuitem", { name: "Expand" }));
    expect(onOpenKnowledgePage).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-quality", surface: "world", selection: undefined });
    expect(onClose).toHaveBeenCalledOnce();
    expect(screen.getByRole("complementary", { name: "Chat workspace panel" })).not.toHaveAttribute("data-expanded");
  });
});
