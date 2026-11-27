# Diagnostics

Run logs are content-free, local, and kept for 14 days. They are not uploaded.

## Diagnostics

Rust writes content-free run summaries to each Loupist's
`.loupist/logs/runs-YYYYMMDD.jsonl`. Custom-connection sign-ins are app-level and
write to `<app-data-root>/logs/runs-YYYYMMDD.jsonl` with `loupistId: null`.
Files use UTC completion dates, retain the current day plus 13 preceding days,
and keep at most 20 MiB per day; when full, the oldest complete lines are dropped.
Retention runs on append. Files stay local and are never uploaded automatically.

From the repository root:

```sh
node scripts/runs.mjs tail --loupist /absolute/path/to/loupist
node scripts/runs.mjs tail --loupist /absolute/path/to/loupist --kind chat --failed
node scripts/runs.mjs summary --loupist /absolute/path/to/loupist --since 24h
node scripts/runs.mjs tail --app /absolute/path/to/app-data
```

`tail` prints the latest 20 matching entries; it does not follow the file.
`summary` reports counts, failures by run kind, nearest-rank p50/p95 duration in
milliseconds, known token totals, model requests and a cost estimate.
`tail` includes requests and cost as `req=` and `cost=`.
`--since` accepts minutes (`30m`), hours
(`24h`), or days (`7d`) and selects runs completed within that interval. Both
commands accept `--kind` and `--failed`. Missing logs produce an empty result;
malformed or incomplete lines are skipped with a count on stderr.

Entries identify the run and optional Chat, model and effort, elapsed time,
terminal status (`ok`, `failed`, `cancelled`, `deferred`), stable failure category,
known token usage, tool/refusal counts, and Memory change counts. Unknown usage is
`null`, not zero. Input counts include cache writes; cached input counts cache
reads. World and learning aggregate the same provider measurements across their
calls without changing World budget accounting. The optional `requests` counts
assistant responses, including retries and errors, plus measured compactions or
sent isolated model calls. The optional `costEstimate { microusd, fromProvider }`
records the estimated cost as integer micro-USD and marks service-reported charges
as billed. Truncated tool traces carry a host-authored note.
Scheduled executions produce both their ordinary Chat entry and a `scheduled`
or `briefing` receipt; summary counts include both kinds, while usage totals
count the same execution once when both entries are selected.
World entries carry `inferenceCalls { llm, embedding }`, counting only calls the
host made (refused requests excluded), `queueWaitMs`, how long the job waited before
this attempt, and `stageMs`, the time paid calls took per extraction stage (calls
sent together overlap, so the sum can exceed the duration), and, when extraction ran,
`entityResolution { proposed, refused, byCode }`: the extracted names the model
merged into a stored node, the merges among them the host refused because the two
names carry different identifying codes and do not agree in name, and the names the
host joined to a stored node by an equal code where the model had not
([codes](../knowledge/worlds.md#identifying-codes-in-entity-resolution)). Chat entries carry `openbbFirstCallMs`, the
duration of the turn's first OpenBB call including any cold start.
Chat entries with a refused connector credential read carry
`connectorKeychainStatuses`, a set of Security status categories. It can include
`no_default_keychain`, `database_locked`, `interaction_not_allowed` and `other`.

When Chat returns an admission or setup error, the entry records
`chatStartFailure { phase, reason }`, for example `model` and `invalid_request`.
The phase is a fixed host checkpoint. The reason is a known command error code.
The entry excludes the rejected input and the raw error message.

A failed Chat commit also records `memoryCommitFailure { stage, reason }`. Both
values come from fixed host enums: the stage identifies the first failed commit
phase (for example `research` or `lens`), and the reason categorizes its code
(for example `journal_entity` or `storage`; unrecognized codes become `other`).
A failure of the Chat transaction itself is classed from the store error: an
invalid record or refused receipt is `invalid_input`, a save past its store budget
or a busy database is `other`, and only other SQLite, JSON, private-storage or lock
failures are `storage`.
It contains no error text, arguments, paths or secrets. The eval trace copies
this category from the terminal host log for that exact run as
`memory_commit_failure`; it never accepts a Pi-authored cause. Older log entries
without the field remain readable, and missing diagnostics do not change the
eval result.

A Chat or scheduled run with a retained answer and skipped Memory can still
finish with status `ok`. Its `memorySaveSkip` records the reason separately
from failures that prevented completion. Chat progress keeps its saving
duration and the time spent waiting for admission.

For Chat turns that reach save admission or complete directly, `chatSavePath`
is `direct`, `queued` (save admission without an observed wait), or `waited`.
Count `waited` among entries with this field to measure how often completion
waits. Older entries and runs that end before selecting a path omit the field.

The logger never takes prompts, answer or Source text, file contents,
credential/header values, or raw error bodies. It applies the shared
credential detector to metadata strings and map keys.
Logging failure does not change a completed run's result; stderr reports only
that diagnostics could not be written. Abrupt process termination may leave no
completed Chat entry; scheduled runs recovered at startup record cancellation.

Settings → Agent → Advanced → Diagnostics reveals the log folder or copies the
latest failed entry. The existing settings scope selects a Loupist's logs; Global
defaults selects app sign-in logs. Empty logs and clipboard failures are reported
in the control. App-data locations are owned by the
[native tooling guide](../../apps/loupist/e2e/README.md#which-tool).
