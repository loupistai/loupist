import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { mockModelSelection, mockProviderStatus } from "../../bridge/mock/models";
import type { ConfigScope, ConfigUpdateRequest } from "../../types";
import { chooseLoupist, headerAction, mainTab, newChatButton, openApp, selectedLoupistButton, openChat, togglePrivateChat } from "../renderApp";

const updateGlobal = async (bridge: MockLoupistBridge, set: ConfigUpdateRequest["set"]) => {
  const global = await bridge.configRead({ scope: { kind: "global" } });
  await bridge.configUpdate({ scope: global.scope, expectedRevision: global.revision, set, unset: [] });
};
const applyPending = { code: "config_apply_pending", message: "Configuration was saved, but connection reconciliation is blocked." };
const composerValue = () => (screen.getByRole("textbox", { name: "Message Loupist" }) as HTMLTextAreaElement).value;
const draftSkill = async (user: ReturnType<typeof userEvent.setup>, loupistId: string, name: string) => {
  await chooseLoupist(user, loupistId === "loupist-quality" ? "Quality Compounder" : "Contrarian Value");
  await user.click(await mainTab(/Loupist settings/));
  await user.click(screen.getByRole("button", { name: "Manage skills" }));
  await screen.findByRole("switch", { name: /Skill Creator/ });
  await user.click(screen.getByRole("button", { name: "Create skill" }));
  const dialog = screen.getByRole("dialog", { name: "Create a skill" });
  await user.type(within(dialog).getByLabelText("Name"), name);
  await user.click(within(dialog).getByRole("button", { name: "Draft with Loupist" }));
  await waitFor(() => expect(composerValue()).toContain(name));
};

it("refreshes the Chat Skill picker after global configuration changes on window activation", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await openApp(bridge);
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "/valuation");
  expect(await screen.findByRole("option", { name: /Valuation/ })).toBeVisible();
  await updateGlobal(bridge, { skills: { valuation: { enabled: false } } });
  act(() => window.dispatchEvent(new Event("focus")));
  await waitFor(() => expect(screen.queryByRole("option", { name: /Valuation/ })).not.toBeInTheDocument());
});

it.each([false, true])("applies current Memory defaults to a new Chat unless the composer choice is explicit (%s)", async (explicit) => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const chatSend = vi.spyOn(bridge, "chatSend");
  await openApp(bridge);
  await user.click(newChatButton());
  await screen.findByRole("heading", { name: "Untitled research session" });
  if (explicit) {
    await togglePrivateChat();
    await togglePrivateChat();
  }
  await updateGlobal(bridge, { chat: { use_memory: false, update_memory: false } });
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Use my current Chat preferences");
  await user.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(chatSend).toHaveBeenCalled());
  expect(chatSend.mock.calls[0][0]).toMatchObject({ use_memory: explicit, update_memory: explicit });
  expect((await bridge.configRead({ scope: { kind: "project", loupistId: "loupist-quality" } })).explicit).toEqual({});
});

it("rereads defaults before sending and keeps composer choices out of configuration", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const chatSend = vi.spyOn(bridge, "chatSend");
  await openApp(bridge);
  await updateGlobal(bridge, { chat: { thinking_level: "high", run_options: { performance: "fast" } } });
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Read this with current defaults");
  await user.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(chatSend).toHaveBeenCalled());
  expect(chatSend.mock.calls[0][0]).toMatchObject({ thinking_level: "high", run_options: { performance: "fast" } });
  await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
  await user.click(screen.getByRole("menuitem", { name: "Advanced" }));
  await user.click(await screen.findByRole("menuitemradio", { name: "Max" }));
  await user.keyboard("{Escape}");
  expect((await bridge.configRead({ scope: { kind: "global" } })).explicit?.chat?.thinking_level).toBe("high");
  expect((await bridge.configRead({ scope: { kind: "project", loupistId: "loupist-quality" } })).explicit).toEqual({});
});

it.each(["the app", "this Loupist"] as const)("lets a Chat default saved for %s replace the composer's earlier pick from the next reply", async (where) => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await bridge.providerConfigure({ provider: "anthropic", api_key: "test-key" });
  const chatSend = vi.spyOn(bridge, "chatSend");
  await openApp(bridge);
  const modelMenu = () => screen.getByRole("button", { name: /Model settings for this message/ });
  await user.click(modelMenu());
  await user.click(await screen.findByRole("menuitemradio", { name: "Claude Sonnet 4.5" }));
  await user.keyboard("{Escape}");
  await waitFor(() => expect(modelMenu()).toHaveAccessibleName(/^Claude Sonnet 4\.5 · /));
  if (where === "the app") {
    await user.click(await mainTab(/Settings/));
    await user.click(screen.getByRole("button", { name: "Chat defaults" }));
  } else {
    await user.click(headerAction("Loupist settings"));
    const page = await screen.findByRole("region", { name: "Loupist settings" });
    await user.click(within(page).getByRole("button", { name: "Chat defaults" }));
  }
  const level = await screen.findByLabelText("Reasoning level");
  await waitFor(() => expect(level).toBeEnabled());
  await user.selectOptions(level, "high");
  expect(await screen.findByText("Saved · Applies to new responses")).toBeInTheDocument();
  await user.click(await mainTab(/Chat/));
  // The chat that used the pick and a new one both answer with the saved default.
  await waitFor(() => expect(modelMenu()).toHaveAccessibleName(/^GPT-5\.6 Luna · .* · High$/));
  await user.click(newChatButton());
  await screen.findByRole("heading", { name: "Untitled research session" });
  expect(modelMenu()).toHaveAccessibleName(/^GPT-5\.6 Luna · .* · High$/);
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Use the saved default");
  await user.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(chatSend).toHaveBeenCalledOnce());
  expect(chatSend.mock.calls[0]![0]).toMatchObject({ model_profile_id: "", thinking_level: "high" });
});

const planModels = {
  openai: [
    "gpt-6.1-sol", "gpt-6.2-astra", "gpt-6.2-sol", "gpt-6.1-terra", "gpt-6-astra",
    "gpt-6-sol", "gpt-6-luna", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna",
  ],
  xai: ["grok-4.7", "grok-4.6", "grok-4.5", "grok-4.3"],
} as const;

describe.each(["Bot", "research"])("model-only Chat preferences in %s", (surface) => {
  it.each([
    { connected: ["xai"], saved: "xai/grok-4.7", expected: "xai/grok-4.7" },
    { connected: ["xai", "openai"], saved: "openai/gpt-6.2-sol", expected: "openai/gpt-6.2-sol" },
    { connected: ["openai"], saved: "openai/gpt-6.2-sol", expected: "openai/gpt-6.2-sol" },
    { connected: ["xai"], saved: "openai/gpt-6.2-sol", expected: "" },
    // A default still listed that cannot run answers like one that left: with Auto.
    { connected: ["xai", "openai"], signedOut: ["openai"], saved: "openai/gpt-6.2-sol", expected: "", shows: "grok-4.7" },
    { connected: ["xai", "openai"], hidden: ["openai/gpt-6.2-sol"], saved: "openai/gpt-6.2-sol", expected: "", shows: "grok-4.7" },
  ] as { connected: readonly ("xai" | "openai")[]; signedOut?: string[]; hidden?: string[]; saved: string; expected: string; shows?: string }[])("sends $expected with $connected connected (signed out: $signedOut, hidden: $hidden)", async ({ connected, signedOut = [], hidden = [], saved, expected, shows }) => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const scope: ConfigScope = { kind: "project", loupistId: "loupist-quality" };
    const snapshot = await bridge.configRead({ scope });
    await bridge.configUpdate({ scope, expectedRevision: snapshot.revision, set: { chat: { model_profile_id: saved } }, unset: [] });
    const initial = await bridge.modelCatalogGet();
    const signedIn = (provider: string) => !signedOut.includes(provider);
    const models = connected.flatMap((provider) => planModels[provider].map((model) => ({
      ...initial.models[0]!, id: `${provider}/${model}`, provider, model, name: model, credential_source: signedIn(provider) ? "saved" as const : "missing" as const,
    })));
    const providers = initial.providers.map((provider) => connected.some((id) => id === provider.id) && signedIn(provider.id)
      ? { ...provider, credential_source: "saved" as const, status: mockProviderStatus("plan") }
      : { ...provider, credential_source: "missing" as const, status: mockProviderStatus("none") });
    const catalog = { ...initial, models, providers, hidden_model_profile_ids: hidden };
    vi.spyOn(bridge, "modelCatalogGet").mockResolvedValue({ ...catalog, default_selection: mockModelSelection(catalog) });
    const read = bridge.configRead.bind(bridge);
    vi.spyOn(bridge, "configRead").mockImplementation(async (request) => {
      const next = await read(request);
      const selection = mockModelSelection(catalog, next.effective.chat);
      // The published host omitted controls when the requested model was explicit.
      return { ...next, modelSelection: selection.is_default ? selection : { ...selection, run_options: {} } };
    });
    const chatSend = vi.spyOn(bridge, "chatSend").mockRejectedValue({ code: "invalid_request", message: "Request captured." });
    render(<App bridge={bridge} />);
    if (surface === "research") {
      await user.click(await screen.findByRole("button", { name: "New research" }));
      await screen.findByRole("heading", { name: "Untitled research session" });
    }
    const composer = await screen.findByRole("textbox", { name: "Message Loupist" });
    // The composer names the model that will answer, and Chat can send.
    if (shows) await waitFor(() => expect(screen.getByRole("button", { name: /Model settings for this message/ })).toHaveAccessibleName(new RegExp(`^${shows.replace(".", "\\.")} · `)));
    await user.type(composer, "Hello");
    await user.click(await screen.findByRole("button", { name: "Send" }));
    await waitFor(() => expect(chatSend).toHaveBeenCalledOnce());
    const request = chatSend.mock.calls[0]![0];
    expect(request).toMatchObject({
      model_profile_id: expected,
      thinking_level: expected ? "medium" : "",
      run_options: expected ? { performance: "standard" } : {},
    });
    expect(request.thread_id.startsWith("home-")).toBe(surface === "Bot");
  });
});

it("keeps saving appearance after Agent settings were saved with application pending", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const update = bridge.configUpdate.bind(bridge);
  vi.spyOn(bridge, "configUpdate").mockImplementationOnce(async (request) => { await update(request); throw applyPending; });
  await openApp(bridge);
  await user.click(await mainTab(/Settings/));
  await user.click(screen.getByRole("button", { name: "Chat defaults" }));
  await user.click(await screen.findByText("Advanced"));
  const retries = await screen.findByLabelText("Maximum retries");
  await user.clear(retries);
  await user.type(retries, "5");
  await user.click(screen.getByRole("button", { name: "Save Maximum retries" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Configuration was saved");
  await user.click(screen.getByRole("button", { name: "Appearance" }));
  const dark = await screen.findByRole("button", { name: "Dark" });
  await user.click(dark);
  await waitFor(() => expect(dark).toHaveAttribute("aria-pressed", "true"));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  const saved = await bridge.configRead({ scope: { kind: "global" } });
  expect(saved.explicit?.pi?.retry?.max_retries).toBe(5);
  expect(saved.explicit?.appearance?.theme).toBe("dark");
});

it("keeps appearance current after connector configuration is saved with application pending", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const configure = bridge.marketplaceConnectorConfigure.bind(bridge);
  vi.spyOn(bridge, "marketplaceConnectorConfigure").mockImplementation(async (request) => { await configure(request); throw applyPending; });
  await openApp(bridge);
  await user.click(await mainTab(/Connections/));
  await user.click(await screen.findByRole("button", { name: "Web Research settings" }));
  const dialog = screen.getByRole("dialog", { name: "Web Research settings" });
  await user.click(within(dialog).getByRole("combobox", { name: "Search provider" }));
  await user.click(screen.getByRole("option", { name: "Model search only" }));
  await user.click(within(dialog).getByRole("button", { name: "Save setup" }));
  expect(await within(dialog).findByRole("alert")).toHaveTextContent("Configuration was saved");
  await user.click(within(dialog).getByRole("button", { name: "Close" }));
  await user.click(await mainTab(/Settings/));
  await user.click(screen.getByRole("button", { name: "Appearance" }));
  await user.click(await screen.findByRole("button", { name: "Dark" }));
  await waitFor(async () => expect((await bridge.configRead({ scope: { kind: "global" } })).explicit?.appearance?.theme).toBe("dark"));
  expect((await bridge.configRead({ scope: { kind: "global" } })).explicit?.connectors?.["community.web-research"]).toEqual({ search_policy: "model_only" });
});

it("turns Skill Creator on for that Loupist and drafts without sending", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0, userSkillsAvailable: true });
  const chatSend = vi.spyOn(bridge, "chatSend");
  const update = vi.spyOn(bridge, "configUpdate");
  await openApp(bridge);
  await draftSkill(user, "loupist-quality", "Quarterly review");
  expect(composerValue()).toContain("$skill-creator");
  expect(update).toHaveBeenCalledWith(expect.objectContaining({
    scope: { kind: "project", loupistId: "loupist-quality" }, set: { skills: { "skill-creator": { enabled: true } } },
  }));
  expect(chatSend).not.toHaveBeenCalled();
});

it("drafts a Skill in the chosen available Loupist when the current Loupist is disconnected", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0, userSkillsAvailable: true });
  const listed = await bridge.loupistList();
  const unavailable = { status: "workspace_unavailable" as const };
  vi.spyOn(bridge, "loupistList").mockResolvedValue(listed.map((loupist) => loupist.id === "loupist-quality" ? { ...loupist, availability: unavailable } : loupist));
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async (id) => {
    const workspace = await select(id);
    return id === "loupist-quality" ? { ...workspace, loupist: { ...workspace.loupist, availability: unavailable } } : workspace;
  });
  const chatSend = vi.spyOn(bridge, "chatSend");
  render(<App bridge={bridge} />);
  await openChat("Downside scenario review");
  await chooseLoupist(user, listed[0].name);
  await screen.findByText(/Workspace folder unavailable/);
  await draftSkill(user, "loupist-value", "Target Loupist workflow");
  expect(selectedLoupistButton()).toHaveTextContent(listed.find((loupist) => loupist.id === "loupist-value")!.name);
  expect(chatSend).not.toHaveBeenCalled();
});
