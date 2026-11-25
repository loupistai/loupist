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
supplied by the user, is enough for `artifact_read`. A delivered publish result is
also a read: its edit token (chart or table) or revision (document) can be used to
revise in this reply. Reads resolve delivered drafts before storage, and a revision
replaces the draft without using another of the four artifact slots. Call `artifact_list` only to
discover an unknown target id.
[Connections](../capabilities/connections.md) owns IDs and credential behavior.

`research_open` (always permitted, no component, Bot only, one per reply) hands a
question that needs depth or several steps to a separate Chat: `title`, `question`,
`brief`, `pins` of `{lens}` or `{world}` ids and a `requestKey`. It stages; the reply's
commit inserts one `chat_handoffs` row and the separate Chat in the same transaction, a
repeated key creates nothing, and a reply that does not finish hands nothing off. After
the commit the host runs that Chat once through ordinary Chat admission with the
unattended allowlist, the Bot turn's model and Memory flags, and the Loupist's
Chat default thinking level (not the Bot's effort). Bot posts a `handoff` card when that Chat's run ends, and nothing while it works ([Bot](../ui/home-thread.md#home-thread)). A separate Chat or an
unattended run does not list the tool and the broker refuses it by surface.

Bot can use `chat_history` with the `card_id` from a visible hand-off history
entry to read the final assistant answer linked to that card. History entries
provide the card ID, not a preview or session title. The read returns the
saved answer, session title and finish time. The first slice also returns
`user_guidance`, up to five of the most recent user messages between the initial
request and the linked answer, in transcript order. Each entry gives
`created_at_ms`, up to 600 characters of `content`, and a `truncated` flag.
The read works with Memory off and returns the answer, title and guidance
content as untrusted text. If the answer is truncated, repeat with the same
card ID and `offset=next_offset` (UTF-8 bytes).
Missing, hidden, unfinished, failed and out-of-scope cards all return the same
unavailable error.

The general `schedules` tools are independent of Memory. `bot_offer` and `ask_later` are unattended-allowed and create no task by themselves. `schedule_briefing`
reads and saves Memory settings, and requires Memory access and Save learning; a Private chat or either missing grant
refuses them before any read or write, even though their catalog permission is
`always`. Generic `schedule_set_status` and `schedule_run_now` also require both
grants when their saved target is a Briefing task, before any status mutation or
run admission; custom tasks remain independent of Memory. Generic update and
delete already refuse host-authored Briefing tasks. Other schedule tools keep
their existing permission classes. Their run
receipts expose control metadata (IDs, status, times and a fixed failure notice);
briefing context and answer summaries stay in the run's Chat, because they can
contain Memory and third-party text. Read current IDs/revisions
before editing and keep a stable creation key within the turn. List receipts
include task `kind` (`briefing`, `custom`); use `schedule_briefing`
to change one host briefing edition (`morning` or `evening`). A request for existing work updates that task
or leaves it unchanged. A new custom task created in Bot with a live title (NFKC, case-folded,
whitespace collapsed) is refused, while same-key retries and distinct titles
at the same time remain valid. Agent modes are
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

`pasted_capture` (core, `always`, Bot and interactive Chats only; the unattended
allowlist, host hand-offs and a Chat other than the bound one refuse it) takes `text`
(1 to 131,072 characters), optional `url` and optional `publisher` (up to 200
characters). Rust finds `text` in the user's own messages of the bound Chat with only
whitespace differing and returns the user's exact bytes as `content`, with
`source_class: user_provided`, `pasted_at` (that message's send time), `pasted`
(`message_id`, `created_at_ms`), any `stated_url` and `stated_publisher`, a
`result_ref` and a `citation_url`, plus a `next` when saving is on. A miss is refused
by field (`text`, `url`, `publisher`). The run result has no origin, so the evidence
carries no link. `source_save` with that `result_ref` stages a
[user-provided Source](../knowledge/sources-notes.md#user-provided-sources); it counts
toward the 16 Source saves per reply like any other.

[Asset pages](../knowledge/asset-pages.md#asset-pages) expose saved context in
`asset_page_read`. The tool takes an `assetId` or a ticker or name and returns the
thesis, linked World entity, stored price and saved call. Its citation describes
the page. A price read or other dated source supplies original evidence. Under
an as-of cutoff, the result keeps the dated reasons, lines and moves and leaves
out the current verdict, saved call, stored price and entity link.

`asset_lookup` submits a name or ticker through the `openfigi.lookup` grant.
The result contains bounded listing fields and uses ordinary retention.
`price_read_save` stages an acknowledged provider call and a field mapping for
replay. The path is described in [Saved price reads](../knowledge/asset-pages.md#saved-price-reads).

`fx_rate` is a core read on every surface, without `capability_load`, taking
an uppercase three-letter `from` code and an optional `to` code in the same form.
Omitting `to` uses the display currency captured at turn admission. Without that
setting the caller must supply `to`. An explicit valid target takes precedence.
It reads the ECB daily euro reference table through this Loupist's
`openbb.platform` grant. `to_per_from` is
units of `to` per one `from`, rounded to six significant digits, with the table's
own date, `ecb:YYYY-MM-DD` revision, source and non-euro legs. Cross pairs say
`computed: cross rate through EUR`; an inverse ending in EUR says
`computed: inverse of the reference rate`; the published EUR-to-other direction
omits `computed`. The first successful table read pins the turn's revision and
repeated pairs return the same retained result and citation, whether the target
was defaulted or explicit. If a delivery is
discarded before retention, a later call stages evidence for its own delivery
against the same pinned table. As-of turns are refused before any read, identical codes are refused on `to`, and a missing code
is refused on its field with the table's available codes. The sidecar read is
upstream evidence when the turn performs it. A turn that joins a display refresh
gets the shared table and creates its own FX receipt without display-side
upstream evidence. [Provenance](../analysis/provenance.md#provenance-and-cutoffs)
owns the cache, freshness and typed failures.

Pass the rate's `to_per_from` as `inputs.rate` to
[`calculate convert`](../analysis/calculation.md#calculation-1) and link the rate's
`citation_url` in `input_sources.rate`. The rate's pair must match the amount's
`unit` currency and the conversion's explicit `to`, in that order. The rate's publication
date, source and revision stay with the conversion receipt.

`world_schema_update` (Memory save in `memory-save`, interactive turns only, one call
per reply, not on the scheduled-run allowlist) takes up to two `types` and three
`relations` for a kind of thing or link the user asked World to track. Rust checks
names against the live vocabulary and its limits when the call arrives, stages the
change, and applies it with the reply together with the asking message; a stopped
reply applies nothing, and a change whose name World gained since staging is
skipped and listed in the answer's `memory_update.skippedKinds`. [Worlds](../knowledge/worlds.md#presets-changes-and-undo) owns the rules.

`loupist_setup` (Memory save in `memory-save`, Bot and interactive Chats only, one call
per reply, refused in scheduled and delegated runs by the unattended allowlist and by
the chat surface) sets a Loupist up from what the user says. It takes `follow` (at most
ten `{name, reference?}`), `unfollow` (at most ten `{asset}`), `focus`
(1–280 characters, replacing the research focus) and `preset` (a `LoupistTemplate`
id). Rust checks each field when the call arrives and refuses by field
(`follow[1].name`, `focus`, `preset`); nothing applies until the reply commits, and a
stopped reply changes nothing. `follow` and `unfollow` change this Loupist's own
followed list; `unfollow` matches only what this Loupist follows. The followed list
lives in the app database, outside the reply's transaction, so its changes apply just
before that transaction through the Following commands' own store functions
(`store/user_context.rs`) and stay if the commit then fails; a reference nobody gave
stays empty, and a follow that matches an existing catalog asset (the same reference
first, else the same name where no reference conflicts) reuses it, another Loupist's
included, instead of adding another. In the reply's
transaction the focus is written to the Loupist profile and the World settings'
`research_focus` together (it files no Lens page), and the preset adds its curated kinds World lacks as one
`preset` change carrying the asking message. A preset's default focus (the New
Loupist dialog's copy) fills only an empty focus, so a repeated preset changes
nothing. The answer's `memory_update.setup` lists each applied change (`followed`,
`unfollowed`, `focus`, `tracks`) for the receipt; preset kinds World has no room for
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
the page still fits at commit.

`journal_save` accepts `thesis {asset, role}` for a reason or line the user stated.
Pass an asset id, ticker or name. See the host’s
[resolution order](../knowledge/asset-pages.md#open-and-match-an-asset). A `line`
needs a check with optional `byDate`. A thesis entry needs no `sourceReads`,
`worldReads` or `lensReads`. One reply may save one reason
and one checked line for the same asset, in either order; the commit writes the
reason first, atomically with the line and Chat. Other second entries and any
third entry are refused. Thesis receipts add `saved_as` (`reason` or `line`) and
`next`: only a line is tested against figures; a line's `next` says where it is
kept, how it is tested and whether it comes due, for the answer to pass on; and a
line without an active or staged reason asks for one because readings need it.
The tool descriptions carry
these rules, and `SYSTEM.md` does not repeat them: `journal_save` saves the user's
stated reasons and lines with `thesis`; `thesis_update` moves the read one step only
on a dated original fact read in the turn, never to agree with the user and never
under an as-of date. Reviewed account fields show as `[hidden on this computer]` in an account read.
`SYSTEM.md` tells the agent to explain that the host keeps hidden fields local, offer
a next step from public information, and not reconstruct them from other results
([redaction](../architecture.md#trust)).

`grid_publish` (`charts` pack) publishes one checked table in the answer; its contract is
in [Grid cells](../analysis/grids.md#grid-cells). `SYSTEM.md` does not name the tool: the `charts`
pack line in the capability index and the `grid_publish` description say when a checked
table (layout `assets`) or a conflict card (layout `conflict`) fits and give the cell
rules. `SYSTEM.md` keeps the two rules that hold with or without a table: measures
compared side by side cover like periods on the same basis, and when sources disagree the
answer says what each reports and which it uses and why, or that it cannot tell; it
never averages or silently picks one, and differences within rounding are not conflicts.
The comparison Skill publishes its table the same way, one row per asset and one column
per measure, leaving values on another basis empty rather than mixing bases, and shows
disagreeing figures as a conflict card. A `world_read` with disputed claims returns
`sources_disagree` to prompt the same card.

`interface_labels` (core, Bot and interactive Chats only) reads product and navigation
labels in the appearance captured for the turn, with or without Memory. It takes
only `keys`, an array of one to seven distinct keys from the tool catalog. The result
maps each requested key to its localized label. Keys include Lens, Following,
Journal, Sources and the five Lens groups. Invalid arguments share one repair.
[Turn context](context-delivery.md#memory-and-context) describes how unattended turns
receive interface wording.

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
briefing disables; Lens section retirement; unfollowing an asset or
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
account numbers, and never for permission to run a tool. Scheduled runs and all
host-started research hand-offs refuse it, including a World-link hand-off on the
Session surface. The tool is omitted and the broker refuses a direct call. The card and
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
