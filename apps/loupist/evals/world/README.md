# World and Memory evaluations

These suites measure whether retained evidence — Sources, World and Lens — improves
later research. Product behavior is owned by
[Knowledge and Memory](../../../../docs/knowledge-model.md); shared evaluation rules
are in the [evaluation guide](../README.md). [GRADING.md](GRADING.md) owns semantic
review of the lifecycle and acquisition grader.

| Input | Grader | Measures |
| --- | --- | --- |
| `corpus-v1.json` + `oracle-v1.json` | `grade.mjs` | Lifecycle, matched off/clean/conflict comparisons, held-out repeats |
| `acquisition-corpus-v1.json` + `replay/acquisition-v1.json` + `acquisition-oracle-v2.json` | `grade.mjs` | Web acquisition into Memory, then retained reuse |
| `scale-corpus-v1.json` (`scale-oracle-v1.json` lists required measurements) | `world_scale_eval` report | Retrieval at 0, 20 and 100 unrelated Sources |
| `implicit-memory-*.json` | `memory-grade.mjs` | Uncued saving, recall and restraint |
| `ontology-corpus-v1.json` + `ontology-oracle-v2.json` | `ontology-grade.mjs` | World entity/relation typing in English, Korean and mixed input |
| `temporal-corpus-v2.json` + `temporal-oracle-v2.json` | `temporal-grade.mjs` | [Temporal invalidation](#temporal-invalidation): a later Source with an explicit effective instant ends an earlier claim; a denied allegation does not |
| `template-seed-corpus-v2.json` | `template-seed-grade.mjs` | The asset-class template seeds (Crypto, Commodities, Macro & rates, Real assets) |
| `growth-scenario-v1/` | Rust test + `report.py` | [Five-session product growth](growth-scenario-v1/README.md) |
| List in `src-tauri/src/eval_harness/followed.rs` | Rust test | [Followed assets on the Map](#followed-assets-on-the-map), at creation and followed later |

Corpus, oracle and replay files are versioned inputs. Oracles never enter a Loupist
workspace, Source, prompt, Lens or instruction file. A changed oracle or corpus is a
new version and needs a new baseline; an approved change to grading semantics may
regrade preserved runs into new reports when model-visible inputs and settings are
unchanged, labelled as a retrospective regrade.

## Running

Build the debug-only runners from a checkout whose identity (commit plus
`git status --short` and relevant file digests) you record:

```sh
cargo build --manifest-path apps/loupist/src-tauri/Cargo.toml \
  --features eval-harness --example world_eval --example world_scale_eval
<world_eval-binary> \
  --suite apps/loupist/evals/world/acquisition-corpus-v1.json \
  --out <NEW-acquisition.jsonl> --pi-agent-dir <absolute-agent-directory> \
  --resources <staged-resources> --trials 3 --keep
```

`world_eval` runs lifecycle, acquisition, Memory, ontology and temporal suites;
`world_scale_eval` runs the scale corpus. World jobs and World search run in-process
(`knowledge/world_extract/`);
`LOUPIST_WORLD_STAGE_LOG=<file>` appends one record per in-process model or embedding
call (stage, duration, usage; no prompt text) and one `episode` record with the job's
wall time. A call's duration runs from the host receiving it to its reply; calls of
one episode sent together overlap, so their sum can exceed the episode time. Both
take `--suite`, a new `--out`, an absolute `--pi-agent-dir`, `--resources` for staged
embedding resources, and `--keep` to preserve the Loupist folders for investigation. Grading uses the
canonical Source bytes and completed-job receipts captured in the trace. Other
options: `--scenarios ID,ID`; `--condition clean|off|conflict`; `--model`/`--thinking` for Chat and
`--world-model`/`--world-thinking` for World (defaults `openai-codex/gpt-6-luna`,
`medium` and `high`); `--turn-timeout-ms`, `--world-timeout-ms`, `--stage-timeout-ms`.
Live runs use the connected provider and can incur usage.

Trials are indexed 0–2. `--trials 3 --trial-index N` runs one trial without
renumbering; run independent trials in separate processes with new output paths.
Each trial gets its own temporary Loupist, workspace, database and derived World index;
stages run in order inside it and a failed stage skips its dependents as unmeasured
failures. A combined grading input must contain every row in trial order plus the
combined `.pending.jsonl`, without mixing attempts of one trial.

Controls: verify the exact Chat and World model/effort against the connected catalog
and keep the observed receipts; a different model, effort or executable is a
different comparison. Chat turns time out after 600,000 ms. Every final probe uses
`read_only` Memory. Native permissions may still allow network through web tools.
run_python has no network and there is no shell; record that rather than claiming
isolation. Stop on a provider rejection.

Harness behavior worth knowing when reading traces:

- World inference observations carry bounded failure diagnostics (category, HTTP
  status, stop reason) without provider text or credentials. A failed World
  preparation stage is distinct from a Chat failure.
- Each step checkpoints bounded workspace files to `<out-stem>.workspace-checkpoints/`
  (excluding `.loupist`, symlinks and nonregular files); grade earlier file contents
  from that checkpoint, not a later workspace.
- Optional `seed_lenses` import complete exported method Lens documents after the
  first step, verifying their digest and remapping only declared Source references.
- `world_enabled` controls extraction, not permission to read an existing World.
- Runner queries are diagnostics. A pre-Chat indexed-query failure is not an
  observed failure of the Chat research workflow.

## Lifecycle and comparison corpus

`W-LIFECYCLE` runs first research → same-Chat follow-up → a new contract document →
correction and conflicting evidence → the same question in a new Chat. The final
answer must use the corrected figure, keep the independent conflicting reports and
leave missing forecasts unknown.

`W-COMPARE-OFF/CLEAN/CONFLICT` share byte-identical final questions and current
Sources: `off` disables World processing, `clean` processes current documents, and
`conflict` also processes an old release, mirrors, an assistant-authored scenario and
an injected document. Compare clean versus off for answer benefit and conflict versus
clean for negative transfer. `W-HELDOUT-*` repeats the structure with disjoint
entities; keep it out of implementation decisions until the candidate is fixed, and
replace any case that informed a change.

`W-ACQUIRE` (acquisition corpus) starts from an old saved World, then requires an
actual `web_fetch` and `source_save` of a new issuer URL under Remember authority,
a same-Chat follow-up with empty and failed searches, and a new Chat. Responses
replay at the production web boundary; the correction is never preseeded and the
post-Chat World job must finish before the next probe.

Failure classes covered: entity and person collisions, quarter versus cumulative,
consolidated versus parent-only, currency and units, publication versus event time,
correction versus aging, republication versus independent sources, attributed
conflict, unsupported forecasts, retrieval miss versus absence, assistant synthesis
and document instructions.

| Metric | Acceptance |
| --- | --- |
| Storage precision | Supported, correctly qualified atomic assertions / all evaluated assertions ≥ 80% |
| Storage recall | Required facts retained with entity, period, basis, unit, status and provenance ≥ 80% |
| Utilization | Required answer facts used via an acknowledged Source/World read ≥ 80% on World arms; Source-only reported separately |
| Acquisition | Acquisition-corpus initial facts acquired, saved and processed ≥ 80%, separate from retained utilization |
| Negative transfer | Applicable adverse opportunities that corrupt the answer ≤ 10% |
| Invalid calls | Rejected calls / all Chat calls ≤ 5% |
| Repeated failure | Identical failed tool + arguments in one turn = 0 |
| Answer correctness | Every required fact, uncertainty and citation condition; reported per criterion and per case |

Thresholds must hold in aggregate and in at least two of three trials; reports also
show each trial, pass^3 and clean/conflict pairs. Coverage must be complete before
any completion claim. Integrity gates (exact Source bytes, authority and stable
completed-job publication receipts) must pass on every applicable attempt; see
[the integrity contract](GRADING.md#completion-and-integrity).

```sh
node apps/loupist/evals/world/grade.mjs \
  --suite apps/loupist/evals/world/corpus-v1.json \
  --oracle apps/loupist/evals/world/oracle-v1.json \
  --traces <run.jsonl> --out <NEW-report.json> --audit <NEW-audit.jsonl>
```

Behavior gates outside these corpora need their own tests or runs: explicit World
enablement, Loupist isolation, retry/reprocessing without duplicate support, partial
failure and cancellation, Source revision and deletion, injection resistance, and
reporting empty retrieval as unavailable evidence rather than absence.

## Scale

`scale-corpus-v1.json` replays one focal question with 0, 20 and 100 unrelated
Sources. Prepared claims are imported through canonical Rust services with paid
extraction off, then indexed and searched with production local embeddings; the
audit requires an empty World provider ledger and `maxLlmCalls: 0`. It measures
retrieval growth, not extraction quality. Prepared imports may share an entity node
when name, identifier and aliases agree; cases with several relevant Sources list
`focal_source_ids`. The focal claim is placed beyond the first 20-claim browse page
at larger sizes; record actual page membership, relevance/recall@k, wrong-entity
results, indexing and query latency, tokens, bytes and cost (null when unavailable).

## Uncued Memory loop

Prompts never name Memory, World, Lens, save operations or tool arguments.
Evaluator-only `checks` are excluded from the runner's inputs.

| Suite | Role |
| --- | --- |
| `implicit-memory-corpus-v2.json` | Small smoke suite (3 scenarios) |
| `implicit-memory-regression-v3.json` | Improvement set (12 scenarios, 41 turns): quotations, corrections, long-message paging, separate methods, presentation preferences, conditional exceptions, temporary overrides, conflicting publishers, quoted untrusted instructions |
| `implicit-memory-validation-v3.json` | Earlier validation (8 scenarios): similar names, ended relationships, units, language transfer, exclusions, missing versus zero, permission transitions, Unicode quotations; its results are spent |
| `implicit-memory-promoted-v4.json` | Two failed v3 validation cases promoted to regression |
| `implicit-memory-validation-v4.json` | Current frozen validation (3 scenarios): Spanish-to-Korean recall, absent identifier then exact recall, source-backed method learning |
| `implicit-memory-native-files-v5.json` | Retention after a native file read; a temporary calculation input that must stay out of Memory |

Scenario `workspace_files` exist at setup; step `workspace_files` appear just before
that step (`fixture_delivery: "per_step"`). Paths are relative, outside `.loupist`,
unique per scenario and never replace a file or traverse a symlink parent.

```sh
<world_eval-binary> \
  --suite apps/loupist/evals/world/implicit-memory-regression-v3.json \
  --out <NEW-memory.jsonl> --pi-agent-dir <absolute-agent-directory> \
  --resources <staged-resources> --trials 3 --keep \
  --model openai-codex/gpt-6-luna --thinking max \
  --world-model openai-codex/gpt-6-luna --world-thinking high
node apps/loupist/evals/world/memory-grade.mjs \
  --suite apps/loupist/evals/world/implicit-memory-regression-v3.json \
  --traces <NEW-memory.jsonl> --out <NEW-memory-report.json> --trials 3
```

The grader checks retained original bytes, evidence-backed World endpoints, learned
Lens identity and revision, exact acknowledged reads, new-Chat answers and unchanged
canonical records. Reading the standing perspective cannot pass learned-method
retrieval, and method checks cannot borrow another Lens. Conversation quotations
need an exact read of the original user text (SHA-256, author, message, Unicode
scalar slices; full reads cover through the final page); snippets and paraphrases do
not count. Korean magnitude units match their expanded numbers and `%포인트` matches
`%p`. Regex predicates cover declared facts only; also review answers in the native
readers.

Loop: freeze the suites, run a three-trial baseline, inspect failed or unmeasured
cells, fix the owning layer, and rerun all three trials. Keep validation results
unopened until the candidate is fixed, then report validation failures without
tuning against them and promote them into the next regression version. Acceptance
requires every predicate in all three trials; missing evidence, reused attempts,
mixed harness versions and fixture mismatches cannot pass. `--baseline <report>`
compares only identical suite bytes, controls and grader hash.

## Ontology eval

`ontology-corpus-v1.json` is World-only: three fictional scenarios with no Chat
turns and one English plus one Korean query after ingestion. `O-KO-SUPPLY` and
`O-EN-MARKET` are single-language; `O-MIXED` interleaves the same six Sources by
publication time to measure cross-language identity. `ontology-oracle-v2.json`
(hashes in `ontology-input-manifest-v2.json`) separates the exchange `코스닥 시장`
from the index `코스닥 지수`.

`ontology-grade.mjs` scores each scenario trial's final World snapshot without model
calls; query rows are diagnostics. Names and aliases match after NFKC, trimming and
removal of one trailing parenthetical or bracketed code; a query hit also accepts the
name inside a longer result. `duplicate_entities` counts Organization, Person, Asset,
Product, Place and Policy. An original is retained when a name, alias or
`original_name` matches the oracle original after also removing one corporate suffix
(`주식회사`, `(주)`, `Co., Ltd.`, `Inc.`, `Corp.`, `LLC`).

`measurement_node_share` counts measurement-like names across every node kind,
including custom kinds, and legacy `Metric` nodes, divided by all nodes. It reports
zero for a known empty snapshot and null for a missing snapshot; pooled values use
node counts. The detector recognizes English and Korean quantities, units and
dated measurements while preserving numbered products, tickers, securities and
named events. Qualified actual occurrences remain events when a title contains
a monetary qualifier or verb; a contract-value measurement does not gain that
exception. `entity-name-corpus-v1.json` is a deterministic classifier fixture,
not a model-visible Source or a changed ontology corpus. The adoption check requires
zero measurement nodes, alongside numeric fidelity so dropping figures cannot pass.

Quantitative extraction keeps a metric's exact Source wording, language and
qualifiers in its claim, with an English rendering beside a non-English label.
Accounting terms and acronyms are not interchangeable labels. Scripted publication
coverage checks both the extraction instruction and preservation of the returned
English/Korean labels; real-model numeric fidelity still requires a fresh run after
this model-visible policy changes.

```sh
node apps/loupist/evals/world/ontology-grade.mjs \
  --suite apps/loupist/evals/world/ontology-corpus-v1.json \
  --oracle apps/loupist/evals/world/ontology-oracle-v2.json \
  --arm baseline=<NEW-baseline.jsonl> --arm candidate=<NEW-candidate.jsonl> \
  --out <NEW-ontology-report.json>
```

## Temporal invalidation

`temporal-corpus-v2.json` is World-only: four fictional scenarios of two Sources
each. In `T-CEO`, `T-HQ` and `T-SUPPLY` the later Source states a change with an
explicit UTC effective instant (a
replaced chief executive, a moved headquarters, a terminated contract); `T-DISPUTE`
is the control, where a denied allegation must not end the earlier contract.
`temporal-grade.mjs` reads each trial's final World snapshot without model calls:
claims joining the oracle's entity pair that cite the earlier Source must be
invalidated (`invalidAt` or `expiredAt` set) and, for a supersede case, a claim
joining the new pair from the later Source must be active. The oracle's
pre-registered `acceptance` compares a candidate arm with a baseline: no fewer
expected invalidations, no more control false invalidations, every trial complete.
A retried trial (its own `--out`, listed after the original) is scored by its last
attempt, as in the ontology grader.

Version 1 is retained as a date-only probe. The evidence policy forbids inventing
an exact midnight instant from a calendar date, while the inherited Graphiti
replacement rule requires start instants on both claims. An explicit termination
can close a selected earlier assertion at its end instant without inventing a
new start; the earlier start must precede that end. Version 2 makes those
instants explicit, and removes unrelated prior-role history from the CEO Source
so every old-pair claim concerns the leadership being replaced. Its oracle keeps
the same checks and acceptance criteria; comparisons require matching v2 corpus
bytes in both arms. Baseline and candidate are trace labels: use retained
compatible traces or separately frozen Rust builds for comparisons.

```sh
node apps/loupist/evals/world/temporal-grade.mjs \
  --oracle apps/loupist/evals/world/temporal-oracle-v2.json \
  --arm baseline=<NEW-baseline.jsonl>[,<more.jsonl>] --arm candidate=<NEW-candidate.jsonl> \
  --out <NEW-temporal-report.json>
```

## Template seed check

`template-seed-corpus-v2.json` has four fictional World-only scenarios, `S-CRYPTO`,
`S-COMMODITIES`, `S-MACRO` and `S-REAL-ASSETS`, each six Sources and two queries with the
template's default focus (v2 adds the last two; the first two are unchanged from v1).
They are checked in with `seed: "none"`; the other arm sets `seed: "template"`, and the
harness applies the product's curated seed for `template` before the first Source, as a
new template Loupist gets it. A removed seed's fixture stays in the no-seed corpus for
future comparisons; requesting its template arm fails validation. Historical template
traces can still be graded, but reproducing them requires the recorded seed and runner.
`template-seed-grade.mjs` counts active claims with a core
relation (`typed_core_relations`) and the share typed by a core or seed relation
(`typed_share`). A template seed ships only when both are no lower than without the seed on
complete trials; otherwise the report says `hold` (or `unmeasured`).

```sh
node apps/loupist/evals/world/template-seed-grade.mjs \
  --suite apps/loupist/evals/world/template-seed-corpus-v2.json \
  --arm none=<NEW-none.jsonl> --arm template=<NEW-template.jsonl> \
  --out <NEW-template-seed-report.json>
```

## Followed assets on the Map

`live_followed_assets_reach_the_map` (opt-in, paid) creates a Loupist that follows a US
stock, a Korean stock under its Korean name, an ETF, a crypto asset, a commodity, a symbol
typed without a name or market and a holding under its Korean name, and lets the real
World job read its "What you follow" Source through this checkout's Rust engine. It
then follows two named assets and two fictional typed symbols through the production
follow path, waits for the burst to settle and reads the second Source. Each JSONL record
lists every followed asset with the Map node the Map's own matching finds, its type and
whether an `entity_added` event names it, plus the unmatched watchlist and holdings.
Phase records include cumulative helper calls, tokens and bounded inference receipts.
The sibling `.receipts.json` file captures the final ledger after shutdown for cost and
model/effort auditing. A provider failure stops the check before another phase runs.

```sh
LOUPIST_FOLLOWED_PI_AGENT_DIR=/absolute/existing/pi-directory \
LOUPIST_FOLLOWED_RESOURCES=/absolute/staged/src-tauri/resources \
LOUPIST_FOLLOWED_OUT=/absolute/new/followed.jsonl \
cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml \
  --locked --offline --features eval-harness --lib live_followed_assets_reach_the_map \
  -- --ignored --nocapture
```

`LOUPIST_FOLLOWED_MODEL` and `LOUPIST_FOLLOWED_THINKING` default to
`openai-codex/gpt-6-luna` at `high`. The resources directory supplies the embedding
assets; the World job runs in-process. The retired baseline comparison harness
has been removed; temporal, ontology and template-seed corpora and graders remain.

Grader contracts: `node --test apps/loupist/evals/world/*.test.mjs`.
