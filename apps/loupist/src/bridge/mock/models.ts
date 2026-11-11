/**
 * The mock model catalog: built-in providers, their models and the Rust-side
 * rules (`providers/catalog.rs`) the mock bridge applies to them.
 */
import type * as Contract from "../../types";
import { defaultRunOptions } from "../../lib/modelSelection";

/** Oct 31 2026, 09:00 UTC: a fixed "connected since" date keeps previews stable. */
export const MOCK_CONNECTED_AT_MS = Date.UTC(2026, 9, 31, 9, 0);

export const mockProviderStatus = (
  credential_kind: Contract.ProviderCredentialKind,
  overrides: Partial<Contract.ProviderStatusView> = {},
): Contract.ProviderStatusView => ({
  state: credential_kind === "none" ? "disconnected" : "connected",
  credential_kind,
  notice: null,
  last_limit_at_ms: null,
  connected_at_ms: credential_kind === "none" || credential_kind === "environment" ? null : MOCK_CONNECTED_AT_MS,
  used_by: { schedules: 0, chat_default: false, world_models: 0 },
  ...overrides,
});

const mockProvider = (
  id: string,
  label: string,
  credential_label: string,
  options: {
    description: string;
    api_key?: boolean;
    oauth?: string | null;
    credential_source?: Contract.ModelProviderOption["credential_source"];
    recommended?: boolean;
    group?: Contract.ProviderGroup;
    console_url?: string | null;
    sign_in_flow?: Contract.ModelProviderOption["sign_in_flow"];
    billing_note?: Contract.ModelProviderOption["billing_note"];
    credential_kind?: Contract.ProviderCredentialKind;
  },
): Contract.ModelProviderOption => {
  const credential_source = options.credential_source ?? "missing";
  return {
    id,
    label,
    credential_label,
    description: options.description,
    api_key: options.api_key ?? true,
    oauth: options.oauth ? { label: options.oauth } : null,
    credential_source,
    recommended: options.recommended ?? false,
    group: options.group ?? "more",
    console_url: options.console_url ?? null,
    sign_in_flow: options.sign_in_flow ?? null,
    billing_note: options.billing_note ?? null,
    status: mockProviderStatus(credential_source === "missing" ? "none" : options.credential_kind ?? "api_key"),
  };
};

const MOCK_PERFORMANCE_CONTROL: Contract.ModelRunControl = {
  id: "performance",
  label: "Performance",
  default_choice: "standard",
  choices: [
    {
      id: "standard",
      label: "Standard",
      description: "Use the provider's standard service tier.",
    },
    {
      id: "fast",
      label: "Fast",
      description: "Request the provider's priority service tier.",
    },
  ],
};

/** One model a provider lists, as Pi reports it before Rust adds its profile fields. */
type ProviderModelOption = Omit<Contract.ConfiguredModel, "id" | "provider" | "model" | "credential_source"> & { id: string };

/** The models each built-in provider lists once connected. */
const PROVIDER_MODELS: Record<string, ProviderModelOption[]> = {
  openai: [
    {
      id: "gpt-5.6-luna",
      name: "GPT-5.6 Luna",
      reasoning: true,
      context_window: 272_000,
      max_tokens: 128_000,
      input: ["text"],
      thinking_levels: ["low", "medium", "high", "max"],
      thinking_level_map: { max: "max" },
      run_controls: [MOCK_PERFORMANCE_CONTROL],
    },
    {
      id: "gpt-5.6-sol",
      name: "GPT-5.6 Sol",
      reasoning: true,
      context_window: 272_000,
      max_tokens: 128_000,
      input: ["text"],
      thinking_levels: ["off", "low", "medium", "high", "xhigh"],
      thinking_level_map: { xhigh: "xhigh" },
      run_controls: [MOCK_PERFORMANCE_CONTROL],
    },
    {
      id: "gpt-5.6-terra",
      name: "GPT-5.6 Terra",
      reasoning: true,
      context_window: 272_000,
      max_tokens: 128_000,
      input: ["text"],
      thinking_levels: ["off", "low", "medium", "high"],
      thinking_level_map: {},
      run_controls: [MOCK_PERFORMANCE_CONTROL],
    },
  ],
  anthropic: [
    {
      id: "claude-sonnet-4-5",
      name: "Claude Sonnet 4.5",
      reasoning: true,
      context_window: 200_000,
      max_tokens: 64_000,
      input: ["text", "image"],
      thinking_levels: ["off", "low", "medium", "high"],
      thinking_level_map: {},
      run_controls: [],
    },
  ],
};

/** The catalog a fresh mock bridge starts with: one ChatGPT model, every provider listed. */
export const initialModelCatalog = (): Contract.ModelCatalog => ({
  hidden_model_profile_ids: [],
  models: [
    {
      id: "model-test",
      name: "GPT-5.6 Luna",
      provider: "openai",
      model: "gpt-5.6-luna",
      input: ["text"],
      reasoning: true,
      context_window: 128_000,
      max_tokens: 32_000,
      thinking_levels: ["low", "medium", "high", "max"],
      thinking_level_map: { max: "max" },
      run_controls: [MOCK_PERFORMANCE_CONTROL],
      credential_source: "saved",
    },
  ],
  providers: [
    mockProvider("openai", "ChatGPT", "OpenAI API key", {
      description: "Sign in with ChatGPT, or use an OpenAI API key.",
      oauth: "Continue with ChatGPT",
      credential_source: "saved",
      recommended: true,
      group: "sign_in",
      console_url: "https://platform.openai.com/api-keys",
      sign_in_flow: "browser",
      billing_note: "chatgpt_plan",
      credential_kind: "plan",
    }),
    mockProvider("anthropic", "Anthropic", "Anthropic API key", {
      description: "Connect with an API key stored by Pi in Loupist's private app data.",
      group: "api_key",
      console_url: "https://console.anthropic.com/settings/keys",
    }),
    mockProvider("google", "Google Gemini", "Gemini API key", {
      description: "Connect with an API key stored by Pi in Loupist's private app data.",
      group: "api_key",
      console_url: "https://aistudio.google.com/apikey",
    }),
    mockProvider("xai", "xAI", "xAI API key", {
      description: "Sign in with SuperGrok, or use an xAI API key.",
      oauth: "Continue with SuperGrok",
      group: "sign_in",
      console_url: "https://console.x.ai",
      sign_in_flow: "device",
    }),
    mockProvider("openrouter", "OpenRouter", "OpenRouter API key", {
      description: "Connect with an API key stored by Pi in Loupist's private app data.",
      group: "api_key",
      console_url: "https://openrouter.ai/keys",
    }),
    mockProvider("vercel-ai-gateway", "Vercel AI Gateway", "AI Gateway API key", {
      description: "Connect with an API key stored by Pi in Loupist's private app data.",
    }),
    mockProvider("groq", "Groq", "Groq API key", {
      description: "Connect with an API key stored by Pi in Loupist's private app data.",
    }),
  ],
  custom_providers: [],
  model_notice: null,
});

/** Lists a provider's models in `catalog`, as `provider_models` does after a sign-in or key. */
export function listProviderModels(catalog: Contract.ModelCatalog, provider: string) {
  const options = PROVIDER_MODELS[provider] ?? [
    {
      id: `${provider}-default`,
      name: `${provider} default`,
      reasoning: false,
      context_window: 128_000,
      max_tokens: 32_000,
      input: ["text"],
      thinking_levels: ["off"],
      thinking_level_map: {},
      run_controls: [],
    },
  ];
  const credentialSource =
    catalog.providers.find((item) => item.id === provider)
      ?.credential_source ?? "missing";
  catalog.models = [
    ...catalog.models.filter(
      (model) => model.provider !== provider,
    ),
    ...options.map((option) => ({
      id: `${provider}/${option.id}`,
      name: option.name,
      provider,
      model: option.id,
      input: option.input,
      reasoning: option.reasoning,
      context_window: option.context_window,
      max_tokens: option.max_tokens,
      thinking_levels: option.thinking_levels,
      thinking_level_map: option.thinking_level_map,
      run_controls: option.run_controls,
      credential_source: credentialSource,
    })),
  ];
}

/** The model a run is locked to, as `chat_send` resolves it; an empty profile is Auto. */
export function executionModelLock(
  catalog: Contract.ModelCatalog,
  model_profile_id: string,
  thinking_level: string,
  run_options: Record<string, string>,
): Contract.ExecutionModelLock {
  // An empty profile is Auto: like Rust, take the first connected model shown in Chat at its default controls.
  if (!model_profile_id) {
    const hidden = new Set(catalog.hidden_model_profile_ids);
    const auto = catalog.models.find((item) => !hidden.has(item.id) && item.credential_source !== "missing");
    if (!auto) throw new Error("Connect an AI account in Settings.");
    // Configured reasoning and run options apply when the picked model supports them.
    const thinking = auto.thinking_levels.includes(thinking_level) ? thinking_level : auto.thinking_levels.includes("medium") ? "medium" : auto.thinking_levels[0] ?? "off";
    const options = { ...defaultRunOptions(auto), ...Object.fromEntries(Object.entries(run_options).filter(([key, value]) => auto.run_controls.some((control) => control.id === key && control.choices.some((choice) => choice.id === value)))) };
    return executionModelLock(catalog, auto.id, thinking, options);
  }
  const model = catalog.models.find(
    (item) => item.id === model_profile_id,
  );
  if (!model) throw new Error("The selected model profile was not found.");
  if (catalog.hidden_model_profile_ids.includes(model.id)) {
    throw new Error("The selected model is hidden from Chat.");
  }
  if (!model.thinking_levels.includes(thinking_level)) {
    throw new Error(
      "The selected thinking level is not available for this model.",
    );
  }
  return {
    profile_id: model.id,
    name: model.name,
    provider: model.provider,
    model: model.model,
    thinking_level,
    run_options,
  };
}
