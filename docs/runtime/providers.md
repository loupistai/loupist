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
launch (`gemini-3.5-flash-lite` on `google`); the extension answers each support
command before any model request, so it is never called, and its key variable
gets a non-secret placeholder.

Adding a featured provider is one manifest entry. The remaining per-provider
code is a native-search adapter (registered in `agent/loupist-native-search.mjs`
and listed in `agent_harness.rs`, which a test requires), localized row copy
(without it a row shows the manifest's label or name) and any provider-specific
sign-in code in the provider extension.

## Shared providers and World processing

Settings › AI accounts owns shared sign-ins, keys and the model catalog; it is the first Settings destination, followed by Loupists, Data connections, Chat defaults and Skills, then General (menu-bar residence, the AI notice and a privacy summary), Appearance, Keyboard and Updates. An unset Chat model follows the host’s recommended model. The catalog and scoped Settings snapshots identify the concrete choice before a turn starts. Response speed (Standard or Fast) sits under Advanced and says Fast may cost more on API keys.
Its Add a provider sheet and onboarding share one list of ways to connect
(`components/settings/ProviderWays.tsx`, featured rows from the provider
manifest); the sheet adds Pi's other key providers
under More providers. Connect and add stay at the bottom of the sheet while the
fields scroll, so a short window does not leave the action below the sheet. Each Loupist saves its own World settings and
processing bounds. Its World model follows the connected accounts, trying its Chat
default's provider first, until the user chooses one
([World building model](../knowledge/world-search.md#world-building-model)).

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
Disconnected while schedules, the Chat default or a chosen World model still use the
account, and an Anthropic API key is left alone.

Rust owns each provider's status (`providers/status.rs`): one row per provider in
global `catalog.sqlite3`, sent by the `provider-status` event and in the catalog
view. A card reads Connected, Needs reconnect or Disconnected, with at most one
notice: `plan_limit`, `plan_not_eligible` or `out_of_credit`. Its heading names the
account (ChatGPT, SuperGrok, Anthropic) and the line under it does not repeat the
name ("Signed in since …", "API key saved on this Mac"). Each model's In menu switch
is named "Show <model> in menu". Settings words a built-in account as disconnected
and a custom endpoint as removed, and says AI account, never provider.
Connecting clears the notice and dates the connection. A request that fails with
one of those kinds sets its notice and `needs_reconnect` sets Needs reconnect;
rate limits, outages and a gone model change nothing; any request that goes
through clears both. Disconnecting deletes the row unless schedules, the Chat
default or a chosen World model still use the account (a Recommended one moves to
another account); a Disconnected card then says so,
and that connecting again lets them run, until Remove. Rust serializes provider support operations (sign-in, key
change, model discovery and native search), with native search able to overlap
read-only upgrade discovery. Concurrent model loads from startup, Settings and the
composer queue behind the current support operation instead of reporting contention
as a model failure. A queued load rechecks the credential generation and cache after
admission, and a discovery preempted by a credential change retries against the
new account. Startup discovers every connected provider absent from the catalog,
even when another account already has usable models; intentionally hidden models
stay hidden. A sign-in or key returns the provider's models itself, so Settings
starts no model load while the connect sheet is open. A card shows a localized load
error with Reload models only after discovery fails, never the backend's English error.
Removing a custom endpoint always deletes its card and status row, even when saved model choices still name it.
Startup also removes status rows left by older builds for custom endpoints absent from `models.json`.
Once sign-in is stored, a closed renderer event channel does not fail the completed connection.
Progress and completion messages use host copy, never provider-supplied prose that might contain a credential.
ChatGPT's loopback callback shows a self-contained Loupist page in the current
interface language (English or Korean). Pi confirms receipt of the authorization
code before token exchange. The page reads “Return to Loupist” and says the
provider approved the sign-in and Loupist finishes connecting in the app. It
ends with “You can close this tab.” Settings shows whether the exchange succeeded. On success or failure after handing off to the browser, the host
brings the main window forward. Cancelling from Loupist ends quietly. SuperGrok
uses xAI’s device-code page and has no local callback page.

For one ChatGPT login, including the development Codex client, the provider
extension observes the HTTP response headers and single-chunk body. It recognizes
`text/html` with Pi’s exact document title `Authentication successful` or
`Authentication failed`, preserves the HTTP status and escapes Pi's failure
message and details. Other titles and streaming pages pass through. Replacing a
fixed-length body updates its UTF-8 byte count. The extension restores the HTTP
method when login exits. Remove this wrapper if Pi gains a callback-page option.
The release sidecar check starts both real loopback flows and submits a wrong
state offline.

The callback uses the app's native icon and the light/dark values of
`--gg-color-bg-app`, `--gg-color-text-primary`, `--gg-color-text-secondary` and
`--gg-color-text-tertiary` from `src/styles/tokens.css`. It loads no external assets.

On a rejected exchange, the extension reports the HTTP status and a
fixed OAuth error code. Rust checks the code against its allowlist before logging
it and uses `unknown` for other values. The UI offers a fresh sign-in or an API key.
An explicit eligibility error selects the plan warning. A bare `invalid_grant`
selects `chatgpt_connection_rejected` because it does not identify a plan restriction.
An OpenAI consent cancellation (`access_denied`) keeps the declined wording and offers
the same key action. A failed extension event ends the support operation immediately,
including when Pi already acknowledged the command and writes no credential result;
the waiting card clears and other connection methods remain available. Pi owns OAuth
validation and exchange. The host and its logs receive only typed failure categories.
An API-key change sends the key only over Pi's private stdin RPC answer, once,
after the extension's secret-login callback requests `ctx.ui.input` with the exact
title `loupist-provider/1:api_key`. Rust checks the request type, method, title and
bounded id before answering. Unexpected prompts receive no key; cancellation,
the 10-second input timeout or the support-operation timeout discard pending
answers. The key is absent from launch arguments, environment, transfer files,
results and diagnostics; Pi persists a successful login in its private credential
store as before. Provider exceptions during key login become fixed host text.
Provider support skips Pi's background startup catalog refresh with `--offline`.
During discovery Pi fetches the requested public catalog from
`https://pi.dev/api/models/providers/<id>` with Pi's version and platform
User-Agent. The request needs no provider credential. The catalog supplies
model IDs, capabilities and limits, and Pi caches it for four hours.
App setup starts a background rediscovery for saved sign-ins without an attempt
recorded for the bundled Pi version. The renderer can read the saved catalog while
the pass runs and reloads it when a provider's models are published. The pass
revalidates the public cache and records the bundled version per provider after
an attempt finishes, including failures. If a credential change interrupts
an attempt, it stays pending. The ChatGPT plan list in this pass uses an
unexpired saved access token. When no plan models can be matched to Pi's
catalog, the pass reports the list as unavailable, keeps the saved models and
leaves the attempt pending for a later launch or a manual refresh.
Discovery checks built-in providers' model origins, transports and request headers
against the bundled Pi routes and reports dropped rows. OpenAI, Codex and xAI
also require the exact bundled base URL. Chat, isolated inference
and Pi-backed native search enforce the same check on cached models. xAI native
search sends its API key to a fixed xAI address and refuses redirects.
Personalized catalogs can still rotate OAuth tokens, and forced host shutdown
can interrupt Pi's graceful cleanup.
Discovery logs dropped row positions without catalog content. If no usable
rows remain after filtering out gone models, discovery keeps the saved list
and returns an error to the caller, so a manual refresh in Settings shows its
load error and retry action.
Explicit OAuth, live model discovery and inference can still use the network.
Pi persists rotated OAuth credentials before returning them, and model discovery
adopts that durable generation under its support lease.
Private support request and result files are removed on every normal exit, including errors; startup removes
files left by an interrupted process before another support operation starts.

While a provider needs reconnect or carries a notice, World processing and method
learning wait on it. Background inference records its outcomes too: Pi passes the
provider's error text (at most 8,192 characters) to Rust only for this
classification, and a completed call clears the hold. For a plan limit or out of
credit, one background attempt per 30-minute window
(`PROBE_WINDOW_MS`, claimed once in `provider_probe`) lets an account that
recovered on its own clear; a reconnect or an ineligible plan waits for the user.
An in-process permit excludes concurrent probes. Preparation, runtime launch failure, cancellation before
dispatch and a confirmed `not_sent` result leave the window available; a completed or uncertain dispatch
spends it when its outcome is consumed.

The catalog exposes its current recommended model in `default_selection`. Global and project configuration snapshots carry the effective `modelSelection`, including a model identity, default flag, supported thinking levels and run controls, and the chosen settings. An unavailable selection has a null model and a typed reason; it occurs only when no account can run a model, or the configuration or catalog cannot be read. A composer override belongs to the renderer session and never names a model Chat cannot use: once the model is hidden, removed or its account disconnected, the renderer drops the override and sends the Loupist's default ([Chat composer](../ui/chat-composer.md#chat-composer)). The host keeps the stored Chat preference unset until the user chooses a model.

Each project snapshot can carry a `defaultModelChange` with previous and current identities. Refresh the project configuration when catalog or provider status changes, and acknowledge a displayed change by ID with `model_notice_ack`. The host retains the notice across reads and restart until acknowledgement. The global catalog stores this per-Loupist observation separately from the selected model, using the extensible `app_settings` key space.

Auto is resolved in Rust (`providers/auto.rs`) at each Chat or scheduled
admission: an explicit model that is still offered runs; an empty choice, or one
that left the catalog, uses Auto. A stored choice (a Loupist's or the app's Chat
default, a scheduled task's model, the default a hand-off runs on) that is still
listed but cannot run, because its account signed out or disconnected or the user
hid it, also uses Auto, with the thinking level and run options Auto declares for
the model it picks (`SelectionCatalog::admit`, `auto::stored_fallback`). The stored
choice is not rewritten: it applies again once it can run. The snapshot reports it
in `stored_unavailable`; Chat defaults and the Loupist's settings keep showing the
stored model with "Not available now. Chat uses the recommended model until it is.",
and the composer names the model that runs. Chat refuses only when no connected
account can run any model ("No AI account"). A run records the model that ran. Connected providers rank by connection date,
earliest first and Needs reconnect last; notices are ignored, so Auto never moves to another paid
account because one hit its limit. Each provider in turn offers its first visible
model from [`chat-model-defaults.json`](../../apps/loupist/chat-model-defaults.json)
(bundled at build time) at its listed thinking level, otherwise its newest
reasoning model with at least 20,000 tokens of context; failing all, any visible
connected model. "Newest" is the catalog order: each provider's highest numeric
version first, then, within one version, the higher family tier that file
declares in `family_tiers` (3 for Opus, Pro, Sol or Astra, 1 for Haiku, Mini or Luna). A
family matches a whole word of the model's name or ID, the highest declared tier
wins (`flash-lite` is Flash), and an undeclared family takes the `unknown` tier,
2. A new family is ranked by adding it there. Astra's family tier affects catalog
ordering. Auto excludes Astra from the reviewed Chat defaults and from fallback.
Chat keeps GPT-6.1 Sol first and includes GPT-6.2 Sol at medium when it is available. With nothing connected, admission fails with a pointer to
Settings. An empty choice keeps the requested reasoning and run options only where
the resolved model supports them. A saved model can omit run controls such as
Performance. The settings snapshot and Chat admission fill omitted controls with
the model's declared defaults and keep explicit choices for validation. A missing
or malformed saved sign-in makes that account unavailable for Auto. If the shared
sign-in file is unreadable, the catalog can still offer independently available
environment credentials in builds that support them. Rust remembers each provider’s
last admitted default model. Scoped change notices cover default moves. The
catalog also reports removal of an explicitly configured global model.

Bot and research use the same reasoning-level resolution: an explicit choice,
then the saved Chat default when supported, then the model default from
`chat-model-defaults.json`. There is no surface-specific level override.
Admission resolves the level before sealing the Pi session cache. A research
handoff uses the same saved Chat default, retaining the Bot's model and Memory flags.

The 2026-11-12 World defaults review drops retiring `gpt-5-mini`, `gpt-4.1-mini`,
`claude-sonnet-4-5` and Groq's `llama-3.3-70b-versatile`. It prefers `gpt-6.1-terra` before `gpt-5.6-terra`
when the earlier Luna choices are unavailable. Every remaining recommendation
and the support bootstrap model exists in the bundled Pi catalog. These choices
apply to new Loupists; saved World choices and manual model availability keep
their existing behavior.

A ChatGPT sign-in lists the models its plan offers (`GET /v1/models`, redirects
refused). The list reader retains at most one MiB of response bytes and cancels
the stream as soon as the total exceeds that limit. During sign-in and manual
discovery, a failed call, an oversized or malformed response, or a list sharing
no ID with Pi's catalog falls back to Pi's catalog. Providers without live plan
lists also use Pi's catalog. A 404 `model_not_found` puts the model
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
the address and selected API format. The button reads Test connection. Its result
and the models it found scroll into the sheet's field column, never the window;
that column's grid rows keep their content height, because an auto row gives the
models list (a scroll box of its own) a zero minimum when the fields outgrow a
short window.

At startup `providers/unknown.rs` deletes state that names a provider this build
does not offer (release builds drop `openai-codex`): catalog models, status rows
and gone-model entries. A global Chat default naming one is unset, schedules that
named one use Auto from their next run, and a World model on one is forgotten, so
that Loupist goes back to Recommended. This is deletion, not migration; stored
answers keep their model record.

World extraction builds each bounded prompt/schema in Rust and launches isolated
Pi inference without Chat history, Skills, workspace instructions or tools. Returned JSON is locally validated; uncertain sent calls
are not automatically retried. Method-learning reviews use this same isolated
call, one bounded request at a time, and are not repeated after an uncertain
attempt. Embeddings remain local. See
[World processing](../knowledge/worlds.md#worlds-and-graph-processing) for defaults,
publication and recovery.

Chat tool calls and [saved price replays](../knowledge/asset-pages.md#saved-price-reads)
prepare the bundled market runtime using their captured connection grants. A
replay calls the provider without a model.

## Code

- Pi startup / provider failures: `src-tauri/src/pi.rs`, `src-tauri/src/pi/`, `provider_failure.rs`
- Model providers / Auto: `src-tauri/src/providers/`, `provider-manifest.json`, `chat-model-defaults.json`, `src/chat/providerManifest.ts`, `src/components/settings/`, `src/components/onboarding/ConnectStep.tsx`
