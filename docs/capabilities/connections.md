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

The self-signed Beta has no Apple Team ID, so macOS ties each keychain item to the
build that saved it and asks for the login password the first time an updated build
reads it ("Always Allow" adds the new build until the next update). Keychain reads
and writes never run on the main thread: every command that can reach a saved
secret is `async` and runs its body on a blocking thread (`off_main_thread`), so a
waiting prompt never freezes the window. `finance_credentials` serializes reads behind
one gate and keeps, for the session and without the secret, what each read learned:
the connection's status (so listing connections, Bot, Following, asset pages and run
authority read nothing more) or a refusal. A refused key is `locked`: its connection
alone shows "Locked until you allow access in macOS" with the one-sentence reason and
**Try again** (`marketplace_credential_unlock`, which asks macOS once more); passive
callers, tool calls included, are refused without asking again. The list and every
other connection keep loading. Saving, a
check the user starts and Try again set a remembered refusal aside. Custom
connections and the GitHub sharing token use the same gate and session refusal; the
GitHub account in Settings shows the login recorded at sign-in without reading the
token. Status still lives inside the secret blob, so the first status read of each
saved key after a restart reads the keychain once (after an update, that is the one
prompt per key); recording status outside the keychain would remove it and needs a
store record that does not exist yet.

A replacement key is a candidate until a captured verification succeeds. Verified
means the provider answered the connection's fixed check with data. A refusal is
rejected, with a fixed reason code when the market data helper can tell one (FRED's
"not a 32 character key" is `key_format`); an unreachable provider, or an answer
without data, is "could not check": saved, not verified. A bundled probe that never
sends the key (Nasdaq `equity_search`, EconDB `country_profile`) proves only that
the provider answers. Failed verification preserves the working credential. Blank saved-key fields preserve
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

In Settings → Data connections, expand **Add your own connection (advanced)** and choose **Add a connection**.

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
  Sharing must load every connection record and its saved credentials before scanning;
  a file or Keychain read failure blocks sharing with an unlock-or-reconnect retry message.
  Remote connections cannot discover loopback or unspecified-address OAuth URLs; explicitly local connections may use local authorization.
  A supplied client ID takes precedence over dynamic registration, and browser preconnects or unrelated callback
  requests do not consume the sign-in attempt; an unused preconnect closes without an HTTP reply.
  The callback accepts only the exact loopback listener's Host header and `/callback` path.
  Stopped, expired or failed transports are discarded so the next attempt starts a fresh transport, while live
  server rejections and result-validation failures keep the session; removing a connection also deletes its private command working files.
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
  Memory settings permit them. Saved Sources and Map nodes carry their connection origin.
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

Available connections come first, then "Add your own connection (advanced)", a disclosure
that stays closed until the user has a connection of their own and then opens on its rows.
With none it holds one line and "Add a connection"; its Loupist picker appears once a
connection exists. The search box matches only what a card shows (name,
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
| Runtime missing | Connection unavailable | negative | no |

Needs attention means a stored credential that is rejected, unverified or unavailable,
a saved setting that no longer validates, or a candidate key not yet verified. An optional
entry the user never set up is Not connected. The Needs attention filter and its count
use that flag only. A keyless entry that still asks for a contact email (SEC EDGAR) is
labelled "Contact email only" instead of "No API key required".

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
other exits ask before discarding a key that is typed but not saved.

## Code

- Connections / Skills: `marketplace/`, `src-tauri/src/marketplace/`, `src-tauri/src/skills.rs`
