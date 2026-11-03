# Growth and portability

Growth is an append-only event log of Memory changes. A folder copy is the portability path; there is no second sync store.

## Growth event log

Rust records committed Source, World and Lens changes in the append-only
`knowledge_growth_events` table. Events include retained Sources, added entities,
added/disputed/withdrawn claims, Lens creation/revision/metadata changes and moves
of an asset page's read (`thesis_moved`: the asset as subject and summary, the steps
as before/after summaries, the reason as rationale), with bounded summaries, actor
and available originating Chat/message IDs. They commit
with the corresponding Memory write; rolled-back transactions leave no events and
identical retained content adds none. Lens events preserve bounded before/after
summaries when the current document changes. World diffs cap detailed events at 2,000 and
append an explicit omitted-change summary when needed. The Chat receipt reads
`thesis_moved`; What's new does not list it.

The typed `knowledge_growth_events` bridge command returns Loupist-scoped, time-filtered
pages with a cursor. This ledger stores bounded summaries, not prior document
bodies or a quality score. Chat, Map and Loupist settings read it. There is no
separate Growth page. Method-learning events name the Loupist and the
learning-review or World-job origin, with before/after summaries and a bounded
rationale. When a graph job publishes a Source saved from a Chat, those World
events copy that turn's chat id and message id. A Map-added Source has neither,
so its events stay unattributed. A shared pack's Add and Remove write World events
with actor `import` and the pack's origin (`shared_pack` or `shared_pack_removed`);
an accepted shared method or perspective carries its `shared_pack` origin.

## Saving one completed turn

`source_save`, `note_save` and `lens_update` stage changes using acknowledged material.
`principle_save` uses the [User principles](lens.md#user-principles) contract.
Source retention preserves delivered bytes; note/Journal/Lens evidence must name
exact reads or valid same-turn Source save references. Editing a Lens requires its
explicit acknowledged read. Rust commits the completed Chat, authorized Memory
changes and graph jobs together. Lens conflicts become proposals.

A staged Source returns `saveRef`; job lookup before commit reports
`waiting_for_turn_commit`. Processing begins after completion. Failed/cancelled
turns publish none of these staged changes; native file and shell effects have
already happened. The answer's Memory row reports actual retained changes.

## Folder portability

Close the app before copying a Loupist folder, including hidden `.loupist/`. The folder
contains Chats, Memory, retained originals, datasets, artifacts, attachments,
scheduled definitions/history and the [shared Loupists](sharing.md#sharing) it added. Provider
credentials, the GitHub sign-in, Pi caches and process leases remain app-private.
The derived World index travels in the folder database and rebuilds on first use
when its embedding model or revision does not match.

Open a copied folder to restore its saved identity; moving a registered folder
requires Reconnect folder. The saved profile name survives copying/reconnection. An
empty folder cannot replace saved data, and a registered identity cannot be opened
as a second independent Loupist. Removing its registration preserves the folder.

Registration validates database identity/schema, SQLite integrity/foreign keys,
record ownership and declared blob hashes/sizes. A folder made by an earlier build
is refused with the typed `earlier_version` state and can only be removed; see
[Where data lives](../runtime/storage-authority.md#where-data-lives). Data-directory/database symlinks
are rejected. Re-registered schedules and World processing are paused: opening
saved data does not authorize automatic execution. Interrupted graph jobs retain
the ordinary recovery requirements.

## Code

- Growth event log: `src-tauri/src/knowledge/growth.rs`, `src-tauri/src/commands/knowledge.rs`, `src/contracts/knowledge.ts`
