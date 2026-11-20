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
and never asks. File tools reach the Loupist folder, this run's scratch, this
Chat's attachments and the bounded global instruction paths: global `AGENTS.md`
and `skills/**`. The sealed bundled agent `skills/` root also admits read, ls,
find and grep, never write or edit; the adjacent prompt, extension and catalog stay
private. A declared bundled path that resolves outside that root through `..` or a
symlink is refused even if its target would otherwise be admitted. Other private
app data (credentials, databases, Pi runtime files), home credential locations and
paths outside the admitted roots are refused.
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
names. Instructions and the focus are injected on every Chat turn, including Memory
off and Briefings, and are bounded again at delivery (4,000 and 280 characters). See
[context assembly](../agent/context.md#context-assembly-and-bounds). Native
workspace instruction files still follow the [workspace contract](chat-turns.md#local-workspace-execution).

| Location | Contents |
| --- | --- |
| `~/.loupist/` (default global root) | Shared configuration, instructions, Skills, authentication and private runtime |
| `<workspace>/.loupist/loupist.sqlite3` | Loupist profile, Chats, Memory/growth events, asset pages, results events, watch settings and alerts, graph jobs, the derived World index (`world_index_*`: entity and claim vectors, FTS5), datasets, artifacts, sharing records and scheduled definitions/history |
| `<workspace>/.loupist/knowledge-blobs/` | Retained material referenced by that Loupist's database |
| `<workspace>/.loupist/packs/` | Files of each [shared pack](../knowledge/sharing.md#sharing) added, by import and version |
| Global `catalog.sqlite3` | Folder registrations, environment identity, model-provider status, the shared asset catalog (`assets`), each Loupist's follows (`asset_follows`) and other shared host state |
| Private Chat runtime | Disposable Pi session cache, output spill files and process leases |
| Native credential store | Environment-scoped release connector secrets and the GitHub sharing token; debug builds keep both in process memory |

The catalog owns asset identity (ID, name, reference) for every Loupist and,
keyed by Loupist ID, which assets each Loupist follows and since when. A Loupist
folder owns its asset pages, thesis, saved price reads and chart points. A replay
records the provider's price, the quote kind stated by the selected field and dataset
lineage in that folder. A failed replay keeps the last observation with its kind.
Older prices and listing references open with an unknown kind or exchange name.
These optional labels do not enter asset identity or Knowledge revision digests. Removing a
Loupist removes its follows with its registration; a copied folder opened in
another app brings its pages and no follows ([What you follow](../knowledge/follow.md#what-you-follow)).

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
only an available folder can establish that a Chat was deleted. Draft writes recheck
the Chat in its canonical database and hold that database lock through the
profile-local write, serializing with deletion. The empty-Chat composer remains a
separate draft identity with no Chat row. Shared store-error mapping and Memory
transaction setup keep raw SQLite diagnostics out of renderer messages.

An interrupted Memory transaction rolls back through the current transaction entry
points; it does not create a folder recovery state or require a renderer recovery
command. The host still quarantines pending deletions: hidden Loupists admit no
Chat, scheduled run or Memory write until deletion finishes. Quarantine is private
in-memory host state, rebuilt from the deletion journal at startup, and is not
part of the stored folder format.

The app keeps each Loupist's saved followed list, with no current ownership records or wallet
addresses. Holdings imports, balance previews and wallet reads are removed.
Existing legacy rows are retired by the upgrade below and are unavailable to app
features or the agent.

### Schema versions and upgrades

The global `catalog.sqlite3` and every folder's `loupist.sqlite3` share one schema
(`initialize_schema` in `store/schema.rs`) and one upgrade ladder
(`store/upgrade.rs`). The store schema is version 12 (`PRAGMA user_version`), the
Knowledge schema version 10 (`knowledge_schema`) and the Knowledge format version 1.
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
same shape as a fresh database, ignoring the retired tables below.

Store 7 copies identities from the latest holdings snapshot per source and wallet
into `user_watchlist` once. Copied rows use the snapshot import time as their added
time and carry no origin; an existing follow keeps its name and added time. The
saved list alone defines what is followed after this upgrade. The step uses frozen
SQL in its transaction on the catalog, folder databases and admission copies; fresh
DDL at store 8 contains no ownership tables.

Store 8 retires `user_wallet_addresses`, `user_holdings_snapshots` and
`user_holdings_positions`. If all three together contain no row, the step drops
all three. Otherwise it renames all three to `retired_wallet_addresses`,
`retired_holdings_snapshots` and `retired_holdings_positions`, preserving every row,
column and reference. It also removes `onchain.wallets` capability bindings.
The frozen SQL runs in the ladder transaction with foreign keys off; SQLite's
renames update the references between the retired tables, and the ladder checks
that no foreign-key violation was added. Catalogs, folder databases and admission
copies all use the same step. Retired tables are outside the ladder: fresh schema
comparison ignores them and folder admission accepts them. No app feature reads
them, and no ladder step may drop them. Only the separate export task may
read them into a verified file and then drop them.

Store 9 replaces `user_watchlist` with `assets`, `instruments`, `listings` and
`user_follows`. Each saved ticker keeps the `ast_` id its pages already use as a
permanent handle. Names, route markets (including empty and `UNKNOWN`) and added
times are kept as stored; a legacy hash collision keeps the first row ordered
by added time and rowid. The frozen kind table supplies only a provisional instrument kind,
never a page class. The transactional step drops the old table and runs through
the same catalog, folder and admission-copy ladder; no Knowledge version changes.


Store 10 adds the strict `identifiers` and `events` tables and the value and
subject indexes. It gives every stored listing with a ticker an accepted
assertion, observed at the saved follow time or at listing creation after
unfollow. Identified and local instrument names keep their saved creation
time. Unverified instruments use their saved symbol as the name fallback.
A personal label stays on its handle. The step leaves the event table empty
and keeps handle, listing and price keys as stored.

Store 11 removes embedded asset snapshot and sensitivity grid records from
saved Chat messages, including messages in earlier versions. The original
answer text and evidence links stay in the conversation. A link on its own
opens its evidence, and an inline valuation grid saved by an earlier build
disappears. The pre-upgrade backup contains those old grid cells. The step
runs for catalogs, bound folders and admission copies and keeps Knowledge
at version 8.

Knowledge 8 adds the preferred listing and its revision to each asset page and the
owning series to each close and quote. The step derives the listing reference from
the page's legacy asset id, keeps its ticker as the series symbol and prefixes its
existing price keys with the listing reference. Pages without a ticker retain no
price source, and legacy price rows without a ticker page remain stored and
unselected. Existing Map links are marked `auto`. The same SQL runs in the
catalog, folder and admission-copy ladder and needs only tables in that database.

Store 12 migrates each asset to a name, optional ticker and exchange reference
and creation time. The app-wide followed list belonged to no Loupist, so the
step drops it: after the upgrade no Loupist follows anything, and each asset
keeps its ID, name, reference and every page and saved read that points to it.
It removes the instrument and listing registry, identity history, automatic
results tasks, and watch and results portions of past runs and Bot messages.
Per-Loupist follows live in `asset_follows`, a lazily created catalog table.

Knowledge 9 keeps asset pages, thesis moves, Journal entries and line readings.
It removes host financial observations, filing section deltas, watching and
results records. Saved price reads and their chart points start empty.

Knowledge 10 adds `provided_json` to Source captures: the Chat message, paste time
and stated link and publisher of a
[user-provided Source](../knowledge/sources-notes.md#user-provided-sources). Existing
captures keep none.

After initialization and window setup, a background host task exports the catalog's
retired rows to `<app data directory>/legacy-holdings/holdings-export.json` (or a
numbered name when an existing file cannot be verified). The directory is private
(`0700`) and the UTF-8 JSON file is owner-only (`0600`). It contains every stored
wallet, snapshot and position field, including nulls and decimal strings. The host
syncs the app data directory after creating the export directory and before
reusing it, so a failed parent sync is retried. It then syncs and reads back the
file, checks its SHA-256 and row counts, publishes it without replacing an
existing file, then syncs the directory. A foreign partial
entry is preserved and skipped using a numbered partial name; it is never
followed, deleted or hardened. One catalog transaction writes the `app_settings` receipt `legacy_holdings_export` and drops
the retired tables. That is the only code path that drops retired tables; no
ladder step or release deadline deletes unexported rows. The recovery guarantee
covers crashes, power loss, full disks and I/O errors; another process running as
the same user can still rewrite the app-private export file or catalog at a chosen
instant before or after the receipt commits. File verification stays outside the
catalog transaction so the catalog lock is never held across file I/O. Failures
leave them for
retry and never prevent startup or normal app use. An unwinding export releases
its in-flight slot, wakes joined retries and reports failure so another attempt
can run. Folder databases retain their
retired rows; this exporter covers the catalog only. Before writing an export and
again inside the receipt transaction, the host compares every retired table's
column names and order with store 7, including generated or hidden columns. Any
difference refuses export and deletion.

The receipt records state, filename, SHA-256, counts, export time and notice
dismissal, without a path or positions. Host commands expose only status, retry,
dismissal, file-manager reveal and a native save-copy panel. The renderer never
receives the path or rows, and the file is outside every agent workspace.
A failed attempt and its dismissed notice are remembered only for this process;
a successful export and its dismissal survive restart. Each failed run logs one
line through the host logger with only fixed stage and error-kind tags; no error
message, path, row value, address or amount is logged by the exporter.

What the user sees (`LegacyHoldingsNotice` and `useLegacyHoldings`): one dismissible
status line in the shell, never a dialog and never on Following. `exported` says
holdings are no longer part of Loupist, that what was stored was saved to a file on
this computer and that the assets are still in Following, with Show file, Save a
copy… and Dismiss. `failed` says the stored data has not been deleted and the copy
could not be saved yet, with Try again and Dismiss. `none`, `pending`, a dismissed
status, a status that cannot be read and Following show nothing; the line follows every change
event, and an event wins over a status read that started before it. The status is
read only once the listener is attached, so an export that ends during startup is
not missed. Dismissal goes
to the host, so a dismissed export stays dismissed after restart and a dismissed
failure returns next launch. Settings › General keeps one row with the export date,
Show file and Save a copy… for as long as the status is `exported`, dismissed or
not. No line or row shows a path, a file name or a count.

The export stays until the user deletes it. Pre-upgrade backups beside the catalog
still contain the old tables until later upgrades prune them (the two newest are
kept). Secure erasure is not promised. Remove the exporter and its commands when
the ladder's baseline moves past store 7.

Store 6 adds covering indexes for Source lookups by `span_id` and
`representation_id`. They are non-unique because retained material may appear in
several records; the normal backup and one-step transaction rules apply.

Folder connections start in DELETE journal mode and leave it there on reopen;
the catalog uses WAL. Record reads check already bound folders for ownership before
opening other registrations. A failed ownership probe is skipped so an unavailable
folder cannot block records in another folder; a located owner still passes the
folder and database identity checks.

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
journals, scheduled tasks and runs, research hand-offs, artifacts and their revisions and datasets,
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

Research hand-offs record an optional origin, trust receipt and bounded note
on the queued row. A new row and its target Chat come from the same folder
transaction as the Bot reply or the retained link's opened status. At delivery,
Rust stops a Created row whose authority is missing or invalid. An unavailable
original message or link also stops the row. Completed rows keep their recorded
status and target. The first message of a started hand-off stores its optional
note and action attribution alongside the request.

This pre-release hand-off change keeps store version 11 by owner decision.
Use a build with receipt validation when opening pending hand-offs. An earlier
store-11 launcher has no guard for the added authority.

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

The optional global `appearance.display_currency` key accepts exactly three
uppercase ASCII letters and defaults to unset. The shape is valid even if no
current reference table covers the code. Project configuration refuses both
setting and resetting it. Reset removes the key through the global revision
and diagnostics path. Chat admission freezes the setting for its envelope
and FX tool, and display reads resolve the current setting per request.
The key needs no database migration. Older versions reject it until the line
is removed from global `config.toml`.

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
After a save the renderer tells its other views through the
`loupist-configuration-changed` window event. A committed change to the global
display currency also emits `fx-changed` across all windows. Edits made outside
the app are read at reload, app activation and scope changes.


## Code

- Schema versions and upgrades: `src-tauri/src/store/schema.rs`, `src-tauri/src/store/upgrade.rs`, `src-tauri/src/knowledge/folder.rs`, `src-tauri/src/app.rs` (`open_app_store`), `src/components/app/LoupistAvailabilityBoundary.tsx`
- Stored JSON: `src-tauri/src/domain/chat.rs`, `src-tauri/src/scheduled_tasks/types.rs` (`TaskKind`), `src-tauri/src/scheduled_tasks/store.rs` (`readable`), `src-tauri/src/store/folder.rs` (`drop_pi_session_caches`)
- Scoped configuration: `src-tauri/src/scoped_config.rs`, `src-tauri/src/commands/configuration.rs`, `src-tauri/src/marketplace/connector_config.rs` (`ignored_config_entries`)
- Followed assets: `src-tauri/src/store/user_context.rs`, `src-tauri/src/commands/assets.rs`; legacy table retirement: `src-tauri/src/store/upgrade.rs`; account-field privacy: `src-tauri/src/privacy/account_fields.rs`; DeFi research: `src-tauri/src/finance_data/defillama.rs`.
