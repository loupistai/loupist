# Connections

A connection's secret stays in Rust. Enabling it is per Loupist, and a custom server is a client of a server the user picked, not a Loupist endpoint.

## Connections and credentials

Connection details separate shared setup, credentials and provider context,
including for keyless services. Readiness/access/search filters help locate entries.
Model-provider sign-ins and keys are managed in Models & providers;
[Shared providers](../runtime/providers.md#shared-providers-and-world-processing) owns which
providers sign in and which take a key only (OpenRouter is key-only). Rust opens a
provider's key and usage pages from fixed addresses (`provider_open_console`,
`provider_open_usage`); the renderer sends a provider ID, never a URL. Connector
keys in release builds use the OS keyring under the catalog's environment identity;
debug builds keep them in process memory. Secrets are not returned to the renderer or placed in
TOML, tool arguments or receipts.

A replacement key is a candidate until a captured verification succeeds. Failed
verification preserves the working credential. Blank saved-key fields preserve
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

## Custom connections

Users can add their own MCP data sources under Settings → Connections → Your connections.
- **Adding:** by URL (Streamable HTTP; HTTPS, or loopback for local servers), by pasting `mcpServers` JSON, or, under
  Advanced, as a local command with a pinned package version. Nothing runs until the user confirms an entry, and the
  full command line is shown first. Confirming from Add enables the Loupist that opened the dialog, then tool review
  records Allow or Don't allow. Done closes the dialog. Confirming from the connection detail does not enable a Loupist. The detail sheet covers the right side of Connections, and the page behind it wraps into the visible strip so a line is not sliced.
  The detail switch still chooses which Loupists may call the connection. A new connection keeps “Don't save results to
  Memory” on until that switch is turned off.
- **Sign-in:** MCP authorization: protected-resource and authorization-server discovery, Client ID Metadata Documents
  when the product client URL is configured (otherwise Dynamic Client Registration or a user-supplied client ID),
  PKCE, the `resource` parameter, exact issuer validation, refresh, and step-up. Tokens, secrets, headers and env
  values live in the OS keyring and never reach the renderer, logs, traces or the model; tool results are scrubbed of
  protected values.
- **Tools:** only tools annotated read-only and not destructive are eligible, and names or descriptions matching
  trading, transfer or payment patterns are blocked. The user approves each eligible tool per Loupist when adding the
  connection (Allow or Don't allow); the approval is pinned to a hash of the tool definition, and a changed
  definition revokes it and flags the connection for review. Unapproved calls are refused with `needs_approval` and
  linked from the answer. The model sees host-generated, sanitized tool descriptions; the raw server description is
  shown only to the user.
- **Outbound block:** after a custom tool result enters a turn, Rust marks the turn tainted, and outbound-capable
  tools (native Bash, web, browser, other connections) are refused for the rest of that turn.
- **Memory:** "Don't save results to Memory" is on by default per connection. When it is on and the connection
  taints a turn, that turn writes no Memory; otherwise Sources and Map nodes carry their connection origin.
- **Unattended runs:** briefings and scheduled runs may call only Always-approved read-only custom tools.
  Custom-enabled turns start a cold Pi session.

## Code

- Connections / Skills: `marketplace/`, `src-tauri/src/marketplace/`, `src-tauri/src/skills.rs`
