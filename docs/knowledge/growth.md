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

World publications also record when a Lens section becomes Superseded.
The asset read-change calculation counts claim additions, endings, conflicts
and anchored Lens state changes after the asset's latest thesis move. It
retains the eight most recent rows and keeps the full counts by kind.

The typed `knowledge_growth_events` bridge command returns Loupist-scoped, time-filtered
pages with a cursor after checking that the Loupist is available, including deletion
quarantine. This ledger stores bounded summaries, not prior document
bodies or a quality score. Chat, World and Loupist settings read it. There is no
separate Growth page. Method-learning events name the Loupist and the
learning-review or World-job origin, with before/after summaries and a bounded
rationale. When a graph job publishes a Source saved from a Chat, those World
events copy that turn's chat id and message id. A World-added Source has neither,
so its events stay unattributed. A shared pack's Add and Remove write World events
with actor `import` and the pack's origin (`shared_pack` or `shared_pack_removed`);
an accepted shared method or perspective carries its `shared_pack` origin.

World publications record `claim_ended` when a claim gains an effective end and
`claim_conflict` when it gains a conflict link. A linked ending claim is included in
the growth summary. Ending a claim leaves its evidence status unchanged. A Lens
section that still relies on that claim can produce `lens_superseded`. These events
use the publication's existing limits and truncation counts. The host also
reconciles previously published endings when their effective dates arrive. It
records a `claim_ended` event at evaluation time and makes that transition and
any Lens changes available to checks resuming the recorded change feed.

## Saving one completed turn

`source_save` and `note_save` stage changes using acknowledged material.
`principle_save` uses the [User principles](lens.md#user-principles) contract.
Source retention preserves delivered bytes; note and Journal evidence must name
exact reads or valid same-turn Source save references. Chat retains its answer
before waiting for Memory. Rust commits the final Memory receipt, authorized
changes and graph jobs together. Lens pages change only
through `KnowledgeWrite::file` ([filing](lens.md#filing)): every op lands or is
logged with its outcome, and a stale section edit is skipped, never a proposal.
`lens_file` ([turn tools](../agent/turn-tools.md#turn-tools)) stages page deltas
whose basis handles Rust resolved against the turn's reads; the commit files them
with `FilingOrigin::Turn` (chat, assistant message, user message, unattended) in the
reply's transaction after the Sources, so a same-turn `saveRef` names the retained
Source. The answer's `memory_update` reports each filed page once with its op
outcomes, and `learned_digest` derives the hand-off and delivery cards' `learned`
from the log.
The receipt holds every change the turn's budgets admit: 16 Sources, a note's
Journal pair or working note, 8 principles and 8 Lens pages. A build-time check
keeps that bound in step with the budgets, because a refused receipt rolls back
the whole save. Each save step has 10 seconds to reach the store, counted on every
lock wait before and during its writes; once all of a transaction's writes have
succeeded, it commits even past that budget.

A staged Source returns `saveRef`; job lookup before commit reports
`waiting_for_turn_commit`. Processing begins after completion. Failed/cancelled
turns publish none of these staged changes. A completed answer whose save is
skipped also publishes none of its staged Memory; one that kept part of its
turn publishes only that part (below). The receipt records the
reason and up to 32 distinct discarded item kinds and titles. It takes these
from acknowledged writes still staged when the host checkpoints the answer.
Titles are limited to 160 characters. The receipt keeps those titles across
a reopen or interrupted save, including on scheduled and hand-off deliveries.
Native file writes and `run_python`
copy-back have already happened. There is no agent shell. The answer's Memory row reports actual retained changes.

### When the commit fails

Each attempt is one transaction: it lands whole or not at all, so nothing is
committed twice. If an attempt fails because one stage refused an item
(`invalid`), the next attempt leaves that stage out and saves the rest. A stage
is Sources with their note or Journal entries, principles, Lens filing, Lens
delivery and citation receipts, theses and price reads, World vocabulary, or Bot
memory; each is left out at most once. A Lens op that cited a left-out same-turn
Source loses that basis, as for any missing Source. The receipt, setup and a
hand-off are never left out: their failure fails the save. If the store was busy
past its budget (`store_busy`) or could not be written (`storage`), the whole
commit is tried again after 1 and then 3 seconds: three attempts, about 34 seconds
at most. An attempt that failed after its receipt was written, at the final write
or COMMIT, is not tried again; the store is read to learn whether it landed. A
setup preset lands with setup, so leaving World vocabulary out never lists it. The receipt tells the truth either way: a partial save lists the kept
changes as saved and each left-out item under Not kept with its cause; a save that
fails after its last attempt is skipped with the cause of that attempt. The run
log keeps the first failure only when something was left out.

### Pending save (planned for 0.1.9)

A completed turn whose staged Memory is not kept becomes a pending save bound to
its message, so nothing it fetched or filed is lost and no model is asked again.
Four cases lead there: a commit that still fails after its attempts above; a save
that waited for other conversations of the Loupist past ten minutes
([Chat turns](../runtime/chat-turns.md#chat)); a quit while a save waits, which
today ends it as interrupted; and a crash or kill while it waits or commits, which
today leaves only the interrupted-save receipt. With deep research runs of up to
30 minutes in 0.1.9, a save queued behind one must survive this way.

The stored form is a `pending_saves` row per message in the Loupist's own database,
written with the answer checkpoint before the save waits: the staged commit inputs
(Sources with their provenance, notes, Journal entries, principles, Lens deltas,
theses, price reads, vocabulary, Bot memory, setup and hand-off), the turn's cutoff,
taint and Don't-save state, and the staged Sources' delivered bytes in the private
blob store, under the same protections as retained Sources. World, search, Lens
reads, exports and shared Loupist packs never read it. A commit that lands deletes
the row in its own transaction.

A pending save is applied on its own when the Loupist is free: after the save
queue empties, at launch once no run of that Loupist reads or writes Memory, and
never ahead of an older pending save of the same chat. The skipped receipt gains
a pending flag and the notice a Save again action, which queues the same commit
at once. Either way the commit above re-runs from the stored items under the
stored turn's rules: it never widens what the original turn could keep, asks no
model and charges nothing. A stale Lens edit or conflict is a per-operation
outcome; an item refused on retry is named and left out, and the rest saves. The
row is deleted when the save lands, when the user discards it, when its chat is
deleted, and when Memory is reset (a retry after a reset is refused and says so).
It expires after 7 days; one Loupist keeps at most 64 MiB of pending material,
oldest first out, and an expired or evicted save keeps its titles under Not kept.
Two failed turns in a chat keep two rows, applied in order. A restart keeps them,
and folder copies and backups carry them like the rest of `.loupist/`.

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

## In World

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
A World change records the publishing revision and, when a Source job published
it, that job's Source record and revision. Corrections, merges and shared pack
imports have no single Source. Historical events remain readable without that
identity. A new name and the facts added about it in that publication share one
row, with the fact count next to its kind. Facts about an existing name keep
their own row.

One definition of "new" serves the drawer, its groups, the toolbar badge and
World's "new this week" (`firstSeenAfter` and `whatsNewTotals` in
`components/map/whatsNew.ts`). A name is new in a range when World first saw it
after the range's start (its first `entity_added` ever, the overview's
`firstSeenAtMs`) and it exists now. It is listed and counted once, in the row or
group of the change that first added it in the range; a later Source that adds
it again or reports values about it shows those values as facts about the name,
never the name as new. A name World had before the range that comes back is not
new. One that has left World stays as a dimmed row and is not counted.
Relationships follow the same rule per claim. A name or claim that a bounded
overview leaves out is taken to exist; such a name opens by its id. Every count is
computed over all pages of the range's events, after they join. Two World names
that share a spelling are two names here, as on the canvas.

Four or more rows from the same Source in the range shown collapse into one
expandable row, however many publications they came from: a long Source is
processed in chunks minutes apart, and its counts add up in one group. The group
carries the time of its latest change and keeps its disclosure as further chunks
arrive. Its title opens the Source in the Library. The group shows the
numbers of new names and facts. Its children list the newest change first; within
one publication, names come first, ordered alphabetically. The group only
presents the rows: Replay still reveals each publication at its own time. The
reader keeps disclosure state while the drawer
stays open, and pages of events join before the drawer counts or renders them.
The drawer's totals add up every row in the range: "+N in World" is the new
names, equal to the groups' "new names" plus the name rows outside groups, and
the relationship and disagreement counts are claims. The toolbar badge counts the
entries the list shows since the last visit (a Source group counts once). World's
"new this week" is the drawer's name total for its last seven days. Row actions still open the inspector or Lens entry and
dim items that have left World. Replay and date ranges use the underlying
changes. Bot does not repeat this growth. Schema and truncation events are
excluded.

Replay reveals the current projection by first-seen timestamps; it is not a
historical snapshot or revision browser. Export creates a PNG or a replay video
(MP4 when supported, otherwise WebM) with Loupist/date/count/disclosure context.
React captures the canvas; Rust `export_save` owns the native save dialog,
size/extension validation and file write. Cancellation returns no saved path.
