# Autonomous research development evaluation

The production-path check used fictional retained originals, `openai/gpt-6.1-sol`,
medium thinking, and a fresh private profile for each probe. The fixture and agent
snapshot digests are recorded in the trace. This is a development demonstration,
not the frozen held-out outcome benchmark.

## Attempts

- `e02039e`: the first attempt completed two research answers but retained only one
  experience. The second question used the filings Skill and skipped note saving.
  No candidate was created. The failed trace is preserved as `attempt-1.jsonl`.
- `9cc7748`: common completion guidance covered that route. The second attempt
  completed all six turns in 337.74 seconds. Two original-bound experiences
  produced one candidate through the production learning worker, without new
  World claims or a user Journal. The matching fresh question exact-read and
  marked the method applied. The simple conversion did neither. Every read-only
  probe committed no Memory. The trace is preserved as `attempt-2.jsonl`.

The artifacts are local under `apps/loupist/output/research/`; raw traces are not
committed. `/tmp/loupist-research-live-1.log` and
`/tmp/loupist-research-live-2.log` hold the execution results.

## Semantic inspection

Both Source-only B and Sources-plus-learned-method D correctly identified 10%
comparable firm-backlog growth, rejected the unsupported 40% funded-demand and
next-quarter cash claims, and refused to infer an investable opportunity without
an identified instrument and valuation. D explicitly separated the backlog
balance from order intake and aggregate cash flow from the relevant order cohort.
B also gave useful counterevidence and falsifiers. Both conversion answers were
correct and concise.

D's answer contained one malformed evidence target; the same amount was correctly
cited elsewhere in that answer. B had no unmatched evidence target in this one
comparison. This remains a model-output reliability limit, and the structural
check does not grade citation or answer quality. It must not be reported as a
quality improvement.

| Matching probe | Input tokens | Cached input tokens | Output tokens | Turn time |
| --- | ---: | ---: | ---: | ---: |
| B, Sources only | 35,646 | 90,368 | 2,075 | 82.183 s |
| D, Sources plus learned method | 19,679 | 129,536 | 2,256 | 109.692 s |

These are single observed runs, not speed or cost estimates. Cache use and tool
sequences differed. The learned-method profile used more total input and more
time in this sample. Provider cost was unavailable. The one learning call took
18.600 seconds, with 2,881 input tokens and 660 output tokens.

The six Chat turns together used 127,690 input tokens, 327,808 cached input tokens,
and 7,602 output tokens. The frozen 40-case Lens comparison and prospective
investment outcomes were not run. No research superiority or investment alpha is
established by this demonstration.
