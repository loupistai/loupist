# World processing

Each Loupist has one World. Rust extracts and reconciles in-process, then publishes a sealed snapshot. Search and defaults are in [World search](world-search.md).

## Worlds and graph processing

Each Loupist has at most one World. Its current sealed snapshot contains nodes,
claims, ordered endpoint roles and Source support/opposition/context. World content
in World is read-only; questions or correction requests attach the relevant claim to
Chat. A correction retains its exact Source basis.

Detail reads use the owned World record and do not invoke inference. Graph camera
and selection state survive Source visits. Fit reframes the displayed graph.

Rust extracts and reconciles in-process (`knowledge/world_extract/`, prompts
adapted from Graphiti 0.30.1). A derived index in the folder database (entity and
claim vectors plus FTS5) serves candidate and hybrid search; SQLite snapshots and
retained bytes are canonical, and the index rebuilds from the sealed snapshot. Source retention can
succeed even when processing is disabled or fails. Originals without extracted
text remain readable but cannot supply relationship extraction.

A completed World job keeps a lightweight publication receipt: its input and
output digests, Source record/revision/span binding, selected extraction model,
inference/usage status and sealed revision id. Rust commits publication and job
completion atomically, then discards the extraction JSON. Superseded World revisions
are also removed; these receipts are provenance and deduplication records, not a
World history or restore feature. The renderer resolves a first-page World query
against the owned record's current head in the same read transaction. Cursors
and model reads keep their revision pins. Reads, passage pins and exports for a
Source opened through World check the exact Source revision and span against
the current World head, even when the request names an older World revision.
Job polling returns the receipt fields without the internal extraction
document or retained Source text. Recovery rebuilds the derived index from the
current canonical World and retained Source evidence. An uncertain publication
keeps its extraction JSON while quarantined for reconciliation.
Publication bounds extracted node names and descriptions, drops unreadable nodes
and claims, and drops claims whose endpoints were discarded, so a malformed item
does not block otherwise valid extraction. It also drops a relationship whose ends
are one entity: the same node, or nodes of one kind (or an untyped `Entity`) that
share a name or alias once folded, as "KRW-BTC" and a Bitcoin node aliased "KRW-BTC"
do (`relates_one_entity` in `graph_jobs/worker_output.rs`). Extraction already drops
one before entity resolution and drops one again once resolution has merged both
ends into one node, before any relation-resolution call is spent on it. A `REPORTS`
claim is exempt: it is a value reported about one subject, which it names in both roles.

Long Sources become bounded episodes retaining the original span as evidence.
Chunker 2 (`graph_jobs/chunking.rs`) cuts between blocks: a Markdown table, an HTML
`<table>`, a fenced block, a paragraph. A block that fits one episode stays whole,
except a table of more than 24 data rows, which is cut into the fewest equal runs of at
most 24 rows: one extraction call over the 84-row, six-period Markdown table of the
long-table evaluation (402 cells in its first 7,000-character episode) timed out, and
at 40 rows a call still took 75 to 110 seconds of its 120 and sometimes timed out.
24 rows of five or six periods keep an episode near 120 to 144 cells. A longer table
also splits between rows by characters, and each continuation opens with repeated context:
the table's header and the unit statement of the nearest line within three lines
above it that states a currency, scale or unit as `numbers.rs` reads one, at most 600
characters under a marker line. The unit statement is a whole unit line, such as
"(단위: 백만원)", or the sentence of an introducing paragraph that states the unit,
such as "All amounts are in ₹ crore."; a sentence with no digit is preferred, since
it states no amount. Repeated context counts toward the episode budget but not toward its
character range, which stays a true offset into the Source. Each job stores its
`chunker` (absent means 1, blank-line paragraphs, then fixed character runs). A Source
that already has jobs keeps its chunker, so Try again, Continue and Read now reuse its
chunks. A Source that fits one episode is one episode under both chunkers, whatever its rows.
Each episode runs after the one before it, whose text is its extraction context; once
that episode completed or can no longer finish, the next one runs.
A published chunk has an immutable episode with its character range in the original
span, its exact Source binding and a bounded list of the changes it made. The episode
and World head commit together. A published reference comes from the Source's
publication time. Otherwise the reference comes from the capture time. Only published
assertions set a claim's publication time, and a repeat can move it earlier.

World keeps year, month, date and instant precision for stated validity. The resolver
selects pairs to compare. Explicit validity takes precedence. When the resolver
identifies two different values for the same assertion, each claim's publication days
decide it. If every day of one claim precedes every day of the other, the earlier
claim ends at the other's first day. If the days interleave (January X, February Y,
March X), the claim asserted last stays current and the other ends at that claim's
first day after its own last day. This needs a Source that restated it after that day
to have selected the pair as the same assertion itself; otherwise the two conflict.
Both claims asserted last on one day also conflict. Received-only assertions, unordered
dates and unrelated selections cannot cause that approximate ending. The World keeps
the ending's witness span, and conflicts keep links to up to four other claims. An
ended claim keeps no conflict link to a claim whose only Source witnessed its ending:
the Source that replaced a figure does not also disagree with it. A claim that another
Source also supports keeps the link. A later publication can revise or withdraw a
reference-based ending. A claim kept current that way carries host qualifiers:
`reopened_at` (the approximate day), `reopened_by_span` (the restating Source's span)
and `lapsed` (the gaps it was not current, as `from/to` pairs, at most eight). When an
ending is withdrawn and the claim is current again, the publication records a
`claim_reopened` growth event, and a Lens section superseded by that ending reads
Current again. A stated ending is never reopened. Under a cutoff the same rule runs on
the publication days and witnesses the cutoff knows, so a view as of a date equals the
World that read only the Sources published by then. A stated future end becomes
effective on its date. The outcome is the same in every ingestion order for one set of
resolver selections. A restatement that matches a stored claim word for word skips the
resolver, so it never makes that selection.

Every World extracts with one shared core vocabulary, listed once in
`knowledge/world_extract/ontology.rs`: the entity types Organization, Person, Asset,
Product, Place, Policy, Event, Technology and Segment, and seventeen relation
types (SUPPLIES, COMPETES_WITH, PARTNERS_WITH, SUBSIDIARY_OF, ACQUIRED, INVESTED_IN,
LEADS, OFFERS, USES, REPORTS, LOCATED_IN, AFFECTS, PART_OF, REQUIRES, OPERATES_IN,
ISSUES, OWNS) with allowed endpoint types. `world_schema.rs` reads its reserved
names from that list. A Technology is a science, technique, process, material or
standard more than one organization can use (EUV lithography, hybrid bonding,
proof-of-stake); a Segment is an industry or value-chain stage (advanced
packaging, Seoul office leasing). An Asset is any instrument or holding: its
`asset_class` is exactly the `AssetClass` list (equity, crypto, fund, commodity,
rates_fx, real_asset, prediction, private), rendered from the enum, and its
`instrument` says the form (index, bond, etf, token, pair, property, deal). Real
estate and private deals are Assets of class `real_asset` and `private`. An entity
that fits no type stays `Entity`, and a relationship that fits no type keeps a
specific open name, so the vocabulary guides extraction without closing it.
`Metric` stays reserved for existing World records, which remain readable, but new measured
values are claims about their subject. `REPORTS` uses the same subject in both
endpoint roles, with the exact value, unit, period, basis and status in its text.
Extraction and publication reject new nodes named as quantities, measurements,
figures with units or sentences, including under custom kinds. Numeric product
names, tickers, security maturities, fiscal entities and named events remain valid.
A `REPORTS` claim from an admitted subject to a newly rejected value node keeps
its full text and uses the subject in both roles. Other relationships are not
rewritten, and existing measurement nodes and their claims stay as stored.
Named or uniquely qualified dated occurrences remain Event entities alongside
their participants and quantitative claims. Their identifying names use actors,
action and date or period; reported amounts stay in claims.
A qualified occurrence title can contain a verb or a monetary qualifier without
becoming a measurement. Admission requires an actual occurrence term and a named
participant or subject beyond generic action, date and unit words; a contract's
value or amount remains a measurement. Institutions keep their Organization role
separate from what they issue or oversee, and quantitative claims keep their
specific metric names and stated counterparty roles beside the value.
Measurement claims keep each metric's exact Source wording, language and qualifiers.
English claims retain a non-English metric label with its English rendering beside
it. A synonym, accounting term or acronym cannot replace the stated label:
operating profit, operating income, EBIT and adjusted operating profit stay distinct.

### Figure checks

Before a chunk's extracted facts reach relation resolution, the host checks each
fact's figures against the chunk text, and, for a figure the chunk does not print,
against the whole Source (`knowledge/figure_check.rs`). A chunk job reads the whole
Source's numbers once, however many of its figures the chunk misses. It reads numbers with the
answer checker's reader ([Number matching](../analysis/number-checks.md#number-matching)).
A figure is a currency amount, a percentage, points, a per-share amount or a count
with a scale word; years, dates, ordinals and codes are not figures. Each figure is
`matched` (equal after a scale or unit conversion, such as 1,234억 원 and
KRW 123.4 billion, 1,234p and £12.34, or ₹2.5 crore and INR 25 million),
`rounded` (within half a unit of its last printed digit), `differs` (no passage
number matches, and one of the same kind and currency equals it times a power of ten
from 10^-6 to 10^6, or its negation when both carry a sign or a direction word),
`not_found`, or `unchecked`. A number the reader cannot read one way is `unchecked`,
never `differs`: an unknown shape (`1'234`), a scale word with two meanings (`mil`),
or a lone `1,234` or `1.234` that no other number in the text and no scale word
after it settles. A fact takes its worst figure's verdict, in the order `differs`,
`not_found`, `unchecked`, `rounded`, `matched`; a fact with no figure is
`unchecked`.

A fact that `differs` is not published from that chunk. It neither becomes a claim
nor adds support to an existing claim, it costs no model call, and no other claim's
status changes because of it. A claim that resolution matches with this chunk's
support is checked by its stored proposition, and differing support is not added.
The job counts these facts in `dropped.figureMismatch` and keeps the first eight in
`droppedFigures` (the proposition up to 240 characters, the figure as stated and
the chunk's range). For any other verdict the new support stores `figureCheck`
(`matched`, `rounded` or `not_found`; absent means unchecked or an older snapshot)
and `passage`, the chunk's index and character range in the Source (absent means
unknown). No proposition is rewritten and a figure never makes a claim disputed.
Both fields sit on the evidence entry, so an as-of read that hides the entry hides
them with it.
`world_search` and `world_read` results name only one verdict to the agent: a
relationship with a `not_found` support carries `figure: "not_found"`, the
per-support verdicts are left out, and each support keeps its `passage`.

An `ended` row of `world_changes` whose ending names the claim that ended it
carries `figures {from, to, comparable, direction}`: each proposition's figure as
printed when it states exactly one, and, when both are read one way and have the
same kind, currency and per-share nature, `direction` `lower`, `higher` or `same`.
The bundled `world-change` check takes an optional `direction` (`lower` or
`higher`): a comparable row is reported only when it moved that way, and a row
whose figures do not compare is still reported, marked `direction_unknown`.
Nothing is stored; a check saved on the earlier template keeps its pinned code.

### Queue and interruptions

Schema and learning reviews share a single review slot. World job selection
continues while a review waits for inference. Each review admits one background
call in the transaction that claims it, and reserves that call until the usage
row is stored. A ledger write that fails keeps the reservation. Startup counts
a reservation left by a crash before releasing it, so a review cannot start
once the background allowance is spent and a crash cannot free the call before
it is counted.
Each World job still owns its Loupist's World slot through publication.

World jobs wait in each Loupist's folder database and survive restarts. The graph
worker (`knowledge/graph_service.rs`) runs up to two jobs at once, never two for one
Loupist, since each publishes on its own World head; their paid calls share four
slots, so a provider sees at most four World calls at a time. Among jobs that can
run, a Try again, Continue or Read now attempt goes first, then jobs whose Source
was saved in a Chat turn or by the user (`origin` `chat`), then background work,
oldest first inside each class. The Source save stores the origin admitted for
its turn on the capture (`knowledge_capture_lineage.allowance_origin`): chat
for an interactive turn, background for a scheduled or other unattended run,
including when its task targets an existing Chat. Queueing reads that stored
origin, never the Chat id or a running row, including when backlog queues the
Source later. A capture stored without it is chat only when the user saved or
retained it directly. A job the daily allowance cannot start is skipped
and does not block the jobs after it. After three chat-origin claims in a row,
the next claim is one runnable background job. A job that ran without progress
waits until something outside the loop changes (a save, a setting, a connection).
The claim also runs at startup, when settings change, when a job or run finishes,
and at local midnight.

Within an episode, calls whose inputs do not depend on each other go out together:
entity resolution with relation extraction (relations name the entities as
extracted, as Graphiti's `semaphore_gather` runs them), then relation resolution with
attributes. Prompts, the number of calls and the order the call budget is spent in
are those of one call after another (`Bridge::llm_many`). The next episode's entity
extraction does not start early: its instructions name the relations already on
World, which the episode before it can change.

Each request has a time limit, four minutes by default and at most five
(`DEFAULT_TIMEOUT_MS`, `MAX_TIMEOUT_MS`); saved limits, including 60 seconds, are
preserved. The clock starts after a paid call acquires its shared slot, so waiting
for another job does not consume its provider time. One attempt's lease allows
every model wave at its full limit (`attempt_budget_ms`); an active host worker
keeps ownership while waiting. Entity and relation extraction share the provider's
8192-answer-token ceiling; thinking may use its separate bounded allowance.

A newly queued Source span has one frozen model-call allowance across its episodes.
Each model dispatch reserves a call durably, a refusal refunds it, and completed
or uncertain calls stay spent across retries and restarts. The next episode gets
its predecessor's unused calls once that predecessor has completed, been
reconciled, or been cancelled. A retryable stop (`failed_extraction`,
`failed_before_mutation`) keeps those calls and parks the chunks after it, so
they cannot spend them or block Try again on the chunk that failed. Splitting a
Source cannot increase its total.
Previously saved positive per-chunk allowances stay as saved, since an old
completed receipt does not say how many of its calls were unused.
An episode that exhausts its allowance pauses its unfunded tail without repeating
paid calls or quarantining other Sources; resume does not replay its paid attempt
or requeue its blocked followers.
The explicit `knowledge_graph_retry_source` bridge action can retry one stopped
span of the Source's current revision with a fresh allowance from current settings.
The span runs once the chunk before it is settled for good: completed, or retired
as `reconciled_without_retry`. A tail stopped behind a retired chunk is therefore
retryable, and Sources already stopped that way become retryable with no data
change, since the rule reads the stored statuses. The retired chunk stays as it
ended: its text does not reach World and none of its calls runs again.
This keeps the paid-call rule: a dispatched call, completed or uncertain, keeps its
reservation and is never refunded or sent again. Work runs again only as a new
attempt on the calls its chunk has left (the interruption rules below) or on a
fresh allowance the user grants with Try again, which never reruns a retired
chunk, a chunk whose outcome is uncertain or may have published, or a follower
that already dispatched a call.
It requeues only that span's unfunded tail, sharing the new allowance; it refuses
active, uncertain, quarantined, stale-revision or user-paused work. Duplicate clicks
while queued cannot refill it. If this attempt exhausts its allowance again it
stops again; only another explicit Source retry can fund it. Resuming World building,
settings changes and restarts never refill a spent allowance.
Such a chunk never resumes by itself. Its Library row and the Source reader then
read "World building paused for this source: it reached the request allowance set
in World building settings.", without a spinner. World's status line, the Library's
status line and the sidebar's World row count instead: "3 Sources paused at the
request limit" (`waitingLabel` in `knowledge/mapProcessing.ts`). World's status
line, the Library status and the reader carry the control that opens World building
settings (Maximum model requests per source,
1–64, default 32 per Loupist, since a clean run of a 20 KB filing needs up to 24 on
every recommended model; that per-Source count is separate from the local-day
allowance below). World settings saved before
Loupists recorded their World model choice that still hold the old default, 16, read
as 32 and record it on the next save; settings saved since keep what they hold. Chat's status still
reads "World: building paused for this source (request allowance reached)". A row reads "World building
paused" only while the user has paused World building (`source_processing`
and `knowledge_graph_job` report such a job as `paused`, an exhausted one keeps
`paused_budget`). Chat reads parked jobs once on opening and offers Refresh status
without scheduling work.
Only in `paused_budget` do the Library row, the reader and Chat's save line offer a
neutral "Try again", which uses one more allowance of model requests for that Source
as set in World building. This is the only renderer caller of the bridge action
(`components/knowledge/SourceRetry.tsx`). It is disabled while the host decides,
shows the host's refusal inline, and on success refreshes the row or reader and
follows the returned job like other World building progress. The reader's settings
button continues to manage global processing.
World's status line also offers "Continue all" for the Sources it counts
(`ContinueStoppedSources`, bridge action `knowledge_graph_retry_sources`). It asks
first, naming the count and the most model requests it can use, the Sources times
the allowance saved when the question opens ("Continue reading 3 Sources? This can
use up to 96 model requests."); nothing resumes without that click.
`retry_sources_processing` then runs the single Source retry for each Source the
question named, in one transaction, and refuses the whole step when the saved
allowance no longer matches the one it named. A Source that retry refuses (still
queued after an earlier click, running, uncertain, stopped behind a chunk that
cannot settle, price data or gone) is skipped and counted. A second click while the
batch waits therefore funds nothing, and once some Sources finish or stop again the
line counts only those still stopped. When none of the named Sources can continue,
the question says so and points to Try again in the Library's Needs attention list.
Revision and growth-event timestamps reflect extraction completion.

An attempt that publishes nothing ends one of four ways (`finish_unpublished` in
`knowledge/graph_jobs.rs`). Publication is one transaction, so World is unchanged
in each case. Driver errors settle their claimed attempt. Each scheduler pass also
settles expired leases that have no live host worker, using the same retry and
quarantine rules, without restarting the app. Replacement attempts have a new
identity: an old worker cannot record calls or publish after reclamation.

- **The World settings changed under it**: the model, thinking level, run
  options, request allowance or timeout saved since the attempt started differ
  from the ones it froze. The host refuses its next call (`current_inference`
  answers `settings_changed`), or the attempt ends while they differ, for instance
  after a call that was in flight when they changed. A Recommended model that
  follows a newly connected or disconnected account counts the same way. This is a
  host interruption, not a failure: the chunk queues again however often the
  settings change, spends neither its one retry nor a host interruption, and is
  never quarantined. Its next claim takes the saved model and controls and keeps
  the call count it has left (`claim_running`'s rule for a requeued span); the
  calls already dispatched, completed or uncertain, stay spent and are never
  refunded. A chunk whose allowance ran out this way pauses at its next claim.
  An attempt that may have published (`mutation_sent`) still follows the last rule.
- **Nothing paid was sent** (the provider went away): the attempt cost nothing and
  queues again, up to three times, then fails without quarantining World; Try
  again can run it. Enqueuing that Source or
  backfilling saved Sources requeues its failed or cancelled chunks only when
  no paid or uncertain attempt remains, preserving their frozen allowances and
  binding them to the current generation.
- **The host cut it short**: the app quit, crashed or lost power (the job is found
  running at the next start, `recover_interrupted`), or the Mac slept through it
  (the wall clock moved on more than 30 seconds past the monotonic clock). It runs
  again, up to two times, before the rule below.
- **Otherwise** a paid call that was interrupted, timed out or failed runs once more
  (`retry_once`), on the app's next pump or after a restart. A second such attempt
  quarantines its generation and is not retried. A Source is charged at most two
  attempts this way, four when the host cut two short.
- **The model reply could not be read** (`invalid_json`, `schema_mismatch`,
  `incomplete_response`, `output_size`, `content_type`): the job ends as
  `failed_extraction`. It records `failure.kind` and does not quarantine the World
  or become `reconciled_without_retry`.
- **The World is nearly full**: before the first paid call, a head that already
  holds at least 95% of the node limit or the claim limit pauses as `paused_budget`
  with `failure.kind` `world_full`. The later hard limit still applies. Resume does
  not clear this pause; only Try again checks the limit again, and no model call
  is made while the head is still that full.

A queued Source span also waits on the local-day allowance (`scheduled_tasks::allowance`).
The claim asks `admit_world_attempt` for the job's remaining `max_llm_calls` and does
not change that count. That comparison is the job's own calls against what remains:
the five-call floor that keeps a scheduled run from starting does not apply to World.
Chat-origin work, including a user attempt, draws on the chat bucket. Background work draws on the background bucket and its World share, and stops
for a provider's `plan_limit` or `out_of_credit` until the next local day. A chat-origin
job does not. A user attempt is never refused. While the attempt runs, its `origin`
is `chat` when it counts against the chat bucket, and each model call is written to
the usage ledger before the outbox budget drops, so the allowance does not undercount
calls that have already been sent. A job the allowance will not start stays `queued`.
Nothing is stored to hold it and nothing has to resume it: the next claim sees the
new day. The job view reports `hold.cause` (`allowance`, `plan_limit`, or
`out_of_credit`) and `hold.provider` for the provider stops. The status stays
`queued`. The Library row, the chat World status, and World settings read
`hold.cause`: `allowance` says "Waiting for tomorrow's allowance." and offers
Read now; `plan_limit` and `out_of_credit` say "Your AI plan's limit was reached.
Reading resumes tomorrow." and offer Read now.

Try again, Continue and Read now mark the attempt `user_attempt` and `origin` `chat`.
Read now on a Source that is still queued because the daily allowance will not
start it promotes that job in place and keeps the calls it has left. The
admission lock is released when that write commits, before the hold is read
back, so the command can return and wake World building. A hold lookup that
fails after that save leaves the hold unset and still wakes the worker. Every successful
single-Source retry does the same, including Try again on a stopped span,
which still grants a fresh allowance.
Try again accepts a stopped span in `paused_budget` (the allowance ran out, or the
World is full), `failed_extraction`, or `failed_before_mutation`, and nothing else.
A chunk retired after recovery stays as it ended; it settles the span after it, which
can then be retried. The job view reports `failure.kind` and `retryable`. One bad
attribute or relation-resolution row is dropped and counted on the job
(`dropped.attributes`, `dropped.edges`); the rest of that batch is kept. A `REPORTS`
edge is kept only when both roles name the same subject.

After a quarantine the graph worker queues the recovery itself
(`recover_world_unattended`): the rebuild calls no model, re-queues the jobs that
never ran, and enqueues Sources saved during the quarantine. If that rebuild fails,
the next one waits for World building recovery in Loupist settings, so a broken
rebuild cannot loop.
After recovery, Chat and the Library show why a retired attempt stopped when
the host recorded the cause: interruption, timeout or unconfirmed model usage.
When the cause is unavailable, the status says that World building did not finish.
Try again continues a Source's allowance-stopped span under the rules above.
Only a committed `reconcile_required` receipt may add a generation to quarantine;
a rolled-back attempt cannot leave a quarantine without a recovery receipt.
Pausing processing pauses every queued chunk of a Source, including Sources saved
while paused; recovery rebuilds remain runnable while extraction is paused.

### Price data

A retained Source that is a numeric market-data result of the price kind is never
queued for World building. It stays kept, searchable and citable like any other
Source; World building looks for names, facts and relationships, and a price table
holds none of them. `knowledge/price_data.rs` decides from what the host recorded,
never from the title:

- **The producing tool**: a retained dataset whose recorded producer and request
  name a price route, with the recorded `trust: numeric`. The price routes are the
  bundled market-data routes for price history and bars at any interval, quotes,
  snapshots and price performance, exchange rates, and index and commodity levels
  (`PRICE_OPENBB_TOOLS`, each on the reviewed numeric-only list), the KRX daily quote,
  and the KIS operations that return prices, order books, trades, bars or index levels.
  Yield curves, treasury and policy rates, and other macro series are not price routes.
- **The result's shape**, when the tool did not decide: rows whose columns are only
  dates, instrument identifiers (symbol, ticker, pair, currency) and price fields,
  with a date or an instrument and at least two price-family columns (open, high,
  low, close, adjusted close, volume, vwap, and quote fields such as price, bid and
  ask, including the Korean headers 시가, 고가, 저가, 종가 and 거래량). A lone `rate`,
  `yield` or `exchangerate` column decides nothing. A number fills every price cell.
  This covers a stored bar table whose dataset row is gone, and a table the model
  composed from such results and saved from the workspace (CSV, including quoted
  fields, TSV, a Markdown pipe table or JSON rows).

Company statements and fundamentals (income statement, balance sheet, cash flow,
metrics, ratios, estimates) stay queued, since their figures can be facts worth
keeping, and so do economic statistics, screens and rankings, holdings, ownership
and calendars.

Saving such a Source writes one final `price_data` row instead of extraction jobs:
no text, no allowance, no model call. Method learning counts the Source as read,
since World will add nothing from it. Try again refuses it, and a chunk World
read before the rule existed still shows as read. Without World building set up no
row is written; the Library row and the Source reader still read "Price data · not
added to World" (`source_processing`), Chat's save line reads "Source saved · Price
data, not added to World", and What's new never lists it, since World gains
nothing from it. `source_save` answers `graph: not_added_price_data` for it.

Work queued before this rule leaves the queue on the graph worker's next pass,
before it selects a job (`retire_price_jobs`): queued, paused and stopped chunks,
and chunks that ended before any paid call, become `price_data`. Completed, running
and possibly published work keeps its status; calls already spent stay recorded on
the row and none is sent again. The pass reads each waiting span once per process,
changes nothing when run again, and logs only how many jobs left the queue.

### Keeping the Mac awake

Loupist settings → World building offers one application-wide choice, off by default:
keep this Mac awake during World building (`keep_awake` in the host
`application_settings` row, read once at startup). Rust holds a single
`PreventUserIdleSystemSleep` IOKit assertion exactly while the choice is on, World
jobs are running and the Mac is not on battery or UPS power; an unknown power source
counts as AC (`power.rs`). The display may still sleep and closing the lid still
sleeps the Mac. While the hold is wanted the graph worker re-checks the power source
every minute, so unplugging releases it; a queue that cannot move on its own
(stalled, blocked, waiting for a provider) releases it too, and shutdown drops it.
The panel shows "Keeping this Mac awake · n sources left" while it holds. Work cut
off by sleep with the choice off resumes as [interruptions](#queue-and-interruptions)
describe. Other platforms report the choice as unsupported and the panel hides it.

### Presets, changes and undo

Each Loupist may add a small extension on top of the core: at most four entity types
and six relations. The extension applies to Sources processed from then on;
extraction does not reclassify nodes already extracted. The live extension is the
`after_json` of the newest row in the append-only `knowledge_schema_changes` table
(change id, before and after JSON, producer, evidence, origin Chat and message,
time). `GraphProcessingSettings.extension` is derived from it on load and never
stored. Every change validates, applies at once and writes a `schema_extended` or
`schema_reduced` growth event per kind. Four producers write changes:

- **Preset.** A semiconductors, dividends or macro & rates Loupist starts with its
  template's curated extension (`curated_seed(template)` in
  `knowledge/world_schema.rs`), whatever its focus text says. `loupist_setup` applies
  a preset later from chat: only the curated kinds World lacks, as one preset change
  that keeps the asking message, so repeating it adds nothing. Other templates start
  empty; a new curated seed ships only after the
  [template seed check](../../apps/loupist/evals/world/README.md#template-seed-check)
  (`template-seed-grade.mjs`, no model) shows no fewer typed core relations and no
  lower typed share than the same Sources without it, or it reports `hold`.
- **Chat.** `world_schema_update` (`memory_write`, in `memory-save`, interactive
  turns only, one per turn) is staged and commits with the reply, so a stopped
  turn applies nothing. A change naming a kind World gained after staging is
  skipped, and the answer's `memory_update.skippedKinds` lists its kinds so the
  receipt says they were not added; any other refusal at commit is an error. The
  user's sentence is the approval, and the change keeps the asking message. The Memory envelope carries a `worldKinds` block so the agent can
  tell what is already tracked.
- **Review.** After about ten later Source jobs, once a day and when the World queue
  is idle, the World model may promote what World already shows. Nothing waits
  for the user. The host counts from the snapshot and refuses an unsupported proposal:
  a relationship must be an open relation name with at least six live claims from at
  least three Sources, and a type must cover at least five names that are still
  `Entity`. At most two types and three relations; kinds the Loupist already has are
  dropped. The model-written seed for a custom focus no longer exists.
  A full extension triggers no paid review. When only one kind has room, supported
  additions fill that kind's remaining slots; a full type list still permits
  independent supported relations. The recorded evidence names only the additions
  that are applied.
- **Undo.** "Undo" on a change, or Remove on one kind, appends a change. Undoing a
  change removes only the kinds it added (with links that depended on them) and
  restores the kinds it removed, so an older change can be undone after a newer
  one without touching the newer kinds. A kind a later change added or removed
  stays as that change left it, even under the same name; a change with nothing
  else left to reverse offers no Undo. A review change also posts one Bot message,
  "Your World now also tracks ...", in the labels the user sees, with Undo. A preset
  applied at creation posts nothing. A chat change, a preset applied from chat
  included, is already in the reply. Settings lists these changes, with Undo when
  available, under
  [What World tracks](../ui/map-lens.md#what-world-tracks).

World draws an extension type in that type's
category and labels the inspector with the type's display name. Instruments the
job's Loupist follows are an extraction hint when the episode names them, and each is also an
anchor ([What you follow](follow.md#what-you-follow)). A hint or anchor counts only when the
episode names it as a whole word: a name in any case, a ticker or code (no lowercase
letter) only as written, so ticker `A` is not every "a"; names in other scripts match
anywhere because particles attach to them. The observed relationship hint counts
claims between distinct node IDs after alias mapping. A fact about one name
stays in the claims and evidence while contributing no relationship count.
Organization records a stated ticker, exchange and identifier, and Asset a symbol,
identifier and class. Organization, Person, Asset, Product, Place and Policy record a
non-English original name, which becomes a searchable alias. A spelling the
resolver finds to be an existing node ("Acme Corporation" for "Acme Corp") also
stays an alias of that node, unless a name or alias already folds to it, within
the 16-alias cap, so a Lens anchor filed under that spelling resolves exactly. One attribute call per
episode fills every typed entity's fields. World stores names, summaries, relation
names and fact prose in English, retaining original-language metric labels beside
their English rendering; the original text stays in
the retained Source and its evidence spans. Research focus, preferred relationship
names and anchor entities add emphasis on top of the core schema. Editing this guidance retains saved anchor identifiers as identifiers. Publication time,
observation period and validity remain distinct. Extraction preserves quantitative
units and basis, directed roles and conflicting attributed reports; source content
is data, not extraction instructions. A peer table, screen or ranking is evidence
about its topic. The extraction prompt keeps the rows discussed in surrounding
text, named in the research focus, or stating a relationship to another name.
It requires evidence beyond a name's row of figures to add it to World. The
other rows remain searchable in the retained Source. The extraction text sets a
temporal bound only when the Source states it or a relative expression resolves
against the reference time, at the Source's precision; it adds no time of day and
no first day of a month or year. The request says whether the reference time is the
Source's publication time or only when it was received, and a relative expression is
not resolved against a received time. An explicit termination can close an earlier
claim at its stated end without restating a start. The resolver selects the same
assertion as corrected, retracted or replaced, and lists in `same_assertion_facts`
the selected facts that state the same thing with a different value: the same
subject and role, or the same metric for the same period on the same basis, forecast
against forecast and actual against actual. Only those can end by the Sources'
dates; different relation labels or endpoints do not by themselves establish a
replacement. Denials, rumours, allegations, a forecast beside its later actual and
unrelated later facts do not establish an ending.
End-bounded resolution supplies the relation, participant identities and stored
validity beside the fact text, distinguishing an open relationship described in
the past tense from a historical event. A different explicit end cannot disappear
through verbatim or model-selected deduplication; ending an older claim still
requires the resolver to select it and the time guards to pass. Other resolutions
keep their existing fact-only requests. Restating an earlier start with an explicit
end closes the old interval at the end, preserving the stated start.

### Identifying codes in entity resolution

Extraction names an entity with the identifying codes its Source states.
Resolution asks the model which stored node an extracted name is. The host then
holds the codes over that answer (`knowledge/entity_codes.rs`, `apply_resolution`
in `world_extract/pipeline.rs`).

- A code is any of these:
  - a listing ticker with or without its exchange (`0489.HK`, `HKEX:489`);
  - a ticker as the trailing capital word after a written name ("Stellantis STLA");
  - an ISIN, FIGI or LEI whose check digits hold;
  - a CIK, a DART corp code or another identifier a node's properties state.
- Exchange suffixes compare by the venue families that followed assets already use
  (`identity::market_family`). `0489.HK` and `HKEX:489` are one code. One ticker
  on two families is two codes.
- A trailing capital word is not a ticker in these cases:
  - the other name spells it as one of its own words ("Samsung SDI");
  - it is a legal form, a roman numeral or a share class;
  - it is the initials of the name before it ("General Motors GM").
- Two names agree when, without their code, legal forms, punctuation and case, they
  are equal or one's words begin the other's ("Toyota", "Toyota Motor", "Toyota
  Motor Corporation"). A spelling stored on the node counts the same way. Names in
  different scripts do not agree unless one of those spellings does.
- A single letter after a dot is a share class kept on the ticker (`BRK.B`,
  `NYSE:BRK.B`), not an exchange. A bare number names no market: six digits are
  Korean, Shanghai and Shenzhen codes, four digits Tokyo and Hong Kong codes. A bare
  code joins a suffixed or prefixed code only when the node's stored `exchange`
  names the market or the names agree, so `KRX:600731` and `SSE:600731`, or
  `TSE:6481` and `HKEX:6481`, never join through the number alone. A node's codes
  include those its stored spellings state, so a node with no code of its own that
  took one share class's name refuses the other. A trailing capital that abbreviates the name
  (`LG Energy Solution LGES`) is not a code. Different tickers in one market never
  join (`005930.KS` and `000660.KS`, `BRK.A` and `BRK.B`), and a shared CIK or other
  identifying code does not override that split. A node that carries only that
  shared code cannot put both classes in one repair group, and a model merge
  into the other class is refused. A chunk that names both
  `Berkshire Hathaway (NYSE:BRK.A)` and `Berkshire Hathaway (NYSE:BRK.B)` keeps
  both; the shared issuer name does not collapse them first. Repair and the
  resolver both use this join.
- Which names become one node does not depend on their order. Names in one chunk
  that share a name key or a code the host joins by, and stored nodes the host
  joins in repair, form groups by connection. A group that holds two names the
  codes keep apart loses each name that joins both to a group of its own; when no
  such name explains the conflict, the group stays unmerged. In one chunk the
  name kept is the more specific type, then the longer name, then the first in
  code-point order.
  Across markets, names must agree through a spelling or the two must share an
  identifying code such as an ISIN.
- The host refuses a merge the model proposed when the codes differ and the names do
  not agree ("Stellantis STLA" into "Dongfeng Motor 0489.HK"), and when the tickers
  split one market ("BRK.A" into "BRK.B", "Alphabet GOOG" into "Alphabet GOOGL").
  When the codes differ across markets but the names agree, the model's decision
  stands ("Toyota TM" and "Toyota 7203.T").
- The host joins a name to a stored node without the model when a code is equal and
  either the code identifies the entity on its own or the names agree. The node must
  be of the same kind, or one of the two untyped. Codes that identify on their own:
  - a ticker both sides write with its exchange suffix or exchange;
  - a checked ISIN, FIGI or LEI;
  - a CIK or a DART corp code.
  "Tesla TSLA" joins "Tesla Inc TSLA". An equal bare trailing capital under names
  that do not agree ("Google AI" and "Meta AI", "Apple TV" and "Samsung TV")
  stays the model's decision. When two stored nodes qualify and their codes differ,
  the codes decide nothing.
- Every other case is the model's decision, as before, including every name without
  a code.
- A resolved spelling becomes an alias unless the host would refuse it beside the
  node's name and codes.
- Each World run log entry counts these decisions as `entityResolution`
  ([Diagnostics](../dev/diagnostics.md#diagnostics)).

A World resolved before these rules can hold what they refuse. Every publication
from a Source repairs it before the seal (`entity_codes/repair.rs`, called from
`graph_jobs::publish_and_complete`), so the first publication after an update
repairs the existing World:

- An alias the host refuses beside its node is dropped.
- Nodes the host joins merge into the one World saw first (its first
  `entity_added`). Its name and aliases, missing properties and missing summary text
  join the survivor, within the alias, property and description caps.
- Claims keep their ids, text, status and evidence and move to the survivor. A
  relationship between two merged nodes is now about one entity and is withdrawn,
  as publication already withdraws such relationships. Growth events stay as written.
- Lens anchors and asset pages that named a merged node name the survivor.
  Discovered connections, journal entity ids and Lens `world:` links in the
  current page text name the survivor too. That page is reindexed on its new
  revision the way Lens publication indexes a page. A connection whose ends
  become one node is dropped. Growth events and the op log stay as written.
- The repair touches a node only under those two rules. Names whose codes differ but
  agree in name ("Toyota TM" aliased "Toyota 7203.T") stay as the model resolved them,
  and so do names that share only an equal bare trailing capital.
- The pass compares only nodes that share a code value. It reads growth events only
  when two nodes do. It repeats while a merge gives the survivor a new code, so a
  second pass over its result changes nothing.

### Discovery after publication

Every World publication runs `world_followups::after_seal` in the transaction that
writes its growth events, with no model call. Three things follow from the diff
between the previous and the new snapshot. Lens sections whose `claim` basis names
a claim the publication disputed, withdrew or dropped become contested (one page
revision and one `lens_contested` growth event per page; the section's text is
kept). Pending subject name anchors resolve by a node's stored spellings or
identifying codes ([Lens](lens.md#lens)); a shared entity key does not store a
node. When any resolves, when a node a
confirmed subject anchor names has another name or alias set than in the
previous snapshot, or while any live page still has a pending subject anchor
(method learning may read it as the company the Sources it cites confirm,
without storing that), method learning rechecks its evidence, at once or when
the review queued or running at that moment settles. The revision's
dependencies say which Sources it has read; learning trusts a cited Source's
claims about a pending page only once that Source is among them. Then
`connections::detect` walks two or three hops from each seed over claims that are
not withdrawn, keeping each claim's direction and endpoint roles, to a name the seed
could not reach within two hops before, with at least one hop on a claim this
publication added. Seeds are the Loupist's followed assets that `resolve_followed` matches
exactly (an ambiguous symbol seeds nothing; that Loupist's saved followed list is read from
the app database before the Loupist transaction opens) plus the nodes open idea
pages are anchored to. A middle node with more than 40 neighbours is never walked
through, and a Place or Metric node is neither a far name nor a middle: a shared
country or a reported number is not a connection. The score prefers a far name in another World category, short paths,
low-degree middles, parallel support and recent claims. Work is bounded (claims per
node, expansions per publication, 50 candidates), and a result says when it was
cut short. `knowledge_connections` keeps one row per (seed, far) pair for ever, with
`status` moving `new → shown → opened | dismissed | filed`. Automatic Bot link
posts are disabled: extraction of a company's products or segments does not establish
personal relevance. Re-enabling delivery requires a current followed-list
snapshot read before the Loupist transaction, exact identity resolution at both
ends (or a followed asset linked to an event with a fact date), and at most one
card per reply. Existing detected paths remain in World silently. A link is new to this
Loupist, never a claim that the market missed it.


An asset page retains its selected entity across ticker correction and undo
while the node is present in the published World. Clearing the link records
a rejection in the page's Loupist. Future matches exclude that node. If a
linked node disappears, the matcher can choose from the remaining eligible
nodes in the current World.

## Code

- Discovery: `src-tauri/src/knowledge/connections.rs` (pure detector), `src-tauri/src/knowledge/world_followups.rs` (post-seal steps and the `knowledge_connections` table)
- World extraction and index: `src-tauri/src/knowledge/world_extract/` (`pipeline`, `bridge`, `index`, `search`, `job`)
- Identifying codes in entity resolution: `src-tauri/src/knowledge/entity_codes.rs`
- Queue, scheduling and interruptions: `src-tauri/src/knowledge/graph_service.rs` (worker loop and slots), `src-tauri/src/knowledge/graph_jobs.rs` (job states, retries, request limits)
- Keep-awake: `src-tauri/src/power.rs` (rule and macOS assertion), `src-tauri/src/commands/power.rs`
