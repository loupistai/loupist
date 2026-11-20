# Growth and portability

Growth is an append-only event log of Memory changes. A folder copy is the portability path; there is no second sync store.

## Growth event log

Rust records committed Source, World and Lens changes in the append-only
`knowledge_growth_events` table. Events include retained Sources, added entities,
added/disputed/withdrawn claims, Lens page creation, revision, archive and restore (one event per page a filing changed) and moves
of an asset page's read (`thesis_moved`: the asset as subject and summary, the steps
as before/after summaries, the reason as rationale), with bounded summaries, actor
and available originating Chat/message IDs. They commit
with the corresponding Memory write; rolled-back transactions leave no events and
identical retained content adds none. Lens events preserve bounded before/after
summaries when the current document changes. World diffs cap detailed events at 2,000 and
append an explicit omitted-change summary when needed. The Chat receipt reads
`thesis_moved`; What's new does not list it.

The typed `knowledge_growth_events` bridge command returns Loupist-scoped, time-filtered
pages with a cursor after checking that the Loupist is available, including deletion
quarantine. This ledger stores bounded summaries, not prior document
bodies or a quality score. Chat, Map and Loupist settings read it. There is no
separate Growth page. Method-learning events name the Loupist and the
learning-review or World-job origin, with before/after summaries and a bounded
rationale. When a graph job publishes a Source saved from a Chat, those World
events copy that turn's chat id and message id. A Map-added Source has neither,
so its events stay unattributed. A shared pack's Add and Remove write World events
with actor `import` and the pack's origin (`shared_pack` or `shared_pack_removed`);
an accepted shared method or perspective carries its `shared_pack` origin.

## Saving one completed turn

`source_save` and `note_save` stage changes using acknowledged material.
`principle_save` uses the [User principles](lens.md#user-principles) contract.
Source retention preserves delivered bytes; note and Journal evidence must name
exact reads or valid same-turn Source save references. Rust commits the completed
Chat, authorized Memory changes and graph jobs together. Lens pages change only
through `KnowledgeWrite::file` ([filing](lens.md#filing)): every op lands or is
logged with its outcome, and a stale section edit is skipped, never a proposal.
`lens_file` ([turn tools](../agent/turn-tools.md#turn-tools)) stages page deltas
whose basis handles Rust resolved against the turn's reads; the commit files them
with `FilingOrigin::Turn` (chat, assistant message, user message, unattended) in the
reply's transaction after the Sources, so a same-turn `saveRef` names the retained
Source. The answer's `memory_update` reports each filed page once with its op
outcomes, and `learned_digest` derives the hand-off and delivery cards' `learned`
from the log.

A staged Source returns `saveRef`; job lookup before commit reports
`waiting_for_turn_commit`. Processing begins after completion. Failed/cancelled
turns publish none of these staged changes. Native file writes and `run_python`
copy-back have already happened. There is no agent shell. The answer's Memory row reports actual retained changes.

## Folder portability

Close the app before copying a Loupist folder, including hidden `.loupist/`. The folder
contains Chats, Memory, retained originals, datasets, artifacts, attachments,
scheduled definitions/history and the [shared Loupists](sharing.md#sharing) it added. Provider
credentials, the GitHub sign-in, Pi caches and process leases remain app-private.
The derived World index travels in the folder database and rebuilds on first use
when its embedding model or revision does not match.

Open a copied folder to restore its saved identity. Use Reconnect folder when
you move a registered workspace or replace its directory with a copy at the
same path. Select the restored directory and confirm that you trust it before
opening its pages. The saved profile name survives copying and reconnecting.
An empty folder cannot replace saved data, and a registered identity cannot be
opened as a second independent Loupist. Removing its registration preserves the
folder.

Asset pages carry their local ID, saved name, thesis and saved price data
in the folder. A copied folder keeps those pages. Each Loupist's followed
list, display preferences, grants and credentials live in the app profile:
another app that opens a copy or fork shows its pages and reasons with
nothing followed, and the original app keeps the original's follows.
Removing a Loupist's registration removes its follows.

Unix device numbers may change after a reboot or remount. On reading a registered
profile, Rust silently refreshes a changed device number only if the stored path
still opens as a directory without following its leaf as a symlink, its inode is
unchanged, and its filesystem birth time proves the same directory generation.
The private runtime requires the same evidence. A saved birth time must match
exactly. For legacy identities without one, birth must be no later than that
catalog profile's update time, compared at full birth-time precision; an ambiguous
final millisecond fails closed. A legacy workspace also requires birth no later
than the folder profile's positive `created_at_ms`, checked against the catalog's
original creation time when available. That value is captured before database
creation and ordinary edits/reconnect preserve it, so later profile updates cannot
advance this cutoff. An unusable creation value (zero) leaves the catalog cutoff
alone; this is a known limit. Missing, zero or negative birth times never permit
silent recovery on any platform. macOS uses descriptor `st_birthtime` and Linux
uses `statx` `stx_btime`.

Legacy private runtimes keep only the catalog cutoff: their directories are
legitimately allocated after profile creation is sampled, so the folder creation
value cannot bound them. This is a known limit without a tolerance window. That
identity check guards accidental replacement; a process able to recreate this
private directory can already write inside the existing one. The directory holds
Chat caches/CWD files, attachments and activity, while shared authentication and
run capability files live elsewhere. After first recovery, or for new bindings,
the stored birth time must match exactly.

The folder must pass reconnect admission with exactly one profile bearing this
Loupist's ID. Recovery keeps a no-follow database descriptor and the same SQLite
connection open from admission through commit, checks that SQLite and the live
path still identify that held file, and repeats ownership, digest and foreign-owner
validation inside its transaction. The folder remains SQLite's main database;
the catalog is attached to it. Root and runtime identities, including optional
`birthTime { seconds, nanoseconds }`, in both profile JSON copies and the root
device/inode columns change in one transaction with rollback journals, before
restoring catalog WAL. The optional identity metadata also travels in deletion
journals; SQL columns and renderer contracts do not change. Manual reconnect
compares its runtime against the pre-recovery saved profile.

Existing bound connections are never rebound. A missing path, changed inode or
birth time, newer replacement, foreign or missing data, or symlink still requires
the ordinary unavailable-folder recovery; a failed identity write rolls back both
databases. Ordinary access and deletion enforce a saved birth time when present.

Registration validates database identity/schema, SQLite integrity/foreign keys,
record ownership and declared blob hashes/sizes. A folder in an earlier format is
validated as an upgraded copy and upgraded on registration; a pre-ladder format is
refused as `earlier_version` and one a newer build saved as `newer_version`; see
[Schema versions and upgrades](../runtime/storage-authority.md#schema-versions-and-upgrades). Data-directory/database symlinks
are rejected. Re-registered schedules and World processing are paused: opening
saved data does not authorize automatic execution. Interrupted graph jobs retain
the ordinary recovery requirements.

## Code

- Growth event log: `src-tauri/src/knowledge/growth.rs`, `src-tauri/src/commands/knowledge.rs`, `src/contracts/knowledge.ts`

## On the Map

What's new lists growth events since the last visit (the same per-device value as
the morning message), 7 or 30 days, or a date, grouped by day with one row per
subject: names, relationships, reported values, conflicts, methods and principles. Relationship and
conflict rows are built from local names and the translated relation. A claim whose two ends are
one name (the same node, or names that share a spelling, `oneName` in `knowledge/mapItems.ts`) is
a value reported about it, never "A → A": it reads "6 new facts about KRW-BTC" and opens that name,
whose Key facts list the values; a disputed one reads "Two sources disagree: KRW-BTC (reports)".
Changes one publication made that would read the same (kind, ends, relation and Source) are one
row, and a reported-values row counts them; changes that differ in any of these stay separate.
The same rule keeps the Bot's morning questions from asking how a name is linked to itself.
Rows open the inspector or the Lens entry; rows for items no longer on the Map are dimmed. The
toolbar badge counts changes since the last visit; Bot does not repeat this growth.
Schema and truncation events are excluded.

Replay reveals the current projection by first-seen timestamps; it is not a
historical snapshot or revision browser. Export creates a PNG or a replay video
(MP4 when supported, otherwise WebM) with Loupist/date/count/disclosure context.
React captures the canvas; Rust `export_save` owns the native save dialog,
size/extension validation and file write. Cancellation returns no saved path.
