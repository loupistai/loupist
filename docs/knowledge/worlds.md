# World processing

Each Loupist has one World. Rust extracts and reconciles in-process, then publishes a sealed snapshot. Search and defaults are in [World search](world-search.md).

## Worlds and graph processing

Each Loupist has at most one World. Its current sealed snapshot contains nodes,
claims, ordered endpoint roles and Source support/opposition/context. World content
in Map is read-only; questions or correction requests attach the relevant claim to
Chat. A correction retains its exact Source basis.

Detail reads resolve the exact Loupist/revision and do not invoke inference. Graph
camera/selection state survives source visits; Fit explicitly reframes the displayed graph.

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
World history or restore feature. Job polling returns the receipt fields without
the internal extraction document or retained Source text. Recovery rebuilds the derived index from the
current canonical World and retained Source evidence. An uncertain publication
instead keeps its extraction JSON while quarantined for reconciliation.
Publication bounds extracted node names and descriptions, drops unreadable nodes
and claims, and drops claims whose endpoints were discarded, so a malformed item
does not block otherwise valid extraction.

Long Sources become bounded episodes retaining the original span as evidence.
Each episode runs after the one before it, whose text is its extraction context; once
that episode completed or can no longer finish, the next one runs.
Every World extracts with one shared core vocabulary, listed once in
`knowledge/world_extract/ontology.rs`: the entity types Organization, Person, Asset,
Product, Place, Policy, Event, Metric, Technology and Segment, and seventeen relation
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

### Queue and interruptions

World jobs wait in each Loupist's folder database and survive restarts. The graph
worker (`knowledge/graph_service.rs`) runs up to two jobs at once, never two for one
Loupist, since each publishes on its own World head; their paid calls share four
slots, so a provider sees at most four World calls at a time. A Loupist with a Chat
turn in the last 30 minutes goes first, and among its jobs the Sources that Chat
saved come before an older backlog (`queued_loupist_heads`). The other Loupists take
turns, the one that started a job longest ago first, so one Loupist's backlog does
not hold back another's research. A job that ran without progress waits until
something outside the loop changes (a save, a setting, a connection).

Within an episode, calls whose inputs do not depend on each other go out together:
entity resolution with relation extraction (relations name the entities as
extracted, as Graphiti's `semaphore_gather` runs them), then relation resolution with
attributes. Prompts, the number of calls and the order the call budget is spent in
are those of one call after another (`Bridge::llm_many`). The next episode's entity
extraction does not start early: its instructions name the relations already on
the Map, which the episode before it can change.

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
its predecessor's unused calls; splitting a Source cannot increase its total.
Previously saved positive per-chunk allowances stay as saved, since an old
completed receipt does not say how many of its calls were unused.
An episode that exhausts its allowance pauses its unfunded tail without repeating
paid calls or quarantining other Sources; resume does not replay its paid attempt.
Revision and growth-event timestamps reflect extraction completion.

An attempt that publishes nothing ends one of three ways (`finish_unpublished` in
`knowledge/graph_jobs.rs`). Publication is one transaction, so the World is unchanged
in each case. Driver errors settle their claimed attempt. Each scheduler pass also
settles expired leases that have no live host worker, using the same retry and
quarantine rules, without restarting the app. Replacement attempts have a new
identity: an old worker cannot record calls or publish after reclamation.

- **Nothing paid was sent** (the provider went away, its settings changed under
  it): the attempt cost nothing and queues again, up to three times, then fails
  without quarantining the World; Try again can run it. Enqueuing that Source or
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

After a quarantine the graph worker queues the recovery itself
(`recover_world_unattended`): the rebuild calls no model, re-queues the jobs that
never ran, and enqueues Sources saved during the quarantine. If that rebuild fails,
the next one waits for World processing recovery in Loupist settings, so a broken
rebuild cannot loop.
Only a committed `reconcile_required` receipt may add a generation to quarantine;
a rolled-back attempt cannot leave a quarantine without a recovery receipt.
Pausing processing pauses every queued chunk of a Source, including Sources saved
while paused; recovery rebuilds remain runnable while extraction is paused.

### Keeping the Mac awake

Settings → World processing offers one application-wide choice, off by default:
keep this Mac awake while the World is processing (`keep_awake` in the host
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
  a preset later from chat: only the curated kinds the map lacks, as one preset change
  that keeps the asking message, so repeating it adds nothing. Other templates start
  empty; a new curated seed ships only after the
  [template seed check](../../apps/loupist/evals/world/README.md#template-seed-check)
  (`template-seed-grade.mjs`, no model) shows no fewer typed core relations and no
  lower typed share than the same Sources without it, or it reports `hold`.
- **Chat.** `world_schema_update` (`memory_write`, in `memory-save`, interactive
  turns only, one per turn) is staged and commits with the reply, so a stopped
  turn applies nothing. A change naming a kind the map gained after staging is
  skipped, and the answer's `memory_update.skippedKinds` lists its kinds so the
  receipt says they were not added; any other refusal at commit is an error. The
  user's sentence is the approval, and the change keeps the asking message. The Memory envelope carries a `worldKinds` block so the agent can
  tell what is already tracked.
- **Review.** After about ten later Source jobs, once a day and when the World queue
  is idle, the World model may promote what the World already shows. Nothing waits
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
  else left to reverse offers no Undo. A preset or review change also posts one
  Bot message, "Your map now also tracks ...", in the labels the user sees, with
  Undo. A chat change, a preset applied from chat included, is already in the reply. Settings shows these under
  [What the map tracks](../ui/map-lens.md#what-the-map-tracks).

The Map draws an extension type in that type's
category and labels the inspector with the type's display name. Instruments the
user follows are an extraction hint when the episode names them, and each is also an
anchor ([What you follow](follow.md#what-you-follow)). A hint or anchor counts only when the
episode names it as a whole word: a name in any case, a ticker or code (no lowercase
letter) only as written, so ticker `A` is not every "a"; names in other scripts match
anywhere because particles attach to them. The observed relationship hint counts canonical names after that
alias mapping.
Organization records a stated ticker, exchange and identifier, and Asset a symbol,
identifier and class. Organization, Person, Asset, Product, Place and Policy record a
non-English original name, which becomes a searchable alias. A spelling the
resolver finds to be an existing node ("Acme Corporation" for "Acme Corp") also
stays an alias of that node, unless a name or alias already folds to it, within
the 16-alias cap, so a Lens anchor filed under that spelling resolves exactly. One attribute call per
episode fills every typed entity's fields. World stores names, summaries, relation
names and facts in English whatever the Source language; the original text stays in
the retained Source and its evidence spans. Research focus, preferred relationship
names and anchor entities add emphasis on top of the core schema. Editing this guidance retains saved anchor identifiers as identifiers. Publication time,
observation period and validity remain distinct. Extraction preserves quantitative
units and basis, directed roles and conflicting attributed reports; source content
is data, not extraction instructions. Temporal bounds require explicit effective
instants; date-only precision is retained in the fact text. Invalidation needs
ordered start instants on both claims, so a date-only correction can be retained
without structurally ending the earlier claim.

### Discovery after publication

Every World publication runs `world_followups::after_seal` in the transaction that
writes its growth events, with no model call. Three things follow from the diff
between the previous and the new snapshot. Lens sections whose `claim` basis names
a claim the publication disputed, withdrew or dropped become contested (one page
revision and one `lens_contested` growth event per page; the section's text is
kept). Pending subject name anchors resolve by canonical name or alias, exactly,
and by nothing else ([Lens](lens.md#lens)); when any resolves, when a node a
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
publication added. Seeds are the followed assets that `resolve_followed` matches
exactly (an ambiguous symbol seeds nothing; the watchlist and holdings are read from
the app database before the Loupist transaction opens) plus the nodes open idea
pages are anchored to. A middle node with more than 40 neighbours is never walked
through, and a Place or Metric node is neither a far name nor a middle: a shared
country or a reported number is not a connection. The score prefers a far name in another Map category, short paths,
low-degree middles, parallel support and recent claims. Work is bounded (claims per
node, expansions per publication, 50 candidates), and a result says when it was
cut short. `knowledge_connections` keeps one row per (seed, far) pair for ever, with
`status` moving `new → shown → opened | dismissed | filed`; the Bot shows at most
three a day ([link posts](../ui/home-thread.md#home-thread)). A link is new to this
Loupist, never a claim that the market missed it.

## Code

- Discovery: `src-tauri/src/knowledge/connections.rs` (pure detector), `src-tauri/src/knowledge/world_followups.rs` (post-seal steps and the `knowledge_connections` table)
- World extraction and index: `src-tauri/src/knowledge/world_extract/` (`pipeline`, `bridge`, `index`, `search`, `job`)
- Queue, scheduling and interruptions: `src-tauri/src/knowledge/graph_service.rs` (worker loop and slots), `src-tauri/src/knowledge/graph_jobs.rs` (job states, retries, request limits)
- Keep-awake: `src-tauri/src/power.rs` (rule and macOS assertion), `src-tauri/src/commands/power.rs`
