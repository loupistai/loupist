# Scheduled runs

Scheduled work reuses ordinary Chat admission. Scheduled research and briefings use the AI account for each run.
The process has to be running, including while the window is hidden. The Schedule page is [Schedule](../ui/schedule.md).

## Process lifetime

World jobs and World search run in-process through Rust and the local embedding
model. Existing folders keep their canonical SQLite snapshot; leftover
`graph-instance/` data is ignored and never migrated or read.
Quit, menu-bar Quit and update restart stop schedule and Chat admission,
cancel active Chat and World work, and let the existing receipt and Memory
completion paths settle. Tray menu refreshes are queued on the main thread so
saving a receipt can finish while the native exit callback is waiting. Idle
MCP workers stop in parallel after the run drain.

The host gives graceful cleanup up to 26 seconds within the existing 30-second
quit budget. Before exiting, it fences worker launches, sends SIGKILL to
remaining owned Unix process groups, and clears their leases once they have no
live members. This includes Pi, MCP and Python workers, the login-shell probe
and one-shot macOS helper commands. Signals go to every remaining group before waiting on any one
group. The host logs counts and deadline events without run content. The
remaining time also lets cancelled runs finish saving after their workers
stop. A visible window can remain until native Quit finishes. Quit adds no
progress dialog. The updater shows its existing restart status until the
restart begins.

If the host is killed, the next launch recovers durable leases before starting
new workers. Recovery verifies process identity before signaling. On Linux
this includes the boot ID, process start ticks and executable identity through
procfs. Windows uses kill-on-close Job Objects for worker trees. Other Unix
hosts fail closed with `UnsupportedPlatform`.

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

Settings → General also offers **Open Loupist at login**, initially off. Turning it
on registers the current user's main app with macOS 13+ `SMAppService.mainApp`
and explicitly enables menu-bar residence. Turning residence off unregisters the
login item first. Turning login off leaves residence alone. General rereads the
system status when opened or focused. When approval is required or macOS reports
an error, **Open Login Items** opens the system pane. Only explicit user action
requests registration.

Registration requires the release build and its bundle identifier, installed in
Applications or a user's Applications folder. The host checks the canonical
main bundle path. The Developer ID-signed main app uses the system
Service Management framework directly, with no extra entitlement, helper or
service plist. During startup it reads the open-application Apple event's
`keyAEPropData` for `keyAELaunchedAsLogInItem` while AppKit handles the launch.
The installed app creates the window hidden and shows it immediately for a normal
launch. A login launch leaves it hidden unless the tray fails to initialize.

Existing v0.1.6 tasks keep their authority when their saved status is active
or completed, or a retained run snapshot records an active task. That version
did not store a separate approval event. A paused custom task without that
evidence asks for approval before any run. New approvals survive later pauses.

## Triggers

A trigger is a pinned script version the host runs on a custom task's schedule, with no model call. The definition, the script's `script_state` (the lifecycle field is `state`), counters and up to 20 evaluations live on the task. The code lives in `scripts`. The host runs only the approved hash. A pending version waits while the approved one keeps running with its own arguments and tool and provider grants. A successful dry run saves that version’s terms, arguments, observed grants and replay scores. The card shows the candidate’s terms and arguments and the tool receipt returns its own dry run. Library changes and bundled presets need Test now before approval. Test now validates the displayed task revision, script version and hash before running the check or making a provider read. Editing an unapproved check clears its dry run and requires another test. Going back restores the tested configuration for the approved version. Evaluations use a sealed Python snapshot: no workspace files, no attachments, and nothing writable except scratch. The host serves approved reads between worker starts. Run now evaluates an enabled trigger with the same verdict requirement as the due poll. A trigger preset always uses the check proposal path. Not met writes no run. The host reserves the remaining daily read allowance before each evaluation and charges the actual reads when the worker returns. Capacity adds the live dry run’s reads per check on each local calendar day and excludes the definition being replaced. Weekly checks contribute only on their run dates, and interval checks count the active window on each eligible day. Met posts a card on the home Thread and starts at most three agent runs a day for that check, and at most six triggered runs a Loupist a day. Three code errors in a row mark the trigger `failing` and stop evaluations. Learning tasks are not in this build, so a check sits on a custom task. The task's schedule is when the check runs, at least every five minutes, daily or weekly.

Checks can use `followed` and `price` for saved data, KRX daily bars and
OpenDART filing lists, and bundled OpenBB equity history, filing lists and
news. Provider reads use the Loupist's current connection grants and the
usual as-of rules. OpenBB news has no historical cutoff support. A price-move
preset with named subjects checks only followed assets matching those names or
the selected asset IDs. The candidate’s reviewed subjects and terms take effect
together on approval, and reverting restores the prior subjects and terms. A
figure in a verdict requires a successful read
with a retained source. Rate limit retry hints in the read ledger defer
the next check even when another read succeeds and the verdict is usable.

The host refuses a check whose script was saved after outside text. It screens
the complete run before keeping a replay ledger and screens the reading,
evidence, error and carried state before keeping or displaying them.
Checks use the task's Memory setting. A proposal test can read Memory when
the originating turn also allows it. Schedule tool results include a bounded
check history when both settings allow it. Chat receipts identify the task
and its recorded action. The task store holds the worker state and replay data.
Check readings, errors and evidence can contain provider-authored text. The
host treats those fields as outside text in proposal results, schedule history,
Bot cards and the explanation turn. It raises the trust tier before handing
the model those fields.
Proposal replay starts with empty state on the oldest of the previous 20
weekdays. It carries state forward through successful evaluations and charges
every data attempt, including refusals and unavailable results, against the
80-read proposal budget. The current follow list and saved quote cannot supply
a historical snapshot. Version comparisons start from empty state at each
recorded reset, including approval of the same script version and revert.
Each saved evaluation carries its boundary and position. A missing read ledger
makes later checks in that sequence not comparable, even after the loss ages
out of history. The comparison also skips a sequence whose reset or prefix
was pruned, or whose records lack the freshness state at reset. The candidate
replays every retained input, including reads from a failed live check, and
carries its own state through successful results. If the candidate needs an
unrecorded input, it and subsequent checks stay not comparable until a reset.
The update card counts only paired verdicts from the currently approved
script pin and shows how many of its checks were not comparable. The task store prunes the oldest evaluations when the serialized definition approaches its reader limit. The latest result keeps the displayed evidence and source names without duplicating the replay ledger. A row that cannot be decoded still appears in Schedule as unreadable.

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
running World load (Recover World building, or the load after a shared pack's Add or
Remove) runs before any extraction; the load after Add or Remove cancels an earlier
load still waiting.

### Scheduled Chats

Scheduled work reuses ordinary Chat admission, execution and persistence. The task view and schedule tools resolve the requested model against the current catalog when read. An unset task keeps following the host default, including after an account change. The tool receipt names the current model profile or gives its unavailability reason. New briefings keep an unset Chat model unset. A saved
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
opens from the run reader’s Continue in Chat (Open the session after the user writes) and Search; Bot’s delivery opens the reader; the sidebar
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
without a scheduled task. Typed hand-offs use the unattended allowlist; World-link
hand-offs use the Session surface. A `created` row with no run is sent once by
the next delivery pass in Bot, a `running` row whose run is gone is marked
interrupted, and a run that started is never sent again. Legacy
confirmation-pending tasks remain paused. Bot proposes activations and recurring
edits for the user to confirm, including briefings and schedules handed off
from Chat. An existing task keeps its approved definition while a replacement
waits. A triggered watch also keeps its approved script and check settings
until a schedule edit is confirmed. Script approval binds the check card’s task revision, version, and hash. The host rejects a stale review and generic schedule controls leave unapproved checks paused.

The shared Rust service handles create/edit, pause/resume, delete, Run now and
Stop. Edits check definition revisions; Chat creation keys prevent duplicate
schedule creation. Proposals persist immediately and remain available if the creating reply fails.
A replacement is applied in the same transaction that consumes its proposal. A task cannot overlap itself and shares ordinary Chat capacity.
Every schedule created by Bot or handed off from Chat waits for user confirmation, including requests to save it paused. The host blocks both manual and automatic runs until that first confirmation. Run now attempts admission immediately; it is not a durable manual-run queue.
Pause prevents future starts; Stop cancels the current run separately. Run now also
starts a paused task without turning on its future weekday schedule. Task detail offers Run now and Pause/Resume; a completed one-time task needs an
edit before it can resume. Custom tasks have Delete under More, with confirmation,
disabled while running; it takes the task's run history and the run sessions the
user never wrote in ([run sessions](#scheduled-chats)). Briefing settings turn each edition, morning or evening, on or off. A run delivered `to: briefing` is not posted in Bot; the next briefing lists it under `backgroundRuns`. The user can
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
receipts from that window count. If admission returns `provider_unavailable` because
Auto has no connected AI account, a recurring task pauses on its first failure.
The stored pause status survives changes to the account hint. The sink carries typed NeedsReconnect/ModelGone
failures into the task policy so rejected credentials or a missing model pause
immediately, even though the Chat's failure text is the generic placeholder. A newer edit still prevents an old completion from changing the definition.
History keeps a task's last 50 runs and records status, timing, error and target Chat, where the reply and its evidence remain. Renderer discovery polls every two
seconds while the window is visible and every 30 seconds while hidden, and reads
immediately when the window becomes visible or regains focus. A finished run is also
announced in the Loupist's [Bot](../ui/home-thread.md), and so is one that recovery
found unfinished after a crash, as one that could not finish (`deliverable` in
`chat/home.rs`); a Stop, a quit and a quiet run post nothing. Desktop notifications
identify custom tasks by title and distinguish stopped runs from
failures. The notification queue commits each group’s reservation before
submitting it to the OS. A confirmed refusal can retry, and an uncertain
acknowledgement consumes the reservation. The Bot result stays available if
the app exits before the OS displays its notification. Only a user-stopped run says "stopped"; crash recovery and other
interruptions say "couldn't finish". There is no meaningful-change filter, saved
per-task connection allowlist, or per-task native-execution preference.

Each scheduled task is `custom` or `briefing`. A Loupist has one morning briefing
and one evening briefing, each with its own Memory-enabled Chat. The morning
default is 08:00 and the evening default is 18:00, Monday through Friday in the
device time zone. Morning rows written before editions stay morning. At run time Rust supplies
the date and the start of the latest successful briefing across both editions,
both at the user's UTC offset, and the elapsed time since then. Growth and
background results use that same lower bound, so results that finish while a
briefing is running can appear in the next one. Rust also supplies the followed names and reference
fields with saved-read state, principles, up to 40 World growth summaries, and up
to five due Journal pointers. The briefing context keeps up to 20 disputed
entity IDs for the World filter. The unattended agent chooses its own data reads
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
When an enabled data connection's saved key still needs macOS permission,
Chat omits its tools from executable authority and includes the recovery reason
in the model context. Direct calls return the permission refusal. Scheduled
runs and hand-offs proceed without these connections and carry their names
and the reason into the result. Schedule readers and Bot posts render that
reason in the interface language. Users grant permission in Settings >
Data connections ([credentials](../capabilities/connections.md#connections-and-credentials)).
The inbox is read with `inbox_list`, `inbox_mark_read` and `inbox_unread_counts`.
The agent's schedule reader still uses the internal run history. Completed,
failed and interrupted receipts share one unread state.

### Quiet custom runs

An alert is a custom task with a prompt specifying its condition and the evidence
to check. A run that completes the check and finds nothing to report can call
`schedule_quiet`. The first run and every run of an explicit `every_run` task require a report. The host also refuses quiet delivery if any tool was skipped or a
data call was attempted without any acknowledged original data result. The
reply and its Memory still commit if the run can no longer be marked quiet.
The quiet decision belongs to the saved answer. It can survive a skipped
Memory save, which the run records separately. A non-quiet answer with skipped
Memory still reaches the Schedule history and Bot. The run, its delivered
record and the next run's `previous_run` carry the same skip reason.
A successful quiet run stays in history, where Schedule marks it "Nothing to
report" and never unread. It leaves the Chat's sidebar recency unchanged and adds
neither an inbox entry nor a Bot announcement. Failed runs use regular failure
handling.

### Watch definitions and delivery

Watch metadata lives with the task definition in its existing JSON record. The store validates the eight-subject and one-line terms limits and caps active watches at 20 per Loupist. A proposal stays paused until the user confirms the revision displayed on the card. The activation transaction rejects a stale revision. Run now uses the same approval guard as the due poll. A recurring watch becomes due as soon as it is created active or confirmed. A one-time follow-up keeps its appointed time.

Each run freezes the task definition it used. Its report policy controls quiet delivery, and its delivery destination controls whether the result enters the Bot inbox. A provider transport failure recorded as `unreachable` stays in run history and leaves the pause streak unchanged. Counted failures in one streak update one Bot card.

A preset occurrence resolves its selected subjects against the live followed list. When a filings watch loses a followed asset, the host refreshes the bundled prompt’s asset slot for that run and retains its original language. The saved prompt stays intact and continues to identify the watch’s previous runs. If an explicit scope has no subjects left, the host pauses the watch.
