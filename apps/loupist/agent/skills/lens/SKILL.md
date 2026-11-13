---
name: lens
description: Use when the user teaches or corrects a reusable method, asks to keep a check, or wants this Loupist's methods, pages or outlook inspected or tidied. Skip a one-off.
---

# Lens

Lens is this Loupist's wiki: the standing perspective, methods (when and how to
check something, and what would disprove it), subject pages, ideas and the
user's principles. The system prompt says when to read and when to file; this
Skill covers how to write a method and how to review the library.

## Teach or correct a method

A method is its own page, kind `method`, titled after what it checks. Only the user creates or changes one in a turn: by teaching a procedure with steps, by correcting a method page that exists, or by asking you to keep a check you worked out or to save a method page. A rule that fits in a sentence or two ("from now on divide by the average share count") is a principle: save it with `principle_save` and file no method for it. Never file a method from your own analysis, general knowledge or a single company's case; that belongs on the subject or idea page.

1. Search first (`lens_search`), then `lens_read` the method you would change.
2. Extend an existing method only when the new rule refines that same method;
   otherwise create a page. A create with a taken title merges into that page.
3. Put only the sections that change (`use-when`, `steps`, `checks`, `limits`),
   each with `expected` set to the digest from your read. A section that changed
   since is skipped: read again and refile.
4. Every op on a method cites the user's words this turn: a section's basis
   includes `{"said": true}` (plus `{source}` when a file grounds it), and a
   `retire` or `summary` op carries `"said": true`.
5. Give the page a one-line `summary` that says when the method applies: the
   index shows it, and later turns choose methods from that line.
6. State in `limits` only the scope the user's evidence supports. Keep World,
   Source and Journal references out of a method's body.

For a method grounded in a newly acquired file or page, `source_save` the
original first and cite its `saveRef` as `{source}`; a capture or fetch
`result_ref` alone is not a Source reference. For an existing Source, exact-read
it and cite the returned `readRef`.

Change the standing perspective only when the user changes their overall
investing philosophy. Do not rewrite a prior judgment. Keep competing
interpretations when their assumptions differ. A local file edit is not a Lens
revision.

Link a related method as `[title](lens:recordId)` with a discovered id, and say
whether it is a prerequisite, alternative, refinement or countercheck.

## Replace a principle

`principle_save` with `replaces_record_id` needs the principle's current
revision, which the `User principles` context or a `lens_read` delivers.

## Review the library

Separate missing targets from duplicate scope, conflicting assumptions, missing
limits and unsupported generalizations. A missing backlink alone is not a
defect. Read both methods before a repair, and do not claim a full audit from a
partial page.
