import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { chooseLoupist, mainTab, newChatButton, openApp } from "../renderApp";

type User = ReturnType<typeof userEvent.setup>;
const composer = () => screen.getByRole("textbox", { name: "Message Loupist" });
async function attachSource(user: User) {
  await user.click(await mainTab(/Map/));
  await user.click(await screen.findByRole("button", { name: "More actions" }));
  await user.click(await screen.findByRole("menuitemcheckbox", { name: "Library" }));
  await user.click(await screen.findByRole("tab", { name: "Sources" }));
  await user.click(await screen.findByRole("button", { name: /RFC 9110 HTTP semantics/ }));
  await user.click(await screen.findByRole("button", { name: "Add to question" }));
}
async function discussLens(user: User) {
  await user.click(await mainTab(/Lens/));
  const lens = await screen.findByRole("region", { name: /What .* knows/ });
  await user.click(await within(lens).findByRole("button", {name:/Compare independent accounts/}));
  await user.click(await within(lens).findByRole("button", { name: "Ask Bot to revise" }));
}

describe("World and Lens in the single Chat flow", () => {
  it("attaches an exact source to the current draft and sends it only with the next message", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const create = vi.spyOn(bridge, "chatCreate");
    const chatSend = vi.spyOn(bridge, "chatSend");
    await user.type(composer(), "Keep my draft");
    await attachSource(user);
    expect((composer() as HTMLTextAreaElement).value).toContain("Keep my draft");
    expect(document.querySelector('.composer-prompt-highlight [data-inline-reference="source"], .composer-prompt-highlight [data-inline-reference="source_span"]')).toHaveTextContent("RFC 9110");
    expect(composer()).toHaveFocus();
    expect(screen.getByRole("list", { name: "Added to this message" })).toHaveTextContent("RFC 9110");
    expect(create).not.toHaveBeenCalled();
    expect(chatSend).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(chatSend).toHaveBeenCalledOnce());
    expect(chatSend.mock.calls[0][0]).toEqual(expect.objectContaining({ thread_id: "thread-margin", pins: [expect.objectContaining({ kind: "source", objectId: "source-rfc-9110" })] }));
  });

  it("creates a new Chat only on the first send and keeps attached references", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const create = vi.spyOn(bridge, "chatCreate");
    const chatSend = vi.spyOn(bridge, "chatSend");
    await user.click(newChatButton());
    await attachSource(user);
    expect(create).not.toHaveBeenCalled();
    await user.type(composer(), "Read this source");
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(chatSend).toHaveBeenCalledOnce());
    expect(create).toHaveBeenCalledOnce();
    expect(chatSend.mock.calls[0][0].pins).toEqual([expect.objectContaining({ objectId: "source-rfc-9110" })]);
  });

  it("isolates and restores unsent text, files and references across Chats and Loupists", async () => {
    const user = userEvent.setup();
    await openApp();
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:saved-draft");
    await user.type(composer(), "First draft");
    await user.upload(screen.getByLabelText("Upload files"), new File(["source"], "draft.txt", { type: "text/plain" }));
    await attachSource(user);
    const firstDraft = (composer() as HTMLTextAreaElement).value;
    expect(firstDraft).toContain("First draft");
    await user.click(newChatButton());
    expect(composer()).toHaveValue("");
    expect(screen.queryByRole("list", { name: "Added to this message" })).toBeNull();
    await user.type(composer(), "Second draft");
    await user.click(screen.getByRole("button", { name: "How should we read the margin decline?" }));
    expect(composer()).toHaveValue(firstDraft);
    expect(screen.getByRole("list", { name: "Added to this message" })).toHaveTextContent("RFC 9110");
    await chooseLoupist(user, "Contrarian Value");
    expect(composer()).toHaveValue("");
    expect(screen.queryByRole("list", { name: "Added to this message" })).toBeNull();
    await chooseLoupist(user, "Quality Compounder");
    await user.click(screen.getByRole("button",{name:"How should we read the margin decline?"}));
    expect(composer()).toHaveValue(firstDraft);
    expect(screen.getByRole("button", { name: "Remove draft.txt" })).toBeVisible();
  });

  it("queues attached references for the next response instead of steering the live one", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 500 });
    await openApp(bridge);
    const steer = vi.spyOn(bridge, "chatSteer");
    const chatSend = vi.spyOn(bridge, "chatSend");
    fireEvent.change(composer(), { target: { value: "Start research" } });
    await user.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByRole("button", { name: "Stop response" });
    await attachSource(user);
    await user.type(composer(), "Include this source");
    expect(screen.getByRole("button", { name: "Guide this response" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Send next" }));
    expect(steer).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Pending chat instructions")).toHaveTextContent("Include this source");
    expect(chatSend).toHaveBeenCalledOnce();
    await user.click(screen.getByRole("button", { name: "Stop response" }));
  });

  it("opens a page's Source chip at its exact Source and returns to the page", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
    const source = catalog.sources[0];
    const read = vi.spyOn(bridge, "knowledgeSourceRead");
    await openApp(bridge);
    await user.click(await mainTab(/Lens/));
    await user.click(await screen.findByRole("button", { name: /^TCP, Topic/ }));
    const overview = (await screen.findByRole("heading", { name: "Overview" })).closest("section")!;
    await user.click(within(overview).getByRole("button", { name: "Source" }));
    expect(await screen.findByRole("button", { name: "Add to question" })).toBeVisible();
    expect(read).toHaveBeenCalledWith(expect.objectContaining({ recordId: source.recordId, revisionId: source.revisionId }));
    // The Source stays in the Lens tab, and Back returns to the page.
    const page = screen.getByRole("region", { name: /What .* knows/ });
    await user.click(within(page).getByRole("button", { name: /Back to/ }));
    expect(await screen.findByRole("heading", { name: "Overview" })).toBeVisible();
  });

  it("appends a Lens revision question once without replacing or sending the draft", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const chatSend = vi.spyOn(bridge, "chatSend");
    await user.click(screen.getByRole("button",{name:"Bot"}));
    await user.type(composer(), "Keep this research question.");
    await discussLens(user);
    const first = (composer() as HTMLTextAreaElement).value;
    expect(first).toContain("Keep this research question.");
    expect(first).toContain("Discuss this Lens:");
    expect(document.querySelector('.composer-prompt-highlight [data-inline-reference="lens"]')).toHaveTextContent("Compare independent accounts");
    await discussLens(user);
    expect(composer()).toHaveValue(first);
    expect(chatSend).not.toHaveBeenCalled();
  });
});
