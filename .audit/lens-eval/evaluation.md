# Lens creation and update: evaluation record

Goal: most users should feel Lens pages are created and updated well, without
over-fitting. In the product, that means no method generalized from one or two
narrow turns and no stale page steering later research. In development, it means
no tuning to our own eval. Agent model for every run: `xai/grok-4.7`, medium
thinking, development profile. Judge: `codex exec` `gpt-6-astra` (another family).
Raw outputs are under the ignored `apps/loupist/evals/output/`; paths below are
relative to it.

## What the eval measures

| Dimension | Check | Source of the idea |
| --- | --- | --- |
| Filing | A research turn leaves one subject page whose basis cites the retained original (`lens_pages`) | Letta write task, MemoryAgentBench |
| Update | A newer original revises the same page, and the stored page presents the new value as current (`lens_pages` + `page-current` rubric over the stored pages) | LongMemEval knowledge-update, Zep invalidation, STALE |
| Stale premise | A question built on the old value is corrected | STALE premise resistance |
| Retention | A small update keeps the page's earlier detail | ACE context collapse |
| Reuse | A new Chat exact-reads the page and answers from it (`lens_read`, rubric) | LongMemEval retrieval scored apart from QA |
| Restraint | Single-case turns leave no method; read-only and light turns log no Lens op (`lens_unchanged`) | Letta extraneous-op penalty, Mem0 NOOP |
| Scope | A method is applied where it fits and not to an unrelated question | ExpeL/AWM negative transfer |
| Requested method | A check the user asks to keep becomes one scoped method a later Chat reads | balances the restraint checks |
| Learning | Research in two Chats on two companies starts one review; one Chat never does (live Rust eval) | PR 36, ExpeL evidence counts |

Goodhart controls: state checks read the store, not the reply; every restraint
check has a positive counterpart (LL-009, LH-004); a sector held-out set run once
per arm; a different-family judge; `lens-pages` renders every page for a person to
read; two cross-family code reviews (Codex, Grok) of the branch.

## Results

Development suite (`lens-lifecycle-v1`, 9 scenarios, 3 trials, 60 turns per arm).
Baseline is commit 929ef67's product; candidate is the final branch.

| Metric (checks) | Baseline | Candidate |
| --- | ---: | ---: |
| save_recall | 33/33 | 33/33 |
| save_precision | 141/143 | 144/145 |
| lens_update | 39/42 | 37/42 |
| carry_forward | 18/18 | 18/18 |
| negative_transfer | 6/6 | 6/6 |
| retrieval (reported only) | 9/12 | 9/12 |
| invalid tool calls | 91/676 | 88/664 |

- The baseline filed an unsourced method from an insurer explainer in 2 of 3
  trials (LL-008); the candidate in 0 of 3. LL-006 (one company's results) left no
  method in either arm.
- `lens_update` misses: LL-003 declines to size 2027 revenue at risk in all six
  trials of both arms (answer helpfulness, not Lens); LL-005 lost the July segment
  mix and mill list on a dividend update in 1 of 3 candidate trials (2 of 6 across
  both candidate runs, 0 of 3 baseline; Fisher p≈0.5, and no change touched update
  guidance). Context collapse stays an open risk.
- `retrieval` misses are LL-008 reading the seeded method to rule it out, which the
  Lens Skill asks for; applying it is judged separately and never happened.

Held-out suite (`lens-lifecycle-heldout-v1`, 4 scenarios in biotech, shipping,
REITs and food, 3 trials, run once per arm; one candidate unit was rerun after an
xAI `provider_error`).

| Metric (checks) | Baseline | Candidate |
| --- | ---: | ---: |
| save_recall | 18/18 | 18/18 |
| save_precision | 71/71 | 72/72 |
| lens_update | 17/18 | 17/18 |
| carry_forward | 4/6 | 3/6 |
| negative_transfer | 3/3 | 3/3 |
| retrieval | 6/6 | 6/6 |
| invalid tool calls | 50/322 | 39/299 |

The `carry_forward` misses are a rubric defect found after the run: LH-004's probe
gives no FFO, maturity or occupancy figures, and both arms read the saved method and
listed its three checks while saying the data was missing. Fix the rubric in v2.

Live learning eval (`live_lens_research_v1`, one run per arm, descriptive): with
two Chats on two manufacturers, one review created "Judge whether ramp-period margin
pressure is temporary"; every section cites both reports; `limits` says it was seen
in "only two fictional manufacturers" and "does not establish … a sector rule". The
third-company probe exact-read it and used its upper-bound reasoning; the insurer
probe read nothing; neither wrote anything. The one-Chat control filed two turns on
two Sources and started no review (`lens-research-v1-run2.json`, gate before the
review fixes). On the final code (33992fb, clean tree) the result held: one review
created "Judging if ramp margin pressure is temporary" citing both reports, whose
`limits` names "only two fictional manufacturers" and incomplete ramps; the probe
read it and marked it applied; the insurer probe read nothing; the one-Chat control
started no review (`lens-research-v1-run4-final.json`). Two other attempts failed on
xAI `provider_error` before any training turn finished and are kept as such.

## Findings fixed on the way

- Every sourced subject page failed `save_precision`: the judge criterion predated
  subject pages. Rewritten for the page model, with page kind and stored sections in
  the judge input.
- 81 of 652 baseline calls were refused `calculate`/`grid_publish` citations that
  Grok repeated identically; the refusal now lists this turn's citation URLs. The
  root cause (Grok tool arguments lose the evidence id) is open.
- The live research eval's broker socket path exceeded the Unix limit.
- Codex and Grok reviews found ways learning could still use narrow or withdrawn
  evidence; all eight were fixed with mutation-checked tests (commit "learn only
  from current, independent research").

## Open

- Pi's native `grep` needs ripgrep, which the app does not ship.
- Grok drops the evidence id from citation tool arguments.
- A learning refine can drop Loupist-written limits the reviewer never saw.
- LH-004's rubric asks for figures the probe never gives.
- Context collapse on updates (LL-005) needs a larger sample or a writer-side check.
- The held-out set is a sector holdout with the development turn shapes, not an
  independent behavioral test.
