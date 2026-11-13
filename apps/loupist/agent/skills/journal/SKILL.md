---
name: journal
description: Record the user's own stated judgment in their Journal, save a check they stated or accepted, or revisit a due check with facts. Use when the user states a judgment for the record or a dated check has come due; an investment question alone needs no Skill.
---

# Your journal

A Journal entry records what the user thinks and why, in their words. Your own
analysis goes to Lens pages; a request for your view never creates an entry.

## Save the user's words

When saving is on and the user states their own judgment this turn, call
`journal_save` with `judgment` in their words (at most 1,000 characters),
optional `reasoning` (at most 2,000) and optional World entity ids. Never
originate or infer the stance. Keep their wording even when it names a trade.

A check is optional: `{claim, metric, direction: above|below, threshold, unit?,
byDate}`. Save it only when the user stated it or accepted your draft. You may
draft one from their words, but show the draft in the answer and wait. A level the user states themselves ("I'd change my mind if gross margin falls below 20%") is a stated check, not a draft: save it. Do not resolve an ambiguous metric or date yourself: ask which they mean with `ask_user` and save the check with their answer; with no answer, save the judgment without a check and say the level is not being tracked.

Confidence is optional and the user's: 10%, 20%, …, 90%. Never infer, require or
round it; when they give another value, keep their words and ask which permitted
value they want.

Cite at least one acknowledged Source, World or Lens read; a same-turn
`source_save` saveRef counts. For a followed asset use `thesis {symbol, market, role}`, which needs no read: role `reason` for why the user follows it, role `line` for a level that would change their mind, with a `check` whose `byDate` is optional. Only lines are tested against later figures, so a stated level is never saved as a reason. A message that gives both is two `journal_save` calls, the reason first.

Confirm the saved judgment in one sentence.

## Revisit with facts

Due entries in the turn context are pointers. Exact-read the entry with
`conversation_memory` (`journalId`) and get fresh acknowledged evidence dated on
or after `byDate`. Then call `journal_review` with the entry's readRef,
`sourceReads` and `{observed, observedAt, note?, unclear?}`. Do not review an
entry without a check, an archived entry or one already reviewed.

State the observed figure, metric, date and result: "came true", "did not" or
"unclear" (only when the evidence does not resolve the check; say what is
missing). A recurring analytical error is a reason to propose a method change
to the user.
