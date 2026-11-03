# World evidence grading

`grade.mjs` scores the lifecycle and acquisition corpora from frozen corpus/oracle
files and runner JSONL. It makes no model calls and never changes raw runs. Run
setup and acceptance thresholds are in the [World guide](README.md).

```sh
node apps/loupist/evals/world/grade.mjs \
  --suite apps/loupist/evals/world/corpus-v1.json \
  --oracle apps/loupist/evals/world/oracle-v1.json \
  --traces <run.jsonl> --out <NEW-report.json> --audit <NEW-audit.jsonl>
node apps/loupist/evals/world/grade.mjs \
  --suite <corpus.json> --oracle <oracle.json> --traces <run.jsonl> \
  --labels <coordinator-reviews.jsonl> --out <NEW-reviewed-report.json>
```

Generate the audit first, review it, then write a new reviewed report. The sibling
`<run.jsonl>.pending.jsonl` is merged by exact attempt identity. The default matrix
is three trials per scenario; `--scenarios ID,ID` limits scope. Missing, interrupted,
failed, ambiguous and unlabeled stages stay in the matrix and cannot certify
completion. Multiple attempts for one cell are ambiguous: an explicit
`--selection` file (`schema: "loupist-world-selection/1"`, `trials` entries with
`scenario_id`, `trial`, `run_id`, `attempt_id`) chooses a whole scenario/trial and
cannot mix stages. Every attempt stays in cost, tool and failure accounting.

## Audit and labels

Each audit row contains the original canonical Source bytes, span digests, times,
World claims and node descriptions, endpoint identities, the actual Chat tool
results and read receipts, the final answer, numeric checks and review items. Runner
search results are diagnostics, not Chat utilization. Review claims, descriptions
and annotations, including unsupported extras.

Labels are JSONL, one per item:

```json
{"item_id":"world:<audit item hash>","kind":"answer_check","review_type":"coordinator_review","reviewer":"coordinator identifier","reviewed_at":"2026-10-20T00:00:00Z","rationale":"Describe the direct source–snapshot–answer comparison.","pass":false,"evidence_refs":["answer"]}
```

| Kind | Required fields |
| --- | --- |
| `storage` | `reviewed_all_assertions: true`, `atoms: [{text, semantic_key, supported, fact_ids, evidence: [{span_id, quote}]}]`. An empty list requires `no_factual_assertions: true`. |
| `answer_check`, `trace_check` | `pass: boolean`, `evidence_refs`. Trace checks cover acquisition, saving and conversation reuse separately from answer correctness. |
| `utilization` | One item per required answer fact: `pass: boolean`, `evidence_refs`, and, when passing, `source_evidence: [{span_id, quote}]` linked to an acknowledged exact Source/World read. |
| `acquisition` | Initial-acquisition fact: `pass: boolean`, `evidence_refs` including `answer` and the current `web_fetch`/`source_save` calls, and passing `source_evidence: [{span_id, quote}]` bound to the verified committed original and completed World job. |
| `adverse` | `occurred: boolean`, `applicable: boolean`, `evidence_refs`. Explain stage-specific inapplicability in the rationale. |

Use exact refs from `actual_chat_evidence` (for example `read:final-probe:0`,
`tool:final-probe:2`) and exact quotes from the indicated canonical span. Bind a
storage atom's evidence to its claim or neighboring node claims, and use consistent
semantic keys for repeated identical assertions: precision counts distinct reviewed
atoms per checkpoint, not graph edges. False or unsupported atoms cannot satisfy
recall. Historical, disputed and analysis assertions keep their qualifiers; an old
value returned as current fails. A source quoting another inherits its provenance
for independence, and assistant or user hypotheses never count as verified
publisher facts. Review `validAt`/`invalidAt` as fact validity, independent of
publication time and system expiry. Coordinator reviews are explicit evidence, not
deterministic checks or human calibration; preserve earlier labels when correcting one.

## Utilization

Storage recall keeps every required stored-fact opportunity. Utilization counts the
question-specific required answer facts: for the core corpus they come from
[`answer-facts-v2.json`](answer-facts-v2.json) (SHA-256
`1d8771ac8d96405de597f044900d9d663d1a66c14c9a650efe904fde6652730c`, verified with the
corpus, oracle and each question/criterion digest; do not edit it); other suites
declare `required_answer_fact_ids` in their oracle. Affirmative requests are
separated from negative-transfer guards: a direction not to confuse another
company does not require restating its figures, but any actual conflation fails the
correctness or adverse checks.

Review each fact against the entire answer — entity, period, basis, unit, time,
status and attribution. Correct numeric JSON cannot rescue contradictory prose about
the same fact, while an unrelated failed answer criterion does not erase an
independently correct fact. World-enabled utilization accepts a Source or World read,
so it measures the full Memory path rather than a graph-only benefit. World and
Source-only results are reported separately.

## Acquisition

`acquisition-oracle-v2.json` (`world-acquisition/2`) gives every checkpoint an
`evidence_phase`:

- `initial_acquisition` (first answer, three facts per trial): a passing fact needs
  the current Chat's successful full `web_fetch`, that exact `result_ref` in an
  acknowledged `source_save`, a matching Source commit, and completed post-chat World
  processing bound to that Source revision and span. The fetched URL and transport
  hash must match the source manifest, and the retained wrapper and extracted article
  must match the verified original (HTML transport and extracted text hash
  differently). Snippets, another URL or save reference, an uncommitted save or an
  unfinished World job cannot pass; the answer's value and attribution are still reviewed.
- `retained_memory` (follow-up: one fact; new Chat: three): a current-stage
  acknowledged exact Source/World read of the retained revision, span and fact. An
  earlier same-Chat read does not count.

The phases have separate goals, totals and per-trial values (nine acquisition and
twelve retained opportunities); both must meet 80% in aggregate and in two of three
trials. There is no pooled score. An unavailable search and a completed zero-hit
search must be distinguished; known synthetic HTTP 503 responses stay observed
failures. Map retained spans to the supplied HTML body rather than assuming
transport and extracted-text offsets match.

## Completion and integrity

`completion.measurements_complete` needs complete three-trial coverage, semantic
reviews and observed threshold and integrity results. `primary_thresholds_met` adds
the numeric thresholds, majority requirements and hard integrity gates.
`answer_checks_pass` reports whether every whole-answer check passes; `completion.pass`
is the stricter diagnostic that requires both.

The completed-job integrity gate audits the durable publication receipts in the
trace, deduplicated by run, attempt, scenario, trial and job. Each completed job
must retain valid input/output SHA-256 digests, its Loupist and World identity, a
sealed revision id and settled usage/lease/redrive flags. Source-extraction jobs
also need their Source record/revision/span binding, Source payload digest,
selected model and inference-attempt flag. The binding must match the exact
retained Source identity and bytes in the trace. Receipts must agree across every
observed checkpoint; optional inline extraction bytes must match the output digest.
The report exposes `completed_job_integrity` and states this contract in
`integrity_contract`.

Historical extraction JSON and superseded World revisions are disposable derived
copies, not retained evidence or recovery inputs. Requiring a blob for every
completed job incorrectly imposed an unbounded World archive. Rust atomically
publishes the current canonical World, keeps lightweight job receipts for provenance
and deduplication, and rebuilds the index from that World and retained Sources.
The exact Source-byte and authority gates remain mandatory and independent;
receipt validity does not certify extraction quality. `--keep` preserves folders
for investigation, but grading these traces needs no live temporary workspace.
This corrects the earlier retained-artifact gate; original reports and raw traces
remain unchanged, and revised reports do not certify missing trials or reviews.

World inference ledgers are cumulative and deduplicated by run, attempt and call
index; unknown usage, bytes and billing stay null.
