import { headerAction } from "../renderApp";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { workspaceOverlayMedia } from "../../components/workspace/useWorkspacePanelResize";
import { newChatButton, openApp } from "../renderApp";

type User = ReturnType<typeof userEvent.setup>;
const send = async (user: User, text: string) => {
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), text);
  await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
  await user.click(screen.getByRole("button", { name: "Send" }));
};

// Wait for the actual document event before querying accessible names. Repeated role scans
// of the whole shell during streamed deltas can starve the mock transport's timer queue.
async function requestDocument(user: User, bridge: MockLoupistBridge) {
  const admitted = vi.spyOn(bridge, "chatSend");
  await send(user, "Create a Markdown document");
  await waitFor(() => expect(admitted).toHaveBeenCalledWith(
    expect.objectContaining({ prompt: "Create a Markdown document" }), expect.any(Function),
  ));
  await waitFor(() => expect(document.querySelector(".message-output-card")).toBeInTheDocument());
}

describe("Loupist · Chat workspace", () => {
  it("restores a saved reader through initial Loupist selection", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const source = (await bridge.knowledgeCatalog({ loupistId: "loupist-quality" })).sources[0];
    const sourceRead = vi.spyOn(bridge, "knowledgeSourceRead");
    await bridge.workspaceUiWrite({
      version: 1, open: true, placement: "bottom", width: 610, height: 390,
      sessions: [{ loupist_id: "loupist-quality", thread_id: "thread-margin", tabs: [
        { id: "files", kind: "files", title: "Files", path: "reports", filePath: "reports/result.md" },
        { id: "saved-source", kind: "memory", title: "Restored source", record_id: source.recordId, location: { loupistId: "loupist-quality", surface: "source", recordId: source.recordId, revisionId: source.revisionId, selection: { kind: "source", sourceId: source.objects[0].id } } },
      ], active_tab_id: "saved-source" }],
    });
    await openApp(bridge);
    const panel = await screen.findByLabelText("Chat workspace panel");
    expect(panel).toHaveAttribute("data-placement", "bottom");
    // A saved reader reopens as the Evidence tab's current entry.
    expect(within(panel).getByRole("tab", { name: "Evidence" })).toHaveAttribute("aria-selected", "true");
    expect(within(panel).getByText("Saved source · Quality Compounder")).toBeVisible();
    await waitFor(() => expect(sourceRead).toHaveBeenCalledWith(expect.objectContaining({ recordId: source.recordId, revisionId: source.revisionId, sourceId: source.objects[0].id })));
  });

  it("chooses a panel and split from the header while preserving the current chat draft", async () => {
    const user = userEvent.setup();
    await openApp();
    const composer = screen.getByRole("textbox", { name: "Message Loupist" });
    await user.type(composer, "A question I am still writing");
    expect(screen.queryByLabelText("Chat workspace panel")).not.toBeInTheDocument();
    const kind = screen.getByRole("button", { name: /Panel kind/ });
    await user.click(kind);
    const kinds = screen.getAllByRole("menuitemradio");
    expect(kinds).toHaveLength(3);
    expect(kinds[0]).toHaveAccessibleName("Map");
    expect(kinds[1]).toHaveAccessibleName("Lens");
    expect(kinds[2]).toHaveAccessibleName("Chat materials");
    expect(kinds[1]).toHaveAccessibleDescription("Your research principles and methods");
    expect(kinds[2]).toHaveAccessibleDescription("This chat's map, evidence and outputs");
    await user.click(screen.getByRole("menuitemradio", { name: "Lens" }));
    const workspace = await screen.findByLabelText("Chat workspace panel");
    expect(workspace).toHaveAttribute("data-panel-kind", "lens");
    expect(composer).toHaveValue("A question I am still writing");
    await user.click(screen.getByRole("button", { name: "Split top and bottom" }));
    expect(workspace).toHaveAttribute("data-placement", "bottom");
    expect(screen.getByRole("button", { name: "Split top and bottom" })).toHaveAttribute("aria-pressed", "true");
    await user.click(kind);
    await user.click(screen.getByRole("menuitemradio", { name: "Map" }));
    expect(workspace).toHaveAttribute("data-panel-kind", "world");
    expect(workspace).toHaveAttribute("data-placement", "bottom");
    expect(composer).toHaveValue("A question I am still writing");
    await user.click(kind);
    await user.click(screen.getByRole("menuitemradio", { name: "Chat materials" }));
    expect(workspace).toHaveAttribute("data-panel-kind", "materials");
    await user.click(within(workspace).getByRole("tab", { name: "Evidence" }));
    await user.click(within(workspace).getByRole("button", { name: "Panel menu" }));
    await user.click(await screen.findByRole("menuitem", { name: "Expand" }));
    expect(workspace).toHaveAttribute("data-expanded", "true");
    await user.click(within(workspace).getByRole("button", { name: "Back to chat" }));
    expect(workspace).not.toHaveAttribute("data-expanded");
    await user.click(within(workspace).getByRole("button", { name: "Close workspace" }));
    expect(screen.queryByLabelText("Chat workspace panel")).not.toBeInTheDocument();
    // Selecting the current kind reopens it. Keyboard selection moves to Close.
    kind.focus();
    await user.keyboard("{Enter}");
    await user.keyboard("{End}{Enter}");
    const close = await screen.findByRole("button", { name: "Close workspace" });
    await waitFor(() => expect(close).toHaveFocus());
    await user.keyboard("{Enter}");
    expect(kind).toHaveFocus();
    await user.keyboard("{Meta>}\\{/Meta}");
    expect(await screen.findByLabelText("Chat workspace panel")).toBeInTheDocument();
    expect(composer).toHaveValue("A question I am still writing");
  });

  it("traps focus in the narrow workspace dialog and restores the background on Escape", async () => {
    const user = userEvent.setup();
    const matchMedia = vi.mocked(window.matchMedia);
    const previous = matchMedia.getMockImplementation();
    matchMedia.mockImplementation((query: string) => ({
      matches: query === workspaceOverlayMedia, media: query, onchange: null,
      addEventListener: vi.fn(), removeEventListener: vi.fn(), addListener: vi.fn(), removeListener: vi.fn(), dispatchEvent: vi.fn(),
    }));
    try {
      const bridge = await openApp();
      const composer = screen.getByRole("textbox", { name: "Message Loupist" });
      await requestDocument(user, bridge);
      // The overlay sheet would take focus, so a finished document never opens it by itself.
      expect(screen.queryByRole("dialog", { name: "Chat workspace panel" })).not.toBeInTheDocument();
      await user.click(headerAction("Panel"));
      const dialog = await screen.findByRole("dialog", { name: "Chat workspace panel" });
      // The overlay sheet offers Close only; Expand and Dock belong to the docked panel menu.
      expect(within(dialog).queryByRole("button", { name: "Panel menu" })).not.toBeInTheDocument();
      expect(screen.queryByRole("menuitem", { name: "Expand" })).not.toBeInTheDocument();
      expect(within(dialog).getByRole("button", { name: "Close workspace" })).toBeVisible();
      composer.focus();
      fireEvent.keyDown(document, { key: "Tab" });
      expect(dialog).toContainElement(document.activeElement as HTMLElement);
      fireEvent.keyDown(window, { key: "Escape" });
      await waitFor(() => expect(screen.queryByRole("dialog", { name: "Chat workspace panel" })).not.toBeInTheDocument());
      expect(composer.closest("[inert]")).toBeNull();
    } finally {
      if (previous) matchMedia.mockImplementation(previous);
    }
  });

  it("selects work explicitly and sends only its ID and version for one turn", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    await requestDocument(user, bridge);
    await user.click(screen.getByRole("button", { name: "Open document Research note" }));
    const workspace = await screen.findByLabelText("Chat workspace panel");
    expect(within(workspace).getByRole("heading", { name: "Research note" })).toBeVisible();
    const ask = await within(workspace).findByRole("button", { name: "Ask about this" });
    await waitFor(() => expect(ask).toBeEnabled());
    expect(chatSend.mock.calls[0][0].selected_artifact).toBeUndefined();
    await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Update the conclusion");
    await user.click(ask);
    expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveValue("Update the conclusion ⟦Research note⟧ ");
    expect(screen.getByRole("list", { name: "Added to this message" })).toHaveTextContent("Research note");
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2));
    expect(chatSend.mock.calls[1][0].selected_artifact).toEqual({ artifact_id: expect.any(String), revision: 1 });
    expect(screen.queryByRole("list", { name: "Added to this message" })).not.toBeInTheDocument();
    await within(workspace).findByRole("button", { name: "Version 2" });
    await waitFor(() => expect(within(workspace).getByRole("button", { name: "Ask about this" })).toBeEnabled());
    await user.click(within(workspace).getByRole("button", { name: "Ask about this" }));
    await user.click(newChatButton());
    expect(screen.queryByRole("list", { name: "Added to this message" })).not.toBeInTheDocument();
  });

  it("restores an earlier version of the open document as its newest version", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const restore = vi.spyOn(bridge, "chatArtifactRestore");
    await openApp(bridge);
    await requestDocument(user, bridge);
    await user.click(screen.getByRole("button", { name: "Open document Research note" }));
    const workspace = await screen.findByLabelText("Chat workspace panel");
    const ask = await within(workspace).findByRole("button", { name: "Ask about this" });
    await waitFor(() => expect(ask).toBeEnabled());
    await user.click(ask);
    await send(user, "Update the conclusion");
    await within(workspace).findByRole("button", { name: "Version 2" });
    await waitFor(() => expect(within(workspace).getByRole("button", { name: "Ask about this" })).toBeEnabled());
    await user.click(within(workspace).getByRole("button", { name: "Version 2" }));
    await user.click(await screen.findByRole("menuitemradio", { name: /^Version 1/ }));
    await user.click(await within(workspace).findByRole("button", { name: "Restore this version" }));
    await user.click(within(workspace).getByRole("button", { name: "Restore" }));
    expect(await within(workspace).findByText("Version 1 is back as version 3.")).toBeVisible();
    expect(within(workspace).getByRole("button", { name: "Version 3" })).toBeVisible();
    expect(restore).toHaveBeenCalledWith(expect.objectContaining({ revision_id: 1, expected_current_revision: 2 }));
  });

  it("opens answer links in the in-app browser without adding browser state to agent context", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const chatSend = vi.spyOn(bridge, "chatSend");
    const open = vi.spyOn(bridge, "browserTabOpen");
    const navigate = vi.spyOn(bridge, "browserTabNavigate");
    const history = vi.spyOn(bridge, "browserTabHistory");
    await openApp(bridge);
    fireEvent.change(screen.getByRole("textbox", { name: "Message Loupist" }), {
      target: { value: "Review [Example source](https://example.com/report) and [Second source](https://example.com/other)" },
    });
    await user.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByText("Response complete.");
    const answerLink = await screen.findByRole("link", { name: /Example source/ });
    await user.click(answerLink);
    await user.click(answerLink);
    const workspace = await screen.findByLabelText("Chat workspace panel");
    await waitFor(() => expect(open).toHaveBeenCalledTimes(1));
    await user.click(await screen.findByRole("link", { name: "Second source" }));
    await waitFor(() => expect(open).toHaveBeenCalledTimes(2));
    expect(within(workspace).getAllByRole("tab", { name: "example.com" })).toHaveLength(2);

    const address = within(workspace).getByRole("textbox", { name: "Web address" });
    await user.clear(address);
    await user.type(address, "example.org/next");
    await user.click(within(workspace).getByRole("button", { name: "Open" }));
    await waitFor(() => expect(navigate).toHaveBeenCalledWith(expect.any(String), "https://example.org/next"));
    await user.click(within(workspace).getByRole("button", { name: "Go back" }));
    expect(history).toHaveBeenCalledWith(expect.any(String), "back");
    await user.clear(address);
    await user.type(address, "quarterly margin research");
    await user.click(within(workspace).getByRole("button", { name: "Open" }));
    expect(await within(workspace).findByRole("alert")).toHaveTextContent(/web address, not a search query/i);
    expect(navigate).toHaveBeenCalledTimes(1);

    await send(user, "Summarize the conversation without the open page");
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2));
    const followUp = JSON.stringify(chatSend.mock.calls[1][0]);
    expect(followUp).not.toContain("example.com");
    expect(followUp).not.toContain("example.org");
  });
});
