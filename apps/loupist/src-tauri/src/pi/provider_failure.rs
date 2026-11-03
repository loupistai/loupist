//! Typed provider failures for the Chat (X11, providers spec §3.1).
//!
//! Pi reports a failed provider turn with the provider's own error text. That text can
//! carry response bodies, account details or request identifiers, so it never leaves
//! this module: a bounded, case-insensitive look at it picks one static kind, and only
//! the kind's fixed host message travels on. For a busy answer, a provider's
//! retry hint travels on as a bounded number of seconds.
use crate::domain::MAX_RETRY_AFTER_SECONDS;
use serde_json::Value;

/// What went wrong with the provider, as far as the host can tell without keeping its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFailure {
    /// A ChatGPT (or other subscription) plan reached its shared usage limit.
    /// OpenAI's contract: pause, never retry automatically.
    PlanLimit,
    /// The plan cannot be used in other apps (workspace turned it off).
    PlanNotEligible,
    /// Claude sign-in: extra usage is off or used up.
    ExtraUsageOff,
    /// A key account ran out of credit or quota.
    OutOfCredit,
    /// The selected model is no longer offered; nothing was processed.
    ModelGone,
    /// Short-term rate limits on key and local accounts; trying again later may work.
    RateLimited,
    /// The sign-in or key was rejected, revoked or could not refresh; the user must reconnect.
    NeedsReconnect,
    /// The provider could not be reached.
    Network,
    /// The provider answered with a server error or said it is overloaded.
    Unavailable,
    /// The conversation no longer fits the model's context window.
    ContextTooLong,
    /// Anything else.
    Other,
}

/// The host message each kind travels as; `chat_runtime` turns these into `ChatFailureKind`.
pub const PLAN_LIMIT: &str = "Pi provider turn failed: plan limit";
pub const PLAN_NOT_ELIGIBLE: &str = "Pi provider turn failed: plan not eligible";
pub const EXTRA_USAGE_OFF: &str = "Pi provider turn failed: extra usage off";
pub const OUT_OF_CREDIT: &str = "Pi provider turn failed: out of credit";
pub const MODEL_GONE: &str = "Pi provider turn failed: model gone";
pub const RATE_LIMITED: &str = "Pi provider turn failed: rate limited";
pub const NEEDS_RECONNECT: &str = "Pi provider turn failed: needs reconnect";
pub const NETWORK: &str = "Pi provider turn failed: network";
pub const UNAVAILABLE: &str = "Pi provider turn failed: provider unavailable";
pub const CONTEXT_TOO_LONG: &str = "Pi provider turn failed: context too long";
pub const OTHER: &str = "Pi provider turn failed";

/// Inspect at most this much provider text.
const MAX_INSPECTED_CHARS: usize = 8192;

impl ProviderFailure {
    pub fn classify(message: &str) -> Self {
        let message: String = message.chars().take(MAX_INSPECTED_CHARS).collect();
        let message = message.to_ascii_lowercase();
        let any = |patterns: &[&str]| patterns.iter().any(|pattern| message.contains(pattern));
        let status = |code: &str| {
            message.starts_with(&format!("{code} "))
                || message.starts_with(&format!("{code}:"))
                || message.contains(&format!("http {code}"))
                || message.contains(&format!("status {code}"))
                || message.contains(&format!("status: {code}"))
                || message.contains(&format!("status code {code}"))
                || message.contains(&format!("({code})"))
        };
        // Exact plan and account codes first: the generic 429/403 patterns below
        // would otherwise send a plan limit into the busy auto-retry, or a plan that
        // cannot be shared into "reconnect".
        if any(&[
            "subscription_sharing_usage_limit_exceeded",
            "usage_limit_reached",
            "usage_not_included",
        ]) {
            Self::PlanLimit
        } else if any(&["subscription_sharing_user_not_eligible"]) {
            Self::PlanNotEligible
        } else if any(&[
            "subscription_sharing_usage_unavailable",
            "subscription_sharing_user_unavailable",
        ]) {
            Self::Unavailable
        } else if any(&[
            "out of extra usage",
            "draw from your extra usage",
            "extra usage is disabled",
            "extra usage is not enabled",
            "enable extra usage",
        ]) {
            Self::ExtraUsageOff
        } else if any(&["model_not_found"])
            || (message.contains("not_found_error") && message.contains("model"))
        {
            Self::ModelGone
        } else if any(&[
            "insufficient_quota",
            "exceeded your current quota",
            "credit balance is too low",
            "insufficient credits",
            "insufficient_credits",
            "out of credits",
            "billing_hard_limit",
            "billing hard limit",
            "payment required",
        ]) || status("402")
        {
            Self::OutOfCredit
        } else if any(&[
            // Context limits: some providers send them as 400s that mention tokens.
            "context_length_exceeded",
            "maximum context length",
            "context window",
            "prompt is too long",
            "too many tokens",
            "input is too long",
            "request too large",
        ]) {
            Self::ContextTooLong
        } else if status("429") || any(&["rate limit", "rate_limit", "too many requests"]) {
            Self::RateLimited
        } else if status("401")
            || status("403")
            || any(&[
                "unauthorized",
                "invalid api key",
                "incorrect api key",
                "invalid token",
                "expired token",
                "token has expired",
                "token has been revoked",
                "revoked",
                "invalid_grant",
                "reconnect chatgpt",
                "authentication",
                "not logged in",
                "no api key",
                "missing api key",
                "refresh token",
                "token refresh",
            ])
        {
            Self::NeedsReconnect
        } else if status("500")
            || status("502")
            || status("503")
            || status("504")
            || status("529")
            || any(&[
                "overloaded",
                "service unavailable",
                "internal server error",
                "bad gateway",
                "server_error",
            ])
        {
            Self::Unavailable
        } else if any(&[
            "fetch failed",
            "network error",
            "network request failed",
            "connection error",
            "connection refused",
            "connection reset",
            "econnreset",
            "econnrefused",
            "enotfound",
            "eai_again",
            "getaddrinfo",
            "etimedout",
            "socket hang up",
            "offline",
        ]) {
            Self::Network
        } else {
            Self::Other
        }
    }

    pub fn host_message(self) -> &'static str {
        match self {
            Self::PlanLimit => PLAN_LIMIT,
            Self::PlanNotEligible => PLAN_NOT_ELIGIBLE,
            Self::ExtraUsageOff => EXTRA_USAGE_OFF,
            Self::OutOfCredit => OUT_OF_CREDIT,
            Self::ModelGone => MODEL_GONE,
            Self::RateLimited => RATE_LIMITED,
            Self::NeedsReconnect => NEEDS_RECONNECT,
            Self::Network => NETWORK,
            Self::Unavailable => UNAVAILABLE,
            Self::ContextTooLong => CONTEXT_TOO_LONG,
            Self::Other => OTHER,
        }
    }
}

/// Response headers the Pi extension forwards after a 429 or 503 response:
/// timing hints only, lower-case, each value at most 128 bytes.
pub const RETRY_HEADERS: &[&str] = &[
    "retry-after",
    "retry-after-ms",
    "x-ratelimit-reset-requests",
    "x-ratelimit-reset-tokens",
    "anthropic-ratelimit-requests-reset",
    "anthropic-ratelimit-tokens-reset",
    "anthropic-ratelimit-input-tokens-reset",
    "anthropic-ratelimit-output-tokens-reset",
];
const MAX_HINT_BYTES: usize = 128;
/// JSON nodes of an error body looked at for a retry hint.
const MAX_BODY_NODES: usize = 256;

/// How long a busy provider asked to wait before the next try, in whole
/// seconds from 1 to [`MAX_RETRY_AFTER_SECONDS`]. `Retry-After` (seconds or an
/// HTTP date) and OpenAI's `retry-after-ms` win; otherwise the latest named
/// reset (OpenAI's `x-ratelimit-reset-*` durations, Anthropic's
/// `anthropic-ratelimit-*-reset` times), then a hint in the error text (a body
/// field such as Gemini's `retryDelay`, or "try again in 6.5s"). Only the
/// number leaves this module.
pub fn retry_after_seconds(headers: &Value, message: &str, now_ms: i64) -> Option<u32> {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(Value::as_str)
            .filter(|value| value.len() <= MAX_HINT_BYTES)
    };
    let seconds = header("retry-after-ms")
        .and_then(|value| value.trim().parse::<f64>().ok())
        .map(|ms| ms / 1000.0)
        .or_else(|| {
            header("retry-after").and_then(|value| duration(value).or_else(|| until(value, now_ms)))
        })
        .or_else(|| {
            RETRY_HEADERS[2..]
                .iter()
                .filter_map(|name| header(name))
                .filter_map(|value| duration(value).or_else(|| until(value, now_ms)))
                .reduce(f64::max)
        })
        .or_else(|| {
            let message: String = message.chars().take(MAX_INSPECTED_CHARS).collect();
            body_hint(&message).or_else(|| text_hint(&message))
        })?;
    (seconds.is_finite() && seconds >= 0.0).then(|| {
        seconds
            .ceil()
            .clamp(1.0, f64::from(MAX_RETRY_AFTER_SECONDS)) as u32
    })
}

/// Seconds until an HTTP date or RFC 3339 time; zero when it has passed.
fn until(value: &str, now_ms: i64) -> Option<f64> {
    let value = value.trim();
    let at = chrono::DateTime::parse_from_rfc2822(value)
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(value))
        .ok()?;
    Some(at.timestamp_millis().saturating_sub(now_ms).max(0) as f64 / 1000.0)
}

/// A whole duration: "20", "6.5s", "1m30s", "120ms" or "2 seconds".
fn duration(value: &str) -> Option<f64> {
    let (seconds, rest) = leading_duration(value)?;
    rest.trim().is_empty().then_some(seconds)
}

/// The duration a text starts with, and what follows it ("6.5s. Visit …").
fn leading_duration(value: &str) -> Option<(f64, &str)> {
    let mut rest = value.trim_start();
    let mut total = 0.0;
    let mut parts = 0;
    while parts < 4 {
        let digits = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let Ok(amount) = rest[..digits].trim_end_matches('.').parse::<f64>() else {
            break;
        };
        let after = rest[digits..].trim_start();
        let unit_end = after
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(after.len());
        let scale = match after[..unit_end].to_ascii_lowercase().as_str() {
            "ms" | "msec" | "milliseconds" => 0.001,
            "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
            "m" | "min" | "mins" | "minute" | "minutes" => 60.0,
            "h" | "hr" | "hour" | "hours" => 3600.0,
            // A bare number is seconds, as Retry-After writes it.
            "" if parts == 0 => 1.0,
            _ => break,
        };
        total += amount * scale;
        parts += 1;
        rest = after[unit_end..].trim_start();
    }
    (parts > 0).then_some((total, rest))
}

/// A retry field in a JSON error body: `retry_after` or `retryAfter` (seconds),
/// `retry_after_ms` (milliseconds) or `retryDelay` ("20s").
fn body_hint(message: &str) -> Option<f64> {
    fn visit(value: &Value, left: &mut usize) -> Option<f64> {
        if *left == 0 {
            return None;
        }
        *left -= 1;
        match value {
            Value::Object(fields) => fields.iter().find_map(|(key, value)| {
                let text = match value {
                    Value::String(text) if text.len() <= MAX_HINT_BYTES => Some(text.clone()),
                    Value::Number(number) => Some(number.to_string()),
                    _ => None,
                };
                let hint = text.and_then(|text| match key.as_str() {
                    "retry_after" | "retryAfter" | "retry_after_seconds" | "retryDelay" => {
                        duration(&text)
                    }
                    "retry_after_ms" | "retryAfterMs" => {
                        text.parse::<f64>().ok().map(|ms| ms / 1000.0)
                    }
                    _ => None,
                });
                hint.or_else(|| visit(value, left))
            }),
            Value::Array(items) => items.iter().find_map(|item| visit(item, left)),
            _ => None,
        }
    }
    let start = message.find('{')?;
    let end = message.rfind('}')?;
    let body = serde_json::from_str::<Value>(message.get(start..=end)?).ok()?;
    let mut left = MAX_BODY_NODES;
    visit(&body, &mut left)
}

/// "try again in 6.5s" (OpenAI) or "retry in 20.3s" (Gemini) in plain text.
fn text_hint(message: &str) -> Option<f64> {
    let lower = message.to_ascii_lowercase();
    ["try again in ", "retry in ", "retry after "]
        .iter()
        .find_map(|phrase| {
            let at = lower.find(phrase)? + phrase.len();
            leading_duration(&lower[at..]).map(|(seconds, _)| seconds)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_text_maps_to_one_kind_and_never_travels() {
        let cases = [
            // Sign in with ChatGPT (SIWC) on api.openai.com.
            (
                "429 OpenAI API error: {\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\",\"message\":\"Rate limit for org-abc123\"}}\nCheck your ChatGPT usage: https://chatgpt.com/settings/usage",
                ProviderFailure::PlanLimit,
            ),
            (
                "403 {\"error\":{\"code\":\"subscription_sharing_user_not_eligible\",\"message\":\"not allowed\"}}",
                ProviderFailure::PlanNotEligible,
            ),
            (
                "503 {\"error\":{\"code\":\"subscription_sharing_usage_unavailable\"}}",
                ProviderFailure::Unavailable,
            ),
            (
                "OpenAI OAuth token request failed (400): {\"error\":\"invalid_grant\"}",
                ProviderFailure::NeedsReconnect,
            ),
            (
                "Stored OpenAI OAuth credential does not contain an issued client ID; reconnect ChatGPT",
                ProviderFailure::NeedsReconnect,
            ),
            (
                "404 {\"error\":{\"code\":\"model_not_found\",\"message\":\"The model gpt-x does not exist\"}}",
                ProviderFailure::ModelGone,
            ),
            // Legacy Codex client.
            (
                "429 {\"error\":{\"type\":\"usage_limit_reached\",\"plan_type\":\"plus\"}}",
                ProviderFailure::PlanLimit,
            ),
            (
                "usage_not_included: this plan does not include Codex",
                ProviderFailure::PlanLimit,
            ),
            // Anthropic.
            (
                "400 {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"You're out of extra usage. Add more at claude.ai/settings/usage\"}}",
                ProviderFailure::ExtraUsageOff,
            ),
            (
                "400 Third-party apps now draw from your extra usage, not your plan limits.",
                ProviderFailure::ExtraUsageOff,
            ),
            (
                "401 {\"type\":\"error\",\"error\":{\"type\":\"authentication_error\",\"message\":\"OAuth token has been revoked.\"}}",
                ProviderFailure::NeedsReconnect,
            ),
            (
                "404 {\"type\":\"error\",\"error\":{\"type\":\"not_found_error\",\"message\":\"model: claude-old\"}}",
                ProviderFailure::ModelGone,
            ),
            (
                "400 Your credit balance is too low to access the Anthropic API.",
                ProviderFailure::OutOfCredit,
            ),
            // Key accounts and transport.
            (
                "429 Too Many Requests: {\"error\":{\"message\":\"Rate limit reached in org-abc123\"}}",
                ProviderFailure::RateLimited,
            ),
            (
                "You exceeded your current quota, please check your plan and billing details.",
                ProviderFailure::OutOfCredit,
            ),
            (
                "429 {\"error\":{\"code\":\"insufficient_quota\"}}",
                ProviderFailure::OutOfCredit,
            ),
            (
                "401 Unauthorized: invalid api key sk-live-SECRET",
                ProviderFailure::NeedsReconnect,
            ),
            (
                "Your refresh token has expired; sign in again",
                ProviderFailure::NeedsReconnect,
            ),
            (
                "fetch failed: getaddrinfo ENOTFOUND api.example.com",
                ProviderFailure::Network,
            ),
            ("503 Service Unavailable", ProviderFailure::Unavailable),
            ("Anthropic API is overloaded", ProviderFailure::Unavailable),
            (
                "This model's maximum context length is 128000 tokens",
                ProviderFailure::ContextTooLong,
            ),
            ("something odd happened", ProviderFailure::Other),
        ];
        for (text, expected) in cases {
            let kind = ProviderFailure::classify(text);
            assert_eq!(kind, expected, "{text}");
            let message = kind.host_message();
            // Only the fixed host message leaves: no key, organisation, host or body.
            for secret in [
                "sk-live-SECRET",
                "org-abc123",
                "api.example.com",
                "{\"error\"",
                "chatgpt.com",
                "claude-old",
            ] {
                assert!(!message.contains(secret));
            }
            assert!(message.starts_with(OTHER));
        }
    }

    #[test]
    fn plan_codes_win_over_generic_status_patterns() {
        // A plan limit is a 429 and a plan refusal a 403, but neither may become
        // the auto-retrying busy state or a reconnect.
        assert_eq!(
            ProviderFailure::classify(
                "429 too many requests subscription_sharing_usage_limit_exceeded"
            ),
            ProviderFailure::PlanLimit
        );
        assert_eq!(
            ProviderFailure::classify("403 forbidden subscription_sharing_user_not_eligible"),
            ProviderFailure::PlanNotEligible
        );
        assert_eq!(
            ProviderFailure::classify("429 rate limit: billing details"),
            ProviderFailure::RateLimited
        );
    }

    #[test]
    fn host_messages_are_distinct() {
        let kinds = [
            ProviderFailure::PlanLimit,
            ProviderFailure::PlanNotEligible,
            ProviderFailure::ExtraUsageOff,
            ProviderFailure::OutOfCredit,
            ProviderFailure::ModelGone,
            ProviderFailure::RateLimited,
            ProviderFailure::NeedsReconnect,
            ProviderFailure::Network,
            ProviderFailure::Unavailable,
            ProviderFailure::ContextTooLong,
            ProviderFailure::Other,
        ];
        let unique = kinds
            .iter()
            .map(|kind| kind.host_message())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), kinds.len());
    }

    #[test]
    fn inspection_is_bounded() {
        let long = format!("{}429", "x".repeat(MAX_INSPECTED_CHARS + 10));
        assert_eq!(ProviderFailure::classify(&long), ProviderFailure::Other);
    }

    /// A busy provider's own retry hint replaces the fixed 20/40 s wait (B83).
    #[test]
    fn retry_hints_come_from_headers_then_the_error_and_stay_bounded() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-01T12:00:00Z")
            .unwrap()
            .timestamp_millis();
        let wait = |headers: Value, message: &str| retry_after_seconds(&headers, message, now);
        let none = serde_json::json!({});
        // Retry-After in seconds or as an HTTP date; OpenAI's milliseconds win.
        assert_eq!(wait(serde_json::json!({"retry-after": "7"}), ""), Some(7));
        assert_eq!(
            wait(
                serde_json::json!({"retry-after": "Sun, 01 Nov 2026 12:00:30 GMT"}),
                ""
            ),
            Some(30)
        );
        assert_eq!(
            wait(
                serde_json::json!({"retry-after": "7", "retry-after-ms": "1500"}),
                ""
            ),
            Some(2)
        );
        // Named resets: OpenAI durations and Anthropic times; the latest one.
        assert_eq!(
            wait(
                serde_json::json!({"x-ratelimit-reset-requests": "1s", "x-ratelimit-reset-tokens": "6m0s"}),
                ""
            ),
            Some(MAX_RETRY_AFTER_SECONDS)
        );
        assert_eq!(
            wait(serde_json::json!({"x-ratelimit-reset-tokens": "1m30s"}), ""),
            Some(90)
        );
        assert_eq!(
            wait(
                serde_json::json!({"anthropic-ratelimit-tokens-reset": "2026-11-01T12:00:12Z"}),
                ""
            ),
            Some(12)
        );
        // In the error: a body field (Gemini's retryDelay) or plain words.
        assert_eq!(
            wait(
                none.clone(),
                r#"429 {"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"20s"}]}}"#
            ),
            Some(20)
        );
        assert_eq!(
            wait(
                none.clone(),
                "429 Rate limit reached for gpt-4o. Please try again in 6.5s. Visit https://platform.openai.com"
            ),
            Some(7)
        );
        // Past, absent, malformed or oversized hints: nothing, or at least a second.
        assert_eq!(
            wait(
                serde_json::json!({"retry-after": "Sun, 01 Nov 2026 11:00:00 GMT"}),
                ""
            ),
            Some(1)
        );
        assert_eq!(wait(none.clone(), "429 Too Many Requests"), None);
        assert_eq!(wait(serde_json::json!({"retry-after": "soon"}), ""), None);
        assert_eq!(
            wait(serde_json::json!({"retry-after": "9".repeat(200)}), ""),
            None
        );
        assert_eq!(
            wait(serde_json::json!({"retry-after": "86400"}), ""),
            Some(MAX_RETRY_AFTER_SECONDS)
        );
    }
}
