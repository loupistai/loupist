# Answer failures

Provider failures are typed in Rust. No provider text reaches the renderer. Account and plan limits are not retried as ordinary rate limits.

Provider failures are classified in Rust (`pi/provider_failure.rs`; at most 8,192
characters inspected, then dropped) into typed kinds: `plan_limit`,
`plan_not_eligible`, `extra_usage_off`, `out_of_credit`, `model_gone`,
`needs_reconnect`, `rate_limited`, `offline`, `provider_unavailable` and
`context_too_long`. Exact plan and account codes match before the generic 429 and
403 patterns, so a plan limit never enters the busy retry and an ineligible plan
never reads as a sign-in problem. A stored kind this build does not know reads as
`other`. No provider text reaches the renderer. A busy key or local account counts down
and retries by itself twice, then says "try again later". The wait is the provider's own
hint when it sent one (`retry-after-ms`, then `Retry-After`, then the latest OpenAI
`x-ratelimit-reset-*` or Anthropic `anthropic-ratelimit-*-reset`, then an error-body
hint such as Gemini's `retryDelay` or "try again in 6.5s"), read in
`pi/provider_failure.rs` from the headers the Pi extension forwards for 429 and 503
responses, bounded to 1–120 s and stored as the answer's `retry_after_seconds`;
otherwise 20 s, then 40 s. A plan limit
is never retried automatically; it offers Manage usage, Try again and Use another
account. An ineligible plan offers an API key, extra usage off opens Claude
settings, needs reconnect offers Reconnect, and a context that is too long has no
retry. The account kinds also set the [provider status](../runtime/providers.md#shared-providers-and-world-processing).
A `model_gone` failure (404 `model_not_found`) before any text, tool call or
accepted steer prunes the model and asks once more on Auto, in the same turn on a
fresh cold Pi (`commands/chat_runtime/model_gone.rs`); the answer stores
`replaced_model` and names the model that answered instead. While offline a banner
shows and sent messages wait in the ordinary queue, sending on reconnect. Otherwise, when a data read in
the latest answer failed with the tool error `offline` (the completed message's
`offline_reads`), the banner says some data couldn't be fetched because a source
or the network couldn't be reached.
