# Context delivery

Under an as-of date, automatic context keeps to the cutoff. Shared text keeps its delimiters. Briefings add their fixed prompt on the same path.

## Memory and context

With saving enabled, what a turn establishes (a dated fact with its source, a
mechanism, a value-chain link, a contradiction, a check that mattered) is filed into Lens
pages with `lens_file`. A turn creates a method only from the user's teaching or
correction; its own checks stay on subject and idea pages until
[method learning](../knowledge/lens.md#method-learning) sees them recur. This Chat's progress and user-stated facts use `note_save`;
handed-over originals use acknowledged capture followed by `source_save`. Chatter, light questions and
explicit one-offs are not saved. A reply promising to remember is not a Memory write.
World processing extracts relationships from retained Sources. Research filed in
two Chats from two retained originals queues a
[method-learning review](../knowledge/lens.md#method-learning). Methods also change through Chat tools;
[User principles](../knowledge/lens.md#user-principles) have a separate stored-rule
contract and bounded turn context. The [profile contract](../runtime/storage-authority.md#where-data-lives)
distinguishes editable Loupist fields from the
[host instruction snapshot](../runtime/chat-turns.md#local-workspace-execution).

Before substantive research, discover relevant Source/World evidence, methods and
notes, then exact-read useful matches. Existing retained originals can answer dated
questions without refetching unchanged files. Fresh retrieval is for missing
coverage, newer evidence or an explicit request to inspect the current original.
Native file reads and Bash output alone do not retain Sources.

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
continuation on work that depends on earlier context.


Under an as-of date, automatic context follows
[the cutoff rules](../knowledge/memory.md#memory-under-a-cutoff): a perspective (1) or
catalog method (6c) whose current version is later is left out, Journal pointers and
counts (6b, 6d) use only entries created by then with later outcomes read as pending,
and later notes (6e, 6g) are left out; principles (2) stay as they are today.

Automatic context never serializes holdings position fields. It reads the Map's
identity-only SQL projection and serializes an explicit symbol/market/name allowlist.
The followed-companies hint supplies relevance and identity, not evidence of
position quantities or values. Full evidence behind a working note or due Journal
stays behind exact-read tools; recursively retained Journal inputs do not enter
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

Briefings use the same Chat path and additionally supply the fixed briefing prompt
plus bounded host data: instructions (≤4,000 characters), explanation enum, ≤60
symbol/market identities, ≤30 principles selected within the 6,000-character rule
budget, ≤40 growth events with ≤240-character summaries, ≤5 bounded due-Journal
pointers, `assetDeltas` (≤24 followed assets with a material change, ≤3 rows each
with fact date, facts and source titles/URLs), `earnings` (≤8 results events with Rust's
snapshot view), `watchAlerts` (≤24 watcher alerts since the previous run with dates,
facts and source titles/URLs; [selection](../runtime/scheduled-runs.md#scheduled-chats)), language,
date and previous-run timing. Briefing principle records retain
statement/why/example limits of 280/500/500 characters respectively; their count
is bounded independently of stored history. The ordinary Chat envelope also carries the Loupist's definition.
The briefing prompt asks the run to explain each `assetDeltas` row with its date and
source, retain the risk factors and management's discussion (사업의 내용, 이사의
경영진단) of a new 10-K, 10-Q, 사업보고서 or 분기보고서 with `source_save`, and change a read
only with `thesis_update` on evidence read in that run. For each `earnings` entry it says
what was expected and what was reported using the app's differences, records the figures
and any new guidance of one reported since the last briefing with
`earnings_snapshot_save` after reading its release, and names the date of one coming up;
it explains each `watchAlerts` entry with its date and source link and adds no others.

`agent/prompts/earnings.md` holds the before and after capture procedures (guidance
first, then consensus only from FMP or Intrinio; reported figures with their stage and
new guidance; never a difference, beat or miss). The product's captures make no model
call, so only evaluations send it (`capture_prompt`, built with the `eval-harness`
feature). The filings Skill's `references/earnings-review.md` carries the same snapshot
procedure for replies.

Envelope digests exclude live time and the transient standing read reference.
Unchanged warm turns retain prior perspective/catalog/principle blocks and send an
unchanged digest plus current instructions, followed identities, the Loupist's tone,
due reviews, record counts and relevant pointers. A changed definition produces a
new digest and full context; compaction forces full context delivery again. Stable
Loupist blocks precede per-turn data without rewriting the system prefix. Provider
cache counters, not this structural property alone, establish observed cache reuse.
