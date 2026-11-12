# Scheduled runs

Scheduled work reuses ordinary Chat admission. The process has to be running, including while the window is hidden. The Schedule tab is [Schedule](../ui/schedule.md).

## Process lifetime

World jobs and World search run in-process through Rust and the local embedding
model. Existing folders keep their canonical SQLite snapshot; leftover
`graph-instance/` data is ignored and never migrated or read.
App exit stops schedule admission, cancels/drains World work and shuts down MCP
workers and leased processes. The Tauri exit callback allows 30 seconds; unfinished
cleanup retains leases for startup recovery. Recovery verifies process identity
before signaling. Process leases are implemented for macOS, Windows and Linux.
Linux verifies the boot ID, process start ticks and executable identity through
procfs and requires the process group to remain verifiable. Other Unix hosts
fail closed with `UnsupportedPlatform`.

Keep running in the menu bar for briefings defaults on, and turning a briefing on
sets it again. Closing the main window hides it while that setting is on.
Menu Quit and Cmd+Q exit. The tray menu opens the app, selects a Loupist's Schedule
tab, or quits. A finished briefing posts an OS notification when permission is
granted. That notification does not open the item. Tray selection emits the
typed `inbox-open` event. Schedules run only while the app process is running,
including while the window is hidden.

## Runs

The run coordinator allows at most four model workers and rejects concurrent runs
against the same Loupist target. Canonical finalization is serialized through Rust.
Completed runs also append a content-free local diagnostic. Paths, retention and
the reader are in [Diagnostics](../dev/diagnostics.md#diagnostics).
Update installation shares a maintenance gate with Chat, graph processing, manual
Memory writes and Loupist registration: admitted work and installation exclude each
other. World jobs run sequentially per Loupist; their recovery is owned by
[Knowledge and Memory](../knowledge/worlds.md#worlds-and-graph-processing), including
completed-job receipts and disposal of derived extraction copies. A queued or
running World load (Recover World processing, or the load after a shared pack's Add or
Remove) runs before any extraction; the load after Add or Remove cancels an earlier
load still waiting.

### Scheduled Chats

Scheduled work reuses ordinary Chat admission, execution and persistence. A saved
definition has title, prompt, schedule, target, model/reasoning options and separate
Memory read/save flags. Supported schedules are once, interval, daily and weekly;
daily/weekly use IANA time zones. A run can target a new Chat per run, the task's
own continuing Chat, or an existing Chat. Bot manages schedules. A research session can submit a typed schedule handoff with its own model and Memory flags. The host registers the task idempotently and posts a receipt in the same Loupist’s Bot. A busy Bot receives pending receipts on the next delivery pass or workspace load. Unattended runs cannot manage or hand off schedules. A research hand-off from the Bot
(`chat_handoffs`, [Home Thread](../ui/home-thread.md#home-thread)) is a delegated run on
the same admission path with the same unattended allowlist, without a scheduled task: a
`created` row with no run is sent once by the Bot's next delivery pass, a `running` row
whose run is gone is marked stopped, and a run that started is never sent again. Legacy confirmation-pending tasks remain paused. The Bot’s briefing and watch tools handle explicit requests separately. Existing briefing-based watcher consent is captured on first access.

The shared Rust service handles create/edit, pause/resume, delete, Run now and
Stop. Edits check definition revisions; Chat creation keys prevent duplicate
schedule creation. Schedule writes commit immediately, including if the creating
reply later fails. A task cannot overlap itself and shares ordinary Chat capacity.
Run now attempts admission immediately; it is not a durable manual-run queue.
Pause prevents future starts; Stop cancels the current run separately. Run now also
starts a paused task without turning on its future weekday schedule. The user
asks the Bot to run a task now; Schedule shows its definition and run history.

The scheduler sleeps until the next due occurrence or a schedule-change signal,
with a 60-second maximum to recheck folders and wall-clock changes; an overdue
occurrence blocked on admission retries after five seconds.

Task lists batch profile and Chat ownership checks, preserving per-row corruption
isolation.

Missed recurring occurrences coalesce into one due run. Startup marks unfinished
runs interrupted; a failure in one Loupist's recovery is skipped so other
Loupists can still run. An undecodable running receipt is skipped individually,
and its run id is logged once per app process; other orphans in that Loupist still
recover. Recovery opens a write transaction only when it finds an
orphaned run. Failed/interrupted recurring tasks pause unless a newer edit
superseded the run's definition. History records status, timing, error and target
Chat, where the reply and its evidence remain. Renderer discovery polls every two
seconds while the window is visible and every 30 seconds while hidden, and reads
immediately when the window becomes visible or regains focus. A finished run is also
announced in the Loupist's [home Thread](../ui/home-thread.md). Desktop notifications
identify custom and earnings tasks by title and distinguish stopped runs from
failures. Only a user-stopped run says "stopped"; crash recovery and other
interruptions say "couldn't finish". There is no meaningful-change filter, saved
per-task connection allowlist, or per-task native-execution preference.

Each scheduled task is `custom`, `briefing` or `earnings` (the automatic
[results task](#results-before-and-after), which runs on the host without a Chat turn). A
Loupist has at most one briefing,
with a continuing Memory-enabled Chat titled Briefings. The default briefing is
08:00 Monday through Friday in the device time zone. Rust builds that prompt at
run time from the date, elapsed time since the previous run, followed symbol and
market identities, at most 40 World growth summaries of 240 characters (never a "What you follow" Source), at most
30 principles inside the ordinary 6,000-character budget, at most five due
Journal pointers, and `assetDeltas`: up to 24 followed assets with a material
asset-page change since the previous run (or seven days), three rows each, which
also fill `BriefingContext.assets`; `earnings`: at most eight results events, those
reported since the previous run's day (at least since yesterday) and those announced
within seven days (never a legal deadline), each with Rust's snapshot view, which also
fill `BriefingContext.earnings` with two measures each; and `watchAlerts`: at most 24
watcher alerts since the previous run with their dates, facts and sources
(`BriefingContext.watchEvents`). Quantities and values are excluded. The stored briefing
context also keeps at most 20 entity ids for disputed claims in that window
that touch a followed company or its one-hop neighbors. The reader can open the
Map with the conflicting-evidence filter. Omitted referenced nodes are recovered
with bounded exact World reads.

After its durable claim and Chat admission, a briefing (scheduled or Run now)
[refreshes](../knowledge/asset-pages.md#asset-pages) followed assets once for that
occurrence without a model call. Its prompt cutoff is captured after those reads,
so newly refreshed evidence is included. Busy, not-yet-due and already-running tasks make
no refresh reads. The refresh releases the scheduler admission lock, so other
schedules can attempt admission: at most 24 assets (watched, then held), skipping assets
checked in the last 30 minutes, one at a time under a Memory-mutation activity,
within 90 seconds, and abandoned when the scheduler stops. Assets not reached stay
unchecked for that run. Wallet balances are then read for the held flags, within 30
seconds.

Unattended runs, both briefing and custom, may call only the fixed allowlist
(`briefings/policy.rs`, reading each method's `unattended` column in
`broker/tool_class.rs`): read-only data tools, `calculate`, `run_python`, and selected Memory reads and saves
(including `asset_page_read`, `thesis_update` and `earnings_snapshot_save`).
Journal saves, principle saves, publishing charts, tables or documents through
the publish tools, native file tools and schedule management are refused
and recorded on the run as skipped tools; a delegated hand-off run has no scheduled
run row, so its refusals are not recorded, and a scheduled run whose row cannot take
the record fails the call. A custom MCP call still requires an
unchanged Always approval.
The inbox is read with `inbox_list`, `inbox_mark_read` and `inbox_unread_counts`.
The agent's schedule reader still uses the internal run history. Completed,
failed and interrupted receipts share one unread state.

### Results before and after

Each Loupist that follows an equity has one automatic task of kind `earnings` (id
`earnings-{loupist}`, "Results before and after", `commands/earnings.rs`). Rust creates it
when the Schedule tab lists tasks, when the open Loupist follows an asset or gets
proactive consent, and on each watch tick. It starts active when the briefing is active
(proactive consent) and paused otherwise; consent turns an untouched paused task on. It
targets the home Thread and is stored as an hourly interval; after each look Rust moves
its next run to the next capture due, at most six hours away, and a look with nothing
due records no run. Rust refuses Edit and Delete. Pause, Resume and Run now work,
turning it on needs no model, and a failed run does not pause it.

A run is claimed through scheduler admission (`claim_host_run`, which keeps it out of
interrupted-run recovery) but makes no model call and no Chat turn. It keeps
[results events](../analysis/results.md#results-events) in step with the asset pages'
observations, runs at most eight due captures under a Memory-mutation activity,
schedules due captures it did not attempt for the next minute, and records
what each capture did as structured summary items (`RunSummaryItem`: expected or
reported values with the asset name, period and count, a failed capture, or nothing due)
that the Schedule tab words in the reader's language, keeps a failure's reason in the
run's error, and posts what the Thread is owed. A successful before-capture posts
nothing. Captures read
through each Loupist's host executor, as the watchers do. Statement rows match the
event's period-end date first; fiscal labels are used only when the payload has no
dated rows. What they read and how values
are compared is in [Results before and after](../analysis/results.md#results-before-and-after). The
results card reads `earnings_snapshot_get` and `earnings_events_list` and shows
"Capturing what's expected…" while a capture runs; Prepare now and Try again
(`earnings_prepare`) run the event's pending side at once and return a conflict when
another capture already owns the task.
