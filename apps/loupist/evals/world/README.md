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
| `temporal-corpus-v3.json` + `temporal-oracle-v3.json` | `temporal-grade.mjs` | [Temporal invalidation](#temporal-invalidation): dates, publication-time fallback, backfill order, controls and Lens propagation |
| `temporal-corpus-v4.json` + `temporal-oracle-v4.json` | `temporal-grade.mjs` | [Temporal invalidation](#temporal-invalidation): version 3 unchanged, plus Japanese, German, Indian English and Chinese Sources, numeric dates whose day and month could swap, and a restatement after an ending |
| `temporal-corpus-v4.json` + `temporal-oracle-v5.json` | `temporal-grade.mjs` | [Temporal invalidation](#temporal-invalidation): the same corpus under the reopen rule for an assertion restated after it was replaced |
| `smoke-v018.json` | `smoke-grade.mjs` | [Release extraction smoke](#release-extraction-smoke): a filing excerpt, a news item and a table |
| `smoke-global-v1.json` | `smoke-grade.mjs` | [Release extraction smoke](#release-extraction-smoke): four tables and two prose Sources from six market profiles |
| `smoke-long-tables-v1.json` | `long-tables-grade.mjs` | [Long tables](#long-tables): two table Sources longer than one chunk, with 36 labelled cells each |
| `figures-corpus-v1.json` | `figures-grade.mjs` | [Structured figures](#structured-figures): sixteen Sources, two per market profile, with every figure labelled |
| `figure-corpus-v1.json` | Rust table test of the figure check | [Figure corpus](#figure-corpus): 64 labelled passage and proposition pairs, eight per market profile |
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
| `implicit-memory-regression-v4.json` | Current improvement set: subject filing permitted, method/principle/perspective precision scoped by `lens_kinds` |
| `implicit-memory-regression-v3.json` | Earlier improvement set (12 scenarios, 41 turns): quotations, corrections, long-message paging, separate methods, presentation preferences, conditional exceptions, temporary overrides, conflicting publishers, quoted untrusted instructions |
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

Lens retrieval accepts an acknowledged exact read, or the current revision of
a principle delivered in this turn’s context and linked in the final answer
([user principles](../../../../docs/knowledge/lens.md#user-principles)). Methods
and subject pages require exact reads. Old traces without `lens_receipts` are
unknown when no exact read is present. Reports split accepted retrieval evidence
into `exact_read` and `context_link` counts.

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

On **2026-11-13**, `non_english_fact_share` changed from “contains any Hangul”
to a deterministic script heuristic for the sentence's prose. After NFKC
normalization, it ignores short original-language labels and name aliases in
parentheses, quotes or slash pairs in an English frame. This includes a label
before its parenthesized English rendering. An English frame has at least two
Latin words outside the annotation; ignored spans contain only non-Latin letters,
spaces, hyphens or middle dots, at most 64 letters and six words. Sentence
punctuation and common Korean clause endings prevent prose from being ignored as
a label. A fact counts as non-English when at least 25% of the remaining letters
are non-Latin; digits, punctuation and whitespace do not dilute that proportion.
This keeps mostly or half-Korean prose flagged while allowing English sentences
to retain Source metric labels and company aliases. It is a script heuristic,
not a general language-identification model. The adoption limit of 5% is unchanged.
This approved grading-only revision requires a **retrospective regrade of both
baseline and candidate** with the same grader; all other predicates stay unchanged.

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

Version 3 measures calendar-date replacements, publication-time fallback, raised
guidance, a corrected figure and explicit departure notices. Three duplicate
publication arms ingest the same January, February and March Sources in separate
profiles. All three must end the January assertion in February while retaining
March as support and keeping January as its earliest publication. The controls
cover reporting periods, a forecast and its later actual, a denied allegation,
same-day disagreement and an undated paste. The same-day scenario also includes
a separate pair with explicit starts that must end at the later start.

The `pre_file` fixture tells the runner when to file a subject page section and
how to find its claim by Source and selector. The runner needs to consume it
after the named step's World jobs finish. The January-backfill arm files after
the other two Sources are already present, exercising stale-basis handling on
write. Keep the oracle out of ingestion and filing.

Use `--trials 1 --keep` so the grader can read each trial's
`.loupist/loupist.sqlite3`. The v3 grader checks the final World snapshot and
the active Lens head in read-only SQLite. It checks the end value, precision,
basis, approximation, witness span and replacement claim. It requires
reciprocal links for the same-day conflict, equal validity across the three
order arms, zero unexpected endings and a Superseded Lens section.

When a step declares `as_of_reads`, the runner needs to call the product World
reader at each cutoff after publication and record each unmodified view in the
step's `as_of_views`, keyed by date. The April view keeps both the future
departure and the late-notice claim current and omits ending metadata. The
later view confirms the corresponding end. An absent cutoff view, final step
or required profile database makes the trial incomplete.

```sh
node apps/loupist/evals/world/temporal-grade.mjs \
  --oracle apps/loupist/evals/world/temporal-oracle-v3.json \
  --arm candidate=<NEW-candidate.jsonl> \
  --out <NEW-temporal-report.json>
```

Version 4 keeps the twelve scenarios of version 3, apart from one selector, and adds
six, so a version 3 arm is still comparable scenario by scenario. `T-SAMEDAY` selects
its conflicting pair by proposition (`kestle bay`, `amber quay`) from each Source rather
than by endpoints, because a model may give the distribution centre its own node and
join that node, not the company, to the place. `T-JP-FY` (Japanese: revised
guidance for a fiscal year ending in March), `T-DE-CFO` (German: two half-year reports
naming different finance chiefs, no dates) and `T-IN-RESTATE` (a restated quarter in
crore beside an unchanged figure in lakh) must end the earlier claim at the later
publication date, approximate, on the `reference` basis. `T-ZH-PERIOD` (Chinese: revenue
of two quarters) is a control. Their selectors accept the figure in the Source's own
scale word or converted, because the grader does not fix the language of a proposition.
`T-AMBIGUOUS-DATE` is a control with two same-day Sources whose only dates are
`03/04/2026` and `04/03/2026`: nothing may end, and its `max_date_precision: "month"`
fails a start or end date written to the day (`pooled.invented_dates`). `T-RESTATED`
has `check: "record"`: an assertion is replaced and then restated by a later Source.
The grader writes what it observed in `recorded` (`first_ended`, `restated_as`:
`support_on_ended`, `support_on_open`, `new_claim` or `absent`, `second_ended`) and
passes every complete trial; the scenario adds to no gated count.

From version 3 on, every trial reports `restated_after_end`: how many ended claims gained
support from a Source published after the Source that witnessed their ending (null when a
publication time is unknown). The arm sums it per scenario. A restatement after an ending
is support on the ended claim today, so the count is a baseline for a reopen rule and
never changes `failed` or the acceptance. `--trials <n>` grades more trials than the
oracle's one without editing the oracle.

```sh
node apps/loupist/evals/world/temporal-grade.mjs \
  --oracle apps/loupist/evals/world/temporal-oracle-v4.json --trials 3 \
  --arm candidate=<t0.jsonl>,<t1.jsonl>,<t2.jsonl> \
  --out <NEW-temporal-report.json>
```

Oracle version 5 grades the same corpus (`temporal-corpus-v4.json`) under the reopen rule.
It differs from version 4 only in the four scenarios where an assertion is replaced and
then restated by a still later Source with no dates: the three `T-DUP` orders and
`T-RESTATED`, now `check: "restated"` with the restating Source and its day. Each accepts
two outcomes: `reopened` (the first assertion is current, and the second ended
approximately at the restatement's day on the `reference` basis, witnessed by the restating
Source) or `conflict` (both current, linked to each other). `first_ended_second_current`,
which version 4 expects, fails: the newest Source says the first holds. The three orders
must still agree. A claim's `reopened` and `lapsed` qualifiers are read into its
description and counted in `pooled.reopened_claims`; they are lineage, so an ingestion order
in which the first assertion never ended has none and passes the same. The pre-filed Lens
section of those four scenarios is not gated here. Use version 5 for a build that has the
reopen rule and version 4 for one that does not; `pooled.restated_outcomes` counts the
trials by outcome.

The [agent change-feed suite](../agent/suites/change-feed-v1.json) grades how a
scheduled agent uses the feed, compares two filings and answers across a cutoff.
Its scenarios state their Sources and what each says as data
([seeds a suite states](../README.md#scenario-suites)), so the agent runner stages
the endings and the conflict through production code without a World model call.

## Release extraction smoke

Run `smoke-v018.json` with `--trials 1`. Each profile has one fictional Source.
The offline grader requires a completed trace, at least one active claim tied to
the fixture's retained bytes, at least one associated World node and recorded
input and output usage. It checks typed failure evidence and the 100-call cap
across the three profiles.

```sh
node apps/loupist/evals/world/smoke-grade.mjs \
  apps/loupist/evals/world/smoke-v018.json <run.jsonl> <NEW-smoke-report.json>
```

`smoke-global-v1.json` runs the same way and under the same checks and cap. Its six
Sources are four tables (Korea in 억 and 조, Germany with a decimal comma and Mio. and
Mrd., India in crore and lakh, the United Kingdom in pounds, pence and GBX) and two
prose items (Japan, Brazil), each in its market's language; a scenario's `profile` and
`form` are for the report and the runner ignores them. For every scenario the report
gives `figures_left_out` and `left_out_examples`: the facts the figure check left out at
publication, read from the job receipts (`dropped.figureMismatch`, `droppedFigures`). A
receipt omits a zero count, so the count is 0 once the World evidence carries
`figureCheck`; a build with neither leaves it null. They gate nothing: after a live run,
read every left-out fact against its passage.

## Long tables

`smoke-long-tables-v1.json` runs like the smoke, one trial per Source. Its two fictional
Sources are tables longer than 7,000 characters: segment information in Markdown (India,
₹ crore, six periods) and divisional results in HTML (United Kingdom, £m and head counts,
five periods). Each scenario lists 36 `cells`, `{row, column, value, unit, currency}`,
spread over the whole table so that most lie where a chunk no longer holds the header row.
`cells`, `profile` and `form` are grader-only: the runner ingests the Source text alone.
The figures come from a seeded generator and the test checks every label against the
figure the Source prints at that row and column.

`long-tables-grade.mjs` calls no model. For each arm it reads the active claims that cite
the Source and gives every labelled cell one outcome: `found` when a claim names the row
(by the first word of its name, in its text or an endpoint's name, and a word of its
measure in its text), states the column's period and
states the value, as printed or restated in another scale; `wrong` when a claim names the
row and the period and states only other figures; `missing` otherwise. A full-year column
is not stated by a quarter or a half of that year. An incomplete trial has no counts. Two
arms of the same Sources compare two chunkers: the later one should find at least as many
labelled figures and no more wrong ones. Read the `wrong` cells' propositions before
drawing a conclusion: the grader matches words, and a paraphrase of a measure is `missing`.

```sh
node apps/loupist/evals/world/long-tables-grade.mjs \
  --suite apps/loupist/evals/world/smoke-long-tables-v1.json \
  --arm chunker1=<one.jsonl> --arm chunker2=<two.jsonl> --out <NEW-long-tables-report.json>
```

## Figure corpus

`figure-corpus-v1.json` is an array of `{id, profile, passage, proposition, expect}`,
eight cases for each of eight market profiles (`us`, `korea`, `japan`, `germany`,
`united_kingdom`, `india`, `brazil`, `hong_kong_china`), all fictional. A Rust table test
runs the figure check over it; `figure-corpus.test.mjs` checks only the file's shape and
coverage. `expect` is the verdict of the whole proposition, the worst of its amounts:

| `expect` | The label means |
| --- | --- |
| `matched` | every amount equals a passage amount exactly, after a scale or unit conversion (억 to billion, crore to million, pence or GBX to pounds, a decimal comma, a parenthesized negative) |
| `rounded` | an amount is a passage amount rounded to the digits the proposition prints |
| `differs` | a planted error: a passage amount of the same kind and currency times a power of ten, or the same amount with the opposite stated sign or direction |
| `not_found` | an amount the passage does not state: a rate derived from two passage numbers, or a percentage where the passage gives percentage points |
| `unchecked` | no verdict: the proposition states no amount (a year, a date and a code are not amounts), or a scale word is outside the recognized formats (`F-JP-05` 兆, `F-DE-08` TEUR) |

Sixteen cases are planted errors, two per profile. A case never labelled `differs` must
never come out `differs`. A recognized format that changes makes a new corpus version.

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
`openai-codex/gpt-6.2-sol` at `medium`. The resources directory supplies the embedding
assets; the World job runs in-process. The retired baseline comparison harness
has been removed; temporal, ontology and template-seed corpora and graders remain.

## Structured figures

`figures-corpus-v1.json` (not `figure-corpus-v1.json`, the check's table above) has sixteen
fictional Sources, two for each of the eight market profiles: a report of results and a
statement of outlook. Each scenario's `cells` label every figure of its Source as stated:
`{metric, period, period_end, basis, value, scale, unit, currency}`. `metric` and `period`
are the Source's own words in its language; `period_end` is a date at the precision the
Source states (`2027-03` for a fiscal year named only by its month); `scale` is a
multiplier (1e8 for 억, 億 and 亿, 1e7 for crore, 0.01 for pence); `unit` is `amount`,
`percent` or `count`. No two cells of one Source have the same amount. `cells` and
`profile` are grader-only.

`figures-grade.mjs` calls no model and grades one trial. A kept structure is the `figure`
qualifier of an active claim that cites the scenario's Source. It is right when one
labelled cell has its amount (value times scale, in its currency, whatever spelling states
the scale), its period end and its basis. `forecast` and `guidance` are one basis, a
company's own outlook under two markets' names; `estimate`, a third party's, and `actual`
are distinct, and `unknown` is never right. A period end finer than the Source states
(`2027-03-31` for `2027-03`) is right and counted in `finer_period_end`. The gate is
precision: the run passes when it is complete, kept at least one structure, and at least
95% of the kept structures are right. Recall (labelled cells with a right structure) and
`metric_as_stated` are reported, by profile too, and `not_right` lists every other
structure with its claim and the reason (`value`, `period`, `basis`, `unreadable`).

```sh
node apps/loupist/evals/world/figures-grade.mjs \
  apps/loupist/evals/world/figures-corpus-v1.json <run.jsonl> <NEW-figures-report.json>
```

Grader contracts: `node --test apps/loupist/evals/world/*.test.mjs`.
