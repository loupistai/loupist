# Chat turns

A turn freezes its authority in Rust, then Pi runs in the selected workspace. The workspace is a starting directory, not a sandbox.

## Chat

SQLite owns the transcript. Pi JSONL is a derived cache. Rust prepares the turn
once in `commands/chat_runtime/setup.rs`, capturing model, Memory policy, connectors,
Skills and workspace resources. Each turn launches a disposable Pi process with
fixed authority. Cache reuse requires matching transcript, runtime, configuration,
resource and connector seals; changed valid seals rebuild from SQLite. The runtime
seal includes the turn's as-of date, so changing the date starts a cold session.
Confirmed Pi shutdown is required before cache reuse or cleanup.

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
[Artifact history](../analysis/charts.md#visualizations) describes retention and revision reads. [Item surfaces](../analysis/surfaces.md#where-each-item-appears) owns that UI mapping.

### Local workspace execution

Pi's CLI CWD stays in a private runtime directory. Native `read`, `write`, `edit`,
`ls`, `find` and `grep` operate from the selected workspace, the Loupist
folder. Rust refuses file calls outside admitted roots: the folder, this run's
scratch and this Chat's attachments. `run_python` is the code execution tool.
It stays allowed after third-party text and in unattended runs, without a card
([Native tool permissions](permissions.md)).
File changes are immediate and survive a failed or cancelled reply. Python may
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

The composer keeps unsent drafts and pending turns per Chat during the app
session. Live guidance accepts text; attachments/references belong to a later turn.
Try again resubmits the original failed/stopped request, its saved attachments and its
as-of date using current settings after clearing its derived cache. Accepted `steer`
messages stay in the history and are skipped when choosing that request; local file effects remain.

### Turn authority and finalization

Rust independently gates Memory reads and writes. Search, Skill selection and
renderer inspection do not grant exact-read or write authority. Broker delivery
uses acknowledgment and a commit barrier; an indeterminate barrier aborts the turn.
One successful host transaction publishes the completed Chat and authorized Memory
changes, graph jobs and artifacts. A Bot reply's [Bot memory](../ui/home-thread.md#bot-memory)
changes and review queue ride in the same Chat compare-and-swap. It also records acknowledged Lens delivery receipts
and usage, including principles actually attached to bounded context when the turn
saves no new Memory. The host records each receipt's kind and revision; model text
cannot set it. A conflicting Lens update becomes a proposal. Failed or cancelled
turns publish no staged Memory or completed delivery usage; immediate file,
dataset-import and schedule changes have their own persistence boundaries.

A turn's as-of date (`ChatSendRequest.as_of`, `YYYY-MM-DD`) must lie between
1990-01-01 and today on the device; admission refuses any other value before the
user turn is appended. Rust stores the date on the request and on its answer, and
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

At commit Rust writes three records on the answer and sends them with the completed
event; the renderer only reads them and model text cannot set them. `number_checks`
(at most 200) holds the [number checks](../analysis/number-checks.md#number-checks) and
`sensitivity_cards` (at most eight) the [grid cards](../analysis/calculation.md#sensitivity-cards). An
as-of turn's `as_of_report` holds its cutoff; the tools refused with `as_of_unsupported` (with
their connection) and the reads that returned `as_of_empty` (with a target taken from
arguments such as `symbol`, `corp_code` or `query`), each listed once and at most 16;
and the Memory left out, counted by kind across context assembly and tool reads.
Message validation allows them only on answers and checks their bounds.

A settled turn with acknowledged Memory writes but no answer text can receive a
host-composed receipt summary. Unfinished tools, provider/process failures and
storage errors do not qualify. Host diagnostics are bounded/redacted and exclude
raw credential-bearing payloads.

Pi's `agent_settled` remains the completion barrier; a visible final text chunk
alone cannot commit Chat or Memory, bypass Stop, or discard acknowledged steering.
The extension defers automatic compaction after a finished answer until the next
input. A cancelled or deferred attempt leaves no successful or failed compaction step in the
answer's progress. Compaction before a prompt and between tool rounds retains its
normal error, timeout and cancellation handling.

## Code

- Chat execution: `src-tauri/src/commands/chat_runtime.rs`, `src-tauri/src/commands/chat_runtime/`, `src-tauri/src/chat/`
