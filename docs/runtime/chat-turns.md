# Chat turns

A turn freezes its authority in Rust, then Pi runs in the selected workspace. The workspace is a starting directory, not a sandbox.

## Chat

Chat admission returns `memory_busy` while a Memory writer holds the Loupist.
A queued submission can retry when that active `memory_write` leaves the run list.
Completed chats waiting their turn to save keep their thread reserved and leave
other conversations free to start. A same-thread conflict or a full worker pool
keeps its own refusal.

SQLite owns the transcript. Pi JSONL is a derived cache. Rust prepares the turn
once in `commands/chat_runtime/setup.rs`, capturing model, Memory policy, connectors,
Skills and workspace resources. Each turn launches a disposable Pi process with
fixed authority. Cache reuse requires matching transcript, runtime, configuration,
resource and connector seals; changed valid seals rebuild from SQLite. The runtime
seal includes the turn's as-of date, so changing the date starts a cold session.
Confirmed Pi shutdown is required before cache reuse or cleanup.
Pi's boot and its first pipelined controls may take 60 seconds, because a loaded
Mac boots Pi slowly; a later control on a running Pi has 10 seconds. Stop or a Pi
exit ends either wait at once (`pi/execution.rs`).
Cold sessions retain historical attachment metadata even when an original is missing
or changed. Restored text prefixes have a separate 1 MiB budget; at most two
historical images share the turn's 5 MiB image budget with current images.
Extracted Markdown is labelled separately
from digest-verified original content.

On the host's first locked open of a Loupist folder, Rust settles an admitted
request that still lacks a terminal reply. This happens during startup for
available folders and when an unavailable folder is opened again. The
transcript gets one stopped reply and keeps the request and accepted guidance.
A saved draft or other terminal reply stays as written. Live chunks and pending
question or approval controls are process-local. The session, history and export
all use the stored status. An independent Bot post can arrive after the stop and
the stopped turn still offers **Try again**. A scheduled request runs again
through its task's **Run now**. A completed answer waiting for Memory keeps its
answer and interrupted-save receipt.

Brokered reads become usable evidence only after client acknowledgment. The
turn-local `result_ref` records payload and provenance; it expires with the turn.
Persisted `ChatEvidenceRef` snapshots remain inspectable even when Memory is off
or a reply fails. A read receipt proves delivery, not truth. Native file calls and
`run_python` produce Chat activity, not broker evidence or immutable datasets.

Host-issued citation links resolve against the message's evidence. A citation of a
credential-free web page opens that page in a browser tab, one tab per address.
The saved passage stays in the Evidence reader (one tab per Chat with back/forward
history) and is one step away when the page cannot open. Citations without a usable
URL open that reader directly. An equity price-quote basket stays an inline receipt
and does not open the raw excerpt. Derived and saved-context results stay inline under their step. Free-form text cannot mint evidence
authority. Chat artifacts are Markdown documents, charts and `grid` tables; a table's
cells name evidence by id, and `chat_artifact_read` returns each retained revision
with the cited evidence of the message that published it, so its
[checked cells](../analysis/grids.md#grid-cells) and exports work after a reload or a Chat
branch change. The lazily created `chat_artifact_revisions` table keeps bounded
snapshots in the same database, without changing its schema version; deletion of
an artifact cascades to its history. Publication, restore and pruning are atomic.
A delivered `chart_publish` or `grid_publish` call also streams its staged content
(`artifact_staged`) so the card fills at once. A revision in the same reply replaces
the staged card under the same artifact id and revision. That event writes nothing;
only the reply's commit stores one final revision per artifact and sends its
`artifact` reference. A failed or undelivered revision leaves the earlier draft,
and cancelling the reply drops all drafts.
[Artifact history](../analysis/charts.md#visualizations) describes retention and revision reads. [Item surfaces](../analysis/surfaces.md#where-each-item-appears) owns that UI mapping.

An acknowledged data read keeps its ordinary evidence receipt. The saved price
read path can use that receipt to register a repeatable provider call, then
replay it outside a model turn. See [Saved price reads](../knowledge/asset-pages.md#saved-price-reads).

Host progress keeps finish times even for same-millisecond
steps, and `failureReason` is one of `timeout`, `unreachable`, `provider_error`,
`empty_result`, or `invalid_input`. Only these fixed codes cross the presentation boundary;
provider error prose stays out of the row's reason.

### Local workspace execution

Pi's CLI CWD stays in a private runtime directory. Native `read`, `write`, `edit`,
`ls`, `find` and `grep` operate from the selected workspace, the Loupist
folder. Rust refuses file calls outside admitted roots: the folder, this run's
scratch and this Chat's attachments. `run_python` is the code execution tool.
It stays allowed after third-party text and in unattended runs, without a card
([Native tool permissions](permissions.md)).
Workspace reads and folder selection report a bounded, actionable error rather
than raw filesystem diagnostics. File changes are immediate and survive a failed
or cancelled reply. Local-tool activity
records are published immediately; their latest file snapshots are synced together
at the turn boundary, including cancellation and recorder drop. Approval and
question waits remove their registry entries when their response future is dropped. Python may
write only host-granted workspace subfolders, `outputs/` and this run's scratch.

`workspace-tools` holds pinned, digest-verified `rg` for offline content search
(`apps/loupist/scripts/stage_workspace_tools.py`). Rust supplies a bounded manifest
of individually authorized entries. `ls` and `find` display/filter that list;
`grep` passes identity-checked file descriptors to `rg` as stdin, so the engine
cannot traverse private subtrees. Rust reads only admitted local `.gitignore` and
`.ignore` files; host git config and `.git/info/exclude` are never consulted.
Sidecar checks fail if `rg` is missing, linked or altered.

`run_python` does not use a shell PATH. The worker receives no host environment.
Its offline packages are the pinned WASI stack staged by
`apps/loupist/scripts/stage_python_runtime.py`.

Rust snapshots bounded instructions in this order: global `AGENTS.md`, one selected
workspace-root instruction, then `.loupist/AGENTS.md`. Root precedence is
`AGENTS.override.md`, `AGENTS.md`, `AGENTS.MD`, `CLAUDE.md`, `CLAUDE.MD`.
Ancestor/nested instructions, automatic project settings and JavaScript extensions
are not loaded automatically. Skill precedence is bundled, project `.loupist/skills`,
`.agents/skills`, `.pi/skills`, then global `skills`. Rust and Pi verify the admitted
paths, bounds and hashes. Changes invalidate the session cache.

Explicit `$skill-name` mentions select methods without changing Memory permissions.
`@` context supplies selected references or file paths; it does not acknowledge a
read. For a small text attachment inside the workspace, the host supplies its relative path and digest for workspace_capture. The response cites the returned evidence link. Saving the original as a Source requires Memory writes.
Rust labels the captured attachment from the message's upload record. Reader
and receipt labels use that name while the capture keeps its exact file locator.

The composer keeps unsent drafts and pending turns per Chat during the app
session. Live guidance accepts text; attachments/references belong to a later turn.
Try again resubmits the original failed/stopped request, its saved attachments and its
as-of date using current settings after clearing its derived cache. Accepted `steer`
messages stay in the history and are skipped when choosing that request; local file effects remain.

### Turn authority and finalization

Rust independently gates Memory reads and writes. Search, Skill selection and
renderer inspection do not grant exact-read or write authority. Broker delivery
uses acknowledgment and a commit barrier; an indeterminate barrier aborts the turn.
After Pi settles, Chat stores the generated answer, checked numbers, and
published artifacts before waiting to save Memory. The live saving state
separates remaining storage work from writing the reply. The save waits in FIFO
order and takes exclusive admission in the same coordinator step that confirms
every reader has drained. **Skip saving** ends the wait and keeps the answer
and its outputs. The wait
also ends at ten minutes. If the app exits before saving, the stored answer
and artifacts carry the interrupted-save receipt. Scheduled results and
research hand-offs still deliver the completed answer with its skip reason.
Qualified quiet checks stay quiet and keep their reason in run history.
If writing the answer itself fails, the live view keeps the generated text
available for copying and shows the save error. Rust tries to store a generic
failure placeholder so reopening the Chat shows the error.

The Memory transaction updates the Chat receipt with authorized changes and
graph jobs. In Bot, changes to [Bot memory](../ui/home-thread.md#bot-memory)
(tool `bot_memory`) and its review queue ride in the same Chat compare-and-swap. It also records acknowledged Lens delivery receipts
and usage, including principles actually attached to bounded context when the turn
saves no new Memory. The host records each receipt's kind and revision; model text
cannot set it. A conflicting Lens update becomes a proposal. Failed or cancelled
turns publish no staged Memory or completed delivery usage; immediate file,
dataset-import and schedule changes have their own persistence boundaries.

A user question or approval parks its tool call. Once all parallel broker
calls finish or park, the Chat releases reader admission while keeping its
thread and Stop control. The tool re-enters as a reader before it continues.
An unpinned read or search can see a newer head after the wait. Exact delivery
receipts keep the revision that was actually read. If an applied Lens link
points to a different current revision, or the item is paused, the host leaves
out that application row and records it in the run diagnostics. A Memory reset
during the turn skips the turn's staged Memory transaction. The background
method worker and World enqueue use the current learning settings.

Chat progress keeps the saving start time, terminal save time when observed,
time spent waiting for Loupist admission, and the last single blocking
conversation identified by the coordinator. The live event carries the same
facts as the stored progress. An abrupt quit can leave a saved answer without
an observed saving end time.

A turn's as-of date (`ChatSendRequest.as_of`, `YYYY-MM-DD`) must lie between
1990-01-01 and today on the device; admission refuses any other value before the
user turn is appended. Rust stores the date on the request and on every terminal
answer, including failed and stopped replies; those replies also retain any replaced
model name. The host's
`chat_retry_last` returns it so Try again repeats it. The turn's tool policy carries
it, and a tool's own `as_of` argument can only narrow it. With a date, setup filters
automatic context ([Memory under a cutoff](../knowledge/memory.md#memory-under-a-cutoff)),
the turn envelope gains a `loupist-as-of/1` block beside live time (the cutoff, its
end of day, the rule and `not_evidence`), and each tool applies the cutoff
([point-in-time answers](../analysis/point-in-time.md#point-in-time-answers)). As-of refusals keep
their own broker error codes: `as_of_unsupported` (the source cannot show that
date), `as_of_empty` (nothing dated by then), `as_of_later` (a saved item or page
from after the date) and `as_of_before_check` (a Journal review before its check
date). `source_save` refuses undated material with an argument error whose message
starts `as_of_undated`, under the code `malformed_request`. A step whose tool failed
with `as_of_unsupported` gets the progress status `skipped`, not `failed`.

Rust records the checks before Memory admission and supplies them to the
renderer when saving begins. `number_checks`
(at most 200) holds the [number checks](../analysis/number-checks.md#number-checks); a
number link past that bound stays a citation with its text as written. An
as-of turn's `as_of_report` holds its cutoff; the tools refused with `as_of_unsupported` (with
their connection) and the reads that returned `as_of_empty` (with a target taken from
arguments such as `symbol`, `corp_code` or `query`), each listed once and at most 16;
and the Memory left out, counted by kind across context assembly and tool reads.
Message validation allows them only on answers and checks their bounds.

A settled turn with acknowledged Memory writes but no answer text can receive a
host-composed receipt summary. Unfinished tools, provider/process failures and
storage errors do not qualify. Host diagnostics are bounded/redacted and exclude
raw credential-bearing payloads. A failed commit keeps a content-free
[phase and reason category](../dev/diagnostics.md#diagnostics) in the local run log
and eval trace; the host never copies the raw storage error into that category.

Pi's `agent_settled` remains the completion barrier; a visible final text chunk
alone cannot commit Chat or Memory, bypass Stop, or discard acknowledged steering.
While saving, Chat keeps the session active and lets the reader copy the answer
and open its citations, figures, files and Activity. The status names and opens
a single identified blocking conversation. New messages queue, and feedback and
edits wait for the turn to finish. Other conversations can start during the
wait. The Memory busy notice applies during the exclusive transaction.

On completion the receipt shows any skip reason. The warning also appears on
delivered Bot results and in the Schedule run reader. Reasons include a user
request, the wait deadline, app exit, save failure or a Memory reset.

The extension defers automatic compaction after a finished answer until the next
input. A cancelled or deferred attempt leaves no successful or failed compaction step in the
answer's progress. Compaction before a prompt and between tool rounds retains its
normal error, timeout and cancellation handling.

### Research hand-offs

A successful Bot reply can commit one acknowledged `research_open` request.
Its assistant message and live completion carry the new research session’s ID
from the same transaction.
The child model receives the user's original message in Bot. Rust resolves it
by Loupist, thread and message ID, including retained Chat versions. Bot's title,
question and optional brief arrive separately in the hand-off note. The transcript
projects the brief, or the question when the brief is absent, as a Bot-authored
opening. This projection also applies to sessions saved by 0.1.2. An identical
retry of that source turn returns the same receipt. A later Bot turn can reuse the model's
`requestKey` and start its own hand-off.

A retained **Look into it** action supplies its scoped connection and the
user's current composer choices, including Auto. Rust records the opened
status with the target. While the request is queued or running, identical
retries return the same receipt and different settings conflict. After it
finishes, fails or stops, the next click starts a fresh session and keeps the
earlier one in history. **Try again** on a stopped link session uses the same
scoped action and opens its new session. An unavailable link leaves storage unchanged.

Bot posts nothing while the run works. While this app holds the run, the session's
thread carries `handoff_run`, and showing the session attaches to the same Chat
stream used for an ordinary turn: the host sends the opening transcript, replays
current progress, then follows new events. Each viewer can detach without
cancelling; a lagging viewer gets a reset signal and can reload the snapshot.
Guidance (`chat_steer`) steers the existing run, as it does a reply the user
started, and Stop in the session cancels it. When the run ends, Bot gets one post
with the outcome ([Bot](../ui/home-thread.md#home-thread)).

This app holds a Running hand-off while its Chat run is registered or its live
lease is alive. The registration keeps the same thread through the wait to save,
and the terminal sink owns the live lease through delivery. A pass that finds
a Running row with neither settles it in one transaction. A retained answer
completes the row and carries any Memory skip reason. If the session kept no
answer, the hand-off stops as interrupted.

Typed Bot hand-offs use Delegated. A verified World link uses the ordinary Session
surface with its own language context. Neither host-started path offers
`ask_user`; a new user-started reply uses the ordinary interactive policy.

Both paths use the admitted model, thinking level and run options and preserve
the allowed Memory choices. Rust checks any advisory pins against owned records
and revisions. Paused, removed, stale and Memory-off pins can be omitted.
Attachments, as-of dates and chosen
Skills are outside the hand-off. An interrupted or unverifiable pending hand-off
shows a reason when it stops. The user can request the work again in a new turn.

## Code

- Chat execution: `src-tauri/src/commands/chat_runtime.rs`, `src-tauri/src/commands/chat_runtime/`, `src-tauri/src/chat/`
