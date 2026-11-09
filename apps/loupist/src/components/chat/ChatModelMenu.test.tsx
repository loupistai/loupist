import { TooltipProvider } from "../ui/tooltip";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ChatModelMenu } from "./ChatModelMenu";
import type { ModelProviderOption, ProviderStatusView } from "../../types";

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
  render(<TooltipProvider><ChatModelMenu models={[model]} providers={[provider]} selection={{ model_profile_id: model.id, thinking_level: "", run_options: {} }} onSelectionChange={onSelectionChange} auto={false} onResetToAuto={vi.fn()} /></TooltipProvider>);
  await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
  expect(screen.queryByRole("menuitemradio", { name: "Max" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("menuitem", { name: "Advanced" }));
  await user.click(screen.getByRole("menuitemradio", { name: "Max" }));
  expect(onSelectionChange).toHaveBeenCalledWith({ model_profile_id: model.id, thinking_level: "max", run_options: {} });
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
  expect(screen.getByRole("dialog")).toBeInTheDocument();
});

it("shows Auto without a model name until a model is chosen, and resets to Auto from the menu", async () => {
  const user = userEvent.setup();
  const onResetToAuto = vi.fn();
  const onSelectionChange = vi.fn();
  const model = { id: "openai-codex/gpt-5.6-sol", name: "GPT-5.6 Sol", provider: "openai-codex", model: "gpt-5.6-sol", input: ["text" as const], reasoning: true,
    context_window: 272_000, max_tokens: 128_000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "saved" as const };
  const props = { models: [model], providers: [], selection: { model_profile_id: model.id, thinking_level: "medium", run_options: {} }, onSelectionChange, onResetToAuto };
  const { rerender } = render(<TooltipProvider><ChatModelMenu {...props} auto /></TooltipProvider>);
  const trigger = screen.getByRole("button", { name: /Model settings for this message/ });
  expect(trigger).toHaveTextContent(/^Auto$/);
  expect(document.body).not.toHaveTextContent("GPT-5.6 Sol");
  await user.click(trigger);
  expect(screen.getByRole("menuitemradio", { name: "Auto (recommended)" })).toHaveAttribute("aria-checked", "true");
  expect(screen.getByRole("menuitemradio", { name: "GPT-5.6 Sol" })).toHaveAttribute("aria-checked", "false");
  await user.click(screen.getByRole("menuitemradio", { name: "GPT-5.6 Sol" }));
  expect(onSelectionChange).toHaveBeenCalledWith(props.selection);
  await user.keyboard("{Escape}");

  rerender(<TooltipProvider><ChatModelMenu {...props} auto={false} /></TooltipProvider>);
  expect(screen.getByRole("button", { name: /Model settings for this message/ })).toHaveTextContent("GPT-5.6 Sol");
  await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
  await user.click(screen.getByRole("menuitemradio", { name: "Reset to Auto" }));
  expect(onResetToAuto).toHaveBeenCalled();
});

it("asks to connect an AI account when no model can run", async () => {
  const user = userEvent.setup();
  const onManageModels = vi.fn();
  const model = { id: "openai-codex/gpt-5.6-sol", name: "GPT-5.6 Sol", provider: "openai-codex", model: "gpt-5.6-sol", input: ["text" as const], reasoning: true,
    context_window: 272_000, max_tokens: 128_000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "missing" as const };
  render(<TooltipProvider><ChatModelMenu models={[model]} providers={[]} selection={{ model_profile_id: "", thinking_level: "", run_options: {} }}
    onSelectionChange={vi.fn()} onManageModels={onManageModels} auto onResetToAuto={vi.fn()} /></TooltipProvider>);
  await user.click(screen.getByRole("button", { name: "Connect an AI account" }));
  expect(onManageModels).toHaveBeenCalled();
});

it("names the plan behind the answer under the model list and turns it to a warning at the limit", async () => {
  const user = userEvent.setup();
  const onOpenUsage = vi.fn();
  const model = { id: "openai/gpt-6.1-sol", name: "GPT-6.1 Sol", provider: "openai", model: "gpt-6.1-sol", input: ["text" as const], reasoning: true,
    context_window: 272_000, max_tokens: 128_000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "saved" as const };
  const props = { models: [model], selection: { model_profile_id: model.id, thinking_level: "medium", run_options: {} }, onSelectionChange: vi.fn(), onResetToAuto: vi.fn(), onOpenUsage, auto: true };
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

it("does not describe the last used model as Auto's next execution after a remount", () => {
  const terra = { id: "openai/gpt-6.1-terra", name: "GPT-6.1 Terra", provider: "openai", model: "gpt-6.1-terra", input: ["text" as const], reasoning: true,
    context_window: 272_000, max_tokens: 128_000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "saved" as const };
  const props = { models: [terra], providers: [chatgpt()], selection: { model_profile_id: terra.id, thinking_level: "medium", run_options: {} }, onSelectionChange: vi.fn(), onResetToAuto: vi.fn() };
  const first = render(<TooltipProvider><ChatModelMenu {...props} auto={false} /></TooltipProvider>);
  expect(screen.getByRole("button", { name: /Model settings for this message/ })).toHaveAttribute("aria-description", "GPT-6.1 Terra · Medium");
  first.unmount();
  render(<TooltipProvider><ChatModelMenu {...props} auto /></TooltipProvider>);
  const trigger = screen.getByRole("button", { name: "Auto · Model settings for this message" });
  expect(trigger).toHaveAttribute("aria-description", "Uses your default model settings. The automatic choice is resolved when you send.");
  expect(trigger).not.toHaveAccessibleDescription(/Terra/);
});
