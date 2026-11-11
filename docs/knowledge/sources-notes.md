# Sources, notes and Journal

A Source records origin, coverage and exact bytes. Notes and Journal entries snapshot their own inputs. Neither is a full history of every Memory document.

## Working files and retained knowledge

Each Loupist uses a local workspace; creation and import are described in
[Architecture](../runtime/storage-authority.md#where-data-lives). Native file changes happen
immediately; Memory saves use separate Rust transactions. Files previews do not
supply model context. Ask Loupist adds a file path for explicit submission.
Instruction and Skill files do not grant Memory permission or replace Lens records.
See the [workspace contract](../runtime/chat-turns.md#local-workspace-execution).

There is no automatic file indexing or two-way synchronization into Memory.
`workspace_capture` reads a bounded UTF-8 snapshot, optionally checking its preview
digest. After acknowledgment, `source_save` can retain it. Native `read` and
`run_python` output alone do not create Source receipts. Binary originals can be retained through
Add source. A generated report is analysis, not independent original evidence.

## Sources and exact references

A Source records origin, authorship, publication/acquisition time and coverage
(full, excerpted, truncated or metadata only). Original files, extracted text and
structured representations remain distinct. Exact spans identify retained bytes;
a digest proves those bytes, not their attribution or truth. A Source a publisher, a search or a custom
connection authored comes back to the model delimited and raises the
[untrusted-text tier](../architecture.md#trust).

Acquisition deduplication includes origin, attribution, coverage and material
lineage. Repeated acquisition or a changed title is not independent corroboration.
An unchanged retained source can be reused without another extraction. Manual
source saves commit immediately; Chat saves stage until completion.

Current Source, World and Lens bodies are replaced atomically with an expected
revision check. The database keeps one current revision per record; it does not
provide a prior-revision browser or restore action. Retained Source material is
immutable, and notes/Journal entries capture their own evidence inputs. Those snapshots
must not be confused with a complete history of every Memory document. A stale
record reference can become unavailable after replacement.

Source readers distinguish retention from processing status, show coverage and
exact passages, and link current World claims. Metadata-only Sources cannot cite
absent original bytes. Binary export checks owner, revision and span before
writing the retained original through a native save dialog.

Source details display retained context notes as plain text. Source origins show
the hostname for HTTP(S) URLs or the retained label for other origins.

## Working notes and Journal

Substantial work can have one current Working note per Chat; the Bot keeps
[Bot memory](../ui/home-thread.md#bot-memory) instead. An update replaces its
body in the completed-turn transaction and checks the expected identity; failed or
cancelled turns preserve the existing note. Earlier Chats' notes can be referenced
by a new note, and discovery can expose later corrections. Discovery reads the
1,000 most recently stored current candidates per Loupist, then sorts that bounded
window by recorded time; repeated replacement of other Chats' notes does not
consume that window. Notes remain available
within the Loupist after their originating Chat is deleted. They are neither original
evidence nor standing instructions. Research readers expose the Source excerpts,
World view and Lens documents captured with the saved work.

A Journal entry records the user's own judgment, with optional reasoning, linked
World entities and a dated check. Entity links are validated against the complete
current World, including entities outside the Map's display limit. The Loupist’s
own view belongs in research notes, not in the user’s Journal.
Map → Library → Notes lists Your journal before Research notes. Focusing a field in that form scrolls it clear of the sticky search. The user can
create, edit and archive entries there through `journal_create`, `journal_update`,
`journal_archive` and `journal_list`. Chat uses `journal_save`
and `journal_review` only. Rust accepts `journal_save` for a judgment the user
stated in the current turn, and a review only after an exact acknowledged Journal
read plus acknowledged evidence. The [Journal Skill](../../apps/loupist/agent/skills/journal/SKILL.md)
owns elicitation. Research or a request for advice alone does not establish a
user judgment.

A check contains a claim, metric, above/below direction, finite threshold and date
(optional only for an asset page's [line](thesis.md#thesis-and-the-read)); unit and
user-entered confidence are optional. Confidence uses percentages from 10 to 90 in
steps of ten. A due review requires the exact acknowledged Journal entry and dated
evidence on or after its check date, no later than today. Under an as-of date
earlier than the check date the review is refused with `as_of_before_check`;
otherwise the as-of date takes the place of today, and the outcome keeps it as
`reviewedAsOf`. Rust records came true, did not, or unclear; the reader shows the
observation and date. Reviewed checks cannot be silently rewritten, and archived
entries are excluded from due reviews and summary counts. Journal entries without
checks remain valid judgments.

The Journal reader opens each entry with Then · Now. Then shows the creation date,
the judgment, the Sources captured in its `inputs` (each opens its reader; "No sources
were saved with this entry." when there are none) and Look back. Now shows the
outcome with its note, and "Reviewed as of {date}" after a review under an as-of
date; otherwise "Due {date} · Review" (in warning once late) or "No check set". Look
back, also in the menu of a reason on an asset page, creates a topic Chat titled
"Looking back: {judgment}" and fills its composer with the entry's local creation
day as the as-of date, the entry as a `journal` pin and a question about how well
what was known then supported the judgment. It never sends. When the workspace
covers the chat, Look back closes that cover so the new draft is on screen.
