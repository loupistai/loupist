# Memory

Memory reads and saves are Rust transactions. Delivery counts are not influence or correctness.

## What the Loupist knows

About you is a centered 720px reading column and shares its edges with the header and tabs in the memory pane. Other tabs collapse from a list/reader split based on the pane's width, at 972px. The tabs and search stay whole above an open method, including in the side panel. Model-provider cards fill their own 800px settings column.

"What {Loupist} knows" opens from the Loupist's name in the header as a page over the
current tab: About you (followed assets, each opening its [asset page](asset-pages.md#asset-pages),
with remove and add, holdings as a count and date per kind (stocks and funds, crypto)
from `holdings_summary` with Update opening [Your holdings](../runtime/storage-authority.md#where-data-lives)),
Your principles (edit, pause/resume, archive, origin chat), Methods (the
Lens reader with pause/resume and archive), and History (learning reviews, taught
principles and names added). Each item lists the chats that used it. Rust's
`knowledge_memory_usage` counts distinct completed turns that received the item in
bounded context or an acknowledged exact read, or recorded an application; repeated
delivery in one turn counts once. This measures delivery, not influence or correctness.
Saved principles and methods from a receipt open here. Back from a Source opened
inside a method returns to that method. A paused banner appears when both learning
switches are off, and Resume enables them again. Tone, instructions and learning
preferences live on the [Loupist settings page](../ui/sidebar.md#sidebar).

Loupist settings → Learning offers Reset what it learned. Rust previews counts for principles,
methods, names, relationships and Sources; the dialog requires RESET (초기화 in
Korean) and sends those counts back with confirmation. One transaction removes only
items with known Loupist authorship. User-created or user-revised Lens items, shared
items, the standing perspective, manually saved Sources, Journal, watchlist and
holdings stay. Relationships with independent user/shared support and names still
used by preserved relationships stay. A Source also stays if it is the sole basis
of a protected user/shared/unknown-authored relationship. Immutable removed Sources and Lens records
are hidden, and removed Lens titles are released for reuse. Pending extraction of
removed Sources is cancelled and the derived World index reloads through the same
path as pack removal. A running World job refuses with `world_busy`; changed counts
require a fresh preview. Rust records the reset in History.

## Memory permissions and retrieval

The composer exposes one per-chat switch, Don't remember this chat. Ordinary Chat
submits both Memory read and save as true; private Chat submits both as false.
Loupist settings and Chat preferences use a positive Remember new chats switch, on
by default. Turning it off stores `chat.use_memory` and `chat.update_memory` as
false for new Chats and leaves existing Chats unchanged. Private Chat still
persists its transcript and does not undo native workspace file changes. There
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
| Methods and the standing perspective | Only when their current version was written by then; otherwise left out of search, reads (`as_of_later`) and automatic context |
| Principles | Every active principle, as today |
| Journal | Entries created by then; an outcome observed later is removed, so its check reads as pending. Due pointers count from the cutoff day; a later entry read fails with `as_of_later` |
| Notes | Working and research notes recorded by then, with pointers only to later notes written by then; the current Chat's working note and relevant-note pointers follow the same rule |
| Chat history and earlier Chats | Messages written by then, or answered under a cutoff no later than this one (`chat_history`, `conversation_memory`); a later message read fails with `as_of_later` |
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
Rust refuses `note_save`, `principle_save`, `lens_update` and Journal writes whose
text looks like a credential; the refusal is a tool refusal with reason
`sensitive_memory`, and the chat says once that the item was not saved.

## Code

- Memory writes / World: `src-tauri/src/knowledge/`, `src-tauri/src/commands/knowledge.rs`, `src-tauri/src/commands/memory_updates.rs`
