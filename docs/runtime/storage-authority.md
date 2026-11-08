# Storage and authority

## Authority and credentials

| Component | Authority |
| --- | --- |
| React | Display state and submit typed intent through `src/bridge/tauriBridge.ts` |
| Rust | Ownership checks, credentials, SQLite transactions, broker permissions and process lifetime |
| Pi | Chat reasoning and native workspace tools; separate isolated provider calls |
| World extraction (in-process, `knowledge/world_extract/`) | Derived extraction, reconciliation and retrieval through isolated Pi inference; returned JSON is untrusted until Rust validates it and publishes canonical Memory |
| OpenBB MCP worker | Loupist-scoped read-only financial data under captured connector grants |

Native tools run as the user and can use installed runtimes. Rust decides every
native call before it runs and never asks: file tools reach the Loupist folder, this
run's scratch and this Chat's attachments, and refuse the app's private data
(credentials, the database, Pi runtime files), home credential locations and
everything outside the folder; Bash runs inside an OS sandbox (bubblewrap on Linux;
no other host has one yet) that confines writes to the folder and scratch, hides the
same private data, and drops the network once the reply has read third-party text
([Native tool permissions](permissions.md)). Bash receives an allowlisted environment
without app credentials or shell startup hooks; its PATH is described under
[Local workspace execution](chat-turns.md#local-workspace-execution). A command can
still read other user-readable files outside the masked set, the policy's residual
risk. The folder-opening UI requires an explicit trust check; there is no saved
per-Loupist native-execution switch.

Release builds do not forward host `*_API_KEY` variables to Pi; Pi uses the saved
credential in its private `auth.json` (debug builds other than `e2e` still forward
host keys). The per-turn broker token reaches the extension on an inherited pipe
at fd 3 (`LOUPIST_BROKER_TOKEN_FD`; on Windows, a one-shot private file the
extension deletes) and is never in Pi's environment or argv. The extension takes
it at load, before any native tool runs. Residual risk: `auth.json` stays readable
by same-user processes, including commands the agent runs; there the boundary is
the prompt and the user's machine.

Sidecars are private app resources, verified against staged inventories. OpenBB's
host restrictions cover its audited clients, not arbitrary hostile native code.
Rust controls secrets, tool visibility, result bounds and worker lifetime.
Bundled and custom MCP sessions share one protocol constant in
`apps/loupist/src-tauri/src/mcp.rs`: the host requests `2025-11-25` and accepts
`2025-06-18` or `2025-03-26` in the `initialize` answer.

Custom MCP connections are clients of user-selected servers, not a Loupist
endpoint. Rust stores their secrets, enables each connection per Loupist, and pins
tool approval to a definition hash. Confirming from the Add dialog enables the
Loupist that opened it before tool review. The renderer receives no secret values.
After a custom result enters a turn, Rust blocks later outbound tools for that
turn. [Custom connections](../capabilities/connections.md#custom-connections) owns the
user-facing rules.

## Where data lives

New Loupist uses `loupistCreate { name, focus?, avatar? }`; Rust validates the profile
and allocates an app-managed local workspace. `loupistImport { workspacePath }` is
separate and requires an existing Loupist folder; `packAdd` can also create one from a
[shared pack](../knowledge/sharing.md#sharing). Each Loupist can contain multiple
independently created, renamed and deleted Chats.

The Loupist settings page edits name, focus, avatar, tone and instructions through
`loupist_profile_update`. Rust persists the profile; names are independent of folder
names. Instructions are injected on every Chat turn, including Memory off and
Briefings, and are bounded again to 4,000 characters at delivery. See
[context assembly](../agent/context.md#context-assembly-and-bounds). Native
workspace instruction files still follow the [workspace contract](chat-turns.md#local-workspace-execution).

| Location | Contents |
| --- | --- |
| `~/.loupist/` (default global root) | Shared configuration, instructions, Skills, authentication and private runtime |
| `<workspace>/.loupist/loupist.sqlite3` | Loupist profile, Chats, Memory/growth events, asset pages, results events, watch settings and alerts, graph jobs, the derived World index (`world_index_*`: entity and claim vectors, FTS5), datasets, artifacts, sharing records and scheduled definitions/history |
| `<workspace>/.loupist/knowledge-blobs/` | Retained material referenced by that Loupist's database |
| `<workspace>/.loupist/packs/` | Files of each [shared pack](../knowledge/sharing.md#sharing) added, by import and version |
| Global `catalog.sqlite3` | Folder registrations, environment identity, model-provider status, the watchlist, holdings snapshots, read-only wallet addresses and other shared host state |
| Private Chat runtime | Disposable Pi session cache, output spill files and process leases |
| Native credential store | Environment-scoped release connector secrets and the GitHub sharing token; debug builds keep both in process memory |

Rust routes canonical operations to the owning Loupist. An exclusive folder lock
prevents concurrent app instances from writing the same data. Loupist databases use
full synchronous rollback journaling. The host store schema is version 3 (shared-pack
update and origin columns), the Knowledge format version 1 and
the Knowledge schema version 5 (class watcher kinds and routes);
unsupported data is rejected without migration or replacement. A folder made by an
earlier build (store schema older than 3 or Knowledge schema older than 5) is never opened: listing marks it
`earlier_version`, and binding its storage (turns, briefings, Memory and asset work) or
importing it fails with `loupist_earlier_version`. Remove is its only action. A global
`catalog.sqlite3` from an earlier build is set aside as
`catalog.sqlite3.earlier-<UTC stamp>` (with its `-wal` and `-shm` files) and the app
starts with a new, empty catalog (`open_app_store` in `app.rs`).
Removing a Loupist unregisters it and preserves its folder. Missing or replaced
folders require reconnection. Copy/reopen rules are in
[Folder portability](../knowledge/growth.md#folder-portability).

Holdings are app-level. Each import is a snapshot of one source (`csv`, `kis`,
`crypto_file`, or one per wallet address), and holdings are the union of the latest
snapshot per source (`LATEST_SNAPSHOTS` in `store/user_context.rs`): a crypto import
never replaces stock holdings, and a symbol held in two sources is one item. Your
holdings (Update under About you, or the onboarding watchlist step) imports stock CSV
files, Korea Investment, exchange balance files and up to 10 read-only wallet addresses.
For an exchange file Rust detects the format (a Kraken ledger, a balances file, or the
asset and balance columns the user picks), merges Kraken codes and staking variants into
the base asset, skips fiat and zero rows, refuses a trade history and keeps the preview
for 30 minutes. The preview shows quantities before commit; afterwards the store hands
out only identities (symbol, market, name) and counts and dates per source and per kind
(stocks and funds, crypto; `holdings_summary`). A wallet address is checked for its network (Bitcoin, EVM or
Solana), and text that looks like a recovery phrase or private key is refused
(`secret_refused`) without being stored, logged or echoed. The renderer sees a masked
address (first six and last four characters), network, status and dates, never balances;
the full address serves only Copy address. Balances come from
[host reads](../capabilities/data-services.md#public-blockchains-and-defillama) on Add, Refresh and
before each briefing, within 30 seconds each time; a failed read keeps the last good
snapshot.

### Scoped configuration

Rust owns global `config.toml` and `<workspace>/.loupist/config.toml`. Theme, interface
language, model
visibility and declared nonsecret connector settings are global. Chat defaults,
optional/custom Skill enablement and supported Pi options can be overridden per
Loupist. World processing, connection grants and schedules live in the Loupist database.
The app-wide Chat defaults and Skills pages edit global configuration. Per-Loupist
overrides are edited only on that Loupist's settings page, with its scope fixed.

Fields resolve project → global → app default; arrays replace. Reset removes the
scoped field. Explicit composer selections override Chat defaults, and new Chats
inherit Memory defaults while existing Chats retain their saved policy. Scheduled
runs use their saved model (empty means [Auto](providers.md#shared-providers-and-world-processing))
and Memory selections. Invalid configuration blocks new dependent runs without
replacing the file. Saves check revisions and preserve
comments; admission rereads and freezes the effective settings.


## Code

- Scoped configuration: `src-tauri/src/scoped_config.rs`, `src-tauri/src/commands/configuration.rs`
- Holdings, wallets and on-chain reads: `src-tauri/src/store/user_context.rs`, `src-tauri/src/commands/user_context.rs`, `src-tauri/src/finance_data/onchain.rs`, `src-tauri/src/finance_data/defillama.rs`, `src-tauri/src/privacy/holdings.rs`, `scripts/generate-kis-read-api.py`, `src/components/holdings/`
