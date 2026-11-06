# Turn tools

Setup freezes the turn's tools, Memory boundary and schedule edits. Failures name the field and a repair.

`commands/chat_runtime/setup.rs` freezes effective configuration, connector
authority, Skills and workspace resources in `AgentHostContext`. Keep every cache
seal: transcript, Memory, model/runtime, credentials, configuration and resource
hashes. A Skill mention does not change Memory access. Invalid host context must
fail startup rather than become model text.

The model receives a compact capability index and uses `capability_load` with an
exact ID to expose tools. Mentioned connections are preloaded. Reuse tools already
loaded in this Chat, and load the same id again when the host says those tools are
unavailable or tells the model to reconnect. Repeated loads return a short receipt.
Unavailable bundled entries retain enough purpose/context
to name the connection the user needs to configure. Loading cannot grant access.
An artifact id already returned in this Chat, including by `chart_publish`, or
supplied by the user, is enough for `artifact_read`. Call `artifact_list` only to
discover an unknown target id.
[Connections](../capabilities/connections.md) owns IDs and credential behavior.

`research_open` (always permitted, no component, Bot only, one per reply) hands a
question that needs depth or several steps to a research session: `title`, `question`,
`brief`, `pins` of `{lens}` or `{world}` ids and a `requestKey`. It stages; the reply's
commit inserts one `chat_handoffs` row and the session Chat in the same transaction, a
repeated key creates nothing, and a reply that does not finish hands nothing off. After
the commit the host runs the session once through ordinary Chat admission with the
unattended allowlist and the Bot turn's model and Memory flags, and the Bot shows a
`handoff` card ([Home Thread](../ui/home-thread.md#home-thread)). A session or an
unattended run does not list the tool and the broker refuses it by surface.

The `schedules` component is independent of Memory. Read current IDs/revisions
before editing and keep a stable creation key within the turn. Agent modes are
fresh history or the task's own dedicated Chat; scheduled runs cannot mutate
schedules. Use [Scheduled Chats](../runtime/scheduled-runs.md#scheduled-chats) and the
[scheduling evaluation](../../apps/loupist/evals/README.md#scheduling-evaluation).

For Memory, preserve the discovery/exact-read boundary. `world_search` and
`conversation_memory` return `readArgs`; copy those opaque IDs instead of guessing.
World search combines local discovery with the hybrid World index Rust keeps in
the folder database, and returns partial results when that branch fails (for
example without the local embedding assets). Empty partial results do not prove absence.
A Source read returns retained attribution/coverage alongside its bytes. A World
read can supply `sourceTargets` for the original spans.

`lens_file` (Memory save in `memory-save`, on the unattended allowlist, at most two
calls, 8 pages and 40 ops per reply) files what the turn established into Lens
pages: each entry of `pages` names a page read with `lens_read` this turn
(`page.recordId`) or a create (`page.create` with `kind`, `title` and `about`), and
its `ops` are `put`, `retire`, `summary` and `idea_status`
([filing](../knowledge/lens.md#filing)). A section's basis cites the turn's handles:
`{world: readRef, claimId}` must name a claim that `world_read` delivered,
`{source: readRef | saveRef}` a Source read or a same-turn `source_save` (its span
resolves at commit, once the Source exists), `{said: true}` the user's message of
this turn (refused in unattended runs) and `{lens: recordId, key?}` a page read this
turn. Rust refuses an unknown or foreign handle by its field with a repair hint, a
`put` or `retire` with `expected` needs that page's acknowledged exact read, a
create with `expected` is refused, and credential-like text returns
`sensitive_memory`. Staging also runs the writer's text policy and create
resolution (`KnowledgeWrite::screen_delta`): a refused text is refused by its field;
a create landing on a page this turn exact-read revises it, and one landing on an
unread page may only add sections ("read and revise" otherwise). Each call reserves
its pages and ops under one lock, so parallel calls share the budget and an
undelivered call frees its share. The reply's commit writes every staged delta through
`KnowledgeWrite::file` with `FilingOrigin::Turn`; an op the policy refuses by then
(say, a node the World dropped) is logged `refused_text`, and an op on a page archived
or removed since is logged `refused_inactive`; the reply still commits. The answer's `memory_update` lists
each filed page once (kind `Lens`, operation `create` or `revise`, `outcomes` with
`opId`, `outcome` and `key`), and a stopped reply files nothing.

[Asset pages](../knowledge/asset-pages.md#asset-pages) have three catalog tools, all on the
unattended allowlist. `asset_page_read` (Memory read, by `assetId` or symbol and
market) returns the user's reasons and lines, the current read with its latest
reason, the last 20 dated rows with sources and `watchAlerts` (the last ten
[watcher](../knowledge/watchers.md#watchers) alerts with kind, fact date, facts and sources),
plus a Korean equity's latest [investor flows](../knowledge/what-changed.md#what-changed) as
`investorFlows`; it is a saved-context receipt, never original evidence, never creates
a page and filters by date under `as_of` ([rules](../knowledge/asset-pages.md#asset-pages)), keeping only alerts
dated by then. Outside an as-of turn it adds `results`: the two latest
[results events](../analysis/results.md#results-events) whose report date is at most 14 days
away, each staged as its own saved-context result (`earnings_snapshot`) with a
`citation_url` and Rust's differences (`differencesComputedByRust`), filed steps and
revisions. An answer paragraph that is only a link to that URL renders the compact
results card.

`world_schema_update` (Memory save in `memory-save`, interactive turns only, one call
per reply, not on the scheduled-run allowlist) takes up to two `types` and three
`relations` for a kind of thing or link the user asked the map to track. Rust checks
names against the live vocabulary and its limits when the call arrives, stages the
change, and applies it with the reply together with the asking message; a stopped
reply applies nothing, and a change whose name the map gained since staging is
skipped and listed in the answer's `memory_update.skippedKinds`. [Worlds](../knowledge/worlds.md#presets-changes-and-undo) owns the rules.

`thesis_update` (Memory save, staged until the reply commits) takes `assetId`, 1–3
`evidence` citation URLs from the turn and either `to` with a `reason` (≤ 280
characters, optional `reasonId`) or `lineReading {journalId, observed}`; its result
reports the checked step, crossing and fact date, and says the update is saved only if
the page still fits at commit. `earnings_snapshot_save` (Memory save in `memory-save`,
staged until the reply commits) records guidance, consensus, a broker's estimate,
reported figures or the call's tone for one results event; its
[rules](../analysis/results.md#results-before-and-after) refuse a value no cited source prints,
wrongly dated evidence and any as-of or private turn, and its result reminds the model
that the app computes every difference.

`journal_save` accepts `thesis {symbol, market, role}` for a reason or line the user
stated; a `line` needs a check, whose `byDate` is then optional, and a thesis entry
needs no `sourceReads`, `worldReads` or `lensReads`. `SYSTEM.md` tells the agent to
read the page for a followed asset, save the user's stated reasons and lines with
`thesis`, and move the read one step only on a dated original fact read in the turn,
never to agree with the user and never under an as-of date. It also points the agent at
the page's `results`: cite a result's `citation_url` alone in a paragraph to show its
card, record values with `earnings_snapshot_save` from evidence read in the turn
(consensus from FMP or Intrinio only), and never compute a difference or call results a
beat or a miss. Held quantities show as `[hidden on this computer]` in an account read.
`SYSTEM.md` tells the agent to explain that local privacy limit and offer analysis
from available public information, without asking the user to transcribe hidden
account details or reconstructing them from other results
([redaction](../architecture.md#trust)).

`grid_publish` (`charts` pack) publishes one checked table in the answer; its contract is
in [Grid cells](../analysis/grids.md#grid-cells). `SYSTEM.md` sends a comparison of two or more
assets on two or more measures to a `grid_publish` table (layout `assets`) instead of
prose or a Markdown table, with derived cells computed by `calculate` first, empty cells
with their reason, and each column's period and basis; a small single-asset table stays
Markdown with linked numbers. When acknowledged sources give different values for the
same measure, entity and period, or opposite claims, it publishes a conflict card (layout
`conflict`) and says which report the answer uses and why, or that it cannot tell; it
never averages or silently picks one, and differences within rounding are not conflicts.
The comparison Skill publishes its table the same way, one row per asset and one column
per measure, leaving values on another basis empty rather than mixing bases, and shows
disagreeing figures as a conflict card. A `world_read` with disputed claims returns
`sources_disagree` to prompt the same card.

`plan_update` is presentation only. For replies with more than three steps the
model sets two to six short steps with a status each; Rust validates the list,
projects it into the reply's progress plan, keeps each step's first activation
time and marks unfinished steps Not done when the run ends. It creates no evidence
and touches no Memory, and scheduled runs may call it.

Tool failures should identify the field and expected shape, with a useful repair
action and no credentials or private payloads. Argument adapters may lift misplaced
fields or remove supported empty extras, but must not invent evidence or merge
conflicting judgments. Test serialized schemas and Rust validation together.
Chart/data contracts are owned by [Financial analysis](../analysis.md).
