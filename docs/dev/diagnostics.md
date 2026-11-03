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
milliseconds, and known token totals. `--since` accepts minutes (`30m`), hours
(`24h`), or days (`7d`) and selects runs completed within that interval. Both
commands accept `--kind` and `--failed`. Missing logs produce an empty result;
malformed or incomplete lines are skipped with a count on stderr.

Entries identify the run and optional Chat, model and effort, elapsed time,
terminal status (`ok`, `failed`, `cancelled`, `deferred`), stable failure category,
known token usage, tool/refusal counts, and Memory change counts. Unknown usage is
`null`, not zero. Input counts include cache writes; cached input counts cache
reads. World and learning aggregate the same provider measurements across their
calls without changing World budget accounting. Token totals measure consumption,
not a billed currency amount. Truncated tool traces carry a host-authored note.
Scheduled executions produce both their ordinary Chat entry and a `scheduled`
or `briefing` receipt; summary counts include both kinds, while usage totals
count the same execution once when both entries are selected.
World entries carry `inferenceCalls { llm, embedding }`, counting only calls the
host made (refused requests excluded); Chat entries carry `openbbFirstCallMs`, the
duration of the turn's first OpenBB call including any cold start.

The logger never takes prompts, answer or Source text, file contents, holdings
quantities, credential/header values, or raw error bodies. It applies the shared
credential detector and protected-value checks to metadata strings and map keys.
Logging failure does not change a completed run's result; stderr reports only
that diagnostics could not be written. Abrupt process termination may leave no
completed Chat entry; scheduled runs recovered at startup record cancellation.

Settings → Agent → Advanced → Diagnostics reveals the log folder or copies the
latest failed entry. The existing settings scope selects a Loupist's logs; Global
defaults selects app sign-in logs. Empty logs and clipboard failures are reported
in the control. App-data locations are owned by the
[native tooling guide](../../apps/loupist/e2e/README.md#which-tool).
