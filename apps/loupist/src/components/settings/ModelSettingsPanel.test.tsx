import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import type { ModelCatalog, ModelProviderOption, ProviderStatusView } from "../../types";
import { ModelSettingsPanel } from "./ModelSettingsPanel";

const status = (overrides: Partial<ProviderStatusView> = {}): ProviderStatusView => ({
  state: "connected", credential_kind: "plan", notice: null, last_limit_at_ms: null, connected_at_ms: Date.UTC(2026, 9, 31),
  used_by: { schedules: 0, chat_default: false, world_models: 0 }, ...overrides,
});
const provider = (id: string, overrides: Partial<ModelProviderOption>): ModelProviderOption => ({
  id, label: id, credential_label: "", description: "", api_key: true, oauth: null, credential_source: "saved", recommended: false,
  group: "more", console_url: null, sign_in_flow: null, billing_note: null, status: status({ credential_kind: "api_key" }), ...overrides,
});

const catalog: ModelCatalog = {
  providers: [
    provider("openai", { label: "ChatGPT", oauth: { label: "Continue with ChatGPT" }, recommended: true, group: "sign_in", sign_in_flow: "browser", billing_note: "chatgpt_plan", status: status() }),
    provider("xai", { label: "xAI", oauth: { label: "Continue with SuperGrok" }, group: "sign_in", sign_in_flow: "device", console_url: "https://console.x.ai" }),
  ],
  models: [{ id: "openai/model", provider: "openai", model: "model", name: "OpenAI model", input: ["text"], reasoning: false, context_window: 1000, max_tokens: 100, thinking_levels: ["none"], thinking_level_map: { none: null }, run_controls: [], credential_source: "saved" }],
  custom_providers: [],
  hidden_model_profile_ids: [],
  model_notice: null,
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => { resolve = resolvePromise; reject = rejectPromise; });
  return { promise, resolve, reject };
}

function panelProps(): ComponentProps<typeof ModelSettingsPanel> {
  return {
    catalog, loadError: null,
    bridge: {
      providerConnect: vi.fn().mockResolvedValue(catalog), providerConnectCancel: vi.fn().mockResolvedValue(undefined),
      providerConnectOpenBrowser: vi.fn().mockResolvedValue(undefined), providerConnectSubmitCode: vi.fn().mockResolvedValue(undefined),
      providerConfigure: vi.fn().mockResolvedValue(catalog), providerCustomSave: vi.fn().mockResolvedValue(catalog),
      providerPresetSave: vi.fn().mockResolvedValue(catalog), providerLocalDiscover: vi.fn().mockResolvedValue({ engines: [] }),
      providerCustomTest: vi.fn().mockResolvedValue({ outcome: "ok", models: [] }), providerOpenConsole: vi.fn().mockResolvedValue(undefined),
      providerOpenUsage: vi.fn().mockResolvedValue(undefined),
    },
    onLoadModels: vi.fn().mockResolvedValue(catalog),
    onUpdateModelVisibility: vi.fn().mockResolvedValue(catalog), onDisconnect: vi.fn().mockResolvedValue(catalog),
    onRemoveCustomProvider: vi.fn().mockResolvedValue(catalog), onCatalogChange: vi.fn(),
  };
}

it("keeps a discovery failure with its provider and permits retry without locking another provider", async () => {
  const user = userEvent.setup();
  const retry = deferred<ModelCatalog>();
  const props = panelProps();
  props.onLoadModels = vi.fn().mockRejectedValueOnce(new Error("xAI discovery timed out.")).mockReturnValueOnce(retry.promise);
  render(<ModelSettingsPanel {...props} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not load xAI models");
  expect(screen.getByRole("button", { name: "More actions for ChatGPT" })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "Reload models for xAI" }));
  expect(props.onLoadModels).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole("alert")).toBeNull();
  await act(async () => retry.resolve(catalog));
});

it("hides a model from the Chat menu", async () => {
  const user = userEvent.setup();
  const props = panelProps();
  render(<ModelSettingsPanel {...props} />);
  const card = screen.getByRole("article", { name: "ChatGPT" });
  await user.click(within(card).getByRole("button", { name: /Models \(1\)/u }));
  await user.click(within(card).getByRole("switch", { name: "Show OpenAI model in Chat" }));
  expect(props.onUpdateModelVisibility).toHaveBeenCalledExactlyOnceWith({ model_profile_id: "openai/model", visible_in_chat: false });
});

it("adds a provider through the shared connect sheet and reports the connection", async () => {
  const user = userEvent.setup();
  const props = panelProps();
  render(<ModelSettingsPanel {...props} />);
  await user.click(screen.getByRole("button", { name: "Add an AI account" }));
  const dialog = await screen.findByRole("dialog", { name: "Add an AI account" });
  // The picker lists the same ways as onboarding; no logos or monograms.
  expect(dialog.querySelector(".settings-provider-monogram")).toBeNull();
  await user.click(within(dialog).getByRole("button", { name: /^xAI/u }));
  await user.type(within(dialog).getByLabelText("API key"), "new-key");
  await user.click(within(dialog).getByRole("button", { name: "Connect" }));
  expect(props.bridge.providerConfigure).toHaveBeenCalledExactlyOnceWith({ provider: "xai", api_key: "new-key" });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(props.onCatalogChange).toHaveBeenCalledWith(catalog);
  expect(screen.getByText("xAI is connected.")).toBeInTheDocument();
});

it("opens the sheet another surface asked for once, and dismisses a disconnected account without a confirmation", async () => {
  const user = userEvent.setup();
  const props = panelProps();
  props.catalog = { ...catalog, providers: [catalog.providers[0], provider("anthropic", { label: "Anthropic", credential_source: "missing", oauth: null, sign_in_flow: null, status: status({ state: "disconnected", credential_kind: "none", used_by: { schedules: 1, chat_default: false, world_models: 0 } }) })] };
  const consumed = vi.fn();
  render(<ModelSettingsPanel {...props} connectTarget={{ kind: "provider", provider: "openai", method: "api_key" }} onConnectTargetConsumed={consumed} />);
  const sheet = await screen.findByRole("dialog", { name: "OpenAI" });
  expect(consumed).toHaveBeenCalledTimes(1);
  // An API key replaces the ChatGPT sign-in in the one OpenAI slot.
  expect(within(sheet).getByText(/This replaces your ChatGPT sign-in on this Mac/u)).toBeInTheDocument();
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  const disconnected = screen.getByRole("article", { name: "Anthropic" });
  expect(disconnected).toHaveTextContent("1 schedule still uses this account.");
  await user.click(within(disconnected).getByRole("button", { name: "Remove" }));
  expect(props.onDisconnect).toHaveBeenCalledWith("anthropic");
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("edits and removes a custom endpoint from its card", async () => {
  const user = userEvent.setup();
  const custom = { id: "custom-my-ollama", name: "My Ollama", base_url: "http://localhost:11434/v1", api: "openai-completions" as const, model_ids: ["qwen2.5:7b"], credential_source: "saved" as const, status: status({ credential_kind: "local" }) };
  const props = panelProps();
  props.catalog = { ...catalog, custom_providers: [custom] };
  render(<ModelSettingsPanel {...props} />);
  const card = screen.getByRole("article", { name: "My Ollama" });
  expect(card).toHaveTextContent("Running on this Mac · localhost:11434");
  await user.click(within(card).getByRole("button", { name: "More actions for My Ollama" }));
  await user.click(await screen.findByRole("menuitem", { name: "Reconnect…" }));
  expect(await screen.findByRole("dialog", { name: "Edit custom provider" })).toBeInTheDocument();
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await user.click(within(card).getByRole("button", { name: "More actions for My Ollama" }));
  await user.click(await screen.findByRole("menuitem", { name: "Remove My Ollama" }));
  const confirm = await screen.findByRole("dialog", { name: "Remove My Ollama?" });
  await user.click(within(confirm).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(props.onRemoveCustomProvider).toHaveBeenCalledWith("custom-my-ollama"));
  expect(props.onDisconnect).not.toHaveBeenCalled();
});

it("lets a ChatGPT sign-in bring its own models instead of loading them alongside it", async () => {
  const props = panelProps();
  const signIn = deferred<ModelCatalog>();
  props.bridge.providerConnect = vi.fn().mockReturnValue(signIn.promise);
  const chatgpt = catalog.providers[0];
  props.catalog = { ...catalog, providers: [{ ...chatgpt, credential_source: "missing", status: status({ state: "disconnected", credential_kind: "none" }) }], models: [] };
  const { rerender } = render(<ModelSettingsPanel {...props} connectTarget={{ kind: "provider", provider: "openai", method: "sign_in" }} />);
  await screen.findByRole("dialog");
  await waitFor(() => expect(props.bridge.providerConnect).toHaveBeenCalledTimes(1));

  // Back from the browser: the app's refreshed catalog has the new sign-in and no models yet.
  rerender(<ModelSettingsPanel {...props} catalog={{ ...catalog, providers: [chatgpt], models: [] }} />);
  await act(async () => signIn.resolve({ ...catalog, providers: [chatgpt] }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  rerender(<ModelSettingsPanel {...props} catalog={{ ...catalog, providers: [chatgpt] }} />);
  expect(props.onLoadModels).not.toHaveBeenCalled();
  expect(screen.getByText("ChatGPT is connected.")).toBeInTheDocument();
  expect(screen.queryByText("Could not load ChatGPT models")).toBeNull();
});

it("words a busy model load in the reader's language and offers to try again", async () => {
  const user = userEvent.setup();
  const props = panelProps();
  props.catalog = { ...catalog, providers: [catalog.providers[0]], models: [] };
  props.onLoadModels = vi.fn()
    .mockRejectedValueOnce({ code: "conflict", message: "another provider support operation is already active" })
    .mockResolvedValueOnce(catalog);
  render(<ModelSettingsPanel {...props} />);
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("Could not load ChatGPT models");
  expect(alert).toHaveTextContent("Another account change is still finishing. Try again in a moment.");
  expect(alert).not.toHaveTextContent("another provider support operation");
  await user.click(within(alert).getByRole("button", { name: "Reload models for ChatGPT" }));
  expect(props.onLoadModels).toHaveBeenCalledTimes(2);
});
