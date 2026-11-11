# Datasets

A dataset is the current SQLite snapshot for one fetch or import, with a trust tag. Cutoffs and market reads are in [Provenance](provenance.md).

## Data and provenance

Native finance results and imported local JSON are saved in SQLite as the current snapshot for each dataset. A `DatasetRef` includes a Loupist-scoped dataset ID, revision and SHA-256 digest. Replacing that dataset removes its previous bytes; an old reference fails instead of silently reading newer values. `dataset_describe` returns metadata, row sections, column names and a small sample; `dataset_query` reads a selected JSON Pointer, page and columns. Queries are bounded to 1,000 rows and 1 MiB. Charts can map those same stored values through `from_dataset` and retain their own published data.

Native finance and OpenBB data results record `fetched_at` (UTC) in the dataset
metadata and return it; `dataset_describe` and `dataset_query` return it too.
An optional `max_age` (whole seconds, 0–2,592,000) on `worldbank_series`,
`defillama_read`, `krx_daily_quote`, `kis_read`, `dart_financials`, `dart_filing_search`,
`dart_filing_read`, `dart_major_report`, `dart_ownership`, `dart_dividends` and
OpenBB data tools reuses this Loupist's saved result for the identical request
and cutoff (for `kis_read`, also the same credential revision, environment and
account, by digest) when it was fetched at most that long ago and marks the result
`reused`. Freshness is checked from metadata before loading the payload; reusable
bytes still pass their digest check. The host removes the argument before the
provider sees the request; without it every call fetches. A refetch that returns identical bytes keeps the dataset revision
and updates its `fetched_at`. Each native dataset records the cutoff it was
fetched under. In a turn with a cutoff, `calculate`, `dataset_describe`,
`dataset_query` and `dataset_export` accept only revisions recorded under a
cutoff on or before it and refuse others with `as_of_unsupported`, so a value
fetched in an earlier, uncut turn is refetched instead of leaking later data.

Each retained dataset also records `trust`: `third_party_text` for a route that relays
provider prose (`dart_filing_read` and OpenBB routes outside the numeric-only list),
`numeric` otherwise, and `user_file` for `dataset_import` unless a declared input is not
trusted. `dataset_query` and `dataset_describe` deliver the rows, sample and column
names of any other dataset delimited and raise the
[untrusted-text tier](../architecture.md#trust). That includes a revision with no class,
until it is fetched again. `chart_query` and `artifact_read` apply the same rule
to charts built from a pinned dataset; a replaced or unavailable input revision
cannot turn its retained chart rows into trusted data. Artifact text from a
reply recorded as tainted is also delimited on a later read.

`dataset_export` writes the current snapshot identified by a pinned reference, or a JSON Pointer selection, directly to the selected local workspace. JSON preserves the selected structure; CSV accepts row objects with an explicit column order or stable alphabetical columns. The returned path, byte digest, row count and `.provenance.json` sidecar let local scripts use the same inputs without copying them through model output. The host enforces Loupist ownership, relative paths, no symlink traversal and no overwrite. It stages both outputs before publication and rolls back only files it owns on publication failure; concurrent replacements are preserved. These are immediate local files and remain if the reply stops.
Exports from untrusted datasets also record the exact output and sidecar digests
in the host's private trust registry before publication. Native reads of those
bytes, including unchanged copies, return delimited and raise the untrusted-text
tier; an import of the same bytes stays `third_party_text` even without declared
inputs. External edits or transformations are not inferred from their filenames.

`dataset_import` reads a workspace-relative UTF-8 JSON file (maximum 32 MiB)
through pinned, no-follow file handles and saves a complete parsed JSON snapshot. Import commits immediately, even if
the reply later stops. Exact monetary decimals should use JSON strings.
An optional `expected_sha256` checks the exact file bytes. Optional `input_refs`
name up to 16 owned, currently available dataset references and JSON Pointer selections.
The receipt records the file hash, size, path and declared inputs; it does not
verify execution, external origin or historical availability. Imports are local
derived results, never acquired Source material. No historical cutoff is inherited.
Reimporting changed file bytes or declared inputs replaces that dataset and
invalidates its previous reference. Changing or deleting the workspace file alone
does not change the saved snapshot. Exact JSON payload hashes
and original file-byte hashes are distinct when whitespace changes.

The current-turn `result_ref` still supports immediate Tool composition and Source receipts. It expires with the turn. A durable dataset reference survives Chat changes. Results over 16 KiB are delivered to the model as bounded summaries, while the host retains the complete payload. Retained coverage, model-delivered coverage and computation inputs are recorded separately. A summary does not establish that the model read a full original.

Native file and `run_python` results use Chat activity records for execution metadata and bounded model-visible output. Expanding a step reads its command and output inline without opening a workspace tab. Long commands stay collapsed behind a one-line preview, and output previews have a bounded scrolling area. Detail polling stops when the disclosure closes or Chat becomes inactive. They do not automatically become immutable datasets or Source receipts. `run_python` output that exceeds the model-facing limit stays in the Chat's private runtime temporary directory until Chat removal. Retaining a canonical Source or publishing a chart still uses the corresponding app service.

Provider JSON numbers normalize to decimal strings across small and large
magnitudes. Explicit missing values remain null, and provider decimal strings
keep their decimal digits. Normalization does not recover precision already lost
when a provider supplies a floating-point number.

## Code

- Local analysis: `agent/workspace-tools.mjs`, `src-tauri/src/datasets/`, `src-tauri/src/commands/tool_executor/dataset_import.rs`
