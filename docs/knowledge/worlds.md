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
World history or restore feature. Recovery rebuilds the derived index from the
current canonical World and retained Source evidence. An uncertain publication
instead keeps its extraction JSON while quarantined for reconciliation.

Long Sources become bounded episodes retaining the original span as evidence.
Every World extracts with one shared core schema: the entity types Organization,
Person, Asset, Product, Place, Policy, Event and Metric, and twelve relation types
(SUPPLIES, COMPETES_WITH, PARTNERS_WITH, SUBSIDIARY_OF, ACQUIRED, INVESTED_IN,
LEADS, OFFERS, USES, REPORTS, LOCATED_IN, AFFECTS) with allowed endpoint types. An
entity that fits no type stays `Entity`, and a relationship that fits no type keeps
a specific open name, so the schema guides extraction without closing it.
Each Loupist may store a small World extension on top of that core: at most four
entity types and six relations. The extension applies to Sources processed from
then on; extraction does not reclassify nodes already extracted. A semiconductors,
dividends or macro & rates template whose focus is still the template default
(English or Korean) starts with a curated extension (`curated_seed` in
`knowledge/world_schema.rs`). The Macro & rates seed adds `EconomicIndicator`
with English and Korean labels and the relations SETS_POLICY_FOR, PUBLISHES,
MEASURES and MOVES_WITH. The [template seed check](../../apps/loupist/evals/world/README.md#template-seed-check)
(`template-seed-grade.mjs`, no model) gates a class seed: on its six-Source fixture the
seed must keep no fewer typed core relations and no lower typed share than the same
Sources without it, or the grader reports `hold`. Its fixture covers all four asset
classes. Other templates start empty, and later growth is a proposal
the user applies or dismisses. A new Loupist whose focus is non-empty, who did not
receive that curated seed, and whose World processing is enabled on a model gets
one model-written seed. Editing the focus does not write another seed. A valid
custom-focus seed is a suggestion the user accepts; it never applies itself.
An empty one is dismissed;
an invalid call is recorded only in the run log. Source jobs do not wait for
that seed. A preparing seed that is still preparing after a restart is marked
failed. After about ten later Source jobs, and no more than once a day, the
same model may propose a review once the World queue is idle. A review adds
at most two entity types and three relations, drops kinds the Loupist already
has, and stays pending until the user applies or dismisses it. Open relation
names are not merged. The Map draws an extension type in that type's
category and labels the inspector with the type's display name. Instruments the
user follows are an extraction hint when the episode names them, and each is also an
anchor ([What you follow](follow.md#what-you-follow)). A hint or anchor counts only when the
episode names it as a whole word: a name in any case, a ticker or code (no lowercase
letter) only as written, so ticker `A` is not every "a"; names in other scripts match
anywhere because particles attach to them. The observed relationship hint counts canonical names after that
alias mapping.
Organization records a stated ticker, exchange and identifier, and Asset a symbol,
identifier and class. Organization, Person, Asset, Product, Place and Policy record a
non-English original name, which becomes a searchable alias. One attribute call per
episode fills every typed entity's fields. World stores names, summaries, relation
names and facts in English whatever the Source language; the original text stays in
the retained Source and its evidence spans. Research focus, preferred relationship
names and anchor entities add emphasis on top of the core schema. Publication time,
observation period and validity remain distinct. Extraction preserves quantitative
units and basis, directed roles and conflicting attributed reports; source content
is data, not extraction instructions. Temporal bounds require explicit effective
instants; date-only precision is retained in the fact text. Invalidation needs
ordered start instants on both claims, so a date-only correction can be retained
without structurally ending the earlier claim.

## Code

- World extraction and index: `src-tauri/src/knowledge/world_extract/` (`pipeline`, `bridge`, `index`, `search`, `job`)
