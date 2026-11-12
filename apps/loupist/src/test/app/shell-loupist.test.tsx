import { headerAction } from "../renderApp";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatThread, LoupistSummary, ModelCatalog } from "../../types";
import { chooseLoupist, mainTab, newChatButton, openApp, selectedLoupistButton } from "../renderApp";

const recovering = (loupist: LoupistSummary): LoupistSummary => ({
  ...loupist,
  availability: { status: "recovery_required", reason: "interrupted_memory_update", action: "recover_memory" },
});

const openModelSettings = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.click(await mainTab(/Settings/));
  await user.click(screen.getByRole("button", { name: "AI accounts" }));
};

/** Settings › Models & providers › Add a provider, then one of its ways to connect. */
const openProviderPicker = async (user: ReturnType<typeof userEvent.setup>, row: RegExp) => {
  await openModelSettings(user);
  await screen.findByRole("article", { name: "ChatGPT" });
  await user.click(screen.getByRole("button", { name: "Add an AI account" }));
  const dialog = await screen.findByRole("dialog", { name: "Add an AI account" });
  await user.click(within(dialog).getByRole("button", { name: row }));
  return dialog;
};

describe("Loupist · Shell and Loupist", () => {
  it("keeps the current Loupist when selection fails and retries the intended Loupist", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const select = vi.spyOn(bridge, "loupistSelect").mockRejectedValueOnce(new Error("The Loupist could not be opened."));
    await chooseLoupist(user, "Cycle Reader");
    const notice = await screen.findByRole("alert", { name: "Loupist selection failed" });
    expect(selectedLoupistButton()).toHaveTextContent("Quality Compounder");
    expect(screen.getByRole("textbox", { name: "Message Loupist" })).toBeVisible();
    await user.click(within(notice).getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(selectedLoupistButton()).toHaveTextContent("Cycle Reader"));
    expect(select.mock.calls[0][0]).toBe(select.mock.calls[1][0]);
  });

  it("changes Loupist from the tree, shows its own Chats and keeps the selected destination", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const create = vi.spyOn(bridge, "chatCreate");
    await user.click(await mainTab(/Map/));
    await chooseLoupist(user, "Contrarian Value");
    expect(await screen.findByRole("button", { name: "Bot", current: "page" })).toBeVisible();
    expect(within(document.querySelector<HTMLElement>(".loupist-navigation")!).getByRole("button", { name: "Downside scenario review" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "How should we read the margin decline?" })).toBeNull();
    await user.click(await mainTab(/Chat/));
    await user.click(newChatButton());
    expect(await screen.findByRole("heading", { name: "New chat" })).toBeVisible();
    expect(create).not.toHaveBeenCalled();
  });

  it("uses desktop shortcuts and leaves IME and open confirmations alone", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const create = vi.spyOn(bridge, "chatCreate");
    fireEvent.keyDown(window, { key: ",", metaKey: true, isComposing: true });
    expect(screen.queryByRole("heading", { name: "Loupists" })).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: ",", ctrlKey: true });
    await screen.findByRole("heading", { name: "Loupists" });
    await user.click(screen.getByRole("button", { name: "Back" }));
    await user.click(screen.getByRole("button", { name: "Options for How should we read the margin decline?" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    fireEvent.keyDown(window, { key: "n", metaKey: true });
    expect(screen.getByRole("dialog", { name: "Delete Chat?" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.keyDown(window, { key: "n", metaKey: true });
    expect(await screen.findByRole("heading", { name: "New chat" })).toBeVisible();
    expect(create).not.toHaveBeenCalled();
  });

  it("collapses and restores the sidebar from the titlebar toggle", async () => {
    const user = userEvent.setup();
    await openApp();
    const toggle = screen.getByRole("button", { name: "Show or hide sidebar" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
  });

  it("creates, renames and deletes Chats from the Loupist list", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const create = vi.spyOn(bridge, "chatCreate");
    const rename = vi.spyOn(bridge, "chatRename");
    const remove = vi.spyOn(bridge, "chatDelete");
    await chooseLoupist(user, "Contrarian Value");
    await user.click(newChatButton());
    await screen.findByRole("heading", { name: "New chat" });
    expect(create).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole("textbox", { name: "Message Loupist" }), { target: { value: "Downside follow-up" } });
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(create).toHaveBeenCalledWith({ loupist_id: "loupist-value" }));
    // The list takes the title when the whole mock reply has streamed; under the gate's load that can take ~25 s.
    await user.click(await screen.findByRole("button", { name: "Options for Downside follow-up" }, { timeout: 45_000 }));
    await user.click(screen.getByRole("menuitem", { name: "Rename" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "Renamed follow-up" } });
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(rename).toHaveBeenCalledWith({ loupist_id: "loupist-value", thread_id: expect.any(String), title: "Renamed follow-up" }));
    await user.click(screen.getByRole("button", { name: "Options for Renamed follow-up" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(remove).toHaveBeenCalledWith({ loupist_id: "loupist-value", thread_id: expect.any(String) }));
    expect(screen.queryByRole("button", { name: "Renamed follow-up" })).not.toBeInTheDocument();
  });

  it("keeps a failed rename visible, ignores IME Enter and retries", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const rename = vi.spyOn(bridge, "chatRename").mockRejectedValueOnce(new Error("Session is busy. Try again."));
    await user.click(screen.getByRole("button", { name: "Options for How should we read the margin decline?" }));
    await user.click(screen.getByRole("menuitem", { name: "Rename" }));
    const dialog = screen.getByRole("dialog", { name: "Rename Chat" });
    const name = within(dialog).getByRole("textbox", { name: "Name" });
    await user.clear(name);
    await user.type(name, "분기 검토");
    fireEvent.keyDown(name, { key: "Enter", isComposing: true, keyCode: 229 });
    expect(rename).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("Session is busy. Try again.");
    expect(name).toHaveValue("분기 검토");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Rename Chat" })).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "분기 검토" })).toBeVisible();
  });

  it("shows a deletion failure inside its confirmation and clears it on reopening", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    vi.spyOn(bridge, "chatDelete").mockRejectedValueOnce(new Error("Could not remove this session."));
    const openDelete = async () => {
      await user.click(screen.getByRole("button", { name: "Options for How should we read the margin decline?" }));
      await user.click(screen.getByRole("menuitem", { name: "Delete" }));
      return screen.getByRole("dialog", { name: "Delete Chat?" });
    };
    const dialog = await openDelete();
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("Could not remove this session.");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(within(await openDelete()).queryByRole("alert")).not.toBeInTheDocument();
  });

  it("ignores a late Chat created for the previously selected Loupist", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    let resolveCreate!: (thread: ChatThread) => void;
    vi.spyOn(bridge, "chatCreate").mockImplementation(() => new Promise<ChatThread>((resolve) => { resolveCreate = resolve; }));
    await user.click(newChatButton());
    await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Late chat from the previous Loupist");
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(bridge.chatCreate).toHaveBeenCalled());
    await chooseLoupist(user, "Contrarian Value");
    await screen.findByRole("heading", { name: "Bot" });
    await act(async () => resolveCreate({
      id: "thread-from-previous-loupist", loupist_id: "loupist-quality", title: "Late chat from the previous Loupist",
      updated_at: "2026-08-08T00:00:00Z", use_memory: true, update_memory: true, messages: [],
    }));
    expect(screen.queryByRole("button", { name: "Late chat from the previous Loupist" })).toBeNull();
    expect(screen.getByRole("heading", { name: "Bot" })).toBeVisible();
  });

  it("creates a Loupist from the sidebar and opens its empty Chat list", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const createLoupist = vi.spyOn(bridge, "loupistCreate");
    await user.click(headerAction("New Loupist"));
    await user.click(await screen.findByRole("button", { name: "Create Loupist" }));
    await waitFor(() => expect(createLoupist).toHaveBeenCalledWith({ name: "Portfolio Loupist", focus: "Companies I hold and watch", avatar: "loupist-01", template: "portfolio", tone: "balanced" }));
    expect(selectedLoupistButton()).toHaveTextContent("Portfolio Loupist");
    expect(screen.getByRole("button", {name:"Bot",current:"page"})).toBeVisible();
  });

  it("does not offer a duplicate create retry after the Loupist was persisted", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const createLoupist = vi.spyOn(bridge, "loupistCreate");
    vi.spyOn(bridge, "loupistList").mockResolvedValueOnce([]).mockRejectedValueOnce(new Error("Refresh failed."));
    render(<App bridge={bridge} />);
    await user.click(await screen.findByRole("checkbox", { name: "I understand" }));
    await user.click(await screen.findByRole("button", { name: "Get started" }));
    await user.click(await screen.findByRole("button", { name: "Create Loupist" }));

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "New Loupist" })).not.toBeInTheDocument());
    expect(createLoupist).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("alert")).toHaveTextContent("The agent was created, but the agent list could not be refreshed.");
  });

  it("removes a Loupist registration from Loupist settings", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const remove = vi.spyOn(bridge, "loupistDelete");
    await user.click(await mainTab(/Loupist settings/));
    expect(screen.getByLabelText("Workspace folder")).toHaveTextContent("/Users/research/Quality Compounder");
    await user.click(screen.getByRole("button", { name: "Remove Loupist" }));
    await user.click(within(screen.getByRole("dialog", { name: "Remove Loupist?" })).getByRole("button", { name: "Remove Loupist" }));
    await waitFor(() => expect(remove).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(selectedLoupistButton()).toHaveTextContent("Contrarian Value"));
  });

  it("recovers an interrupted Memory update while keeping Loupist creation available", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const [firstLoupist] = await bridge.loupistList();
    vi.spyOn(bridge, "loupistList").mockResolvedValueOnce([recovering(firstLoupist)]);
    const recover = vi.spyOn(bridge, "loupistRecover");
    const skills = vi.spyOn(bridge, "agentSkillCatalog");
    render(<App bridge={bridge} />);
    expect(await screen.findByRole("heading", { name: "Memory needs recovery" })).toBeVisible();
    expect(headerAction("New Loupist")).toBeVisible();
    await user.keyboard("{Escape}");
    expect(skills).not.toHaveBeenCalledWith(firstLoupist.id);
    await user.click(screen.getByRole("button", { name: "Recover memory" }));
    await waitFor(() => expect(recover).toHaveBeenCalledWith({ loupist_id: firstLoupist.id, action: "recover_memory" }));
    expect(await screen.findByRole("heading", { name: "Bot" })).toBeVisible();
  });

  it("does not select a recovered Loupist after the user moves to another Loupist", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const listed = await bridge.loupistList();
    const loupist = recovering(listed[0]);
    vi.spyOn(bridge, "loupistList").mockResolvedValueOnce([loupist, ...listed.slice(1)]);
    let finishRecovery: ((loupist: LoupistSummary) => void) | undefined;
    vi.spyOn(bridge, "loupistRecover").mockImplementation(() => new Promise((resolve) => { finishRecovery = resolve; }));
    render(<App bridge={bridge} />);
    await screen.findByRole("heading", { name: "Bot" });
    await chooseLoupist(user, loupist.name);
    await user.click(await screen.findByRole("button", { name: "Recover memory" }));
    await chooseLoupist(user, "Cycle Reader");
    await act(async () => { finishRecovery?.({ ...loupist, availability: { status: "available" } }); });
    await waitFor(() => expect(selectedLoupistButton()).toHaveTextContent("Cycle Reader"));
  });

  it("persists Light, Dark and System appearance preferences", async () => {
    const user = userEvent.setup();
    const listeners = new Set<(event: MediaQueryListEvent) => void>();
    let prefersDark = false;
    const originalMatchMedia = window.matchMedia;
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        get matches() { return prefersDark; },
        media: query,
        addEventListener: (_type: string, listener: (event: MediaQueryListEvent) => void) => listeners.add(listener),
        removeEventListener: (_type: string, listener: (event: MediaQueryListEvent) => void) => listeners.delete(listener),
      })),
    });
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const theme = async () => (await bridge.configRead({ scope: { kind: "global" } })).explicit?.appearance?.theme;
    const view = render(<App bridge={bridge} />);
    try {
      await screen.findByRole("heading", { name: "Bot" });
      await user.click(await mainTab(/Settings/));
      await user.click(screen.getByRole("button", { name: "Appearance" }));
      await user.click(screen.getByRole("button", { name: "Dark" }));
      expect(document.documentElement).toHaveAttribute("data-theme", "dark");
      expect(await theme()).toBe("dark");
      await user.click(screen.getByRole("button", { name: "Light" }));
      expect(document.documentElement).toHaveAttribute("data-theme", "light");
      expect(await theme()).toBe("light");
      prefersDark = true;
      await act(async () => listeners.forEach((listener) => listener({ matches: true } as MediaQueryListEvent)));
      await user.click(screen.getByRole("button", { name: /System Follow your device’s appearance/ }));
      expect(document.documentElement).toHaveAttribute("data-theme", "dark");
      expect(await theme()).toBe("system");
    } finally {
      view.unmount();
      Object.defineProperty(window, "matchMedia", { configurable: true, value: originalMatchMedia });
      delete document.documentElement.dataset.theme;
      document.documentElement.style.removeProperty("color-scheme");
    }
  });
});

describe("Loupist · Model providers", () => {
  it("reconnects an account from Settings while keeping the existing Loupist readable", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    for (const provider of (await bridge.modelCatalogGet()).providers) await bridge.providerDisconnect(provider.id);
    const create = vi.spyOn(bridge, "loupistCreate");
    render(<App bridge={bridge} />);
    expect(await screen.findByRole("heading", { name: "Bot" })).toBeVisible();
    expect(selectedLoupistButton()).toHaveTextContent("Quality Compounder");
    await user.click(screen.getByRole("button", { name: "Connect an AI account" }));
    await user.click(await screen.findByRole("button", { name: "Add an AI account" }));
    const dialog = await screen.findByRole("dialog", { name: "Add an AI account" });
    await user.click(within(dialog).getByRole("button", { name: /Continue with ChatGPT/u }));
    await user.click(await mainTab(/Chat/));
    expect(screen.getByRole("heading", { name: "Bot" })).toBeVisible();
    expect(create).not.toHaveBeenCalled();
  });

  it("saves a write-only API key once and discovers its models", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    let finish: (() => void) | undefined;
    const configure = bridge.providerConfigure.bind(bridge);
    const spy = vi.spyOn(bridge, "providerConfigure").mockImplementation((request) => new Promise<ModelCatalog>((resolve) => {
      finish = () => resolve(configure(request));
    }));
    const dialog = await openProviderPicker(user, /^Anthropic/u);
    await user.type(within(dialog).getByLabelText("API key"), "write-only-secret");
    await user.dblClick(within(dialog).getByRole("button", { name: "Connect" }));
    expect(spy).toHaveBeenCalledExactlyOnceWith({ provider: "anthropic", api_key: "write-only-secret" });
    await act(async () => finish?.());
    const card = await screen.findByRole("article", { name: "Anthropic" });
    expect(card).toHaveTextContent("API key saved on this device");
    await user.click(within(card).getByRole("button", { name: /Models \(1\)/u }));
    expect(await within(card).findByRole("switch", { name: "Show Claude Sonnet 4.5 in Chat" })).toBeChecked();
    expect(screen.queryByDisplayValue("write-only-secret")).toBeNull();
  });

  it("signs in before showing that provider's models", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const connect = vi.spyOn(bridge, "providerConnect");
    await openProviderPicker(user, /^Sign in with SuperGrok/u);
    await waitFor(() => expect(connect).toHaveBeenCalledWith("xai", expect.any(Function)));
    const card = await screen.findByRole("article", { name: "SuperGrok" });
    expect(card).toHaveTextContent(/Signed in with SuperGrok · since/u);
    await user.click(within(card).getByRole("button", { name: /Models \(1\)/u }));
    expect(await within(card).findByRole("switch", { name: "Show xai default in Chat" })).toBeChecked();
  });

  it("cancels an abandoned browser sign-in and allows a clean retry", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    let rejectConnect: ((reason: { code: string; message: string }) => void) | undefined;
    vi.spyOn(bridge, "providerConnect").mockImplementation((_provider, observer) => {
      observer({ type: "opening_browser", message: "Continue in your browser to finish signing in.", opened: true, url: null });
      return new Promise((_resolve, reject) => { rejectConnect = reject; });
    });
    const cancel = vi.spyOn(bridge, "providerConnectCancel").mockImplementation(async () => {
      rejectConnect?.({ code: "cancelled", message: "Provider sign-in was cancelled." });
    });
    const openBrowser = vi.spyOn(bridge, "providerConnectOpenBrowser").mockResolvedValue();
    const dialog = await openProviderPicker(user, /^Continue with ChatGPT/u);
    await user.click(await within(dialog).findByRole("button", { name: "Open the sign-in page again" }));
    await waitFor(() => expect(openBrowser).toHaveBeenCalledTimes(1));
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(cancel).toHaveBeenCalledTimes(1));
    expect(await within(dialog).findByRole("button", { name: "Continue with ChatGPT" })).toBeEnabled();
    expect(within(dialog).queryByRole("alert")).toBeNull();
  });

  it("shows the backend provider error instead of a generic fallback", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    vi.spyOn(bridge, "providerConnect").mockRejectedValue({ code: "internal", message: "OpenAI sign-in could not open the trusted authorization page" });
    const dialog = await openProviderPicker(user, /^Continue with ChatGPT/u);
    expect(await within(dialog).findByText("OpenAI sign-in could not open the trusted authorization page")).toBeVisible();
  });

  it("disconnects a saved provider credential", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const disconnect = vi.spyOn(bridge, "providerDisconnect");
    await openModelSettings(user);
    const connected = await screen.findByRole("list", { name: "Connected providers" });
    expect(within(connected).getByRole("heading", { name: "ChatGPT" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "More actions for ChatGPT" }));
    await user.click(await screen.findByRole("menuitem", { name: "Disconnect ChatGPT" }));
    await user.click(within(await screen.findByRole("dialog", { name: "Disconnect ChatGPT?" })).getByRole("button", { name: "Disconnect" }));
    await waitFor(() => expect(disconnect).toHaveBeenCalledWith("openai"));
    await waitFor(() => expect(screen.queryByRole("button", { name: "More actions for ChatGPT" })).not.toBeInTheDocument());
  });

  it("hides unchecked models and blocks Chat when no visible model remains", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const updateVisibility = vi.spyOn(bridge, "modelVisibilityUpdate");
    await bridge.providerConfigure({ provider: "google", api_key: "fixture-key" });
    await openModelSettings(user);
    const chatgpt = await screen.findByRole("article", { name: "ChatGPT" });
    await user.click(within(chatgpt).getByRole("button", { name: /Models \(1\)/u }));
    await user.click(await within(chatgpt).findByRole("switch", { name: "Show GPT-5.6 Luna in Chat" }));
    await waitFor(() => expect(updateVisibility).toHaveBeenCalledWith({ model_profile_id: "model-test", visible_in_chat: false }));
    await user.click(screen.getByRole("button", { name: "Back" }));
    await user.hover(await screen.findByRole("button", { name: /Model settings for this message/ }));
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Uses your default model settings.");
    await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
    expect(screen.getByRole("menuitemradio", { name: "google default" })).toBeVisible();
    expect(screen.queryByRole("menuitemradio", { name: "GPT-5.6 Luna" })).not.toBeInTheDocument();
    await user.keyboard("{Escape}");

    await openModelSettings(user);
    const gemini = await screen.findByRole("article", { name: "Google Gemini" });
    await user.click(within(gemini).getByRole("button", { name: /Models \(1\)/u }));
    await user.click(await within(gemini).findByRole("switch", { name: "Show google default in Chat" }));
    await waitFor(() => expect(updateVisibility).toHaveBeenCalledWith({ model_profile_id: "google/google-default", visible_in_chat: false }));
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(await screen.findByText(/Show or load a model in Settings to send messages/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
    expect(screen.getByText("No connected models")).toBeVisible();
    expect(screen.queryByRole("menuitemradio", { name: "GPT-5.6 Luna" })).not.toBeInTheDocument();
  });
});

describe("Loupist · Loupist tools", () => {
  it("configures Skills and connections for the selected Loupist", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const update = vi.spyOn(bridge, "configUpdate");
    const disable = vi.spyOn(bridge, "loupistCapabilityDisable");
    await user.click(await mainTab(/Loupist settings/));
    await user.click(await screen.findByRole("switch", { name: "Valuation: enabled" }));
    await waitFor(() => expect(update).toHaveBeenCalledWith(expect.objectContaining({
      scope: { kind: "project", loupistId: "loupist-quality" }, set: { skills: { valuation: { enabled: false } } },
    })));
    await user.click(screen.getByRole("switch", { name: "World Bank Indicators: enabled" }));
    await waitFor(() => expect(disable).toHaveBeenCalledWith({ loupist_id: "loupist-quality", entry_id: "world-bank.indicators" }));
  });

  it("renders the authoritative binding returned by native", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    await user.click(await mainTab(/Loupist settings/));
    await user.click(await screen.findByRole("switch", { name: "World Bank Indicators: enabled" }));
    const enable = vi.spyOn(bridge, "loupistCapabilityEnable").mockResolvedValue({
      entry_id: "world-bank.indicators", enabled: false, granted_permissions: [], available: true,
    });
    await user.click(screen.getByRole("switch", { name: "World Bank Indicators: disabled" }));
    await waitFor(() => expect(enable).toHaveBeenCalledWith({ loupist_id: "loupist-quality", entry_id: "world-bank.indicators" }));
    expect(screen.getByRole("switch", { name: "World Bank Indicators: disabled" })).not.toBeChecked();
  });

  it("does not present an unavailable connection as enabled", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const bindings = await bridge.loupistCapabilityList("loupist-quality");
    vi.spyOn(bridge, "loupistCapabilityList").mockResolvedValue(bindings.map((binding) =>
      binding.entry_id === "koreainvestment.market-data" ? { ...binding, enabled: true, available: false } : binding));
    await user.click(await mainTab(/Loupist settings/));
    expect(await screen.findByRole("switch", { name: "Korea Investment Open Trading API: disabled" })).toBeDisabled();
  });
});
