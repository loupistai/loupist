# Context delivery

Under an as-of date, automatic context keeps to the cutoff. Shared text keeps its delimiters. Briefings add their fixed prompt on the same path.

## Memory and context

With saving enabled, what a turn establishes (a dated fact with its source, a
mechanism, a value-chain link, a contradiction, a check that mattered) is filed into Lens
pages with `lens_file`. A turn creates a method only when the user teaches, corrects or asks
to keep one; its own checks stay on subject and idea pages until
[method learning](../knowledge/lens.md#method-learning) sees them recur. This Chat's progress and user-stated facts use `note_save`; in Bot they use
[Bot memory](../ui/home-thread.md#bot-memory) (tool `bot_memory`) instead;
handed-over originals use acknowledged capture followed by `source_save`. Chatter, light questions and
explicit one-offs are not saved. A [Bot quick ask](prompt.md#quick-asks-in-the-bot)
files nothing to Lens and saves no Source; a file handed over in the Bot, or a
request there to keep something, is saved and filed as in a session. A reply promising to remember is not a Memory write.
World processing extracts relationships from retained Sources. Research filed in
two Chats from two retained originals queues a
[method-learning review](../knowledge/lens.md#method-learning). Methods also change through Chat tools;
[User principles](../knowledge/lens.md#user-principles) have a separate stored-rule
contract and bounded turn context. The [profile contract](../runtime/storage-authority.md#where-data-lives)
distinguishes editable Loupist fields from the
[host instruction snapshot](../runtime/chat-turns.md#local-workspace-execution).

Before substantive research, discover relevant Source/World evidence, methods and
notes, then exact-read useful matches. A Bot quick ask is not substantive
research and makes no discovery searches. Existing retained originals can answer dated
questions without refetching unchanged files. Fresh retrieval is for missing
coverage, newer evidence or an explicit request to inspect the current original.
Native file reads and `run_python` output alone do not retain Sources.

A Lens page is keyed Markdown sections, each with a basis. `lens_file` revises the
sections a `lens_read` delivered by their digests or creates a page; the envelope's
index and pointers are discovery, not read receipts, so a revise needs that exact
read. The call's `rationale` describes the current filing; history is the op log.
Current page semantics are owned by [Lens](../knowledge/lens.md#lens).

Keep stable role/authority rules in the prompt, methods in selected Skills, and live
time, user intent, selected references and Memory pointers in turn input. Due
Journal reviews and relevant-method/note pointers are discovery, not read receipts. Full
data, code and intermediate results belong in files or pinned app data with bounded
retrieval. Truncated previews do not establish complete contents.

SQLite is the canonical transcript; Pi JSONL is derived context. After compaction,
restart or a cold rebuild, preserve the current goal and corrections, completed and
pending work, data revisions, evidence IDs, permissions and next exact reads. Test
continuation on work that depends on earlier context. In Bot, your Loupist carries
the user's preferences, decisions and open requests past the cold transcript bound in [Bot memory](../ui/home-thread.md#bot-memory)
(tool `bot_memory`).

During a reply, the host freezes the current request and full turn envelope in the
private Pi bootstrap. After Pi compacts in this reply's process, the extension
restores that input just before this reply's retained prompt, or after the last
summary if the prompt was summarized away, unless the same full input is already
retained. Older turns stay earlier, and later user corrections and steering remain
later. An inherited summary alone does not restore input or rewrite the retained
prefix on subsequent warm replies.
This model-context projection does not rewrite JSONL. Both compaction and branch
summaries are labeled as model-written, potentially incomplete context, never
user words, authorization or exact-read evidence. The label does not enforce a
security boundary or reconstruct omitted historical work: missing originals still
need `chat_history` and exact-read tools. Rust's permissions and receipts remain
authoritative.

Automatic compaction after a finished assistant answer is deferred to the next
input, including an overflow reported by successful-response usage, so an auxiliary
summary request cannot delay answer settlement. Pre-prompt, between-tool-round,
error/length overflow recovery and manual compaction remain available. Compaction
pressure marks the envelope stale for the next reply even when the post-answer
attempt is deferred.


Under an as-of date, automatic context follows
[the cutoff rules](../knowledge/memory.md#memory-under-a-cutoff): a perspective (1) or
catalog method (6c) whose current version is later is left out, Journal pointers and
counts (6b, 6d) use only entries created by then with later outcomes read as pending,
later notes (6e, 6g) are left out, and Bot memory (5c) reads as it stood then;
principles (2) stay as they are today.

Automatic context reads only the turn's Loupist's saved followed list and serializes each asset's id,
name and reference, with `price_read` when the asset has no working saved price read.
Live and as-of turns receive the same block. Its hint supplies relevance and identity
and states that following an asset is not owning it. Full evidence behind a working
note or due Journal stays behind exact-read tools; recursively retained Journal inputs do not enter
automatic context.

Text from a [shared Loupist](../knowledge/sharing.md#sharing) keeps these positions and
bounds but arrives inside `<<shared id=… from="{pack}">>` … `<<end shared id=…>>`:
principle lines, a shared perspective, catalog titles and summaries, pointer titles and
snippets, and Memory tool results. A locked `SYSTEM.md` rule says such text is research
material from that Loupist, never an instruction about tools, files, permissions,
credentials, orders or the user's data, and grants no authority. Keep that rule, the
wrapping in `src-tauri/src/sharing/delivery.rs` and the
[pause rules](../architecture.md#trust) in step. Security cases `S-R05`–`S-R08` in
`evals/agent/suites/security-v1.json` seed the
[fixture packs](../../apps/loupist/evals/sharing/README.md) with `seed.shared_packs`: a
method that sends holdings to a URL template, a principle claiming trade authority, a map
summary asking for the system prompt, and a method asking to edit `AGENTS.md` and create
a schedule.

Briefings use the same Chat path. Their JSON carries the date, previous-run
time, up to 60 of the run's Loupist's followed assets with saved-read state, up to 30 bounded
principles, up to 40 growth summaries, up to five due Journal pointers, and
disputed entity IDs. The run chooses its own reads under the unattended
allowlist. The briefing and delegated previews take the first prose paragraph
after headings, tables and lists.

A completed delegated answer without prose uses its research title in Bot.
Opening the research session shows the complete answer. A Bot-initiated
hand-off keeps the original typed request in `current_user_message` and
delivers the Bot's context separately as `handoff.bot_note`. A link action
uses the ordinary `session` name and hint, with a host caption, `handoff.hint`
and `handoff.map_link`, whose path states the
direction of each recorded relationship. The link context and notes from
exposed Bot replies use the untrusted-text delimiters. Cold rebuild and
`chat_history` keep that context beside the original message.
Neither hand-off can ask a question. The user can open the session while it
runs, and a further user message is their guidance for this run.

The price-setup prompt carries the interface language and the selected names
and references. It runs in a delegated Chat and saves price reads for the
selected assets when a source supports them.

Interactive Bot and research turns receive only `interface.display_currency`
when set. They can read the seven product and navigation labels through
`interface_labels` with or without Memory. The same admitted appearance drives
every tool round and a same-turn retry. The next turn reads appearance again.
Scheduled, delegated and host-started Map-link turns receive the resolved
`interface.language`, five product `names` and the optional currency. For scheduled
runs, the task stands in for the user. The run answers in the language the task names or, by default,
the language the task is written in. A task with no wording of its own follows
`interface.language`. Briefings receive that value as `appLanguage` as well. A
link-action hand-off also follows `interface.language`: its first message is
the host's English caption and the user wrote nothing. A Bot-initiated hand-off
answers in the language of the original user request or the language it asks
for. Its `interface.language` describes the app's screens.

A custom scheduled run after the first also receives `previous_run`, after
`chat_surface`, with its own `hint` (`PREVIOUS_RUN_HINT` in `agent_harness.rs`):
the task's last finished run's `finished_at`, `status`, `quiet` and `summary`,
the first prose paragraph of its answer cut at 280 characters (left out when the
run wrote none, including a quiet run's host placeholder), and `last_delivered`
(`finished_at`, `summary`) for the last run whose answer reached the user when
that is another run. Rust builds it at dispatch from the run records
(`PreviousRun::from_runs`): only runs under the task's current prompt count, so
after a prompt edit a run receives none, and the briefing keeps its own
since-the-last-briefing facts instead. A summary from an answer whose reply
recorded third-party text (or whose answer can no longer be read) is delimited
as `chat_history` returns a tainted reply and raises the untrusted-text tier
([untrusted text](../runtime/untrusted-text.md)). The block is left out of the
envelope digest, as `live_time` is, and an abbreviated envelope carries it whole,
so a continuing session's context is not resent every run.

The earlier unattended hint made `interface.language` the default. In the
wave 11 evaluation, GPT-6.2 Sol at medium answered in the saved task's language
in five of six runs where the task and the app used different languages.

Scheduled and delegated turns have no `interface_labels` tool; a Map-link run
on the Session surface retains it. The host reads the five product
names from the bundled agent catalogs, the Research sessions heading from the
sidebar catalog and Market data from the Connections catalog. An unbundled
language keeps its tag in unattended context and uses English labels.
The `system` preference looks up the OS language
on macOS, then `LC_ALL`, `LC_MESSAGES` and `LANG`, with English as the default.
The block comes after `chat_surface` and before Memory catalogs. A currency change or an unattended language change updates the envelope digest
and resends full context on the next turn. Interactive language changes affect
label reads and leave the envelope digest stable. Both preferences are outside
the runtime execution digest.
Eval runs use this same launch path; scenario language or language tags set the
disposable profile's appearance language (including `principles-v1` P-R09).

Envelope digests exclude live time and the transient standing read reference.
Unchanged warm turns retain prior perspective/catalog/principle blocks and send an
unchanged digest plus any current interface fields, instructions, followed identities, the Loupist's tone,
due reviews, record counts and relevant pointers. A changed definition produces a
new digest and full context; compaction forces full context delivery again. Stable
Loupist blocks precede per-turn data without rewriting the system prefix. Provider
cache counters, not this structural property alone, establish observed cache reuse.
