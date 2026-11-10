# World search

Retrieval uses the local embedding model. Fresh folders take Rust's processing defaults, and startup resumes or quarantines unfinished work.

Semantic retrieval uses the bundled **potion-base-4M, 128-dimensional** local
embedding model from `src/knowledge/embed_assets/`, with profile `local-en-128`.
Its tokenizer is English-based (`baai/bge-base-en-v1.5`); do not describe it as the
multilingual 256-dimensional model. Hybrid retrieval combines relationship hits
and matched-entity neighborhoods. Lexical aliases and identifiers also participate
in discovery. Because World text is English, the English embedding model covers it;
original-language names match through aliases. Text the English model cannot encode
gets a deterministic character-trigram fallback vector, so vector search never receives
a zero vector; that fallback matches near-identical spellings only. Embedding caches
are disposable and keyed by model/text identity. Rust keeps the loaded model and
tokenizer in process, reading and hashing the asset bytes only on the first use of
an asset directory or after its files change (size, modification time and, on Unix,
file identity/change time). A changed or missing asset is checked before reuse;
the fingerprint still identifies the exact bytes used to load the model. Local
inference callbacks encode their texts in bounded batches, sharing one model
lookup for the batch.

Fresh folders receive World processing settings from Rust. New Loupist uses these
defaults; Settings → World processing exposes enablement and model selection.
Focusing a field there scrolls it into the settings column.
Defaults prefer connected,
supported text models using the ordered policy in
[`world-model-defaults.json`](../../apps/loupist/world-model-defaults.json).
The exact model/effort pair must exist in the catalog. Existing folders keep their
saved choices; changing the Chat model does not change World processing.

An enabled new Loupist without a suitable default stores a pending marker. Connecting
or reloading a supported provider can complete that setup once. Explicitly saving
World preferences clears the marker. Processing consumes the selected provider's
usage and sends retained text to it; account disconnection leaves Sources readable.
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

Until a Source's job settles, its relationships are not on the Map. `world_search`
marks such a Source hit `mapPending`, and the system prompt tells the model that
missing World relationships for it prove nothing.

Startup resumes untouched work and reruns attempts the app's exit cut short within
their allowance ([interruptions](worlds.md#queue-and-interruptions)); an uncertain paid
attempt past it quarantines. Recover World processing rebuilds the derived index from the confirmed
SQLite snapshot with local embeddings. Recovery does not authorize re-extraction
of uncertain work, and preserves pause/uncertainty state. A confirmed rebuild lets
untouched queued work continue. World processing publishes World; a shared pack's
Add and Remove also publish it, without a model ([Sharing](sharing.md#sharing)). Method learning
is a separate review, described under [Method learning](lens.md#method-learning).

## Code

- World defaults: `src-tauri/src/providers/world_defaults.rs`, `src-tauri/src/commands/knowledge/world_setup.rs`
