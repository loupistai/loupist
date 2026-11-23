# World search

Retrieval uses the local embedding model. Fresh folders take Rust's processing defaults, and startup resumes or quarantines unfinished work.

Semantic retrieval uses the bundled **blink-multilingual-64M** local embedding
model: 256-dimensional vectors over a 250,000-token multilingual vocabulary. World
settings name it by the profile id `local-en-128`, which names whichever model is
bundled. `stage-embed-assets.py` downloads it at the revision pinned in the repo-root
`src/knowledge/embed_assets/pin.json`, checks each file's sha256 and stores the
table as int8 with one scale per row. Its license and provenance are in the
[dependency review](../dev/dependencies.md#local-embedding-model). Hybrid retrieval
combines relationship hits and matched-entity neighborhoods. Lexical aliases and
identifiers also participate in discovery. One model embeds English and Korean
text alike, so a query in one language lands near text in the other; names in the
original language also match through aliases. Text the model cannot encode gets a
deterministic character-trigram fallback vector, so vector search never receives
a zero vector; that fallback matches near-identical spellings only.

The first load takes about 2.5 seconds of CPU on Apple Silicon (4 on a busy
machine), nearly all of it the tokenizer registering 30,000 added Chinese and
Japanese words, which are kept. The app starts the load on its own thread at
launch, and nothing interactive waits for it. Until it finishes, knowledge search
ranks by words and World names alone and `world_search` adds a warning that a Source
in another language may be missing, while the World index answers from the snapshot
with its unavailable warning. One look at the model cache gives both the model and
whether it is still loading (`ModelLookup`), so a search that ranked without the
model always carries the warning, even when the load ends a moment later. Only a search in the first seconds after launch meets
this, and waiting would hold that turn for the rest of the load; the warning tells
the model to search again rather than conclude that the material is absent. World
jobs and local embedding callbacks, which run on blocking threads in the
background, wait for the load.

Rust keeps one loaded model per process and shares it. It reads and hashes the
asset bytes only on the first use of an asset directory or after its files change
(size, modification time and, on Unix, file identity/change time). A changed or
missing asset is checked before reuse, and a failed load is retried only after the
files change; the fingerprint identifies the exact bytes used to load the model.
Embedding caches are disposable and keyed by that fingerprint and the vector
width, so a library cached or indexed by another model embeds its texts again on
first use. Local inference callbacks encode their texts in bounded batches,
sharing one model lookup for the batch.

Fresh folders receive World processing settings from Rust. New Loupist uses these
defaults; Loupist settings → World building exposes enablement and the model
([World building model](#world-building-model)). Focusing a field there scrolls it
into the settings column. Processing consumes the provider's usage and sends
retained text to it; account disconnection leaves Sources readable.
[Shared providers](../runtime/providers.md#shared-providers-and-world-processing) owns
credential and isolated inference boundaries.

Enabling/resuming processing backfills eligible readable Sources. Already consumed
spans and attempted jobs are not automatically extracted again. Jobs are durable
and sequential per Loupist; their first extraction captures the latest World snapshot.
How the worker orders and overlaps them is in
[Queue and interruptions](worlds.md#queue-and-interruptions).
A chunk whose frozen budget cannot cover extraction waits without inference.
Rust validates output and commits the new World, its derived index rows and the
job state in one transaction.

Until a Source's job settles, its relationships are not in World. `world_search`
marks such a Source hit `mapPending`, and the system prompt tells the model that
missing World relationships for it prove nothing.

Startup resumes untouched work and reruns attempts the app's exit cut short within
their allowance ([interruptions](worlds.md#queue-and-interruptions)); an uncertain paid
attempt past it quarantines. Recover World building rebuilds the derived index from the confirmed
SQLite snapshot with local embeddings. Recovery does not authorize re-extraction
of uncertain work, and preserves pause/uncertainty state. A confirmed rebuild lets
untouched queued work continue. World processing publishes World; a shared pack's
Add and Remove also publish it, without a model ([Sharing](sharing.md#sharing)). Method learning
is a separate review, described under [Method learning](lens.md#method-learning).

## World building model

A Loupist's World model is **Recommended** unless the user picks one in Settings,
which makes it **Chosen** (`worldModelChoice` in its World settings). New Loupists
start Recommended.

Recommended resolves whenever the host uses it, never once at creation: the first
model in [`world-model-defaults.json`](../../apps/loupist/world-model-defaults.json),
in the list's provider and model order, that a connected account offers with the
listed thinking level, that supports isolated inference and that the user has not
hidden. The provider of the Loupist's Chat default goes first (the account the user
evidently uses; a Loupist's own Chat default before the app's), then the list's
order. With two accounts, OpenAI and Anthropic, the list picks OpenAI's first listed
model; with Anthropic's Claude as the Chat default it picks Anthropic's. With no
such account World waits, saying it waits for a supported AI account. Rust saves the
resolved model on the Loupist when an account, the catalog, a model's visibility or
the Chat default changes, at startup, when the Loupist opens or its World settings
are read, and again before each World job, schema review and method-learning
review. So the next job uses what the list says for the accounts connected then,
and World building's status and the run log name that model. A change between two
jobs asks nothing of the user. Sources saved while it waited, and work the old
model could not do, are offered to the new model as after a saved model change.

Chosen never changes by itself. When its model is gone or its account is
disconnected, World says "Choose another AI model", and Settings and the World
status line offer **Use the recommended model**. A disconnect keeps the provider's
status row only for Chosen models ([Shared providers](../runtime/providers.md#shared-providers-and-world-processing)).
Settings › World building puts **Recommended (model)** first in the model control,
then the connected accounts' models. The thinking level and run options shown for
Recommended are the list's and become editable once a model is chosen; picking the
recommended model itself makes it Chosen. New Loupist exposes no World model.

World settings saved before this record have no `worldModelChoice`. A model, with
its thinking level, that any earlier list named
([`world_defaults_earlier.json`](../../apps/loupist/src-tauri/src/providers/world_defaults_earlier.json),
every entry of every version in `git log -- apps/loupist/world-model-defaults.json`,
frozen) reads as Recommended, as does no model; anything else reads as Chosen. A
Loupist created under one provider and now used with another therefore follows the
connected account, unless its model or effort was changed by hand.

### The World model list

`world-model-defaults.json` is one comment-free JSON array: each provider once, with
its models in preference order as `{"id", "thinking_level"}`. `build.rs` refuses a
list with another field, an empty provider, an id listed twice for a provider or a
thinking level Pi does not accept (`providers/world_model_list.rs`). To add a model:

- Name the exact provider model id Pi reports; Loupist never guesses one, and the
  exact model and effort pair must be in the catalog to be recommended.
- List a provider's newest low-cost model first and keep older ones after it as
  fallbacks. Flagship and specialist models are never listed.
- Give the effort measured on World extraction, not the model's default: GPT-6
  Luna stays at high, because medium was 42% faster but stored 14–18% fewer claims
  on the World ontology corpus.
- `agent/world-model-defaults.test.mjs` sends every pair through Pi's request
  serializer and response parser; it must pass.

Recommended Loupists follow a changed list from the next startup. Chosen ones and
the earlier lists' record do not change.

## Names across languages

Knowledge search (`world_search`'s default scope, over Sources, Lens pages and World
claims) bridges names through World. When a query names a World entity by its name,
an alias or an upper-case ticker, a document that spells the entity another way
counts as matching those query words: `한빛반도체` finds a Source that writes
Hanbit Semiconductor, and the reverse. Nodes whose name keys meet (the
[identity](lens.md#method-learning) components) share their spellings. A name
counts only as a whole word, with a Korean particle allowed after it
(`한빛반도체가`), so a generic word inside a name (`반도체` in `한빛반도체`) and a
longer word that starts like one (`하이브리드`, `하이브`) expand to nothing. A
ticker matches only in upper case (`ALL` is not `all`); a spelling two entities
share, one under three letters or one Hangul syllable, and a number under six
digits are never used. A short form, the first word of a name (`Hanbit`,
`노스윈드`) or the start of a Korean name written as one word (`파인크레스트`),
expands only when it belongs to one entity and at least half of the library's
Sources and Lens pages that use it also spell a full name, so `bank` does not
become the Bank of Korea. Names come from the current World, read once per World
revision; the query goes to no model or network.

Knowledge search ranks by reciprocal rank fusion of the lexical order and the
embedding's ten nearest texts from cosine 0.2, Graphiti's 1/rank as the World index
fuses BM25 and vectors. A hit's `score` is its sum of 1/rank in each list times
1,000,000, and equal values share a rank. So a Source in another language that the
model puts near the query is not buried under a same-language Source that shares
one query word. The score says where a hit ranks, not how much of the request it
matched. The Lens pointer therefore takes a page that matches two of the request's
words, or every term group of a request that is one concept (`cash flow`), and not
a page that matches one word of several. The World-scope merge puts World's
fused rows first and keeps the other hits in knowledge-search order.

Cross-language recall is measured by the [search recall test](../dev/tests.md#search-recall)
(recall@5 in knowledge search; November 2026):

| Ranking | Model | Korean→English | English→Korean |
| --- | --- | --- | --- |
| Before name bridging | potion-base-4M | 0.12 | 0.04 |
| Name bridging, lexical first | potion-base-4M | 0.38 | 0.38 |
| Name bridging, fusion | potion-base-4M | 0.29 | 0.25 |
| Name bridging, lexical first | blink-multilingual-64M | 0.59 | 0.58 |
| Name bridging, fusion | blink-multilingual-64M | 0.85 | 0.83 |

Same-language queries stayed at 1.0 in every row. Lexical first added up to 200
embedding points to a lexical match and admitted an embedding-only match from
cosine 0.55. The English-only potion-base-4M model's misses named no World entity (a
Fed decision, copper prices) or differed in words other than the name, and its
vectors for Korean text are noise, so fusion lowered its recall. In the World index,
blink-multilingual-64M finds all 13 Korean queries' English claims in the top five,
against 3. The multilingual model pays for itself only with fusion, so the two were
adopted together (the last row).

## Code

- World building model: `src-tauri/src/providers/world_defaults.rs` (resolution), `src-tauri/src/providers/world_model_list.rs` (list rules, also run by `build.rs`), `src-tauri/src/commands/knowledge/world_setup.rs` (following the recommendation), `settings_for_work` in `src-tauri/src/knowledge/graph_service.rs` (before each job)
- Knowledge search, rank fusion and names across languages: `src-tauri/src/knowledge/search_index.rs`
- Loading and sharing the embedding model: `src-tauri/src/knowledge/graph_inference.rs`; staging and pins: `apps/loupist/scripts/stage-embed-assets.py`, `src/knowledge/embed_assets/pin.json`
