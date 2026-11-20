# Scheduled runs

Scheduled work reuses ordinary Chat admission. Scheduled research and briefings use the AI account for each run.
The process has to be running, including while the window is hidden. The Schedule page is [Schedule](../ui/schedule.md).

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

Keep running in the menu bar starts off. With no explicit choice, enabling the
first briefing or custom task turns it on automatically. The host preserves an
explicit menu-bar choice. The settings response carries
`keepRunningChoice: null` for the automatic default, or the user's explicit boolean;
`keepRunning` is the effective value. Old files without the choice field retain their
stored boolean as an explicit choice, because its origin cannot be recovered.
Closing the main window hides it while the effective setting is on.
Menu Quit and Cmd+Q exit. The tray menu opens the app, selects a Loupist's Schedule
page, or quits. A finished briefing posts an OS notification when permission is
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
running World load (Recover map building, or the load after a shared pack's Add or
Remove) runs before any extraction; the load after Add or Remove cancels an earlier
load still waiting.

### Scheduled Chats

Scheduled work reuses ordinary Chat admission, execution and persistence. A saved
definition has title, prompt, schedule, target, model/reasoning options and separate
Memory read/save flags. Supported schedules are once, interval, daily and weekly;
daily/weekly use IANA time zones. A run can target a new Chat per run, the task's
own continuing Chat, or an existing Chat. Your Loupist manages schedules in Bot.

Run sessions. A fresh task (`new_chat`) gets a new Chat for every run. A
continuing task (`dedicated_chat`, which the briefing always is) creates its own
Chat on the first run and reuses it, so a run sees the earlier runs' requests and
answers within the usual [transcript bound](../agent/context.md#context-assembly-and-bounds).
Every custom run after the first, fresh or continuing, also receives a short
`previous_run` record of how the task's last run ended and what last reached the
user ([context delivery](../agent/context-delivery.md)); beyond that a fresh run
carries nothing from earlier runs except what Memory kept. The run's
request is stored as a user message with a `scheduled` record (task, run, title,
kind), because the user did not write it then: the session shows it as "Scheduled
run: {task}", a custom task's prompt behind Request and the briefing's host request
not at all, and it has no Edit (`ScheduledTurn.tsx`); a Markdown export or print
heads it the same way (`exportChat.ts`). A Chat the scheduler made
(`scheduled-chat-…`) stays out of Research sessions (`hide_from_recent` in
`commands/types.rs`) while every user message in it is such a request; a message
the user writes there (a reply or guidance) makes it a research session like any
other. A failed or stopped run's reply there offers Run now, which runs the task
again through the scheduler (`scheduled_task_run_now`) and opens the task in
Schedule, instead of Try again, which would send the task's request again as the
user's own turn; the host refuses that retry too (`prepare_chat_retry`). Results reach the user through Bot and Schedule, and the session
opens from Bot's Open, the run reader's Continue in Chat and Search; the sidebar
lists it while it is open ([Sidebar](../ui/sidebar.md#chat-selection)).

Until the user writes in it, a session the scheduler made is its run's record and
goes with it. When a claim pushes a run out of the 50-run history, and when a
task is deleted, the host deletes each session those runs wrote in that no run
left in history and no task's target (a continuing task's own session, an existing
Chat) still points at, provided every user message in it is a scheduled request
(`scheduled_released_sessions`, `service::release_sessions`). So a fresh task keeps
the sessions of its last 50 runs, a continuing task's own session stays while the
task exists and goes with it, and a session the user wrote in, or a Chat of their
own a task writes into, stays theirs. Deletion uses the user's Delete path
(`delete_chat_session` in `commands/records.rs`): artifacts, hand-offs, knowledge
chat origins, turn pins, attachments, the Pi session cache and drafts go with it,
under a deletion journal; Memory, Sources and Lens items the run saved stay. A
session a turn is running in is left for later. At startup the same rule settles
every Loupist that is available (`release_host_sessions`). A session from a build
before the `scheduled` record reads as the user's and stays; an empty one goes.
Bot's card for a run whose session is gone opens the task in Schedule instead, or
says the run's record was removed once the task is gone too
([Bot](../ui/home-thread.md#home-thread)).

A separate Chat can submit a typed schedule handoff with its own model and Memory flags. The host registers the task idempotently and posts a receipt in the same Loupist’s Bot. When Bot is busy, receipts wait for the next delivery pass or workspace load. Unattended runs cannot manage or hand off schedules. A research hand-off from Bot
(`chat_handoffs`, [Bot](../ui/home-thread.md#home-thread)) shares Chat admission
without a scheduled task. Typed hand-offs use the unattended allowlist; Map-link
hand-offs use the Session surface. A `created` row with no run is sent once by
the next delivery pass in Bot, a `running` row whose run is gone is marked
interrupted, and a run that started is never sent again. Legacy
confirmation-pending tasks remain paused. The briefing tool in Bot handles
explicit requests for a recurring briefing.

The shared Rust service handles create/edit, pause/resume, delete, Run now and
Stop. Edits check definition revisions; Chat creation keys prevent duplicate
schedule creation. Schedule writes commit immediately, including if the creating
reply later fails. A task cannot overlap itself and shares ordinary Chat capacity.
Run now attempts admission immediately; it is not a durable manual-run queue.
Pause prevents future starts; Stop cancels the current run separately. Run now also
starts a paused task without turning on its future weekday schedule. Task detail offers Run now and Pause/Resume; a completed one-time task needs an
edit before it can resume. Custom tasks have Delete under More, with confirmation,
disabled while running; it takes the task's run history and the run sessions the
user never wrote in ([run sessions](#scheduled-chats)). Briefing settings turn the briefing on or off. The user can
ask Bot to change a task's timing or prompt.
The scheduler sleeps until the next due occurrence or a schedule-change signal,
with a 60-second maximum to recheck folders and wall-clock changes; an overdue
occurrence blocked on admission retries after five seconds. Task lists batch
profile and Chat ownership checks, preserving per-row corruption isolation.

Missed recurring occurrences coalesce into one due run. Startup marks unfinished
runs interrupted; a failure in one Loupist's recovery is skipped so other
Loupists can still run. An undecodable running receipt is skipped individually,
and its run id is logged once per app process; other orphans in that Loupist still
recover. Recovery opens a write transaction only when it finds an
orphaned run. A failed recurring task or unexpected interruption stays active for
its next occurrence; three consecutive failures pause it, and success resets the
streak. User Stops and graceful app-quit interruptions neither count nor reset it.
Old receipts with the ambiguous quit/crash message are also neutral; new orphan
recovery explicitly records an unexpected interruption and still counts.
Resume and every definition edit start a fresh failure window, persisted in the
task JSON as a start timestamp and a revision tie-breaker for runs begun in the
same millisecond; no schema step or public Task field is added. Only terminal
receipts from that window count. The sink carries typed NeedsReconnect/ModelGone
failures into the task policy so rejected credentials or a missing model pause
immediately, even though the Chat's failure text is the generic placeholder. A newer edit still prevents an old completion from changing the definition.
History keeps a task's last 50 runs and records status, timing, error and target Chat, where the reply and its evidence remain. Renderer discovery polls every two
seconds while the window is visible and every 30 seconds while hidden, and reads
immediately when the window becomes visible or regains focus. A finished run is also
announced in the Loupist's [Bot](../ui/home-thread.md), and so is one that recovery
found unfinished after a crash, as one that could not finish (`deliverable` in
`chat/home.rs`); a Stop, a quit and a quiet run post nothing. Desktop notifications
identify custom tasks by title and distinguish stopped runs from
failures. Only a user-stopped run says "stopped"; crash recovery and other
interruptions say "couldn't finish". There is no meaningful-change filter, saved
per-task connection allowlist, or per-task native-execution preference.

Each scheduled task is `custom` or `briefing`. A Loupist has at most one briefing,
with a continuing Memory-enabled Chat titled Briefings. The default briefing is
08:00 Monday through Friday in the device time zone. At run time Rust supplies
the date, elapsed time since the previous run, the followed names and reference
fields with saved-read state, principles, up to 40 Map growth summaries, and up
to five due Journal pointers. The briefing context keeps up to 20 disputed
entity IDs for the Map filter. The unattended agent chooses its own data reads
and reports what it could check.

Unattended runs, both briefing and custom, may call only the fixed allowlist
(`briefings/policy.rs`, reading each method's `unattended` column in
`broker/tool_class.rs`): read-only data tools, `calculate`, `run_python`, and selected Memory reads and saves
(including `asset_page_read`, `thesis_update` and `price_read_save`).
Journal saves, principle saves, publishing charts, tables or documents through
the publish tools, native file tools and schedule management are refused
and recorded on the run as skipped tools; a delegated hand-off run has no scheduled
run row, so its refusals are not recorded, and a scheduled run whose row cannot take
the record fails the call. A custom MCP call still requires an
unchanged Always approval.
The inbox is read with `inbox_list`, `inbox_mark_read` and `inbox_unread_counts`.
The agent's schedule reader still uses the internal run history. Completed,
failed and interrupted receipts share one unread state.

### Quiet custom runs

An alert is a custom task with a prompt specifying its condition and the evidence
to check. A run that completes the check and finds nothing to report can call
`schedule_quiet`. The host refuses quiet delivery if any tool was skipped or a
data call was attempted without any acknowledged original data result. The
reply and its Memory still commit if the run can no longer be marked quiet.
A successful quiet run stays in history, where Schedule marks it "Nothing to
report" and never unread. It leaves the Chat's sidebar recency unchanged and adds
neither an inbox entry nor a Bot announcement. Failed runs use regular failure
handling.
