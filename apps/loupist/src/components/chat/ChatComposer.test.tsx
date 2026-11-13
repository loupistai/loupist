import { StrictMode, useState, type ComponentProps } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { AGENT_SKILLS, FIXED_AGENT_SKILLS } from "@/lib/agentSkills";
import { TooltipProvider } from "../ui/tooltip";
import { ChatComposer } from "./ChatComposer";

function RunningComposer({ onFollowUp = vi.fn(), onSend = vi.fn(), isRunning = true, onSearchContext, chips, modelCount = 1, skills = [{ id: "research", name: "Research", description: "Research a topic" }] }: {
  modelCount?: number;
  skills?: ComponentProps<typeof ChatComposer>["skills"];
  onFollowUp?: ComponentProps<typeof ChatComposer>["onFollowUp"];
  onSend?: ComponentProps<typeof ChatComposer>["onSend"];
  isRunning?: boolean;
  onSearchContext?: ComponentProps<typeof ChatComposer>["onSearchContext"];
  chips?: ComponentProps<typeof ChatComposer>["chips"];
}) {
  const [prompt, setPrompt] = useState("");
  const [privateChat, setPrivateChat] = useState(false);
  return <TooltipProvider><ChatComposer
    prompt={prompt} onPromptChange={setPrompt}
    privateChat={privateChat} isRunning={isRunning} error={null} streamAnnouncement=""
    models={Array.from({ length: modelCount }, (_, index) => ({ id: index ? `model-${index}` : "model", provider: "test", model: "model", name: index ? `Model ${index}` : "Model", input: ["text", "image"] as ("text" | "image")[], reasoning: true, context_window: 10000, max_tokens: 1000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "saved" as const }))} providers={[]} modelSelection={{ model_profile_id: "model", thinking_level: "medium", run_options: {} }} modelAuto onModelResetToAuto={() => {}}
    skills={skills}
    plugins={[]}
    onPrivateChatChange={setPrivateChat} onModelSelectionChange={vi.fn()}
    onSend={onSend} onFollowUp={onFollowUp} onAbort={vi.fn()}
    onSearchContext={onSearchContext}
    chips={chips}
  /></TooltipProvider>;
}

describe("ChatComposer input", () => {
  afterEach(async () => { await i18n.changeLanguage("en"); });

  it("folds many Skill modes in the production composer and expands them on request", () => {
    const chips = ["a", "b", "c", "d", "e"].map(id => ({ key: `skill:${id}`, kind: "skill" as const, id, label: `Skill ${id}` }));
    render(<RunningComposer isRunning={false} chips={chips} />);
    const tray = screen.getByRole("list", { name: "Message modes" });
    expect(within(tray).getAllByRole("listitem")).toHaveLength(4);
    fireEvent.click(within(tray).getByRole("button", { name: "+2 more" }));
    expect(within(tray).getAllByRole("listitem")).toHaveLength(5);
  });

  it("focuses the textarea when the noninteractive composer addon is clicked", () => {
    const view = render(<RunningComposer isRunning={false} />);
    fireEvent.click(view.container.querySelector('[data-slot="input-group-addon"]')!);
    expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveFocus();
  });

  it("keeps focus in the model search when clicking its portaled input", async () => {
    const user = userEvent.setup();
    // The composer shows search from eight models.
    render(<RunningComposer isRunning={false} modelCount={8} />);
    await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
    const search = screen.getByRole("searchbox", { name: "Search models" });
    await user.click(search);
    expect(search).toHaveFocus();
    await user.type(search, "Mod");
    await user.click(search);
    expect(search).toHaveFocus();
    expect(search).toHaveValue("Mod");
  });

  it("finishes context search while streaming renders replace the search callback", async () => {
    vi.useFakeTimers();
    try {
      const search = vi.fn().mockResolvedValue([{ id: "file:report", group: "Files", label: "report.md", detail: "Quarterly report", path: "report.md" }]);
      const view = render(<RunningComposer onSearchContext={(query) => search(query, 0)} />);
      fireEvent.change(screen.getByRole("textbox", { name: "Message Loupist" }), { target: { value: "@report" } });
      for (let tick = 1; tick <= 3; tick++) {
        await act(async () => { await vi.advanceTimersByTimeAsync(32); });
        view.rerender(<RunningComposer onSearchContext={(query) => search(query, tick)} />);
      }
      await act(async () => { await vi.advanceTimersByTimeAsync(32); });
      expect(search).toHaveBeenCalledExactlyOnceWith("report", 3);
      expect(screen.getByRole("option", { name: /report.md/ })).toBeVisible();
    } finally {
      vi.useRealTimers();
    }
  });

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

  it("turns private chat on from the + menu, shows its chip and keeps the AI notice visible", async () => {
    const user = userEvent.setup();
    render(<RunningComposer isRunning={false} />);
    expect(screen.queryByRole("button", { name: "Private chat" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Add to this message" }));
    await user.click(await screen.findByRole("menuitemcheckbox", { name: /Private chat/ }));
    const off = await screen.findByRole("button", { name: "Turn off private chat" });
    expect(screen.getByText("Private · memory off")).toBeVisible();
    // Focus goes back to the message, so the "+" tooltip does not reopen over the chip.
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveFocus());
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(screen.getByText("Private · memory off").parentElement).toHaveAccessibleDescription("Private chat: your Loupist doesn't use or add to what it remembers. The chat is still saved, and still goes to your AI provider.");
    await user.click(off);
    expect(screen.queryByText("Private · memory off")).toBeNull();
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

  it("lists bundled Skills by their Korean names in the / menu and adds the Skill by id", async () => {
    await act(async () => { await i18n.changeLanguage("ko"); });
    const user = userEvent.setup();
    const map = [...AGENT_SKILLS, ...FIXED_AGENT_SKILLS].find(skill => skill.id === "world")!;
    render(<RunningComposer isRunning={false} skills={[{ id: map.id, name: map.name, description: map.description }]} />);
    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "/지도");
    const option = screen.getByRole("option", { name: /지도/ });
    expect(option).not.toHaveTextContent(/\bMap\b/);
    await user.keyboard("{Enter}");
    expect(screen.getByText("지도")).toBeVisible();
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

  it("turns Send into Stop and back in the same tile, keeping focus in the textarea", async () => {
    const user = userEvent.setup();
    const view = render(<RunningComposer isRunning={false} />);
    const textarea = screen.getByRole("textbox", { name: "Message Loupist" });
    await user.type(textarea, "Margins");
    const send = screen.getByRole("button", { name: "Send" });
    expect(send).toHaveClass("composer-tile");
    expect(send).toHaveAttribute("data-state", "send");
    view.rerender(<RunningComposer isRunning />);
    const stop = screen.getByRole("button", { name: "Stop response" });
    // One button whose icons cross-fade: no remount, so its box and place never change.
    expect(stop).toBe(send);
    expect(stop).toHaveAttribute("data-state", "stop");
    expect(stop).toHaveAttribute("type", "button");
    expect(stop.querySelectorAll(".composer-tile-icon")).toHaveLength(2);
    await user.click(stop);
    expect(textarea).toHaveFocus();
    view.rerender(<RunningComposer isRunning={false} />);
    expect(screen.getByRole("button", { name: "Send" })).toBe(send);
    expect(send).toHaveAttribute("type", "submit");
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
