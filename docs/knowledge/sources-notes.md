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

## User-provided Sources

Original material the user pastes into a Chat message (a filing excerpt, an
article, a notice, a table) can be kept as a Source of class `user_provided`. The
agent calls `pasted_capture` with the passage and, optionally, the `url` and
`publisher` the user gave; then `source_save` retains its `result_ref` like any
capture ([turn tools](../agent/turn-tools.md#turn-tools)). Facts the user states
about their own view still go to a note, Bot memory or the Journal.

The host locates the passage in the user's own messages of this Chat (the bound
Chat, never a tool argument), newest first, comparing each run of whitespace as one
space, and copies the bytes from that message. The agent can point at text but
never supply it: its own words, a paraphrase, a changed figure, tool results, an
assistant reply, attachments (those use `workspace_capture`), a hand-off's opening
request, a Map link, a scheduled task's prompt and another Chat's messages are not
found. A stated `url` must appear verbatim and a stated `publisher` (case and
spacing aside) in the user's messages of this Chat. Scheduled and delegated runs and
host hand-offs refuse the tool, and `source_save` refuses pasted text that looks
like a credential (`sensitive_memory`).

The capture records `knowledge_source_captures.provided_json` (`ProvidedOrigin`:
Chat, message, the message's send time as the paste time, stated link and
publisher), Knowledge schema 10. The Source has authorship `user`, provider `user`,
the stated link as its origin or `Pasted text` without one, and the agent's title
and `publicationTime` as for every Source. None of the stated details are checked
against the original; the Library row, the reader (with the paste date and "not
verified"), the citation chip, a number check's source line and the answer's
receipt all say "User-provided" ("사용자 제공").

Saving the same pasted bytes again with the same coverage in this Loupist
keeps one Source and one cited span. The current Source shows the latest save's
title, stated link, publisher, publication date and paste-message details.
The original capture and its bytes stay available under the same identifiers.

What differs from a fetched Source:

- In the turn it is pasted the text is the user's message, so capturing raises no
  tier and the capture is not delimited. Read back from Memory later (`world_read`,
  `world_search`, a World claim it backs, an export) it is third-party text:
  delimited and raising the [untrusted-text tier](../runtime/untrusted-text.md#untrusted-text),
  whoever pasted it. A `world_read` adds `source.sourceClass` and
  `source.provided` (`pastedAt`, `statedUrl`, `statedPublisher`).
- Its evidence kind is `user_provided` in the capturing turn; a later read stays
  `saved_context` with `user_provided` set. A number linked to either checks as a
  `passage` against the pasted text, so a match means the figure is what the user
  gave, not that the original says it.
- It can back a Lens fact (`{source: saveRef}` or a Source read), as a span the
  reader labels user-provided; Map extraction runs as for any Source.
- [Sharing](sharing.md#sharing) keeps it private: it is user-authored, not a
  publisher's record.

## Working notes and Journal

Substantial work can have one current Working note per Chat; Bot keeps
[Bot memory](../ui/home-thread.md#bot-memory) (tool `bot_memory`) instead. An update replaces its
body in the completed-turn transaction and keeps its research id, so links resolve
to the current body after an update. The expected id checks whether the Chat has
that note; it is not a body revision token. Failed or
cancelled turns preserve the existing note. Earlier Chats' notes can be referenced
by a new note, and discovery can expose later corrections even after the referenced
note is updated. Research search accepts at most 200 Unicode scalar values
(characters), including Korean text. Discovery reads the
1,000 most recently stored current candidates per Loupist, then sorts that bounded
window by recorded time; repeated replacement of other Chats' notes does not
consume that window. Notes remain available
within the Loupist after their originating Chat is deleted. They are neither original
evidence nor standing instructions. Research readers expose the Source excerpts,
World view and Lens documents captured with the saved work.

A Journal entry records the user's own judgment, with optional reasoning, linked
World entities and a dated check. Entity links use current World node ids, not
names or ticker labels. Chat validates them against the complete current World
before staging, including entities outside the Map's display limit; an unknown
link is refused at `journal.entities` so the same turn can omit or repair it. The
completed-turn transaction checks the links again. The Loupist’s
own view belongs in research notes, not in the user’s Journal.
The Map’s Library has separate Sources, Journal and Notes tabs. Notes lists the
Loupist’s saved analysis. In Journal, the user can list, create, edit and archive
entries through `journal_list`, `journal_create`, `journal_update` and
`journal_archive`. Focusing a field in the Journal form scrolls it clear of the sticky search. Chat uses `journal_save`
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
