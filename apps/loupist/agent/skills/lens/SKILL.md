---
name: lens
description: Use when the user teaches or corrects a reusable method, rule, convention, or preference, or when inspecting this Loupist's perspective and methods. Skip a one-off.
---

# Lens

Lens records how this Loupist interprets the world. A default perspective explains
what matters, assumptions, principles, and limits. Methods describe when and how
to analyze and what would disprove the interpretation. A task-specific scenario
or a single anecdote belongs in a working note. The turn's Memory settings govern
reading and saving; selecting this Skill does not change them.

Use the supplied default perspective. If it is marked `readRequired`, exact-read
it before applying it. Select relevant methods by their applicability and limits,
then read their complete documents. Follow a returned link only when it can
change the analysis, and exact-read that method too. Navigation metadata is
discovery, not evidence. Mark a Lens applied only when you actually use it.
Distinguish observed evidence from the interpretation a Lens adds.

A method may guide local code, files, app tools, or research. A workspace script
does not automatically become a Lens. Keep executable details in files and
promote only a reusable, supported methodology.

A newly taught method is its own Lens, kind `method`, with a title that names
the method. Only the user creates a method in a turn, by teaching or correcting
one or by asking you to keep a check you worked out; never file one unasked from
your own analysis, general knowledge or a single company's case. State the scope
the user's evidence supports in `limits`. Search first. Extend a method only when the new rule refines that
same method. Change the standing perspective only when the user changes their
overall investing philosophy. A stated one-off is not a method. Do not
rewrite a prior judgment. Preserve competing interpretations when their
assumptions differ.

When saving is on, file with `lens_file`. Read the current page before an edit
and put only the sections that change, each with `expected` set to the digest
from that read; a section that changed since is skipped, never a proposal, so
read again and refile. A create with a taken title merges into that page.
Replace superseded instructions in their owning section; keep the reason for
the filing in `rationale`. Preserve useful detail, conditions and exceptions
without copying the same instructions across sections. Keep World, Source and
Journal dependencies out of a method's body. A method section cites the user's
words this turn, `{"said": true}`; a perspective section needs no basis. A kind is fixed, so do not write a new method into the standing
perspective. For a method grounded in a newly acquired file or page, save the
original with `source_save` first and cite its `saveRef` as `{source}` in the
section's basis; a capture or fetch `result_ref` alone is not a Source
reference. For an existing Source, exact-read it and cite the returned
`readRef`. Subject and idea pages are filed the same way from research turns;
their sections cite this turn's reads.

Link a related method as `[title](lens:recordId)` with a discovered id, and say
whether it is a prerequisite, alternative, refinement, or countercheck. The link
follows the current method and grants no authority.

For a library review, separate missing targets from duplicate scope, conflicting
assumptions, missing limits, and unsupported generalizations. A missing backlink
alone is not a defect. Read both methods before a repair, and do not claim a
full audit from a partial page.

Explain the method used, a supported change, why it generalizes, and what would
invalidate it. A local file edit is not a Lens revision; only a confirmed memory
commit establishes that change.

## User principles

User principles are the user's own rules. They outrank methods and the standing
perspective. If your answer departs from a principle, name it and say why.
Use `principle_save` only for rules the user has explicitly stated. A user's rule
that names an action (for example "sell if margin falls below 10%") is saved in
the user's words; check its condition against dated evidence.
Never save your own recommendations or rules you inferred. Replacing a principle requires its current revision
in User principles context or an acknowledged `lens_read`; pass its record id as
`replaces_record_id`. The write commits with the completed turn.

When at least one principle is relevant, end the answer with a short section
titled in the user's language (en 'Checked against your principles', ko '내 원칙 점검').
Write one line per relevant principle: the principle as a `lens:` link, the dated
fact you checked, and the result: meets, conflicts, or not enough information.
Skip the section when no principle is relevant.
Include principles that name trade actions and report whether their conditions are met.
