import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatThread } from "../../types";
import { chooseLoupist, newChatButton, openApp } from "../renderApp";

type User = ReturnType<typeof userEvent.setup>;
const composer = () => screen.getByRole("textbox", { name: "Message Loupist" });
const send = async (user: User, text: string, button = "Send") => {
  await user.type(composer(), text);
  await user.click(screen.getByRole("button", { name: button }));
};
const gate = () => {
  let open!: () => void;
  const promise = new Promise<void>((resolve) => { open = resolve; });
  return { promise, open };
};

describe("Chat submission ownership", () => {
  it("reconciles a late admission into the shared queue after leaving and returning to a Loupist", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const native = bridge.chatSend.bind(bridge);
    const admission = gate();
    const chatSend = vi.spyOn(bridge, "chatSend").mockImplementationOnce(async (...args) => { await admission.promise; return native(...args); });
    await openApp(bridge);
    await send(user, "First pending request");
    await waitFor(() => expect(chatSend).toHaveBeenCalledOnce());
    await chooseLoupist(user, "Contrarian Value");
    await chooseLoupist(user, "Quality Compounder");
    await user.click(screen.getByRole("button", {name:"How should we read the margin decline?"}));
    await send(user, "Second queued request", "Send next");
    const queue = screen.getByLabelText("Pending chat instructions");
    expect(within(queue).getByText("First pending request")).toBeVisible();
    expect(within(queue).getByText("Second queued request")).toBeVisible();
    await act(async () => admission.open());
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2));
    expect(chatSend.mock.calls.map(([request]) => request.prompt)).toEqual(["First pending request", "Second queued request"]);
    await waitFor(() => expect(screen.queryByLabelText("Pending chat instructions")).toBeNull());
  });

  it("does not steal navigation or consume a draft when Chat creation finishes after a switch", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const nativeCreate = bridge.chatCreate.bind(bridge);
    let finish!: (thread: ChatThread) => void;
    const create = vi.spyOn(bridge, "chatCreate").mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    await user.click(newChatButton());
    await send(user, "Draft before creation");
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    await user.click(screen.getByRole("button", { name: "How should we read the margin decline?" }));
    const created = await nativeCreate({ loupist_id: "loupist-quality" });
    await act(async () => finish(created));
    expect(screen.getByRole("heading", { name: "How should we read the margin decline?" })).toBeVisible();
    expect(chatSend).not.toHaveBeenCalled();
    await user.click(newChatButton());
    expect(composer()).toHaveValue("Draft before creation");
  });

  it("sends the submitted snapshot while keeping newer edits and files during attachment conversion", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    vi.spyOn(URL, "createObjectURL").mockImplementation(() => `blob:${crypto.randomUUID()}`);
    let release!: (response: Response) => void;
    const fetch = vi.spyOn(globalThis, "fetch").mockImplementationOnce(() => new Promise((resolve) => { release = resolve; }));
    try {
      await user.upload(screen.getByLabelText("Upload files"), new File(["first"], "first.txt", { type: "text/plain" }));
      await send(user, "Submitted snapshot");
      await waitFor(() => expect(fetch).toHaveBeenCalledOnce());
      await user.clear(composer());
      await user.type(composer(), "Newer draft while loading");
      await user.upload(screen.getByLabelText("Upload files"), new File(["later"], "later.txt", { type: "text/plain" }));
      await act(async () => release({ ok: true, blob: async () => new Blob(["first"], { type: "text/plain" }) } as Response));
      await waitFor(() => expect(chatSend).toHaveBeenCalledOnce());
      expect(chatSend.mock.calls[0][0]).toMatchObject({ prompt: "Submitted snapshot", attachments: [{ filename: "first.txt" }] });
      expect(composer()).toHaveValue("Newer draft while loading");
      expect(screen.getByRole("button", { name: "Remove later.txt" })).toBeVisible();
      expect(screen.queryByRole("button", { name: "Remove first.txt" })).toBeNull();
    } finally {
      fetch.mockRestore();
    }
  });

  it("sends a queued attachment when its conversion finishes after the preceding response", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const native = bridge.chatSend.bind(bridge);
    const running = gate();
    const chatSend = vi.spyOn(bridge, "chatSend").mockImplementationOnce(async (...args) => { await running.promise; return native(...args); });
    await openApp(bridge);
    await send(user, "First response");
    await waitFor(() => expect(chatSend).toHaveBeenCalledOnce());
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:queued-file");
    let finishFile!: (response: Response) => void;
    const fetch = vi.spyOn(globalThis, "fetch").mockImplementationOnce(() => new Promise((resolve) => { finishFile = resolve; }));
    try {
      await user.upload(screen.getByLabelText("Upload files"), new File(["follow-up"], "follow-up.txt", { type: "text/plain" }));
      await send(user, "Read this next", "Send next");
      await waitFor(() => expect(fetch).toHaveBeenCalledOnce());
      await act(async () => running.open());
      await waitFor(() => expect(screen.queryByRole("button", { name: "Stop response" })).toBeNull(), { timeout: 30_000 });
      await act(async () => finishFile({ ok: true, blob: async () => new Blob(["follow-up"], { type: "text/plain" }) } as Response));
      await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2), { timeout: 30_000 });
      expect(chatSend.mock.calls[1][0]).toMatchObject({ prompt: "Read this next", attachments: [{ filename: "follow-up.txt" }] });
    } finally {
      fetch.mockRestore();
    }
    // A streamed reply and a file conversion both finish here; a loaded suite needs room.
  }, 60_000);

  it("dispatches each Chat's queued follow-up after its own background response", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const native = bridge.chatSend.bind(bridge);
    const [a, b] = [gate(), gate()];
    const chatSend = vi.spyOn(bridge, "chatSend")
      .mockImplementationOnce(async (...args) => { await a.promise; return native(...args); })
      .mockImplementationOnce(async (...args) => { await b.promise; return native(...args); });
    await openApp(bridge);
    await send(user, "A initial");
    await waitFor(() => expect(chatSend).toHaveBeenCalledOnce());
    await send(user, "A follow-up", "Send next");
    await user.click(newChatButton());
    await send(user, "B initial");
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2));
    await act(async () => a.open());
    await waitFor(async () => {
      const threads = (await bridge.loupistSelect("loupist-quality")).threads;
      expect(threads.find((thread) => thread.id === "thread-margin")?.messages.at(-1)?.status).toBe("complete");
    });
    await user.click(screen.getByRole("button", { name: "How should we read the margin decline?" }));
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(3));
    expect(chatSend.mock.calls[2][0].prompt).toBe("A follow-up");
    await act(async () => b.open());
  });

  it("refuses duplicate guidance and reconciles a late steering receipt into its original Chat", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 2_000 });
    const native = bridge.chatSteer.bind(bridge);
    const receipt = gate();
    const steer = vi.spyOn(bridge, "chatSteer").mockImplementationOnce(async (request) => { await receipt.promise; return native(request); });
    try {
      await openApp(bridge);
      await send(user, "Long analysis");
      await screen.findByRole("button", { name: "Stop response" });
      await user.type(composer(), "Guidance for original Chat");
      await user.click(screen.getByRole("button", { name: "Guide this response" }));
      await user.click(screen.getByRole("button", { name: "Guide this response" }));
      expect(steer).toHaveBeenCalledOnce();
      await user.click(newChatButton());
      await user.type(composer(), "Another Chat's unsent draft");
      await act(async () => receipt.open());
      expect(composer()).toHaveValue("Another Chat's unsent draft");
      expect(within(screen.getByLabelText("Conversation")).queryByText("Guidance for original Chat")).toBeNull();
      await user.click(screen.getByRole("button", { name: "How should we read the margin decline?" }));
      expect(composer()).toHaveValue("");
      expect(within(screen.getByLabelText("Conversation")).getAllByText(/Guidance for original Chat/)).toHaveLength(1);
    } finally {
      bridge.setStreamDelay(0);
    }
  });
  it("keeps the composer focused and mounted when the first message creates the research session", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const create = vi.spyOn(bridge, "chatCreate");
    await openApp(bridge);
    await user.click(newChatButton());
    const draft = composer();
    await user.type(draft, "First question in a new session");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    await screen.findByText("First question in a new session", { selector: ".message.user *" });
    expect(composer()).toBe(draft);
    expect(draft).toHaveFocus();
    // Another new session is a fresh composer.
    await user.click(newChatButton());
    expect(composer()).not.toBe(draft);
  });
});
