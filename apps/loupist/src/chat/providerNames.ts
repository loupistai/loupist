import type { ProviderCredentialKind } from "../types";
import { KEY_NAMES, SIGN_IN_NAMES } from "./providerManifest";

/**
 * The name people know a provider by, for sentences such as "Sign in to
 * ChatGPT again" and for provider cards. One `openai` slot holds either a
 * ChatGPT sign-in or an OpenAI API key, so the name follows the credential;
 * without a credential kind a plan-capable provider reads as its sign-in name.
 * Product names are not translated; they come from the provider manifest.
 */
export const providerShortName = (providerId: string | undefined, kind?: ProviderCredentialKind) => {
  if (!providerId) return undefined;
  if (kind === "api_key" || kind === "environment") return KEY_NAMES[providerId] ?? SIGN_IN_NAMES[providerId];
  return SIGN_IN_NAMES[providerId] ?? KEY_NAMES[providerId];
};

/** Name for the API-key sheet and key rows: "Anthropic", "OpenAI", "xAI". */
export const providerKeyName = (providerId: string, fallback: string) => KEY_NAMES[providerId] ?? fallback;

/** The company name in "Get a key from Upstage" and key messages; the row itself reads "Upstage Solar". */
export const UPSTAGE_COMPANY = "Upstage";
