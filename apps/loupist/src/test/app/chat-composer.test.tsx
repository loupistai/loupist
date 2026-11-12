import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { openApp, privateChatToggle } from "../renderApp";

type User = ReturnType<typeof userEvent.setup>;
const composer = () => screen.getByRole("textbox", { name: "Message Loupist" });
const modelMenuButton = () => screen.getByRole("button", { name: /Model settings for this message/ });
const chooseModelOption = async (user: User, name: string | RegExp) => {
  await user.click(modelMenuButton());
  if (!screen.queryByRole("menuitemradio", { name })) await user.click(screen.getByRole("menuitem", { name: "Advanced" }));
  await user.click(await screen.findByRole("menuitemradio", { name }));
  await user.keyboard("{Escape}");
};
const send = async (user: User, text: string) => {
  await user.type(composer(), text);
  await user.click(screen.getByRole("button", { name: "Send" }));
};

describe("Loupist · Chat composer", () => {
  it("keeps a saved Chat with Memory writes disabled private when sending", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const workspace = await bridge.loupistSelect("loupist-quality");
    workspace.threads[0].update_memory = false;
    vi.spyOn(bridge, "loupistSelect").mockResolvedValue(workspace);
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    expect(privateChatToggle()).toHaveAttribute("aria-pressed", "true");
    await send(user, "Keep this research private");
    await waitFor(() => expect(chatSend).toHaveBeenCalled());
    expect(chatSend.mock.calls[0][0]).toMatchObject({ use_memory: false, update_memory: false });
  });

  it("does not submit while an IME composition is active", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const chatSend = vi.spyOn(bridge, "chatSend");
    await user.type(composer(), "Composed input");
    fireEvent.keyDown(composer(), { key: "Enter", code: "Enter", isComposing: true, keyCode: 229 });
    expect(chatSend).not.toHaveBeenCalled();
    expect(composer()).toHaveValue("Composed input");
  });

  it("steers the active response and sends an explicit follow-up afterwards", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const release = bridge.pauseChatStreams();
    try {
      const chatSteer = vi.spyOn(bridge, "chatSteer");
      const chatSend = vi.spyOn(bridge, "chatSend");
      await openApp(bridge);
      await send(user, "Start the long analysis");
      await screen.findByRole("button", { name: "Stop response" });
      expect(screen.queryByRole("button", { name: "Guide this response" })).toBeNull();
      expect(screen.queryByRole("button", { name: "Send next" })).toBeNull();

      await user.type(composer(), "Focus on downside risk");
      await user.click(screen.getByRole("button", { name: "Guide this response" }));
      await waitFor(() => expect(chatSteer).toHaveBeenCalledWith(expect.objectContaining({ prompt: "Focus on downside risk", thread_id: "thread-margin" })));
      expect(composer()).toHaveValue("");
      // Guidance waits inside the running reply's timeline until it applies.
      expect(within(screen.getByLabelText("Conversation")).getByText(/Focus on downside risk/)).toHaveClass("chat-guiding-row");

      await user.type(composer(), "Queue this next");
      await user.click(screen.getByRole("button", { name: "Send next" }));
      const queue = screen.getByLabelText("Pending chat instructions");
      expect(within(queue).getByText("Queue this next")).toBeVisible();
      expect(chatSend).toHaveBeenCalledTimes(1);
      release();
      await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2), { timeout: 15_000 });
      expect(chatSend.mock.calls[1][0]).toMatchObject({ prompt: "Queue this next" });
      expect(chatSteer).toHaveBeenCalledTimes(1);
    } finally {
      release();
    }
  });

  it("freezes queued model settings, Memory authority and files before later composer changes", { timeout: 30_000 }, async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 8_000 });
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    await send(user, "Start analysis");
    await screen.findByRole("button", { name: "Stop response" });
    await user.click(privateChatToggle());
    await chooseModelOption(user, "Max");
    await user.upload(screen.getByLabelText("Upload files"), new File(["original attachment"], "queued.txt", { type: "text/plain" }));
    await user.type(composer(), "Use this next");
    await user.click(screen.getByRole("button", { name: "Send next" }));
    expect(within(screen.getByLabelText("Pending chat instructions")).getByText(/Memory off/)).toHaveTextContent("1 file");
    await user.click(privateChatToggle());
    await chooseModelOption(user, "Medium");
    bridge.setStreamDelay(0);
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2), { timeout: 15_000 });
    expect(chatSend.mock.calls[1][0]).toMatchObject({
      prompt: "Use this next", use_memory: false, update_memory: false, thinking_level: "max",
      attachments: [{ filename: "queued.txt", media_type: "text/plain" }],
    });
  });

  it("retains a rejected submission with its files and settings while keeping the newer draft", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    let rejectStart!: (error: Error) => void;
    const chatSend = vi.spyOn(bridge, "chatSend").mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectStart = reject; }));
    await openApp(bridge);
    await user.click(privateChatToggle());
    await user.upload(screen.getByLabelText("Upload files"), new File(["keep original"], "kept.txt", { type: "text/plain" }));
    await send(user, "Rejected request");
    await waitFor(() => expect(chatSend).toHaveBeenCalledOnce());
    await user.type(composer(), "Newer unsent thought");
    rejectStart(new Error("Provider is unavailable"));
    await screen.findByRole("button", { name: "Send" });
    const queue = screen.getByLabelText("Pending chat instructions");
    expect(within(queue).getByText(/Memory off/)).toHaveTextContent("1 file");
    await user.click(within(queue).getByRole("button", { name: "Queued message actions" }));
    await user.click(screen.getByRole("menuitem", { name: "Send queued message now" }));
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2));
    expect(chatSend.mock.calls[1][0]).toMatchObject({ prompt: "Rejected request", use_memory: false, update_memory: false, attachments: [{ filename: "kept.txt" }] });
    expect(composer()).toHaveValue("Newer unsent thought");
  });

  it("keeps queued follow-ups when the active response is stopped", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 8_000 });
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    await send(user, "Start a long analysis");
    await screen.findByRole("button", { name: "Stop response" });
    await user.type(composer(), "Ask this after the current turn");
    await user.keyboard("{Enter}");
    await user.click(screen.getByRole("button", { name: "Stop response" }));
    expect(await screen.findByText("Response stopped. Queued messages were kept.")).toBeVisible();
    expect(within(screen.getByLabelText("Pending chat instructions")).getByText("Ask this after the current turn")).toBeVisible();
    expect(chatSend).toHaveBeenCalledTimes(1);
  });

  it("adds Skills and connections from the picker and sends the mentioned connection", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    await bridge.marketplaceConnectorConfigure({ entry_id: "sec.edgar", config: { contact_email: "research@example.com" } });
    await bridge.loupistCapabilityEnable({ loupist_id: "loupist-quality", entry_id: "sec.edgar" });
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    await user.type(composer(), "@sec");
    await user.click(await screen.findByRole("option", { name: /SEC EDGAR/ }));
    await user.type(composer(), "/world");
    expect(await screen.findByRole("option", { name: /^Map\b/ })).toBeVisible();
    await user.keyboard("{Enter}");
    const added = screen.getByRole("list", { name: "Added to this message" });
    expect(added).toHaveTextContent("SEC EDGAR");
    expect(screen.getByRole("list", { name: "Message modes" })).toHaveTextContent("Map");
    expect(chatSend).not.toHaveBeenCalled();

    fireEvent.change(composer(), { target: { value: "$research $world @sec.edgar Samsung Electronics" } });
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(chatSend).toHaveBeenCalled());
    expect(chatSend.mock.calls[0][0]).toMatchObject({ mentioned_connection_ids: ["sec.edgar"] });
  });

  it.each(["$world", "> $lens"])("preserves explicit Memory settings when the prompt contains %s", async (selection) => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    await user.click(privateChatToggle());
    fireEvent.change(composer(), { target: { value: `${selection} inspect the evidence` } });
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(chatSend).toHaveBeenCalled());
    expect(chatSend.mock.calls[0][0]).toMatchObject({ prompt: `${selection} inspect the evidence`, use_memory: false, update_memory: false });
  });

  it("sends the model and performance option selected for that message", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    await bridge.providerConfigure({ provider: "anthropic", api_key: "test-key" });
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    await chooseModelOption(user, "Claude Sonnet 4.5");
    await send(user, "Use this model once");
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(1));
    expect(chatSend.mock.calls[0][0]).toMatchObject({ model_profile_id: "anthropic/claude-sonnet-4-5", thinking_level: "medium", run_options: {} });

    await screen.findByText("Response complete.");
    await chooseModelOption(user, "GPT-5.6 Luna");
    await chooseModelOption(user, "Fast");
    await send(user, "Use the fast tier");
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2));
    expect(chatSend.mock.calls[1][0]).toMatchObject({ model_profile_id: "model-test", thinking_level: "medium", run_options: { performance: "fast" } });
  });

  it("keeps the exact thinking level across turns when Memory authority changes", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    await chooseModelOption(user, "Max");
    await send(user, "First turn keeps Memory enabled");
    await screen.findByText("Response complete.");
    await user.click(privateChatToggle());
    await send(user, "Second turn disables Memory");
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2));
    expect(chatSend.mock.calls.map(([request]) => request.thinking_level)).toEqual(["max", "max"]);
    expect(chatSend.mock.calls[1][0]).toMatchObject({ use_memory: false, update_memory: false });
  });

  it("attaches uploaded files and pasted images to a Chat message", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    await bridge.providerConfigure({ provider: "anthropic", api_key: "test-key" });
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    await chooseModelOption(user, "Claude Sonnet 4.5");
    await user.upload(screen.getByLabelText("Upload files"), new File(["margin bridge"], "notes.txt", { type: "text/plain" }));
    const chart = new File([new Uint8Array([137, 80, 78, 71])], "chart.png", { type: "image/png" });
    fireEvent.paste(composer(), { clipboardData: { items: [{ kind: "file", getAsFile: () => chart }] } });
    await send(user, "Review these inputs");
    await waitFor(() => expect(chatSend).toHaveBeenCalled());
    expect(chatSend.mock.calls[0][0]).toMatchObject({
      prompt: "Review these inputs",
      attachments: [{ filename: "notes.txt", media_type: "text/plain" }, { filename: "chart.png", media_type: "image/png" }],
    });
    // A sent image is a thumbnail that opens the lightbox; other files keep their download row.
    expect(await screen.findByRole("button", { name: "Open chart.png" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Download notes.txt" })).toBeVisible();
  });

  it("keeps the draft and image when the selected model rejects images", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    await user.upload(screen.getByLabelText("Upload files"), new File([new Uint8Array([137, 80, 78, 71])], "chart.png", { type: "image/png" }));
    await send(user, "Review this chart");
    expect(await screen.findByRole("alert")).toHaveTextContent("The selected model does not accept images.");
    expect(composer()).toHaveValue("Review this chart");
    expect(screen.getByRole("img", { name: "chart.png" })).toBeVisible();
    expect(chatSend).not.toHaveBeenCalled();
  });

  it("shows the generated title for a new Chat", async () => {
    const user = userEvent.setup();
    await openApp();
    await user.type(composer(), "/new");
    await user.click(await screen.findByRole("option", { name: "New chat" }));
    await screen.findByRole("heading", { name: "New chat" });
    await send(user, "삼성전자 실적 분석해봐");
    expect(await screen.findByRole("heading", { name: "삼성전자 실적 분석해봐" }, { timeout: 15_000 })).toBeVisible();
    expect(screen.getByRole("button", { name: /^삼성전자 실적 분석해봐$/ })).toBeVisible();
  });
});
