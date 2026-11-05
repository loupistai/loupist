import { StrictMode, useState } from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { TooltipProvider } from "../ui/tooltip";
import { ChatComposer } from "./ChatComposer";

function RunningComposer({ onFollowUp = vi.fn(), onSend = vi.fn(), isRunning = true }) {
  const [prompt, setPrompt] = useState("");
  const [privateChat, setPrivateChat] = useState(false);
  return <TooltipProvider><ChatComposer
    prompt={prompt} onPromptChange={setPrompt}
    privateChat={privateChat} isRunning={isRunning} error={null} streamAnnouncement=""
    models={[{ id: "model", provider: "test", model: "model", name: "Model", input: ["text", "image"], reasoning: true, context_window: 10000, max_tokens: 1000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "saved" }]} providers={[]} modelSelection={{ model_profile_id: "model", thinking_level: "medium", run_options: {} }} modelAuto onModelResetToAuto={() => {}}
    skills={[{ id: "research", name: "Research", description: "Research a topic" }]}
    plugins={[]}
    onPrivateChatChange={setPrivateChat} onModelSelectionChange={vi.fn()}
    onSend={onSend} onFollowUp={onFollowUp} onAbort={vi.fn()}
  /></TooltipProvider>;
}

describe("ChatComposer input", () => {
  afterEach(async () => { await i18n.changeLanguage("en"); });

  it("sends dollar amounts and ticker-shaped text unchanged without opening shortcuts", async () => {
    const user = userEvent.setup();
    const onSend = vi.fn();
    render(<RunningComposer isRunning={false} onSend={onSend} />);
    const text = "Compare $AAPL:NASDAQ (Apple), $NVDA and a $5 fee.";
    await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), text);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Added to this message" })).not.toBeInTheDocument();
    await user.keyboard("{Enter}");
    expect(onSend).toHaveBeenCalledWith(text, [], text);
  });

  it.each([
    ["/brief", "Give me a quick briefing on "],
    ["/compare", "Compare "],
  ])("starts %s as editable prose", async (command, starter) => {
    const user = userEvent.setup();
    const onSend = vi.fn();
    render(<RunningComposer isRunning={false} onSend={onSend} />);
    const textarea = screen.getByRole("textbox", { name: "Message Loupist" });
    await user.type(textarea, command);
    await user.click(screen.getByRole("option", { name: command }));
    expect(textarea).toHaveValue(starter);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(onSend).not.toHaveBeenCalled();
  });

  it("keeps the empty composer short and opens each typed shortcut menu", async () => {
    const user = userEvent.setup();
    render(<div style={{ width: 280 }}><RunningComposer isRunning={false} /></div>);
    const textarea = screen.getByRole("textbox", { name: "Message Loupist" });
    expect(textarea).toHaveAttribute("placeholder", "Ask Loupist…");

    await user.type(textarea, "@");
    const context = screen.getByRole("listbox", { name: "Prompt shortcuts" });
    expect(context).toHaveAttribute("data-symbol", "@");
    expect(context).toHaveTextContent("Context");
    expect(context).toHaveTextContent("Files, memory, and connections");
    await user.clear(textarea);

    await user.type(textarea, "$");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    await user.clear(textarea);

    await user.type(textarea, "/");
    const commands = screen.getByRole("listbox", { name: "Prompt shortcuts" });
    expect(commands).toHaveAttribute("data-symbol", "/");
    expect(commands).toHaveTextContent("Commands");
    expect(screen.getByRole("option", { name: /\/brief/ })).toBeVisible();
    expect(textarea).toHaveAccessibleName("Message Loupist");
  });

  it("localizes the short composer prompt in Korean", async () => {
    render(<RunningComposer isRunning={false} />);
    await act(async () => { await i18n.changeLanguage("ko"); });
    const textarea = screen.getByRole("textbox", { name: "루피스트에게 메시지" });
    expect(textarea).toHaveAttribute("placeholder", "Loupist에게 질문…");
  });

  it("toggles private chat from the keyboard and keeps the AI notice visible", async () => {
    const user = userEvent.setup();
    render(<RunningComposer isRunning={false} />);
    const toggle = screen.getByRole("button", { name: "Private chat: don't remember this chat" });
    toggle.focus();
    await user.keyboard(" ");
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("Private — not saved to memory")).toBeVisible();
    expect(screen.getByText("AI can be wrong. Not a licensed financial adviser.")).toBeVisible();
  });

  it("rejects oversized images when attached while allowing larger documents", async () => {
    const user = userEvent.setup();
    const onSend = vi.fn();
    render(<RunningComposer isRunning={false} onSend={onSend} />);
    const upload = screen.getByLabelText("Upload files");
    await user.upload(upload, new File([new Uint8Array(6 * 1024 * 1024)], "large-image.png", { type: "image/png" }));
    expect(screen.getByRole("alert")).toHaveTextContent("5 MB");
    expect(screen.queryByText("large-image.png")).not.toBeInTheDocument();
    expect(onSend).not.toHaveBeenCalled();
    await user.upload(upload, new File([new Uint8Array(26 * 1024 * 1024)], "too-large.pdf", { type: "application/pdf" }));
    expect(screen.getByRole("alert")).toHaveTextContent("25 MB");
    expect(screen.queryByText("too-large.pdf")).not.toBeInTheDocument();
    await user.upload(upload, new File([new Uint8Array(6 * 1024 * 1024)], "research.pdf", { type: "application/pdf" }));
    expect(screen.getByText("research.pdf")).toBeVisible();
    // The tray shows the file's real size beside its name.
    expect(screen.getByText("6.0 MB")).toBeVisible();
    expect(screen.getByText("research.pdf").closest(".composer-file-chip")).toHaveAttribute("title", "research.pdf · 6.0 MB");
    await user.click(screen.getByRole("button", { name: "Send" }));
    expect(onSend).toHaveBeenCalledWith("", [expect.objectContaining({ filename: "research.pdf", mediaType: "application/pdf" })], "");
  });
  it("selects a skill before Enter queues during an active response", async () => {
    const user = userEvent.setup();
    const onFollowUp = vi.fn();
    const onSend = vi.fn();
    render(<RunningComposer onFollowUp={onFollowUp} onSend={onSend} />);
    const textarea = screen.getByRole("textbox", { name: "Message Loupist" });
    await user.type(textarea, "/res");
    const option = screen.getByRole("option", { name: /Research/ });
    expect(textarea).toHaveAttribute("aria-activedescendant", option.id);
    await user.keyboard("{Enter}");
    expect(screen.getByText("Research")).toBeVisible();
    expect(textarea).not.toHaveValue("/research ");
    expect(onFollowUp).not.toHaveBeenCalled();
    expect(onSend).not.toHaveBeenCalled();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    await user.keyboard("{Enter}");
    expect(onFollowUp).toHaveBeenCalledOnce();
  });

  it("dismisses shortcuts without bubbling Escape and ignores IME Enter", async () => {
    const user = userEvent.setup();
    const onFollowUp = vi.fn();
    const escaped = vi.fn();
    window.addEventListener("keydown", escaped);
    render(<RunningComposer onFollowUp={onFollowUp} />);
    const textarea = screen.getByRole("textbox", { name: "Message Loupist" });
    await user.type(textarea, "/res");
    escaped.mockClear();
    await user.keyboard("{Escape}");
    expect(escaped).not.toHaveBeenCalled();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    fireEvent.keyDown(textarea, { key: "Enter", keyCode: 229, isComposing: true });
    expect(onFollowUp).not.toHaveBeenCalled();
    window.removeEventListener("keydown", escaped);
  });

  it("shows only stop until there is text, then queues on Enter and guides on the modifier", async () => {
    const user = userEvent.setup();
    const onFollowUp = vi.fn();
    const onSend = vi.fn();
    render(<RunningComposer onFollowUp={onFollowUp} onSend={onSend} />);
    expect(screen.getByRole("button", { name: "Stop response" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Send next" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Guide this response" })).toBeNull();
    const textarea = screen.getByRole("textbox", { name: "Message Loupist" });
    expect(textarea).toHaveAttribute("aria-keyshortcuts", "Enter Shift+Enter Control+Enter Meta+Enter");
    await user.type(textarea, "Hold this");
    expect(screen.getByRole("button", { name: "Send next" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Guide this response" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Stop response" })).toBeVisible();
    expect(screen.getByText(/Enter queues this for after the reply/)).toBeVisible();
    expect(screen.getByText(/Shift\+Enter starts a new line/)).toBeVisible();
    const sendNext = screen.getByRole("button", { name: "Send next" });
    const guide = screen.getByRole("button", { name: "Guide this response" });
    // Focus opens each tooltip on its own. Pointer grace between these adjacent
    // buttons keeps the previous tooltip in jsdom, where every box is at 0,0.
    act(() => { sendNext.focus(); });
    expect(await screen.findByRole("tooltip", { name: /Queue this for after the reply/ })).toBeVisible();
    act(() => { guide.focus(); });
    await waitFor(() => {
      expect(screen.getByRole("tooltip", { name: /Add it to the reply in progress/ })).toBeVisible();
      expect(screen.queryByRole("tooltip", { name: /Queue this for after the reply/ })).toBeNull();
    });
    act(() => { textarea.focus(); });
    await user.keyboard("{Shift>}{Enter}{/Shift}");
    expect(onFollowUp).not.toHaveBeenCalled();
    expect(textarea).toHaveValue("Hold this\n");
    await user.keyboard("{Enter}");
    expect(onFollowUp).toHaveBeenCalledOnce();
    expect(onSend).not.toHaveBeenCalled();
  });

  it("names the Windows guide shortcut and keeps a rejected steer in the draft", async () => {
    const user = userEvent.setup();
    const platform = vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    let resolveSend!: (sent: boolean) => void;
    const onSend = vi.fn(() => new Promise<boolean>((resolve) => { resolveSend = resolve; }));
    try {
      render(<RunningComposer onSend={onSend} />);
      const textarea = screen.getByRole("textbox", { name: "Message Loupist" });
      await user.type(textarea, "Use the annual filing");
      expect(screen.getByText(/Ctrl\+Enter adds it to the reply in progress/)).toBeVisible();
      await user.keyboard("{Control>}{Enter}{/Control}");
      expect(await screen.findByText(/Waiting to reach this reply/)).toBeVisible();
      expect(textarea).toHaveValue("Use the annual filing");
      await user.keyboard("{Control>}{Enter}{/Control}");
      expect(onSend).toHaveBeenCalledOnce();
      await act(async () => { resolveSend(false); });
      expect(screen.queryByText(/Waiting to reach this reply/)).toBeNull();
      expect(textarea).toHaveValue("Use the annual filing");
    } finally {
      platform.mockRestore();
    }
  });

  it("shows guidance as waiting until the reply accepts it", async () => {
    const user = userEvent.setup();
    let resolveSend!: (sent: boolean) => void;
    const onSend = vi.fn(() => new Promise<boolean>((resolve) => { resolveSend = resolve; }));
    render(<RunningComposer onSend={onSend} />);
    const textarea = screen.getByRole("textbox", { name: "Message Loupist" });
    await user.type(textarea, "Focus on cash");
    await user.click(screen.getByRole("button", { name: "Guide this response" }));
    expect(await screen.findByText(/Waiting to reach this reply: “Focus on cash”/)).toBeVisible();
    expect(textarea).toHaveValue("Focus on cash");
    await act(async () => { resolveSend(true); });
    expect(await screen.findByText(/Guiding this reply: “Focus on cash”/)).toBeVisible();
    expect(screen.queryByText(/Waiting to reach this reply/)).toBeNull();
  });

  it("queues dropped files for the next response while the current response runs", async () => {
    const user = userEvent.setup();
    const onFollowUp = vi.fn();
    const onSend = vi.fn();
    render(<RunningComposer onFollowUp={onFollowUp} onSend={onSend} />);
    expect(screen.getByRole("button", { name: "Add to this message" })).toBeEnabled();
    fireEvent.drop(screen.getByRole("textbox", { name: "Message Loupist" }), {
      dataTransfer: { types: ["Files"], files: [new File(["notes"], "notes.txt", { type: "text/plain" })] },
    });
    expect(screen.getByText("notes.txt")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Send next" }));
    expect(onFollowUp).toHaveBeenCalledWith("", [expect.objectContaining({ filename: "notes.txt" })], "");
    expect(onSend).not.toHaveBeenCalled();
  });
});


describe("Chat attachment submission recovery", () => {
  afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

  it("creates and releases each attachment URL once in StrictMode", async () => {
    const user = userEvent.setup();
    const create = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:one-attachment");
    const revoke = vi.spyOn(URL, "revokeObjectURL");
    const view = render(<StrictMode><RunningComposer isRunning={false} /></StrictMode>);
    create.mockClear();
    revoke.mockClear();
    await user.upload(screen.getByLabelText("Upload files"), new File(["notes"], "notes.txt", { type: "text/plain" }));
    expect(create).toHaveBeenCalledOnce();
    await user.click(screen.getByRole("button", { name: "Remove notes.txt" }));
    expect(revoke).toHaveBeenCalledExactlyOnceWith("blob:one-attachment");
    view.unmount();
    expect(revoke).toHaveBeenCalledOnce();
  });

  it("keeps the draft and file when reading an attachment fails", async () => {
    const user = userEvent.setup();
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:attachment");
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("Blob unavailable")));
    const onSend = vi.fn();
    render(<RunningComposer isRunning={false} onSend={onSend} />);
    await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Keep this research draft");
    await user.upload(screen.getByLabelText("Upload files"), new File(["notes"], "notes.txt", { type: "text/plain" }));
    await user.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Your draft and files have been kept");
    expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveValue("Keep this research draft");
    expect(screen.getByText("notes.txt")).toBeVisible();
    expect(onSend).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  });

  it("submits a converted snapshot once and keeps files added during submission", async () => {
    const user = userEvent.setup();
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:attachment");
    let resolveFetch!: (value: unknown) => void;
    const fetchFile = vi.fn(() => new Promise((resolve) => { resolveFetch = resolve; }));
    vi.stubGlobal("fetch", fetchFile);
    let resolveSend!: () => void;
    const onSend = vi.fn(() => new Promise<void>((resolve) => { resolveSend = resolve; }));
    render(<RunningComposer isRunning={false} onSend={onSend} />);
    const prompt = screen.getByRole("textbox", { name: "Message Loupist" });
    await user.type(prompt, "Read the file");
    await user.upload(screen.getByLabelText("Upload files"), new File(["notes"], "notes.txt", { type: "text/plain" }));
    await user.click(screen.getByRole("button", { name: "Send" }));
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    await user.type(prompt, " and a new thought");
    fireEvent.submit(prompt.closest("form")!);
    expect(fetchFile).toHaveBeenCalledOnce();
    await act(async () => resolveFetch({ ok: true, blob: async () => new Blob(["notes"], { type: "text/plain" }) }));
    await waitFor(() => expect(onSend).toHaveBeenCalledOnce());
    expect(onSend).toHaveBeenCalledWith("Read the file", [expect.objectContaining({ filename: "notes.txt", url: expect.stringMatching(/^data:text\/plain;base64,/) })], "Read the file");
    await user.upload(screen.getByLabelText("Upload files"), new File(["next"], "next.txt", { type: "text/plain" }));
    await act(async () => resolveSend());
    await waitFor(() => expect(screen.queryByText("notes.txt")).not.toBeInTheDocument());
    expect(screen.getByText("next.txt")).toBeVisible();
    expect(prompt).toHaveValue("Read the file and a new thought");
    expect(fetchFile).toHaveBeenCalledOnce();
  });
});
