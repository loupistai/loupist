# Memory

Memory reads and saves are Rust transactions. Delivery counts are not influence or correctness.

## What the Loupist knows

The tabs collapse from a list/reader split based on the pane's width, at 972px; below it an open page hides the heading, tabs and search, which return with Back. A narrow tab row scrolls instead of painting past the pane. The page title wraps instead of being cut off. An open method fills the card: its title stays inside the reader, and pause and archive follow the document. Focusing a row or a reader action below the fold scrolls it into the pane. Model-provider cards fill their own 800px settings column.

Lens opens beside Chat from the sidebar or header. "What {Loupist} knows"
names its contents, with two tabs, Pages and History. What the user follows, holdings and
Watching live on the [Following page](follow.md#what-you-follow) (`components/briefings/FollowingPage.tsx`):
followed assets in one table per class with a Watch-for checkbox per kind, add (a bare
six-digit code is a KRX listing), remove, a quiet "Adding to the map…" line while
`knowledge_followed_status` reports queued or running work, holdings as a count and date
per kind from `holdings_summary` with Import holdings, Watch all through
`watch_defaults_apply` (also offered on the first follow receipt in chat; both list each kind that
still needs a data connection, linked to Settings › Data connections), Check now and Pause.
It is the `following` page from its sidebar row; Schedule's Watching row opens it. The shared
Loupists list sits under Pages and is hidden [in this release](sharing.md#this-release).
**Pages**: every active Lens page in one list grouped as Topics (`subject`), Ideas
(`idea`), Methods (`method`), Your principles (`principle`) and Perspective (`perspective`);
the groups are 주제, 아이디어, 분석 방법, 내 원칙 and 관점 in Korean. Search waits for a
pause in typing, then runs `knowledge_lens_search` for pages and matches a principle
on its own words. Paused pages remain in this inspection search so the user can
resume them; agent discovery still excludes them. Opening a
row reads it in `PageReader`: Markdown sections under their headings, each with its
basis chip (Claim and Source open the claim on the Map or the retained Source; "You
said"; "Loupist's view" with its uncertainty line; a page chip opens that page), a
"Sources disagree" marker on a contested section, and Forget. Forget calls
`memory_section_forget` and offers Undo through `memory_op_undo`. `world:` links draw as
Map chips, `lens:` links open the page, and the reader lists links, backlinks, the
names the page is anchored to and its last changes. Pause and Archive stay on the page.
A principle keeps its statement, why and example. **History**: from the growth events
and the Lens log, newest first. "Filed after: {chat}" for a page a chat filed, sections
you forgot or brought back, principles taught, names added to the Map and changes to what
the map tracks. The standing conversation reads as Home here and in each page's
"Used in" list, never its stored title.

Each item lists the chats that used it. Rust's
`knowledge_memory_usage` counts distinct completed turns that received the item in
bounded context or an acknowledged exact read, or recorded an application; repeated
delivery in one turn counts once. This measures delivery, not influence or correctness.
Pages and principles from an answer's Learned line open here. Back from a Source opened
inside a page returns to that page. A paused banner appears when both learning
switches are off, and Resume enables them again; a failed update stays visible as an error. Tone, instructions and learning
preferences live on the [Loupist settings page](../ui/sidebar.md#sidebar).

Loupist settings → Learning offers Reset what it learned. Rust previews counts for principles,
pages, sections, names, relationships and Sources; the dialog requires RESET (초기화 in
Korean) and sends those counts back with confirmation. One transaction removes only
items with known Loupist authorship: whole pages the Loupist created that hold no
user or shared section, and Loupist-origin sections retired from pages the user
keeps (a user retire, so the Loupist cannot refile that key), with the summary
cleared when the log shows the Loupist wrote it last (counted with the sections). User-created pages,
shared items, manually saved Sources, Journal, what you follow, holdings and
[Home memory](../ui/home-thread.md#bot-memory) (tool `bot_memory`) stay. Relationships with independent user/shared support and names still
used by preserved relationships stay. A Source also stays if it is the sole basis
of a protected user/shared/unknown-authored relationship. Immutable removed Sources and Lens records
are hidden, and removed Lens titles are released for reuse. Pending extraction of
removed Sources is cancelled and the derived World index reloads through the same
path as pack removal. A running World job refuses with `world_busy`; changed counts
require a fresh preview. Rust records the reset in History.

## Memory permissions and retrieval

The composer’s + menu exposes one per-chat switch, Private chat; an eye-off chip
marks it while on and can turn it off. Ordinary Chat
submits both Memory read and save as true; Private chat submits both as false.
Loupist settings and Chat defaults use a positive Remember new chats switch, on
by default. Turning it off stores `chat.use_memory` and `chat.update_memory` as
false for new Chats and leaves existing Chats unchanged. Private chat still
persists its transcript, sends messages to the AI provider and does not undo native
workspace file changes. The switch also covers [Home memory](../ui/home-thread.md#bot-memory)
(tool `bot_memory`): a Private chat turn in Home neither receives nor changes it. There
is no read-only composer choice. Rust keeps the two flags separate for scoped
defaults and scheduled runs.

Search discovers candidates. Exact acknowledged reads supply evidence for the
turn; renderer inspection and search previews grant no citation authority. Local
search scores a copied snapshot outside the folder lock, then rechecks ownership,
current heads and archived state. Superseded candidates are omitted.

Document vectors live in a disposable app-private cache scoped to Loupist/folder,
model fingerprint and text digest. Missing vectors are computed in bounded batches;
cache failures fall back to computation. No canonical material depends on that cache. The in-process model cache uses the
same asset fingerprint: changed asset bytes cannot reuse another model, including
when a directory is replaced in place. Identical bytes can share the loaded model.

### Memory under a cutoff

In a turn with an as-of date ([point-in-time answers](../analysis/point-in-time.md#point-in-time-answers)),
`commands/knowledge/as_of.rs` lets the model see only the Memory known by the end of
that day. Something this device recorded counts by its local calendar day; a
publication month or year counts at its last day, and an undated publication never
counts.

| Memory | Under a cutoff |
| --- | --- |
| Sources | Known when a capture (for a span, the capture holding it) was published by then; a capture without a publication date counts only as the user's own statement received by then. Search hits need such a capture; a later Source read fails with `as_of_later` |
| World | Only relationships published by then, keeping evidence from Sources known by then; one whose evidence was all published later is left out with the names only it connected. An invalidation or expiry dated later, or undated, is not shown, so a relationship reads as it stood then. Names keep only their name and kind. This holds for `world_read` views and `world_search` hits, hybrid World index hits included; a name hit needs one relationship published by then, and an annotation needs every Source it cites known by then (one that cites none is left out) |
| Lens pages (perspective, methods, subjects, ideas) | A page is known when its whole current version was written by then (`as_of::lens_visible`, one rule for reads, search and context). A page written or revised later is left out of search, reads (`as_of_later`), the index and pointers; a known page's read lists only backlinks from known pages and log entries made by then |
| Principles | Every active principle, as today |
| Journal | Entries created by then; an outcome observed later is removed, so its check reads as pending. Due pointers count from the cutoff day; a later entry read fails with `as_of_later` |
| Notes | Working and research notes recorded by then, with pointers only to later notes written by then; the current Chat's working note and relevant-note pointers follow the same rule |
| Chat history and earlier Chats | Messages written by then, or answered under a cutoff no later than this one (`chat_history`, `conversation_memory`); a later message read fails with `as_of_later` |
| [Home memory](../ui/home-thread.md#bot-memory) (tool `bot_memory`) | As it stood then: changes whose reply passes the chat-history rule, with later entries counted as notes. Forgotten entries never return, and no review is offered |
| Asset pages | Reasons and lines written by then, rows dated by then ([rules](asset-pages.md#asset-pages)) |

What each filter removes is counted for the answer's note
([counts](../analysis/point-in-time.md#point-in-time-answers)).

### Using and correcting Memory in a Chat

The receipt's "Used n principles / methods" segment uses host-recorded Lens
receipts on the completed message: record, revision, digest, kind, title and delivery
route (`context` or `exact_read`). A principle read exactly remains a principle;
answer-text `lens:` links never determine the receipt's kind or imply delivery.
The segment expands the recorded entries under the answer with Not this time and
Edit (Archive for methods). A target no longer in the catalog is struck through and
labelled removed, with no edit or archive action. A failed catalog read does not
prove deletion.

Not this time is stored per chat in Rust (`chat_memory_set_aside`): later turns of
that chat leave the record out of the principles block, the method catalog and the
relevant-method pointers, and the answer shows "Set aside for this chat · Undo".
Rust refuses `note_save`, `principle_save`, `lens_file` and Journal writes whose
text looks like a credential; the refusal is a tool refusal with reason
`sensitive_memory`, and the chat says once that the item was not saved.

## Code

- Memory writes / World: `src-tauri/src/knowledge/`, `src-tauri/src/commands/knowledge.rs`, `src-tauri/src/commands/memory_updates.rs`
