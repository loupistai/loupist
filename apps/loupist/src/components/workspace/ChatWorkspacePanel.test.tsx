import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { renderWithOpen, destinations } from "../../test/open";
import { ChatMapProvider } from "./ChatMapContext";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatArtifact, ChatArtifactView } from "../../types";
import { ChatWorkspacePanel } from "./ChatWorkspacePanel";
import { AssetPageOpenerProvider } from "../assets/assetOpener";
import { GRID_SCHEMA } from "../../contracts/chat";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { EVIDENCE_TAB_ID, WORK_TAB_ID, evidenceEntry, filesWorkspaceTab, pushEvidenceEntry, workWorkspaceTab, type ChatWorkspaceSession } from "../../chat/workspace";

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
    const panel = screen.getByLabelText("Side panel");
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
  it("lets Esc put back an edited web address before it closes the panel", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const onClose = vi.fn();
    const page = { id: "browser:q3", kind: "browser" as const, url: "https://hanul.example/q3", title: "Q3 review", loading: false };
    render(<ChatWorkspacePanel {...panelProps(bridge)} onClose={onClose} session={{ tabs: [page], active_tab_id: page.id }} />);
    const address = screen.getByRole("textbox", { name: "Web address" });
    await user.clear(address);
    await user.type(address, "https://morrow.example");
    await user.keyboard("{Escape}");
    expect(address).toHaveValue("https://hanul.example/q3");
    expect(onClose).not.toHaveBeenCalled();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("lets Esc clear the file filter before it closes the panel", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "workspaceList").mockResolvedValue({ path: "", truncated: false, entries: [{ kind: "file", name: "notes.md", path: "notes.md", size_bytes: 12 }] });
    const onClose = vi.fn();
    const files = filesWorkspaceTab();
    render(<ChatWorkspacePanel {...panelProps(bridge)} onClose={onClose} session={{ tabs: [files], active_tab_id: files.id }} />);
    const filter = await screen.findByRole("textbox", { name: "Filter files" });
    await user.type(filter, "zz");
    await user.keyboard("{Escape}");
    expect(filter).toHaveValue("");
    expect(onClose).not.toHaveBeenCalled();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledOnce();
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

const workItem = (id: string, title: string, kind: "chart" | "markdown" = "markdown", updated = 1_700_000_000_000): ChatArtifact => ({
  id, chat_session_id: "chat", kind, title, current_revision: 2, created_at_ms: updated, updated_at_ms: updated,
});

describe("ChatWorkspacePanel Work tab", () => {
  it("lists the session’s work and opens its published items while a new chart is building", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "chatArtifactList").mockResolvedValue([workItem("note", "Saved note"), workItem("older", "Supplier map", "chart")]);
    const read = vi.spyOn(bridge, "chatArtifactRead").mockResolvedValue({ ...view, artifact: { ...view.artifact, id: "older", chat_session_id: "chat" } });
    const props = { ...panelProps(bridge), session: { tabs: [], active_tab_id: WORK_TAB_ID } };
    render(<ChatWorkspacePanel {...props} buildingWork={[{ key: "building:step", title: "Margin vs utilization" }]} />);
    const list = await screen.findByRole("list", { name: "Work from this conversation" });
    expect(list).toHaveTextContent("Margin vs utilization");
    expect(list).toHaveTextContent("Preparing");
    expect(screen.getByRole("button", { name: /Supplier map/ })).toHaveTextContent("Chart · Version 2");
    await user.click(screen.getByRole("button", { name: /Supplier map/ }));
    expect(read).toHaveBeenCalledWith("loupist", "chat", "older");
    expect(props.onOpenArtifact).toHaveBeenCalledWith(expect.objectContaining({ artifact_id: "older", digest: "a".repeat(64) }), "chat");
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

  it("moves keyboard focus into an opened item and back to its row, and keeps it on the row when the read fails", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "chatArtifactList").mockResolvedValue([workItem("note", "Saved note"), workItem("older", "Supplier map", "chart")]);
    const read = vi.spyOn(bridge, "chatArtifactRead").mockRejectedValueOnce(new Error("gone")).mockResolvedValue(view);
    // The shell keeps the Work tab: opening an item puts it in the tab, "All work" clears it.
    function Shell() {
      const [session, setSession] = useState<ChatWorkspaceSession>({ tabs: [], active_tab_id: WORK_TAB_ID });
      return <ChatWorkspacePanel {...panelProps(bridge)} session={session}
        onOpenArtifact={(artifact, threadId) => setSession({ tabs: [workWorkspaceTab(artifact, threadId)], active_tab_id: WORK_TAB_ID })}
        onUpdateTab={(id, update) => setSession((current) => ({ ...current, tabs: current.tabs.map((tab) => tab.id === id ? update(tab) : tab) }))} />;
    }
    render(<Shell />);
    const note = await screen.findByRole("button", { name: /Saved note/ });
    note.focus();
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not open this item.");
    expect(note).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("heading", { name: "Saved note" })).toBeVisible();
    await waitFor(() => expect(screen.getByRole("button", { name: "All work" })).toHaveFocus());
    expect(read).toHaveBeenCalledTimes(3);
    await user.keyboard("{Enter}");
    await waitFor(() => expect(screen.getByRole("button", { name: /Saved note/ })).toHaveFocus());
  });

  it("does not open an item whose read finishes after the chat changed", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const pending = deferred<ChatArtifactView>();
    vi.spyOn(bridge, "chatArtifactList").mockResolvedValue([workItem("note", "Saved note")]);
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

  it("pins the placement for every kind from the Dock control's menu, naming it, and shows when it is on", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "chatArtifactRead").mockResolvedValue(view);
    const onPinPlacement = vi.fn();
    const mounted = render(<ChatWorkspacePanel {...panelProps(bridge)} onPinPlacement={onPinPlacement} />);
    await user.click(screen.getByRole("button", { name: "Panel menu" }));
    // The choice names where the panel is, so "here" is never a guess.
    const pin = await screen.findByRole("menuitemcheckbox", { name: "Always dock at side" });
    expect(pin).toHaveAttribute("aria-checked", "false");
    await user.click(pin);
    expect(onPinPlacement).toHaveBeenLastCalledWith(true);
    mounted.rerender(<ChatWorkspacePanel {...panelProps(bridge)} placement="bottom" placementPinned onPinPlacement={onPinPlacement} />);
    await user.click(screen.getByRole("button", { name: "Panel menu" }));
    expect(await screen.findByRole("menuitem", { name: "Dock at side" })).toBeVisible();
    const pinned = await screen.findByRole("menuitemcheckbox", { name: "Always dock at bottom" });
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
    const panel = screen.getByRole("complementary", { name: "Side panel" });
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

  it("opens the full World page instead when World tab is shown, leaving the panel for the way back", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const onClose = vi.fn();
    const onOpenKnowledgePage = vi.fn();
    render(<ChatMapProvider bridge={bridge} loupistId="loupist-quality" chatId="chat" messages={[]} onOpen={vi.fn()}>
      <ChatWorkspacePanel {...panelProps(bridge)} loupistId="loupist-quality" session={{ tabs: [], active_tab_id: undefined }} onClose={onClose} onOpenKnowledgePage={onOpenKnowledgePage} />
    </ChatMapProvider>);
    expect(await screen.findByRole("button", { name: "Open World →" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Panel menu" }));
    await user.click(await screen.findByRole("menuitem", { name: "Expand" }));
    expect(onOpenKnowledgePage).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-quality", surface: "world", selection: undefined });
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("complementary", { name: "Side panel" })).not.toHaveAttribute("data-expanded");
  });
});

it("opens a grid row from Work of the version shown by its ticker, through the shell's asset opener", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const at = (day: number) => Date.UTC(2026, 9, day, 9);
  const grid = { layout: "assets" as const, rows: [{ symbol: "BRK.B", market: "NYSE", label: "Berkshire Hathaway" }], columns: [{ id: "revenue", label: "Revenue", unit: "amount" as const }],
    cells: [[{ empty: "not_reported" as const }]] };
  const version = (revision: number, day: number): ChatArtifactView => ({
    artifact: { id: "table", chat_session_id: "chat", kind: "grid", title: "Revenue", current_revision: 2, created_at_ms: at(1), updated_at_ms: at(28) },
    revision: { artifact_id: "table", revision, payload: { kind: "grid", schema: GRID_SCHEMA, grid }, digest: String(revision), source_message_id: `m${revision}`, created_at_ms: at(day) },
    evidence_refs: [], chart_dataset: null,
  } as ChatArtifactView);
  vi.spyOn(bridge, "chatArtifactRead").mockImplementation(async (_loupist, _chat, _artifact, revision) => revision === 1 ? version(1, 27) : version(2, 28));
  vi.spyOn(bridge, "chatArtifactVersions").mockResolvedValue([
    { revision_id: 2, title: "Revenue", digest: "2", source_message_id: "m2", created_at_ms: at(28) },
    { revision_id: 1, title: "Revenue", digest: "1", source_message_id: "m1", created_at_ms: at(27) },
  ]);
  const openAsset = vi.fn();
  const props = { ...panelProps(bridge), session: { tabs: [workWorkspaceTab({ artifact_id: "table", revision: 2, kind: "grid" as const, title: "Revenue", digest: "2" }, "chat")], active_tab_id: WORK_TAB_ID } };
  render(<AssetPageOpenerProvider value={openAsset}><ChatWorkspacePanel {...props} /></AssetPageOpenerProvider>);
  await screen.findByRole("table", { name: "Revenue" });
  await user.click(screen.getByRole("button", { name: "Version 2" }));
  await user.click((await screen.findAllByRole("menuitemradio"))[1]!);
  expect(await screen.findByText(/^You're viewing version 1/u)).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Berkshire Hathaway" }));
  expect(openAsset).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist", ticker: "BRK.B", name: "Berkshire Hathaway" });
});

it("mixes this session's written files with published work, newest first, and opens the file in Files", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const updated = Date.parse("2026-11-01T12:00:00Z");
  const list = vi.spyOn(bridge, "chatArtifactList").mockResolvedValue([workItem("note", "Earlier report", "markdown", updated)]);
  const onOpenFiles = vi.fn();
  const step = { id: "step", kind: "tool" as const, category: "files" as const, operation: "write" as const, action: "Wrote file", status: "succeeded" as const, target: null, href: null, filePath: "outputs/revenue.csv", startedAtMs: updated + 1, finishedAtMs: updated + 2 };
  const answer = { id: "answer", role: "assistant" as const, status: "complete" as const, created_at: new Date(updated + 3).toISOString(),
    content: "[Download](outputs/revenue.csv) · [Input](inputs/report.txt)", progress: { startedAtMs: updated, items: [step, { ...step, id: "unlinked", filePath: "outputs/notes.md" }, { ...step, id: "failed", status: "failed" as const, filePath: "outputs/failed.txt" }], finishedAtMs: updated + 3 } };
  render(<ChatWorkspacePanel {...panelProps(bridge)} workMessages={[{ ...answer, id: "write", content: "" }, { ...answer, progress: undefined }]} onOpenFiles={onOpenFiles} session={{ tabs: [], active_tab_id: WORK_TAB_ID }} />);
  const report = await screen.findByRole("button", { name: /Earlier report/ });
  const file = screen.getByRole("button", { name: /revenue\.csv/ });
  expect(file.compareDocumentPosition(report) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(screen.queryByText("report.txt")).toBeNull();
  expect(screen.queryByText("failed.txt")).toBeNull();
  expect(screen.getByRole("button", { name: /notes\.md/ })).toBeVisible();
  expect(list).toHaveBeenCalledWith("loupist", "chat");
  await userEvent.click(file);
  expect(onOpenFiles).toHaveBeenCalledWith("outputs/revenue.csv");
});

it("lists the files Python wrote even when an answer links an existing input or a run stops early", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "chatArtifactList").mockResolvedValue([]);
  const paths = ["outputs/revenue.csv", "outputs/revenue.png", "outputs/revenue.svg", "outputs/report.md", "outputs/report.pdf"];
  const step = { id: "python", kind: "tool" as const, category: "files" as const, operation: "execute" as const, toolName: "run_python",
    action: "Ran Python", status: "failed" as const, target: null, href: null, filePath: paths[0], writtenPaths: paths, startedAtMs: 1, finishedAtMs: 2 };
  const answer = { id: "answer", role: "assistant" as const, status: "error" as const, created_at: "2026-11-01T12:00:00Z",
    content: "[Source](inputs/filing.txt)", progress: { startedAtMs: 1, items: [step,
      { ...step, id: "retained-only", filePath: "inputs/filing.txt", writtenPaths: undefined }], finishedAtMs: 2 } };
  const onOpenFiles = vi.fn();
  render(<ChatWorkspacePanel {...panelProps(bridge)} workMessages={[answer]} onOpenFiles={onOpenFiles} session={{ tabs: [], active_tab_id: WORK_TAB_ID }} />);
  const list = await screen.findByRole("list", { name: "Work from this conversation" });
  expect(within(list).getAllByRole("button")).toHaveLength(5);
  for (const path of paths) expect(within(list).getByText(path)).toBeVisible();
  expect(within(list).queryByText("filing.txt")).toBeNull();
  await userEvent.click(within(list).getByText("report.pdf"));
  expect(onOpenFiles).toHaveBeenCalledWith("outputs/report.pdf");
});
