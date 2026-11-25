# Connections

A connection's secret stays in Rust. Enabling it is per Loupist, and a custom server is a client of a server the user picked, not a Loupist endpoint.

## Connections and credentials

The official catalog is `apps/loupist/marketplace/marketplace.json` plus its
`plugins/<name>/` folders. Settings → Data connections configures those bundled services
and the user's own [custom connections](#custom-connections). Each Loupist's settings
control which connections it can use. Open those settings from the sidebar's Loupist
switcher or Settings → Loupists. **Loupist access** on the Data connections page
opens the current Loupist’s **Skills & connections** section at its connections list. See
[the Data connections page](#the-data-connections-page).

Connection details separate shared setup, credentials and provider context,
including for keyless services. Readiness/access/search filters help locate entries.
Manage model-provider sign-ins and keys in Settings → AI accounts.
[Shared providers](../runtime/providers.md#shared-providers-and-world-processing) owns which
providers sign in and which take a key only (OpenRouter is key-only). Rust opens a
provider's key and usage pages from fixed addresses (`provider_open_console`,
`provider_open_usage`); the renderer sends a provider ID, never a URL. Connector
keys in release builds use the OS keyring under the catalog's environment identity;
debug builds keep them in process memory. Secrets are not returned to the renderer or placed in
TOML, tool arguments or receipts.

Moving from the self-signed 0.1.2 Beta to Developer ID changes the app’s
identity. Settings > Data connections can ask macOS for access to the saved
items. An item's **Always Allow** decision is for the app's designated
requirement and applies to later versions that meet that requirement.

Host initialization makes zero Keychain calls. The first request for bundled
connection status in a macOS profile that predates credential status records
checks each missing record on a background thread, with one 30-second budget.
It searches the user-domain file Keychain for item references. The search does
not request password data or require permission to decrypt the item. Settings
shows its loading state during the check. It then shows **Set up** on
unconfigured rows, **Check saved key** for a found item that Loupist has yet to
read, and **Try again** when presence could not be determined. That last
action retries the metadata search. A completed presence result is saved, so
later status requests use it directly. A fresh credential namespace needs
no presence search.

The catalog records bundled credentials’ presence, field names, active and
pending state, verification kind and time, and random revisions. Custom
connections keep their own connection records. Sharing displays the saved
GitHub login. No key, token, fragment or hash of a secret goes in these
records. An explicit read or verification updates the record from the saved
item. Windows still checks older credentials only on request. Linux release
builds have no native credential store. AI accounts use their own provider
records, including the account check at onboarding.

The **Allow access** action reads the legacy items in sequence on a dedicated
background thread. Each reserves its own item until macOS answers, without a
30-second deadline. Cancel stops the sequence and discards the result of an
unsettled read. The user closes any open macOS dialog and can retry later. A
denied legacy item remains in the permission notice until an explicit check
succeeds or the item is removed.

Other native operations reserve only the item they touch and wait up to 30 seconds.
A read that finds its item held waits up to five seconds for its turn instead of
failing, because parallel tool calls often read one key; reads take turns so macOS
never stacks two prompts for one item. It reports the key as being changed only
when a change still holds the item, and as unavailable when macOS has not answered
an earlier read. A change, a presence check or another compound operation that
finds the item held reports at once that the operation is still pending.
While macOS is still answering a timed-out read, the connection shows its
permission message and an explicit check action in Settings. The native call
continues, and a late success refreshes the status so the next use can read
the item normally. A denial blocks automatic attempts for the rest of the
session. The user can ask for one new read from Connections. A refused read
preserves the last non-secret status record. Bundled market data reports a
refused saved key only when that record confirms a key exists. A key-less
connection such as Asset lookup also shows no key when macOS has no keychain
(`no_default_keychain`) and the record names none; any other refusal still
offers **Allow access**, since a key saved by an earlier build has no record. An unresolved
prompt leaves other runs, Settings, the scheduler and World building free.
The updater returns its existing
blocked result while a native write or delete is pending. The OS call keeps
that exclusion after a caller timeout and publishes its eventual outcome.
If the app quits during mutation, a pending record cannot grant new access.
The next explicit check or needed read reconciles the item.

A scope change closes execution independently of Keychain. Launch records
the change without reading or deleting the credential. An explicit connection
operation completes revocation and deletes the old item outside shared guards
before attesting the new scope. An unavailable Loupist leaves only that
connection pending. Explicit configuration changes attempt the same cleanup.

Secret values live only inside the operation that uses them. A tool can reuse
its credential for inventory and dispatch. Verification drops its capture
after the provider check and rechecks the durable revision before publishing.
Asset lookup rechecks the actual item before releasing a response, so a key
removed outside Loupist cannot authorize an in-flight result. A sharing scan has its own private context. There is no
process-lifetime cache of raw credentials.

FRED checks the published GDP observation for January 1, 2024. The earliest GDP
row contains a missing-value marker.

A replacement key is a candidate until a captured verification succeeds. Verified
means a credential-dependent check returned data. EconDB reads US CPI with the
candidate token and bypasses its token-independent HTTP cache. A refusal is
rejected, with a fixed reason code when the market data helper can tell one (FRED's
"not a 32 character key" is `key_format`); an unreachable provider, or an answer
without data, is "could not check": saved, not verified. Alpha Vantage compact daily prices can answer invented keys,
and Nasdaq equity search sends no key. Both leave a saved key unverified. Failed verification preserves the working credential. Blank saved-key fields preserve
keys; explicit recheck still contacts the provider. A newer replacement or deletion
invalidates an outstanding check. Declared nonsecret setup fields live under global
TOML `connectors`; scope changes invalidate old credentials and disable access.
Deletion disables bindings before removing keys. Failed deletion remains disabled
and can be retried.

When setup first makes a connector ready, available Loupists receive enabled bindings;
later rechecks preserve individually disabled bindings. Rust rechecks readiness and
captured authority at admission. Save, verification and visible snapshot refresh
are separate outcomes; UI errors must distinguish them. MCP errors use bounded
recovery categories, not raw credential-bearing provider payloads.

Setup can start from Chat. When the user asks for a data source that is not loaded, or
offers a key, the agent calls [`connection_setup`](../agent/turn-tools.md#turn-tools) and
the answer shows a [setup card](../ui/chat-composer.md#chat-composer) that opens this same
setup dialog or turns the connection on for the Loupist; the key is entered in the
dialog, never in the conversation, and the connection works from the next message.

## Custom connections

User-added MCP connections remain in the code and are off in shipped builds. Updating
from 0.1.2 keeps existing connection records, credentials, approvals and results. The
app hides the controls, leaves credentials unopened and excludes their tools from runs.
Bundled connections retain their saved-key check in Data connections.
Previously saved answers and records keep the name of their source. Recorded approval
decisions are read-only. Launch a local debug build with
`LOUPIST_DEV_CUSTOM_CONNECTIONS=1` to restore the custom connection controls and
runtime.

With the debug switch enabled, expand **Add your own connection (advanced)** in Settings → Data connections and choose **Add a connection**.

Paste JSON keeps the draft and points to the first syntax problem by line and column. Review becomes available when the text parses. The host checks the configuration itself.

- **Adding:** by URL (Streamable HTTP; HTTPS, or loopback for local servers), by pasting `mcpServers` JSON, or, under
  Advanced, as a local command with a pinned package version. Nothing runs until the user confirms an entry, and the
  full command line is shown first. Confirming from Add enables the Loupist that opened the dialog, then tool review
  records Allow or Don't allow. Done closes the dialog. Confirming from the connection detail does not enable a Loupist. The detail sheet covers the right side of Data connections, and the page behind it wraps into the visible strip so a line is not sliced.
  The access switch controls whether the selected Loupist can use this connection.
- **Sign-in:** MCP authorization: protected-resource and authorization-server discovery, Client ID Metadata Documents
  when the product client URL is configured (otherwise Dynamic Client Registration or a user-supplied client ID),
  PKCE, the `resource` parameter, exact issuer validation, refresh, and step-up. Tokens, secrets, headers and env
  values live in the OS keyring and never reach the renderer, logs, traces or the model; tool results are scrubbed of
  protected values. Token rotation and client-registration replacement retain the newest 256 distinct retired secret values for that scan; the history never prevents refresh.
  Header and environment values shorter than eight characters are ordinary settings unless their name contains
  token, key, secret, password, auth, cookie or credential (case-insensitive). OAuth tokens and client secrets
  remain protected at every length. Numeric and boolean echoes in structured results are checked too.
  When custom connections are enabled, sharing must load each custom record and its saved credentials before scanning;
  a file or Keychain read failure blocks sharing with an unlock-or-reconnect retry message.
  Remote connections cannot discover loopback or unspecified-address OAuth URLs; explicitly local connections may use local authorization.
  A supplied client ID takes precedence over dynamic registration, and browser preconnects or unrelated callback
  requests do not consume the sign-in attempt; an unused preconnect closes without an HTTP reply.
  The callback accepts only the exact loopback listener's Host header and `/callback` path.
  Stopped, expired or failed transports are discarded so the next attempt starts a fresh transport, while live
  server rejections and result-validation failures keep the session; removing a connection also deletes its private command working files.
  For local commands, the host discards a stdio frame over 32 MiB through its newline.
  If its response ID matches a pending call, that call fails and the session keeps serving: the model reads
  `result_too_large` with the 32 MiB limit, that the connection is still running, and to ask for less.
  HTTP connections report the same refusal with their 4 MiB limit for JSON and event streams.
  One request at a time holds a connection (a call, a connect, a sign-in or a Settings change). Another that
  arrives meanwhile is refused at once as the connection being in use, not as its key being changed; a turn's
  own calls already take turns, so this comes from another conversation or Settings. The model is told the call
  was not sent and the key is fine, and to call once more later in the reply before going on without it.
- **Tools:** only tools annotated read-only and not destructive are eligible, and names or descriptions matching
  trading, transfer or payment action words are blocked. Concatenated action verbs are checked at name word starts;
  descriptions use whole English words and Korean action forms so research terms remain eligible.
  The user approves each eligible tool per Loupist when adding the
  connection (Allow or Don't allow); the approval is pinned to a hash of the tool definition, and a changed
  definition (including title, output schema and metadata) revokes it and flags the connection for review. Unapproved calls are refused with `needs_approval` and
  linked from the answer. The model sees host-generated, sanitized tool descriptions; the raw server description is
  shown only to the user. HTTP call results use the same strict result fields as local commands; all custom content
  and server metadata reach the model inside untrusted delimiters.
  Expanding the definition hash requires one new review of approvals saved by builds that hashed fewer fields.
- **Outbound block:** after a custom tool result enters a turn, Rust marks the turn tainted, and outbound-capable
  tools (web, browser, other connections, every native data read that reaches its provider, keyless World Bank and
  DefiLlama included, and schedule changes) are refused for the rest of that turn. `run_python` stays allowed.
- **Memory.** Each new connection starts with **Save results to Memory** off. When a result from that connection
  enters a turn, the host blocks Memory writes for that turn. Turning the switch on allows saves when Chat’s
  Memory settings permit them. Saved Sources and World nodes carry their connection origin.
  A refused save names that connection policy as its cause, even when Chat saving is also off; it never suggests
  enabling saving to bypass the connection's choice. Tool refusals distinguish a declined approval card, an
  unanswered card and missing standing approval in an unattended run. Outside-text pauses invite the user to ask
  for the paused schedule or connection action in their next message, and outbound-pause guidance is sent once.
- **Unattended runs:** briefings and scheduled runs may call only Always-approved read-only custom tools.
  Custom-enabled turns start a cold Pi session.

## The Data connections page

The access label and setup guidance describe the provider’s signup requirements.
Korea Investment and Tradier require brokerage accounts. BizToc’s free Basic
tier requires a payment card. Other providers can restrict data by plan even
after issuing a free key. The dedicated CFTC connection can search and read
reports anonymously. Saving and verifying a Socrata app token raises the rate
limit for that grant. The BLS series connection requires a registered key
although the public API permits 25 anonymous queries per day.

Alpha Vantage verification and the default daily history read request the latest
100 unadjusted daily prices. Explicitly adjusted prices use the provider’s
premium feed. A plan refusal leaves a working credential available.

Shipped builds show Available connections with the bundled catalogue. The custom connection disclosure is off, including for people updating from 0.1.2 with existing custom connections. With `LOUPIST_DEV_CUSTOM_CONNECTIONS=1` in a local debug launch, "Add your own connection (advanced)" follows Available connections. It stays closed until the user has a connection of their own and then opens on its rows. With none it holds one line and "Add a connection". Its Loupist picker appears once a connection exists. The search box matches only what a card shows (name,
summary, access label and markets), never plugin or provider ids, so the runtime that
serves an entry stays invisible.

`src/marketplace/readiness.ts` maps each connector's host status to one visible state.
The host reports `needs_configuration` both for an entry nobody set up and for one whose
key failed, so the renderer separates them:

| State | Label | Tone | Counts as Needs attention |
| --- | --- | --- | --- |
| Ready | Ready | positive | no |
| Not set up | Not connected | neutral | no |
| Stored key or setting failing | Needs attention | warning | yes |
| Runtime unavailable | Connection unavailable | negative | no |

When a connection cannot start, the unavailable note explains that Loupist retries on the
next question. A repeated failure calls for restarting Loupist before reinstalling.

Needs attention means a stored credential that is rejected, unverified or unavailable,
a saved setting that no longer validates, or a candidate key not yet verified. An optional
entry the user never set up is Not connected. The Needs attention filter and its count
use that flag only. A keyless entry that still asks for a contact email (SEC EDGAR) is
labelled "Contact email only" instead of "No API key required".

A notice above the catalog counts connections whose saved keys still need
macOS permission. **Allow access** checks them one at a time and updates the
progress and each row as it settles. A denied key has a row-level retry.
The sidebar's **Needs attention** entry opens this notice and disappears
after the last key is resolved.

The page reads the current Loupist's bindings (`loupist_capability_list`) with the
catalog. A Ready card whose connection is off for that Loupist adds "Off for {Loupist}",
since the asset page, Following and Chat send the user here with "off for this Loupist".
Its dialog says so beside the status ("On for {Loupist}" when on) and offers "Turn on for
{Loupist}" (`loupist_capability_enable`); "Loupist access" in the footer, for every
Loupist, appears once the connection is set up, because finishing setup turns it on for
all of them.

Within each group, ready entries come first, then entries that need no setup (the keyless
Market data platform leads Market data), then localized display name. Loupist settings
uses the same comparator, translates names, summaries and group labels with the same
catalog, and the two pages agree. Group headings are h3 and card titles h4. English
connector copy is looked up by exact string in the `catalog` namespace, via
`src/lib/connectionCopy.ts`, so `connectionCopy.test.ts` fails when a bundled
summary or name has no catalog entry.

The setup dialog hides KRX service ids and shows a localized sentence for a rejected or
unavailable key instead of the raw host error: KRX's rejection also names its per-market
approval, and an unchecked key says so with "Verify again" (offline, it says the Mac is
offline). With a saved key, nothing typed and settings unchanged, the submit button reads
"Verify again", which is what it does. A key not saved yet has no status line; the optional
OpenFIGI key says what works without it. A result and the progress line sit right under
the fields, and the title stays as the dialog opened even when a failed check leaves the key
saved. When the control used goes away or is disabled
(saving, Turn on, Delete), focus moves to the result. Done, Close, Escape and the dialog's
other exits ask before discarding a key that is typed but not saved. A choice list opened
from the dialog, such as Live or Demo, is portaled onto the page at the dialog's stacking
level and paints above the dialog.

## Code

- Connections / Skills: `marketplace/`, `src-tauri/src/marketplace/`, `src-tauri/src/skills.rs`
