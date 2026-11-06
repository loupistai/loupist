# Model providers

Provider accounts, Auto, and the shared World-processing model are host state. The renderer does not see tokens.

### Shared providers and World processing

Settings › Models & providers owns shared sign-ins, keys and the model catalog.
Its Add a provider sheet and onboarding share one list of ways to connect
(`components/settings/ProviderWays.tsx`); the sheet adds Pi's other key providers
under More providers. Connect and add stay at the bottom of the sheet while the
fields scroll, so a short window does not leave the action below the sheet. Each Loupist saves its own World model, reasoning controls
and processing bounds independently of Chat.

One `openai` slot holds either a ChatGPT sign-in (Pi's `openai-chatgpt` OAuth with
its dynamic client; the installation ID stays in Pi's private settings) or an
OpenAI API key, never both. Loupist passes `agentName: Loupist`, which Pi 1.5.0
sends as `agent_name_hint` (Pi 1.0.1 ignored it and sent `Pi`). As a fallback the
page it opens still rewrites `agent_name_hint` and the development Codex browser
login's `originator` to `Loupist` (`openai_sign_in_presented_as_loupist`). Other query parameters are
left unchanged. An account already connected under Pi keeps that name until the
user signs in again. xAI's device login posts `referrer=pi` to
`https://auth.x.ai/oauth2/device/code`; Loupist rewrites that field to `Loupist`
(`withLoupistSignInIdentity`) and leaves the token request unchanged. Claude
sign-in keeps Anthropic's published client id; replacing it would drop the
sign-in. API-key providers send no sign-in name.
Claude sign-in is billed as Claude extra usage and says
so before its browser opens; xAI signs in with a SuperGrok device code. Gemini,
OpenRouter and Pi's other providers connect by API key only. A sign-in times out
after 10 minutes. The legacy Codex client (`openai-codex`) exists only in debug
builds, for the e2e harness and evaluations.

Rust owns each provider's status (`providers/status.rs`): one row per provider in
global `catalog.sqlite3`, sent by the `provider-status` event and in the catalog
view. A card reads Connected, Needs reconnect or Disconnected, with at most one
notice: `plan_limit`, `plan_not_eligible`, `extra_usage_off` or `out_of_credit`.
Connecting clears the notice and dates the connection. A request that fails with
one of those kinds sets its notice and `needs_reconnect` sets Needs reconnect;
rate limits, outages and a gone model change nothing; any request that goes
through clears both. Disconnecting deletes the row unless schedules, the Chat
default or a World model still use the account; a Disconnected card then says so
until Remove.

While a provider needs reconnect or carries a notice, World processing and method
learning wait on it. Background inference records its outcomes too: Pi passes the
provider's error text (at most 8,192 characters) to Rust only for this
classification, and a completed call clears the hold. For a plan limit, extra usage
off or out of credit, one background attempt per 30-minute window
(`PROBE_WINDOW_MS`, claimed once in `provider_probe`) lets an account that
recovered on its own clear; a reconnect or an ineligible plan waits for the user.

Auto is resolved in Rust (`providers/auto.rs`) at each Chat or scheduled
admission: an explicit model that is still offered runs; an empty choice, or one
that left the catalog, uses Auto. Connected providers rank by connection date,
earliest first and Needs reconnect last; notices are ignored, so Auto never moves to another paid
account because one hit its limit. Each provider in turn offers its first visible
model from [`chat-model-defaults.json`](../../apps/loupist/chat-model-defaults.json)
(bundled at build time) at its listed thinking level, otherwise its newest
reasoning model with at least 20,000 tokens of context; failing all, any visible
connected model. With nothing connected, admission fails with a pointer to
Settings. An empty choice keeps the requested reasoning and run options only where
the resolved model supports them. Rust remembers each provider's last Auto model;
when Auto changes because that model is gone, or an explicit Chat default leaves
the catalog, a line above the composer says so once (`model_notice_ack`).

A ChatGPT sign-in lists the models its plan offers (`GET /v1/models`, redirects
refused). The list reader retains at most one MiB of response bytes and cancels
the stream as soon as the total exceeds that limit. A failed call, an oversized or
malformed response, or a list sharing no ID with Pi's catalog keeps Pi's catalog,
which every other provider uses. A 404 `model_not_found` puts the model
on a persistent gone list (`provider_gone_model`); refreshes leave it out until
the provider is connected again.

Use a model on this Mac (`providers/local.rs`) probes Ollama, LM Studio and
llama.cpp on `127.0.0.1` at their default ports in parallel, 600 ms each, without
proxy or redirects. It reads model lists and serving context (GET requests, plus
Ollama's `POST /api/show`) and saves the chosen model, when its known context is at
least 20,000 tokens, as an OpenAI Chat Completions custom endpoint with a keyless
placeholder. The sheet's engine heading is a `fieldset.connect-engine` legend with `display: block` and a full width. Grid or flex on that fieldset collapses the heading to one character in WKWebView, so the title runs down the sheet one glyph per line.

A custom endpoint is a server that speaks OpenAI Responses, OpenAI Chat Completions
or Anthropic Messages (Ollama, LM Studio, vLLM, a gateway), with a name, address,
optional key and model IDs. Test (`provider_custom_test`) makes one `GET` of the
models list (8 s, redirects off) and sends a key only to the entered address, for
that call only. Rust validates the entry (https, or plain http only on localhost;
no credentials, query or fragment in the address; up to 20 providers and 20 model
IDs) and writes a `custom-` entry to Pi's `models.json` in the private agent
directory. That entry never holds a key, `$NAME` or `!command`. The key goes to
Pi's credential store through the same support command as other API-key
providers; a keyless server gets a placeholder. An edit keeps the saved key unless
a new one is entered; changing the address drops it. Removing signs it out,
deletes the entry and drops its models. Upstage Solar is a preset: Rust tests the
key against Upstage's fixed Chat Completions address and saves its `solar` chat
models as a custom endpoint. Custom providers serve Chat and briefings; World
processing needs isolated inference and does not accept them.
When Test gets a missing-path response, the sheet suggests `/v1` only for a
server-root address; an address that already has a path gets guidance to check
the address and selected API format.

At startup `providers/unknown.rs` deletes state that names a provider this build
does not offer (release builds drop `openai-codex`): catalog models, status rows
and gone-model entries. A global Chat default naming one is unset, schedules that
named one use Auto from their next run, and a World model on one is forgotten, so
that Loupist takes the World default as a new one does. This is deletion, not migration; stored
answers keep their model record.

World extraction builds each bounded prompt/schema in Rust and launches isolated
Pi inference without Chat history, Skills, workspace instructions or tools. Returned JSON is locally validated; uncertain sent calls
are not automatically retried. Method-learning reviews use this same isolated
call, one bounded request at a time, and are not repeated after an uncertain
attempt. Embeddings remain local. See
[World processing](../knowledge/worlds.md#worlds-and-graph-processing) for defaults,
publication and recovery.

## Code

- Pi startup / provider failures: `src-tauri/src/pi.rs`, `src-tauri/src/pi/`, `provider_failure.rs`
- Model providers / Auto: `src-tauri/src/providers/`, `chat-model-defaults.json`, `src/components/settings/`, `src/components/onboarding/ConnectStep.tsx`
