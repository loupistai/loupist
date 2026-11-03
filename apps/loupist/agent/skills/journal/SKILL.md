---
name: journal
description: Evaluate investment choices, record the user's own judgment in their Journal, or revisit a dated check with facts. Use for buy/sell/hold/trim/add/exit, allocation, sizing, or investment go/no-go questions and explicit user judgments; skip research alone and light questions.
---

# Your journal

A Journal entry records what the user thinks and why. Research notes hold the
Loupist's saved analysis.

When the user asks what to do:

1. Name the entity, the information cutoff, and the choice the user is weighing.
2. Build evidence under the system's evidence and Memory rules, including dated
   originals by the cutoff. Keep thin or conflicting evidence explicit.
3. Give two compact points, for and against, each in this order: dated
   observation → next check (metric and timing) → outcomes that strengthen
   and weaken that case. Keep both points in brief follow-ups. A generic
   investigation or one combined watchlist does not test each case. Use the
   next relevant report or event when no calendar date is known; do not invent
   a deadline or numeric threshold.
4. Check relevant user principles before methods and the standing perspective,
   using the system's dated, linked per-principle checklist. Explain departures
   and whether any trade-action condition is met. Skip it when none applies.
5. Lead the answer with your view, then give the reasons, uncertainty, main risk,
   and what would change your view. A request for advice alone never creates a
   journal entry.

For a requested probability about a business or market figure, explain the
reference class or base rate, evidence for and against, and the observable
result that could prove it wrong. An assistant estimate is never the user's Journal confidence.

## Save the user's words

When saving is enabled and the user states their own judgment this turn,
call `journal_save` with `judgment` in their own words (at most 1,000
characters), optional `reasoning` (at most 2,000), and optional World entity
ids. Never originate or infer the stance. Preserve attribution even if the
user's own words name a trade action.

A check is optional: `{claim, metric, direction: above|below, threshold,
unit?, byDate}`. Save it only when the user stated it or accepted a draft.
You may draft a check from their words, but show the draft in your answer
and wait for acceptance before saving it. Do not silently resolve ambiguous
metrics or dates. Save the judgment without a check when those are missing.

Confidence is optional and belongs to the user: 10%, 20%, …, 90%. Never
infer it, require it, or substitute your estimate. Omit it when absent. If
the user gives another value, preserve their words and ask which permitted
confidence they want; never round it silently. No confidence is needed to
save a judgment or a check. Cite at least one acknowledged Source, World,
or Lens read; `source_save`'s same-turn saveRef is valid evidence.

Confirm the saved judgment briefly. The Chat Memory row opens the entry.
Use `note_save` for the Loupist's research notes, independently of the Journal.

## Revisit with facts

Use due journal entries in context as pointers. Exact-read the entry with
`conversation_memory` (`journalId`) and obtain fresh acknowledged evidence.
Once `byDate` has arrived, call `journal_review` with the entry's readRef,
sourceReads, and `{observed, observedAt, note?, unclear?}`. The evidence must
be dated on or after byDate and the observation cannot be in the future.
Do not review an entry without a check, an archived entry, or one already
reviewed. No new judgment or confidence is required for review.

State the observed figure, metric, date, and factual result: “came true,”
“did not,” or “unclear.” Use unclear only when the evidence does not resolve
the check and explain the gap. A recurring analytical
error may justify proposing a method Lens revision.
