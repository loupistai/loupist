# Model providers

Provider accounts, Auto, and the shared World-processing model are host state. The renderer does not see tokens.

## Provider manifest

[`provider-manifest.json`](../../apps/loupist/provider-manifest.json) is the one
table of built-in providers, bundled at build time. Each entry, in the order
Settings lists providers and their models (also Auto's order for providers with
no connection date), holds the labels, the Settings group, the key page, the API
key and its host variable (`api_key`), the sign-in (`sign_in`: label, flow, the
one authorization page Rust opens, the paste rule, billing note, usage page,
live plan models), isolated-inference support, native search (`automatic`, or
`model_only` for xAI), the product names and the featured key-row position.
`development_only` entries (the Codex client) exist only in debug builds.
Presets (Upstage Solar) carry their fixed address, API format, key page and row.
Rust (`providers/manifest.rs`) validates the file at first use and derives the
catalog options, the credential-variable allowlist and every capability check
from it; capability checks also read a debug-only entry, so a stored record on
that client keeps its meaning. The renderer (`src/chat/providerManifest.ts`)
reads the featured sign-ins, key rows and product names from it; whether a row
can connect still comes from the catalog. `agent/provider-manifest.test.mjs`
holds the file to the pinned Pi catalog: each provider exists, its host variable
is the one Pi reads, a sign-in or key matches Pi's auth, isolated inference has
a guarded API, and the native-search adapters are exactly the `native_search`
providers. `support_bootstrap` is the startup model of every provider support
launch (`gemini-2.5-flash` on `google`); the extension answers each support
command before any model request, so it is never called, and its key variable
gets a non-secret placeholder.

Adding a featured provider is one manifest entry. The remaining per-provider
code is a native-search adapter (registered in `agent/loupist-native-search.mjs`
and listed in `agent_harness.rs`, which a test requires), localized row copy
(without it a row shows the manifest's label or name) and any provider-specific
sign-in code in the provider extension.

## Shared providers and World processing

Settings › Models & providers owns shared sign-ins, keys and the model catalog; it is the first Settings destination, followed by Loupists, Connections, Chat defaults and Skills, then General (menu-bar residence, the AI notice and a privacy summary), Appearance, Keyboard and Updates. The global Chat default model's empty choice reads "Auto · recommended model": Auto picks a recommended model at admission and never follows the last answer's model. Response speed (Standard or Fast) sits under Advanced and says Fast may cost more on API keys.
Its Add a provider sheet and onboarding share one list of ways to connect
(`components/settings/ProviderWays.tsx`, featured rows from the provider
manifest); the sheet adds Pi's other key providers
under More providers. Connect and add stay at the bottom of the sheet while the
fields scroll, so a short window does not leave the action below the sheet. Each Loupist saves its own World model, reasoning controls
and processing bounds independently of Chat.

One `openai` slot holds either a ChatGPT sign-in (Pi's `openai-chatgpt` OAuth with
its dynamic client; the installation ID stays in Pi's private settings) or an
OpenAI API key, never both. Loupist passes `agentName: Loupist`. Pi puts it on the
sign-in page as `agent_name_hint`, and on the development Codex browser login as
`originator`. The host opens and stores that page exactly as Pi wrote it. An account
connected while Pi sent its own name keeps that name until the user signs in again. xAI's device login posts `referrer=pi` to
`https://auth.x.ai/oauth2/device/code`; Loupist rewrites that field to `Loupist`
(`withLoupistSignInIdentity`) and leaves the token request unchanged. API-key providers send no sign-in name.
xAI signs in with a SuperGrok device code. Claude, Gemini, OpenRouter and Pi's
other providers connect by API key only. A sign-in times out after 10 minutes. The legacy Codex client (`openai-codex`) exists only in debug
builds, for the e2e harness and evaluations.

Claude connects only by Anthropic API key, because Anthropic does not permit
third-party apps to offer Claude.ai sign-in
([terms](https://code.claude.com/docs/en/legal-and-compliance)). The `anthropic`
catalog entry has no sign-in flow, and `provider_connect` refuses it. At startup
Rust deletes a Claude sign-in grant an earlier build saved in Pi's `auth.json`
(`providers/unknown.rs`) and treats it as a disconnect: the card reads
Disconnected while schedules, the Chat default or a World model still use the
account, and an Anthropic API key is left alone.

Rust owns each provider's status (`providers/status.rs`): one row per provider in
global `catalog.sqlite3`, sent by the `provider-status` event and in the catalog
view. A card reads Connected, Needs reconnect or Disconnected, with at most one
notice: `plan_limit`, `plan_not_eligible` or `out_of_credit`.
Connecting clears the notice and dates the connection. A request that fails with
one of those kinds sets its notice and `needs_reconnect` sets Needs reconnect;
rate limits, outages and a gone model change nothing; any request that goes
through clears both. Disconnecting deletes the row unless schedules, the Chat
default or a World model still use the account; a Disconnected card then says so
until Remove. Rust runs one provider support operation at a time (sign-in, key
change or model discovery) and refuses a second with `conflict`. A sign-in or key
returns the provider's models itself, so Settings starts no model load while the
connect sheet is open, and a card that cannot load its models shows localized text
(a busy load reads "Another account change is still finishing") with Reload models,
never the backend's English error.

While a provider needs reconnect or carries a notice, World processing and method
learning wait on it. Background inference records its outcomes too: Pi passes the
provider's error text (at most 8,192 characters) to Rust only for this
classification, and a completed call clears the hold. For a plan limit or out of
credit, one background attempt per 30-minute window
(`PROBE_WINDOW_MS`, claimed once in `provider_probe`) lets an account that
recovered on its own clear; a reconnect or an ineligible plan waits for the user.

The composer’s Auto tooltip describes admission-time resolution rather than displaying the last answer’s model. Its explicit per-Loupist message override is session state; a saved Chat default remains host state.

Auto is resolved in Rust (`providers/auto.rs`) at each Chat or scheduled
admission: an explicit model that is still offered runs; an empty choice, or one
that left the catalog, uses Auto. Connected providers rank by connection date,
earliest first and Needs reconnect last; notices are ignored, so Auto never moves to another paid
account because one hit its limit. Each provider in turn offers its first visible
model from [`chat-model-defaults.json`](../../apps/loupist/chat-model-defaults.json)
(bundled at build time) at its listed thinking level, otherwise its newest
reasoning model with at least 20,000 tokens of context; failing all, any visible
connected model. "Newest" is the catalog order: each provider's highest numeric
version first, then, within one version, the higher family tier that file
declares in `family_tiers` (3 for Opus, Pro or Sol, 1 for Haiku, Mini or Luna). A
family matches a whole word of the model's name or ID, the highest declared tier
wins (`flash-lite` is Flash), and an undeclared family takes the `unknown` tier,
2. A new family is ranked by adding it there. With nothing connected, admission fails with a pointer to
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
- Model providers / Auto: `src-tauri/src/providers/`, `provider-manifest.json`, `chat-model-defaults.json`, `src/chat/providerManifest.ts`, `src/components/settings/`, `src/components/onboarding/ConnectStep.tsx`
