# Storage and authority

## Authority and credentials

| Component | Authority |
| --- | --- |
| React | Display state and submit typed intent through `src/bridge/tauriBridge.ts` |
| Rust | Ownership checks, credentials, SQLite transactions, broker permissions and process lifetime |
| Pi | Chat reasoning and native workspace tools; separate isolated provider calls |
| World extraction (in-process, `knowledge/world_extract/`) | Derived extraction, reconciliation and retrieval through isolated Pi inference; returned JSON is untrusted until Rust validates it and publishes canonical Memory |
| OpenBB MCP worker | Loupist-scoped read-only financial data under captured connector grants |

Native file tools run as the user. Rust decides every file call before it runs
and never asks. File tools reach the Loupist folder, this run's scratch and this
Chat's attachments, and refuse the app's private data (credentials, the database,
Pi runtime files), home credential locations and everything outside the folder.
`run_python` is the only agent code execution tool. It does not inherit the host
environment, the network or a shell
([Native tool permissions](permissions.md)). Its guest view of the workspace is
described under [Local workspace execution](chat-turns.md#local-workspace-execution).
The folder-opening UI requires an explicit trust check; there is no saved
per-Loupist native-execution switch.

Release builds do not forward host `*_API_KEY` variables to Pi; Pi uses the saved
credential in its private `auth.json` (debug builds other than `e2e` still forward
host keys). The per-turn broker token reaches the extension on an inherited pipe
at fd 3 (`LOUPIST_BROKER_TOKEN_FD`; on Windows, a one-shot private file the
extension deletes) and is never in Pi's environment or argv. The extension takes
it at load, before any native tool runs. Residual risk: `auth.json` stays readable
by same-user processes. The agent has no shell that can read it. Another
same-user process on the machine still can.

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

New Loupist uses `loupistCreate { name, focus?, avatar?, template?, tone? }`; Rust validates the profile
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
full synchronous rollback journaling. Removing a Loupist unregisters it and preserves
its folder, Chats and Memory. Import can reopen an app-created workspace directly
under this profile's `loupists/` directory only when it contains a valid existing
Loupist; the profile root and disposable runtime folders remain private. Missing
or replaced folders require reconnection, which validates the
profile, preserves its Loupist id and requires a newer update time; absent filesystem
identities do not establish that two folders are the same. Copy/reopen rules are in
[Folder portability](../knowledge/growth.md#folder-portability). Versions, upgrades
and refusals are in [Schema versions and upgrades](#schema-versions-and-upgrades).
Workspace tabs retain their saved references while a registered folder is unavailable;
only an available folder can establish that a Chat was deleted. Shared store-error
mapping and Memory transaction setup keep raw SQLite diagnostics out of renderer messages.

An interrupted Memory transaction rolls back through the current transaction entry
points; it does not create a folder recovery state or require a renderer recovery
command. The host still quarantines pending deletions: hidden Loupists admit no
Chat, scheduled run or Memory write until deletion finishes. Quarantine is private
in-memory host state, rebuilt from the deletion journal at startup, and is not
part of the stored folder format.

Holdings are app-level. Each import is a snapshot of one source (`csv`, `kis`,
`crypto_file`, or one per wallet address), and holdings are the union of the latest
snapshot per source (`LATEST_SNAPSHOTS` in `store/user_context.rs`); a successful
file or KIS import prunes older snapshots and their positions for that source;
a backwards host clock that would discard the new snapshot refuses the import. A
crypto import never replaces stock holdings, and a symbol held in two sources is one item. Unknown
stored holdings source kinds, wallet families and wallet statuses fail exact reads
rather than becoming CSV imports, Bitcoin addresses or unread wallets. Holdings
summaries, wallet lists and automatic wallet targets skip unreadable rows and
log once without row contents; selecting one wallet to read stays strict. Your
holdings (Update under About you) imports stock CSV
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

### Schema versions and upgrades

The global `catalog.sqlite3` and every folder's `loupist.sqlite3` share one schema
(`initialize_schema` in `store/schema.rs`) and one upgrade ladder
(`store/upgrade.rs`). The store schema is version 6 (`PRAGMA user_version`), the
Knowledge schema version 7 (`knowledge_schema`) and the Knowledge format version 1.
A fresh database is created at the current versions.

A database from store 4 and Knowledge 6 onward (the format 0.0.1 installs use) is
upgraded one version at a time, each step in its own transaction. Before the first
step Rust writes `<database>.upgrade-backup-<UTC stamp>` beside it with `VACUUM
INTO`, created owner-only (`0600`) and synced. A failed step rolls back, so the
database stays at its last completed version, and the error names the backup.
The catalog upgrades at startup. A folder upgrades when its storage is first bound
(listing, a turn, a briefing or a scheduled run) or when it is registered by import
or reconnection. Admission validates an upgraded in-memory copy and writes nothing
to the folder before registration. To return to a backup, quit Loupist and copy it
over the database.

Each database keeps its two newest backups. Once an upgrade commits, Rust deletes
that database's older backups and keeps the one it just wrote plus the newest
earlier one, ordered by stamp and then by the `-<n>` suffix a same-second backup
takes. Only regular files directly beside the database whose names match
`<database>.upgrade-backup-<YYYYMMDDTHHMMSSZ>[-<n>]` exactly are candidates.
Symlinks, directories and any other name are never touched. A failed upgrade
deletes nothing, and a pruning failure is logged without failing the upgrade.

Steps are frozen: each names exactly what it changes from the shape its version
had. A step that rebuilds a table follows SQLite's twelve-step ALTER procedure and
keeps rowids, indexes and triggers, including the Memory immutability triggers;
it may add no foreign-key violation. `store/tests/upgrade.rs` upgrades the frozen
baseline (`store/tests/fixtures/baseline-store4-knowledge6.sql`) and requires the
same shape as a fresh database.

Store 6 adds covering indexes for Source lookups by `span_id` and
`representation_id`. They are non-unique because retained material may appear in
several records; the normal backup and one-step transaction rules apply.

Folder connections start in DELETE journal mode and leave it there on reopen;
the catalog uses WAL. Record reads check already bound folders for ownership before
opening other registrations, while still checking the owning folder's identity.

Store 5 also restores the `piSessionCache` key that builds before it removed from
stored Chats on reconnect and import. Chat writes compare the stored text with the
re-encoded Chat, so those Chats refused every later write; the step puts the key
back where serialization writes it, and the cache is now cleared in place.

Store 5 and Knowledge 7 moved extensible enumerations (kinds, statuses, origins,
actors and `app_settings` ids) from SQL `CHECK (x IN (...))` lists to Rust: each
write binds an exhaustive Rust enum or a value its write site checks, so adding a
value needs no schema step. Structural checks (booleans, lengths, ranges and row
invariants) stay in SQL. The immutable provenance tables (`knowledge_records`,
`knowledge_source_captures`, `knowledge_source_representations` and
`knowledge_read_receipts`) keep their lists, because those values are part of the
Knowledge format. Scheduled task statuses and lazily created tables also keep theirs.

Tables a module creates on first use (`CREATE TABLE IF NOT EXISTS`, such as sharing,
provider status or artifact history) are outside the ladder: a database may lack
them, and their module creates them at their current shape. Their DDL never changes
shape in place. A shape change is a ladder step that alters the table only when it
exists, together with the module's DDL for databases that never created it. Store 5
adds `application_settings.keep_awake` (off) this way.

Rust refuses data it cannot read and leaves it unchanged:

| Data | Folder | Catalog |
| --- | --- | --- |
| Earlier: store below 4 or Knowledge below 6 (development formats before the ladder) | Listing marks it `earlier_version`; binding storage, import and reconnection fail with `loupist_earlier_version`; Remove is its only action | Set aside as `catalog.sqlite3.earlier-<UTC stamp>` (with its `-wal` and `-shm` files); a new, empty catalog keeps the earlier host environment id, so Keychain secrets stay reachable |
| Newer: a version above this build's | Listing marks it `newer_version`, distinct from a moved folder (`workspace_unavailable`); binding storage, import and reconnection fail with `loupist_newer_version`; Remove unregisters it | A native dialog says a newer Loupist saved the data, and the app quits without a window |

A failed catalog upgrade also shows a native dialog before the app quits. A set-aside
catalog's folder registrations are not carried: only an earlier build wrote that
catalog, so its folders are earlier formats too. Downgrades are not supported: an
older build refuses data a newer build upgraded, and the backup is the way back.

### Stored JSON

`user_version` versions DDL only. Records kept as JSON carry no version of their
own: the `data_json` columns (Chats, profiles, capability bindings, deletion
journals, scheduled tasks and runs, artifacts and their revisions and datasets,
the model catalog and update schedule in `app_settings`), and app-written files
(activity details, run logs, `connections.json`, briefing settings, keychain
envelopes, the KRX cache). Their Rust types accept fields they do not know, so a
record a newer build wrote at the same schema version still opens in an older
one. Change them only this way:

- A new field is optional: `#[serde(default)]`, plus `skip_serializing_if` when
  absence is its default, with a meaning that holds when it is missing. Never
  rename, retype or remove a field, or make one required.
- An older build drops fields it does not know when it rewrites a record by id,
  timestamp or revision (profiles, capability bindings, scheduled tasks and runs,
  `app_settings`, files), so a new field must be safe to lose. Writes to Chats,
  artifacts and deletion journals compare the stored text with the re-encoded
  record, so there an older build's write conflicts and the record stays as the
  newer build left it.
- A new enum value needs a fallback older builds already have.
  `ChatFailureKind::Other` and `ChatToolRefusalReason::Other` show a generic
  failure or refusal. `TaskKind::Unknown` keeps the stored name: the task is
  listed and can be deleted, but is never run, edited, paused or resumed. Roles,
  message, task and run statuses and schedules have no fallback, because no stand-in
  is safe for what they drive (retry, Memory, admission, the next run).
- Anything else is a ladder step even when the DDL does not change: a new value
  of an enum without a fallback, or a removed, renamed or reinterpreted field.
  The step bumps `user_version`, so an older build reports `newer_version`
  instead of misreading the record.

Input that crosses a boundary stays strict (`deny_unknown_fields`): renderer
requests, model tool arguments, Pi and MCP output, bundled catalogs, shared packs
and `config.toml`. A type in both roles gets an exact input-side twin that converts
into the stored type: the briefing schedule, a Work tab's artifact reference,
`ask_user` options, `artifact_publish` Markdown and Pi's run controls. Types the
model or a server also writes directly stay strict in storage too: chart views,
studies, drawings and dataset columns, custom connection tools, World schema
extensions. A record holding one fails alone. The Pi session cache and process
leases are authority records and decode exactly: an unreadable cache is dropped
and the Chat rebuilds cold, and an unreadable lease fails closed. A valid lease
whose leader has a different start identity is removed without inspecting or
signaling the new process, so a reused PID does not block startup. If the start
identity still matches but the executable or group cannot be verified, recovery
keeps the lease without signaling and lets startup continue. Knowledge
records, their revision payloads and their mutable rows belong to the Knowledge
format (`KNOWLEDGE_FORMAT_VERSION` enters every revision digest) and change only
with a format or schema step.

One unreadable record never hides the others. These skip and log it, and leave it
stored as it is: the Chat list, the scheduled task list, a task's run history, the
inbox (its cursor still moves past it), the latest-run activity and the due scan.
Other paths stay strict: reading that one record, every write, recovery of
interrupted runs, and folder admission. Admission takes a folder whole: it refuses
one whose profile, tasks or runs fail to decode or validate, as it refuses a digest
mismatch. Records a newer build wrote under the rules above decode and pass.

SQL addresses stored JSON by serde name (`json_extract`, `json_set`, partial
indexes such as one briefing per Loupist on `$.kind`). `store/tests/chat.rs` and
`scheduled_tasks/tests.rs` serialize the real types and check each path and value
the SQL uses, so a serde rename fails a test instead of a query.

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
replacing the file: malformed TOML, an unknown section or key, an invalid value
of a known key, and any credential in a connector table are errors
(`diagnostics`). The bundled catalog can move under a file the user or another
Loupist version wrote: a connector or connector field it no longer declares, or a
Skill it now keeps always on. Such a setting is ignored and reported as a warning
(`warnings`); its text stays in the file and saves and runs continue. A save
request may name only settings this version has. Saves check revisions and preserve
comments; admission checks the file and freezes the effective settings. Parsed
configuration is cached by path and scope, modification time, length and filesystem
identity (including Unix change time). Each hit still opens the file without
following a link; external replacement, deletion and writes invalidate the cache.
The process keeps at most 64 parsed files.
Rust emits no configuration event: after its own save the renderer tells its other
views through the `loupist-configuration-changed` window event, and edits made outside
the app are read at reload, app activation and scope changes.


## Code

- Schema versions and upgrades: `src-tauri/src/store/schema.rs`, `src-tauri/src/store/upgrade.rs`, `src-tauri/src/knowledge/folder.rs`, `src-tauri/src/app.rs` (`open_app_store`), `src/components/app/LoupistAvailabilityBoundary.tsx`
- Stored JSON: `src-tauri/src/domain/chat.rs`, `src-tauri/src/scheduled_tasks/types.rs` (`TaskKind`), `src-tauri/src/scheduled_tasks/store.rs` (`readable`), `src-tauri/src/store/folder.rs` (`drop_pi_session_caches`)
- Scoped configuration: `src-tauri/src/scoped_config.rs`, `src-tauri/src/commands/configuration.rs`, `src-tauri/src/marketplace/connector_config.rs` (`ignored_config_entries`)
- Holdings, wallets and on-chain reads: `src-tauri/src/store/user_context.rs`, `src-tauri/src/commands/user_context.rs`, `src-tauri/src/finance_data/onchain.rs`, `src-tauri/src/finance_data/defillama.rs`, `src-tauri/src/privacy/holdings.rs`, `scripts/generate-kis-read-api.py`, `src/components/holdings/`
