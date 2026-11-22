import { TooltipProvider } from "../ui/tooltip";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ChatModelMenu } from "./ChatModelMenu";
import type { ModelProviderOption, ModelRunSelection, ProviderStatusView } from "../../types";
import { useState } from "react";


const planStatus = (notice: ProviderStatusView["notice"] = null): ProviderStatusView => ({
  state: "connected", credential_kind: "plan", notice, last_limit_at_ms: null, connected_at_ms: 1, used_by: { schedules: 0, chat_default: false, world_models: 0 },
});
const chatgpt = (status = planStatus()): ModelProviderOption => ({
  id: "openai", label: "ChatGPT", description: "OpenAI models", api_key: true, oauth: { label: "Continue with ChatGPT" }, credential_label: "OpenAI API key",
  credential_source: "saved", recommended: true, group: "sign_in", console_url: null, sign_in_flow: "browser", billing_note: "chatgpt_plan", status,
});

it("keeps reasoning under Advanced, recovers an empty thinking selection, and reopens", async () => {
  const user = userEvent.setup();
  const onSelectionChange = vi.fn();
  const model = { id: "openai-codex/gpt-5.6-sol", name: "GPT-5.6 Sol", provider: "openai-codex", model: "gpt-5.6-sol", input: ["text" as const], reasoning: true,
    context_window: 272_000, max_tokens: 128_000, thinking_levels: ["off", "medium", "high", "max"], thinking_level_map: { max: "max" }, run_controls: [], credential_source: "saved" as const };
  const provider = { ...chatgpt(), id: "openai-codex" };
  render(<TooltipProvider><ChatModelMenu models={[model]} providers={[provider]} selection={{ model_profile_id: model.id, thinking_level: "", run_options: {} }} onSelectionChange={onSelectionChange} /></TooltipProvider>);
  await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
  // One model fits at a glance, so the picker has no search field.
  expect(screen.queryByRole("searchbox")).toBeNull();
  expect(screen.queryByRole("menuitemradio", { name: "Max" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("menuitem", { name: "Advanced" }));
  await user.click(screen.getByRole("menuitemradio", { name: "Max" }));
  expect(onSelectionChange).toHaveBeenCalledWith({ model_profile_id: model.id, thinking_level: "max", run_options: {} });
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
  expect(screen.getByRole("dialog")).toBeInTheDocument();
});

it("names the effective model and checks its real row with a Default tag only while inherited", async () => {
  const user = userEvent.setup();
  const model = { id: "openai/gpt-6.2-sol", name: "GPT-6.2 Sol", provider: "openai", model: "gpt-6.2-sol", input: ["text"], reasoning: true,
    context_window: 272_000, max_tokens: 128_000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "saved" as const };
  const props = { models: [model], providers: [chatgpt()], selection: { model_profile_id: model.id, thinking_level: "medium", run_options: {} }, onSelectionChange: vi.fn(), onChooseDefault: vi.fn() };
  const { rerender } = render(<TooltipProvider><ChatModelMenu {...props} isDefault /></TooltipProvider>);
  expect(screen.getByTestId("composer-model-menu")).toHaveAccessibleDescription("GPT-6.2 Sol · Medium");
  await user.click(screen.getByTestId("composer-model-menu"));
  const row = screen.getByRole("menuitemradio", { name: "GPT-6.2 Sol Default" });
  expect(row).toBeChecked();
  expect(screen.getAllByRole("menuitemradio")).toHaveLength(1);
  await user.click(row);
  expect(props.onChooseDefault).toHaveBeenCalledOnce();
  rerender(<TooltipProvider><ChatModelMenu {...props} isDefault={false} /></TooltipProvider>);
  expect(screen.getByRole("menuitemradio", { name: "GPT-6.2 Sol" })).toBeChecked();
  expect(screen.queryByText("Default")).toBeNull();
});

it("keeps the projected model name visible while the catalog list refreshes", () => {
  render(<TooltipProvider><ChatModelMenu models={[]} providers={[chatgpt()]} modelName="GPT-6.2 Sol" isDefault
    selection={{ model_profile_id: "updated-id", thinking_level: "high", run_options: {} }} onSelectionChange={vi.fn()} /></TooltipProvider>);
  expect(screen.getByTestId("composer-model-menu")).toHaveAccessibleDescription("GPT-6.2 Sol · High");
});

it("asks to connect an AI account when no model can run", async () => {
  const user = userEvent.setup();
  const onManageModels = vi.fn();
  const model = { id: "openai-codex/gpt-5.6-sol", name: "GPT-5.6 Sol", provider: "openai-codex", model: "gpt-5.6-sol", input: ["text" as const], reasoning: true,
    context_window: 272_000, max_tokens: 128_000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "missing" as const };
  render(<TooltipProvider><ChatModelMenu models={[model]} providers={[]} selection={{ model_profile_id: "", thinking_level: "", run_options: {} }}
    onSelectionChange={vi.fn()} onManageModels={onManageModels} noAccount /></TooltipProvider>);
  await user.click(screen.getByRole("button", { name: "No AI account" }));
  expect(onManageModels).toHaveBeenCalled();
});

it("names the plan behind the answer under the model list and turns it to a warning at the limit", async () => {
  const user = userEvent.setup();
  const onOpenUsage = vi.fn();
  const model = { id: "openai/gpt-6.1-sol", name: "GPT-6.1 Sol", provider: "openai", model: "gpt-6.1-sol", input: ["text" as const], reasoning: true,
    context_window: 272_000, max_tokens: 128_000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "saved" as const };
  const props = { models: [model], selection: { model_profile_id: model.id, thinking_level: "medium", run_options: {} }, onSelectionChange: vi.fn(), onOpenUsage, isDefault: true };
  const { rerender } = render(<TooltipProvider><ChatModelMenu {...props} providers={[chatgpt()]} /></TooltipProvider>);
  await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
  expect(screen.getByText("Using your ChatGPT plan")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Manage usage" }));
  expect(onOpenUsage).toHaveBeenCalledWith("openai");
  await user.keyboard("{Escape}");
  rerender(<TooltipProvider><ChatModelMenu {...props} providers={[chatgpt(planStatus({ kind: "plan_limit", at_ms: 1 }))]} /></TooltipProvider>);
  await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
  const limit = screen.getByText("ChatGPT plan at its limit");
  expect(limit).toHaveAttribute("data-tone", "warning");
  await user.keyboard("{Escape}");
  // A key account has no plan line.
  rerender(<TooltipProvider><ChatModelMenu {...props} providers={[chatgpt({ ...planStatus(), credential_kind: "api_key" })]} /></TooltipProvider>);
  await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
  expect(screen.queryByText("Using your ChatGPT plan")).not.toBeInTheDocument();
});

describe("a model picked in the composer", () => {
  const grok = { id: "xai/grok-4.7", name: "Grok 4.7", provider: "xai", model: "grok-4.7", input: ["text" as const], reasoning: true,
    context_window: 272_000, max_tokens: 128_000, thinking_levels: ["low", "medium", "high"], thinking_level_map: {}, run_controls: [], credential_source: "saved" as const };
  const gpt = { ...grok, id: "openai/gpt-6.1-sol", name: "GPT-6.1 Sol", provider: "openai", model: "gpt-6.1-sol", thinking_levels: ["medium", "high"] };
  const catalog = { hidden_model_profile_ids: [], models: [grok, gpt], providers: [], custom_providers: [], model_notice: null };
  function Composer() {
    const [selection, setSelection] = useState<ModelRunSelection>({ model_profile_id: gpt.id, thinking_level: "medium", run_options: {} });
    return <TooltipProvider><ChatModelMenu models={catalog.models} providers={[]} selection={selection} onSelectionChange={setSelection} /></TooltipProvider>;
  }
  const trigger = () => screen.getByTestId("composer-model-menu");
  const pick = async (user: ReturnType<typeof userEvent.setup>, name: string) => {
    await user.click(trigger());
    await user.click(screen.getByRole("menuitemradio", { name }));
    await user.keyboard("{Escape}");
  };

  const level = async (user: ReturnType<typeof userEvent.setup>, name: string) => {
    await user.click(trigger());
    await user.click(screen.getByRole("menuitem", { name: "Advanced" }));
    await user.click(screen.getByRole("menuitemradio", { name }));
    await user.keyboard("{Escape}");
  };

  it("retains the current level when the chosen model supports it", async () => {
    const user = userEvent.setup();
    render(<Composer />);
    // Picking a model keeps the level in use (whatever the surface's resolver showed with nothing chosen).
    const shown = trigger().querySelector(".composer-thinking-level")!.textContent;
    await pick(user, "Grok 4.7");
    expect(trigger()).toHaveTextContent(`Grok 4.7${shown}`);
    await level(user, "Low");
    expect(trigger()).toHaveTextContent(/^Grok 4\.7Low$/);
    expect(trigger()).toHaveAccessibleName(/· Low$/);
    // A model that does not list it runs at its own default, and the label says so.
    await pick(user, "GPT-6.1 Sol");
    expect(trigger()).toHaveTextContent(/^GPT-6\.1 SolMedium$/);
    await level(user, "High");
    // Back to a model that lists the level in use: it stays.
    await pick(user, "Grok 4.7");
    expect(trigger()).toHaveTextContent(/^Grok 4\.7High$/);
  });
});
