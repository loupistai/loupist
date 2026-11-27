# Learning programs

Learning programs run a research brief on a schedule while Loupist is open.
Schedule shows the brief, target subjects, request budget and past runs. Each
run opens a fresh research session, with the most recent completed run as the
starting point for changes. A failed or stopped run keeps the previous starting
point. That start is what `world_changes` calls `previous_run`, and the feed
reports `first_run` true until a run of the program has succeeded. Each
Loupist can enable up to 20 programs. Its config's `learning_program_cap`
sets this from 1 to 50.

Track daily creates one program for a followed asset. After the first run,
it runs on weekdays at 06:30 in the local time zone at activation. It checks for material
developments and can update the asset's Lens page, thesis read and saved price
read. Turning it on again reuses the existing program.

## Presets

A preset is a bundled program template in `agent/learning-presets/`: Track
daily (`asset-daily`), `theme`, `company-filings`, `macro-series`, `question`
and `market`. A preset states its title and brief templates, its slots, the
subject it takes, and its default depth, schedule, scope, keeps, report and
delivery. `company-filings` takes a followed asset, as Track daily does.
`theme`, `macro-series` and `market` take their subject's name from a slot;
`market` has no default market. `question` has no subject.
`scheduled_task_from_preset` fills the slots once and creates the program.
A slot the preset does not declare, or a declared slot left empty, is refused
with `preset_slot`. No preset file states a time zone: the program gets the
user's zone when it is created. When the system zone has no readable name,
the program gets a zone with the system's current offset, not UTC. The
program keeps the preset id in `program.preset` for display. After creation
it is ordinary data. A program saved by Bot keeps the preset id it names.
When Bot also passes `slots` to `learning_program_save`, the preset must be a
bundled learning preset: the host fills its brief and subject through the same
fill as `scheduled_task_from_preset`, and Bot gives no `prompt`. Bot's title,
schedule, depth, scope, keeps, report and delivery stand. The same zone
fallback answers `schedule_list.local_timezone` and the digest's next-briefing
estimate.

## Budget and delivery

One program can run at a time across the app. The allowance view reports whether
that slot is occupied. Automatic learning runs can use up to 132
requests per Loupist per local day, within the shared background allowance.
The Loupist's config sets this as `learning_daily_requests`. At 0 every
learning run is held for the allowance. Any other value must be at least
the hard budget of the largest light update (26 requests), so each light
update can finish. A lower value is refused as `invalid_configuration` with
the reason `below_learning_update` and the minimum. Both learning settings
belong to a Loupist's config, not the global one.
The scheduler gives a due program a request budget. At the soft level the
model receives a note to wrap up. At the hard level the host refuses more
data reads and allows the approved saves. The host stops the run after eight
more requests, five minutes at the hard level, or 30 minutes overall. A run
stopped for its budget discards staged Memory changes.

A program can report to Bot, to the next briefing, or only in Schedule.
Briefing delivery falls back to Bot when the briefing is off at admission.
The first run and programs set to report every time require an answer.
Every program run stays in Schedule with its outcome and recorded usage.

## Editing, Undo all and learned marks

`learning_program_update {loupistId, taskId, revision, patch}` is the user's
own edit from Schedule, so it saves without a proposal. The patch can hold
`title`, `prompt`, `subjects`, `schedule`, `depth`, `sources` (the program's
`scope`), `keeps`, `report` and `delivery`. The patched task passes the same
`Task::validate` that `learning_program_save` reaches through the store. A
refusal is `invalid_request` with detail `{field, reason}`, naming one of
those patch fields. `sources` may not add a connection, or the web, that the
Loupist does not grant, except one the saved scope already held. A revision
that is not the saved one is `stale_revision`. A run already started keeps
its snapshot; the next run reads the new fields.

`learning_program_undo_all {loupistId, taskId, preview?}` undoes every run of
the program, newest first, through the per-message undo, in one transaction.
It returns `{runs, undone, kept}`: the runs it undid, the Lens ops it
restored, and the sections a later change kept, each counted once. A run
already undone, or one that filed nothing, is skipped and not counted, so a
second call returns zeros. With `preview` it counts the same way and rolls
every row back. Sources stay, and a thesis move keeps its own Undo. If
successive runs of the program revised the same section, Undo all restores
the section to its state before those runs. Later edits still in effect,
such as the user's own edit, are kept.

A Lens page's `SectionView.learned {atMs, taskId, runId, programTitle}` is
set when the section's newest op was filed by a learning run's answer, matched
by the run's chat and message in `knowledge_lens_log`. Any later op on the
section, an undo included, clears it. Nothing is stored.

A research session forwards a request for a program with `learning_handoff
{request, subjects?}`. The request waits in Bot's question queue under the
`bot-learning-` prefix and is posted in Bot, which drafts the proposal with
`learning_program_save`. Saving the proposal or declining the request removes
it from the queue. An unanswered request expires after 14 days and is removed
the next time Bot uses a schedule tool. A research turn that has read
third-party text cannot forward a learning request.

## What leaves the Mac

The AI provider receives the program brief, permitted tools and their results,
subject names, and the Memory context needed for the run. Data providers see
the reads the program requests. The program can use the connections already
enabled for its Loupist and can narrow that list or turn off web research.
Its save choices determine whether the run can retain Sources, file to Lens,
or update the saved reads on the asset subjects it follows.

The brief and program settings stay in the Loupist folder. The program cannot
manage schedules or connections. The [learning Skill](../capabilities/skills.md#skills-management)
provides the research procedure.
