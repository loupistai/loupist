# Turn tools

Setup freezes the turn's tools, Memory boundary and schedule edits. Failures name the field and a repair.

`commands/chat_runtime/setup.rs` freezes effective configuration, connector
authority, Skills and workspace resources in `AgentHostContext`. Keep every cache
seal: transcript, Memory, model/runtime, credentials, configuration and resource
hashes. A Skill mention does not change Memory access. Invalid host context must
fail startup rather than become model text.

Ordinary research tools allowed for the turn preload, including enabled web
research. The catalog marks charts and schedules on-demand (`preload: false`);
the index names their tools and when to load them, and marks loaded packs on
subsequent turns. Use `capability_load` with an exact pack or connection ID to
expose its tools; mentioned enabled connections preload too. Reuse loaded connection tools, and load again only when the
host says they are unavailable or asks for reconnection. Repeated loads return a short receipt.
Unavailable bundled entries retain enough purpose/context
to name the connection the user needs to configure. Loading cannot grant access.
An artifact id already returned in this Chat, including by `chart_publish`, or
supplied by the user, is enough for `artifact_read`. Call `artifact_list` only to
discover an unknown target id.
[Connections](../capabilities/connections.md) owns IDs and credential behavior.

`research_open` (always permitted, no component, Bot only, one per reply) hands a
question that needs depth or several steps to a separate Chat: `title`, `question`,
`brief`, `pins` of `{lens}` or `{world}` ids and a `requestKey`. It stages; the reply's
commit inserts one `chat_handoffs` row and the separate Chat in the same transaction, a
repeated key creates nothing, and a reply that does not finish hands nothing off. After
the commit the host runs that Chat once through ordinary Chat admission with the
unattended allowlist and the Bot turn's model and Memory flags, and Bot shows a
`handoff` card ([Bot](../ui/home-thread.md#home-thread)). A separate Chat or an
unattended run does not list the tool and the broker refuses it by surface.

The general `schedules` tools are independent of Memory. `schedule_watch` and
`schedule_briefing` read and save Memory settings, so their executor guards require
both Memory access and Save learning; a Private chat or either missing grant
refuses them before any read or write, even though their catalog permission is
`always`. Generic `schedule_set_status` and `schedule_run_now` also require both
grants when their saved target is a Briefing task, before any status mutation or
run admission; custom tasks remain independent of Memory. Generic update and
delete already refuse host-authored Briefing tasks. Other schedule tools keep
their existing permission classes. Their run
receipts expose control metadata (IDs, status, times and a fixed failure notice);
briefing context and answer summaries stay in the run's Chat, because they can
contain Memory and third-party text. Read current IDs/revisions
before editing and keep a stable creation key within the turn. Agent modes are
fresh history or the task's own dedicated Chat; scheduled runs cannot mutate
schedules. Use [Scheduled Chats](../runtime/scheduled-runs.md#scheduled-chats) and the
[scheduling evaluation](../../apps/loupist/evals/README.md#scheduling-evaluation).

For Memory, preserve the discovery/exact-read boundary. `world_search` and
`conversation_memory` return `readArgs`; copy those opaque IDs instead of guessing.
World search combines local discovery with the hybrid World index Rust keeps in
the folder database, and returns partial results when that branch fails (for
example without the local embedding assets). Empty partial results do not prove absence,
and neither does a Source hit marked `mapPending`, whose World job has not settled.
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
create with `expected` is refused, any op on a method page that does not cite
the user's message is refused (a `put` cites `said` in its basis, a `retire` or
`summary` carries `"said": true`; unattended runs therefore change no method), and credential-like text returns `sensitive_memory`. Staging also runs the writer's text policy and create
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

`loupist_setup` (Memory save in `memory-save`, Bot and interactive Chats only, one call
per reply, refused in scheduled and delegated runs by the unattended allowlist and by
the chat surface) sets a Loupist up from what the user says. It takes `follow` (at most
ten `{symbol, market?, name?}`), `unfollow` (at most ten `{symbol, market?}`), `focus`
(1–280 characters, replacing the research focus) and `preset` (a `LoupistTemplate`
id). Rust checks each field when the call arrives and refuses by field
(`follow[1].symbol`, `focus`, `preset`); nothing applies until the reply commits, and a
stopped reply changes nothing. The followed list lives in the app database, outside the
reply's transaction, so its changes apply just before that transaction through the
Following commands' own functions (`commands/user_context.rs`) and stay if the commit
then fails; a market the user did not give stays unknown, and an asset already
followed under any market is not added again. In the reply's
transaction the focus is written to the Loupist profile, the World settings'
`research_focus` and the perspective page's `focus` section together, and the preset adds its curated kinds the map lacks as one
`preset` change carrying the asking message. A preset's default focus (the New
Loupist dialog's copy) fills only an empty focus, so a repeated preset changes
nothing. The answer's `memory_update.setup` lists each applied change (`followed`,
`unfollowed`, `focus`, `tracks`) for the receipt; preset kinds the map has no room for
are listed in `skippedKinds`. `SYSTEM.md` tells the agent to call it in the reply where
the user says what they research, what to follow or picks a starting point, and never
to send them to settings.

`connection_setup` (core, Bot and interactive Chats only, three calls per reply) takes
one `id`: a catalog entry id (`fred.macro`), the entry's English or Korean display name,
or a plugin id or group name (`World Bank`, `세계은행`) that covers exactly one entry,
compared case-insensitively with `-`, `_` and `.` read as spaces
(`marketplace/entry_lookup.rs`). No field can carry a secret, and an `id` that looks
like a key is refused. A miss lists up to twelve `id (name)` candidates, entries whose
name or id shares a word with the query first. The result is
`{status: "card_shown", id, name, state, notice}`, where `state` is `unavailable`
(this build cannot run the connector, matching the card), `needs_setup` (connector not
ready), `off_here` (ready, binding off for this Loupist) or `ready`. The notice tells
the model the user enters any key in the card and never to ask for or repeat one. A
connection that was on when the reply started is `ready` with the exact
`capability_load {id}` call; any other works from the next message because a turn's
authority is sealed at start. The progress step's target is the resolved entry id,
which the renderer reads to draw the [setup card](../ui/chat-composer.md#chat-composer);
a failed step draws nothing. `SYSTEM.md`, the capability index, `resolveCapability`,
the provider setup notices and the KRX approval text all name this tool instead of
sending the user to Data connections.

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

`ask_user` (core, Bot and interactive Chats only, two calls per reply) lets the agent
settle a real fork without ending the reply. It takes `question` (1-300 characters),
`options` (2-4 of `{label` 1-60 characters, unique ignoring case; `description?` up to
160`}`) and `recommended?` (an index into `options`); Rust refuses by field
(`options[1].label`, `recommended`). The card always adds a free-text answer of up to
500 characters. The call waits for the answer, five minutes (`APPROVAL_WAIT`) or the run
stopping. The result is `{answered: true, choice: "<label>"}` or
`{answered: true, text}`; with no answer it is `{answered: false, assumed: "<recommended
label>" | null, next}`, not an error, and `next` tells the model to continue with the
research or presentation default and say which one it used. The `assumed` label
is only the recommendation. Before publishing the card, Rust raises a reply-local
pending-question counter; while any question is waiting, and after any question
ends unanswered, it refuses schedule deletion, stopping, editing and pausing;
watch or briefing disables; Lens section retirement; unfollowing an asset or
replacing the research focus; and Bot memory update, resolve and forget. The
returned `next` and tool description name this same list. Ordinary reads,
research, calculations, other saves and work products (artifact revision,
native write/edit and Python) keep their existing authority. A supplied `focus`
replaces the focus, so it is guarded even when the old focus is empty. There are
no Chat or artifact delete tools. Pi registers `ask_user` as sequential, which the
pinned runtime applies to its whole tool batch. Answering a card drops only its
pending count; dropping the call without an answer preserves the guard. A later
answer does not clear the
guard in the same reply; a new reply starts fresh. A text answer's URLs count as typed by the user for
the untrusted-text rule, and the answer joins the private text no URL may carry after
third-party text. The description tells the model to ask only when the answer
changes the work materially and no sensible default exists, never for credentials or
account numbers, and never for permission to run a tool. Scheduled and delegated runs
refuse it: the unattended allowlist omits it and the chat surface gates it. The card and
`chat_question_answer` are described in [Chat composer](../ui/chat-composer.md#chat-composer).

Tool failures should identify the field and expected shape, with a useful repair
action and no credentials or private payloads. Argument adapters may lift misplaced
fields or remove supported empty extras, but must not invent evidence or merge
conflicting judgments. Test serialized schemas and Rust validation together.
Chart/data contracts are owned by [Financial analysis](../analysis.md).

Native file calls follow the [path policy](../runtime/permissions.md) without
permission cards. Both native reads and grep check exported-file trust; grep
checks every admitted file candidate before the extension searches it. The trust
scan shares a 32 MiB byte budget and 500 ms deadline per call and checks run
cancellation between bounded reads. A tagged candidate or an incomplete scan
raises the turn's third-party-text tier and delimits the grep response;
ordinary user files keep their existing tier. Path refusals name the applicable
boundary, including read-only attachments, Git metadata and hard-linked aliases.
Memory saves distinguish disabled Memory reads from disabled saving.
`run_python` stays allowed after third-party text, after a
custom-connection result and in unattended runs, without a card. `ask_user` asks
about task choices, never tool permission. Unattended runs retain their host
allowlist, and that allowlist includes `run_python`.
