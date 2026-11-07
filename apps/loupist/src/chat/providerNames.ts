import type { ProviderCredentialKind } from "../types";

/**
 * The name people know a provider by, for sentences such as "Sign in to
 * ChatGPT again" and for provider cards. One `openai` slot holds either a
 * ChatGPT sign-in or an OpenAI API key, so the name follows the credential.
 * Product names are not translated.
 */
const SIGN_IN_NAMES: Record<string, string> = {
  openai: "ChatGPT",
  "openai-codex": "ChatGPT",
  xai: "SuperGrok",
};

const KEY_NAMES: Record<string, string> = {
  openai: "OpenAI",
  "openai-codex": "ChatGPT",
  anthropic: "Anthropic",
  xai: "xAI",
  google: "Google Gemini",
  openrouter: "OpenRouter",
  upstage: "Upstage Solar",
};

/** Short name for sentences; without a credential kind a plan-capable provider reads as its sign-in name. */
export const providerShortName = (providerId: string | undefined, kind?: ProviderCredentialKind) => {
  if (!providerId) return undefined;
  if (kind === "api_key" || kind === "environment") return KEY_NAMES[providerId] ?? SIGN_IN_NAMES[providerId];
  return SIGN_IN_NAMES[providerId] ?? KEY_NAMES[providerId];
};

/** Name for the API-key sheet and key rows: "Anthropic", "OpenAI", "xAI". */
export const providerKeyName = (providerId: string, fallback: string) => KEY_NAMES[providerId] ?? fallback;

/** The company name in "Get a key from Upstage" and key messages; the row itself reads "Upstage Solar". */
export const UPSTAGE_COMPANY = "Upstage";
