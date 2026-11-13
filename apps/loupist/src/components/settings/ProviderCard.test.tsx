import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import type { ConfiguredModel, ModelProviderOption, ProviderStatusView } from "../../types";
import { ProviderCard } from "./ProviderCard";

const NOW = Date.UTC(2026, 10, 1, 12, 0);
const status = (overrides: Partial<ProviderStatusView> = {}): ProviderStatusView => ({
  state: "connected", credential_kind: "plan", notice: null, last_limit_at_ms: null, connected_at_ms: Date.UTC(2026, 9, 31, 9, 0),
  used_by: { schedules: 0, chat_default: false, world_models: 0 }, ...overrides,
});
const option = (id: string, overrides: Partial<ModelProviderOption> = {}): ModelProviderOption => ({
  id, label: id === "openai" ? "ChatGPT" : id === "anthropic" ? "Anthropic" : "Google Gemini", credential_label: "", description: "",
  api_key: true, oauth: id === "openai" ? { label: "Continue" } : null, credential_source: "saved", recommended: false,
  group: id === "openai" ? "sign_in" : "api_key", console_url: null, sign_in_flow: id === "openai" ? "browser" : null,
  billing_note: id === "openai" ? "chatgpt_plan" : null, status: status(), ...overrides,
});
const model: ConfiguredModel = {
  id: "openai/gpt-6.1-sol", name: "GPT-6.1 Sol", provider: "openai", model: "gpt-6.1-sol", input: ["text"], reasoning: true,
  context_window: 272_000, max_tokens: 128_000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "saved",
};

function renderCard(props: Partial<ComponentProps<typeof ProviderCard>> & { option: ModelProviderOption }) {
  const handlers = { onReload: vi.fn(), onConnect: vi.fn(), onOpenUsage: vi.fn(), onRemove: vi.fn(), onSetVisibility: vi.fn(), onSetAllVisible: vi.fn() };
  render(<ul><ProviderCard models={[model]} hiddenModels={new Set()} chatModel={null} loading={false} removing={false} loadError={null}
    locked={false} reloadDisabled={false} visibilityBusyId={null} now={NOW} {...handlers} {...props} /></ul>);
  return { card: screen.getByRole("article"), ...handlers };
}

it("shows a ChatGPT sign-in as Connected with its date, the plan line and Chat's model, never an account name", async () => {
  const user = userEvent.setup();
  const { card, onOpenUsage, onConnect } = renderCard({ option: option("openai") });
  expect(within(card).getByRole("heading", { name: "ChatGPT" })).toBeInTheDocument();
  expect(card).toHaveTextContent("Connected");
  expect(card).toHaveTextContent("Signed in with ChatGPT · since Oct 31, 2026");
  expect(card).toHaveTextContent("Using your ChatGPT plan");
  // Auto is not repeated on every card.
  expect(card).not.toHaveTextContent("Chat default");
  expect(card.querySelector(".connect-state-dot")).toHaveAttribute("aria-hidden", "true");
  await user.click(within(card).getByRole("button", { name: "Manage usage" }));
  expect(onOpenUsage).toHaveBeenCalledWith("openai");
  await user.click(within(card).getByRole("button", { name: "More actions for ChatGPT" }));
  await user.click(await screen.findByRole("menuitem", { name: "Reconnect…" }));
  expect(onConnect).toHaveBeenCalledWith({ kind: "provider", provider: "openai", method: "sign_in" });
});

it("names the key, the local server and an Anthropic key by credential", async () => {
  const user = userEvent.setup();
  const key = renderCard({ option: option("google", { status: status({ credential_kind: "api_key" }) }), chatModel: undefined });
  expect(key.card).toHaveTextContent("API key saved on this Mac");
  expect(key.card).not.toHaveTextContent("Chat default");
  await user.click(within(key.card).getByRole("button", { name: "More actions for Google Gemini" }));
  await user.click(await screen.findByRole("menuitem", { name: "Change key…" }));
  expect(key.onConnect).toHaveBeenCalledWith({ kind: "provider", provider: "google", method: "api_key" });
  document.body.innerHTML = "";

  const custom = { id: "custom-ollama", name: "Ollama", base_url: "http://localhost:11434/v1", api: "openai-completions" as const, model_ids: ["qwen3:32b"], credential_source: "saved" as const, status: status({ credential_kind: "local" }) };
  const local = renderCard({ option: { ...option("custom-ollama", { status: status({ credential_kind: "local" }), billing_note: null }), label: "Ollama", custom } });
  expect(local.card).toHaveTextContent("Running on this Mac · localhost:11434");
  document.body.innerHTML = "";

  const claude = renderCard({ option: option("anthropic", { status: status({ credential_kind: "api_key" }) }) });
  expect(within(claude.card).getByRole("heading", { name: "Anthropic" })).toBeVisible();
  expect(claude.card).toHaveTextContent("API key saved on this Mac");
  expect(claude.card).not.toHaveTextContent(/extra usage|Signed in with/);
  await user.click(within(claude.card).getByRole("button", { name: "More actions for Anthropic" }));
  await user.click(await screen.findByRole("menuitem", { name: "Change key…" }));
  expect(claude.onConnect).toHaveBeenCalledWith({ kind: "provider", provider: "anthropic", method: "api_key" });
});

it("turns Reconnect into the primary action when the sign-in stopped working", () => {
  const { card } = renderCard({ option: option("openai", { status: status({ state: "needs_reconnect" }) }) });
  expect(card).toHaveTextContent("Needs reconnect");
  expect(card).toHaveTextContent("Your sign-in stopped working. Reconnect to keep going; your chats and map are safe.");
  expect(within(card).getByRole("button", { name: "Reconnect ChatGPT" })).toHaveAttribute("data-variant", "default");
});

it("shows one plan-limit notice with Manage usage, then a dated last-limit line for a week", () => {
  const limit = Date.UTC(2026, 9, 31, 15, 12);
  const { card } = renderCard({ option: option("openai", { status: status({ notice: { kind: "plan_limit", at_ms: limit }, last_limit_at_ms: limit }) }) });
  const notice = within(card).getByRole("status");
  expect(notice).toHaveTextContent(/^Reached its plan limit .+\. Background work on this account waits until a request goes through\./u);
  expect(within(notice).getByRole("button", { name: "Manage usage" })).toBeInTheDocument();
  // One Manage usage per card: the plan line above the notice does not repeat it.
  expect(within(card).getAllByRole("button", { name: "Manage usage" })).toHaveLength(1);
  document.body.innerHTML = "";

  const cleared = renderCard({ option: option("openai", { status: status({ last_limit_at_ms: limit }) }) });
  expect(cleared.card).toHaveTextContent(/Last reached its limit (Oct 31|Nov 1), 2026/u);
  expect(within(cleared.card).queryByRole("status")).toBeNull();
  document.body.innerHTML = "";

  const old = renderCard({ option: option("openai", { status: status({ last_limit_at_ms: NOW - 8 * 24 * 60 * 60 * 1000 }) }) });
  expect(old.card).not.toHaveTextContent("Last reached its limit");
});

it("offers Connect and Remove for a disconnected account that schedules still use", async () => {
  const user = userEvent.setup();
  const { card, onConnect, onRemove } = renderCard({ option: option("openai", { credential_source: "missing", status: status({ state: "disconnected", credential_kind: "none", connected_at_ms: null, used_by: { schedules: 2, chat_default: false, world_models: 0 } }) }) });
  expect(card).toHaveTextContent("Disconnected");
  expect(card).toHaveTextContent("2 schedules still use this account.");
  await user.click(within(card).getByRole("button", { name: "Connect" }));
  expect(onConnect).toHaveBeenCalledWith({ kind: "provider", provider: "openai", method: "sign_in" });
  await user.click(within(card).getByRole("button", { name: "Remove" }));
  expect(onRemove).toHaveBeenCalled();
});

it("lists models with their context and an In menu switch, keeping raw IDs in the tooltip", async () => {
  const user = userEvent.setup();
  const { card, onSetVisibility } = renderCard({ option: option("openai"), chatModel: model });
  expect(card).toHaveTextContent("Chat default: GPT-6.1 Sol");
  await user.click(within(card).getByRole("button", { name: /Models \(1\)/u }));
  const row = card.querySelector(".settings-model-row")!;
  expect(row).toHaveAttribute("title", "gpt-6.1-sol");
  expect(row).toHaveTextContent("GPT-6.1 Sol272,000 contextIn menu");
  expect(row).not.toHaveTextContent("gpt-6.1-sol");
  await user.click(within(card).getByRole("switch", { name: "Show GPT-6.1 Sol in Chat" }));
  expect(onSetVisibility).toHaveBeenCalledWith("openai/gpt-6.1-sol", false);
});
