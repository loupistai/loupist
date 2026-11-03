import type { ChatFailureKind } from "@/types";

export function briefingFailure(error?: string | null, kind?: ChatFailureKind): "auth" | "budget" | "unreachable" | "generic" {
  const text = error?.toLowerCase() ?? "";
  if (/\b(401|403|unauthorized|authentication_error|authentication|invalid_api_key|invalid api key|token_expired|expired token|provider_auth)\b/.test(text)) return "auth";
  if (/\b(429|quota|budget_exceeded|budget|rate_limit_exceeded|rate_limit|usage_limit_reached|usage_limit|usage limit|rate limit|insufficient_quota)\b/.test(text)) return "budget";
  if (kind === "provider_timeout" || /\b(econnrefused|enotfound|etimedout|econnreset|provider_unreachable|network_error|network error|could not be reached|unreachable|timed out|timeout|fetch failed)\b/.test(text)) return "unreachable";
  return "generic";
}
