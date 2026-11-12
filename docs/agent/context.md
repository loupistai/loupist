# Context bounds

Rust seals context in a fixed order with byte bounds. Holdings quantities are not in that envelope. Delivery under a cutoff is in [Context delivery](context-delivery.md).

## Memory and context

What a turn may save, and how it should discover evidence before answering, is in [Context delivery](context-delivery.md#memory-and-context). The bounds below are what Rust actually seals.

### Context assembly and bounds

`commands/chat_runtime/setup.rs` freezes profile instructions, the Loupist's tone and
identity-only followed companies before admitting the turn. `agent_harness.rs`
attaches those values and orders the delivered envelope. Tool rules and authority govern the request; user principles outrank Loupist instructions,
methods and the standing perspective. The agent briefly explains a conflict.
Profile instructions apply with Memory on or off, including Briefings. An empty
current instructions block supersedes older copies in the conversation.

The system prefix precedes conversation messages. Rust seals the resources below;
Pi renders them in this order. Installed Skill bodies load only when selected/read.

| System order | Block | Bound | Presence |
| --- | --- | --- | --- |
| 1 | `agent/SYSTEM.md` | Fixed bundled file | Every turn |
| 2 | Host workspace instructions | Each file ≤64 KiB; instruction and custom Skill resources share ≤64 files and ≤256 KiB | Admitted personal/workspace/Loupist instructions exist |
| 3 | Skills XML index | Fixed bundled selection plus custom resources within the same file/byte limits | Enabled Skills; a mention selects a method without changing Memory authority |
| 4 | Selected workspace CWD | One canonical filesystem path | Every turn; private Pi runtime CWD is not exposed |
| 5 | Runtime permissions and connections/capability index | Fixed bundled catalog plus ≤32 custom connections; purpose text ≤160 characters per component, cut at a sentence or word | Every turn; off connections cost one fixed sentence naming `connection_setup` |
| 6 | Personal/Loupist Skill folders | Two host-selected paths and fixed guidance | Personal root available |
| Separate provider field | Active tool definitions | Host-authorized catalog/native definitions; lazy capabilities add their admitted schemas | Permitted active tools only |

On a cold continuation, a bounded SQLite transcript (≤256 KiB text, with omission
metadata) precedes the envelope. Historical attachment restoration has a separate
1 MiB text budget and at most two historical images. On a warm continuation, Pi
retains prior messages. The full envelope has this order; `memory_protocol` fields
are listed in their serialized order inside that block.

| Envelope order | Block | Bound | Presence |
| --- | --- | --- | --- |
| 1 | Perspective | The active perspective page as its exact `PageView` JSON, ≤32 KiB; otherwise a small `readRequired` pointer | Memory on and an active perspective; admitted exact-read reference stays in `memory_protocol` |
| 2 | User principles and revision identities | ≤30 active principles, ≤6,000 characters including overflow notice | Memory on and at least one fitting principle |
| 3 | Instructions from the user for this Loupist | ≤4,000 Unicode scalar characters, checked on save and again at delivery | Every turn, including Memory off and an empty definition |
| 4 | What the user follows | ≤60 unique `(symbol, market)` identities; holdings fill the cap first, then the followed list, alphabetical within each list; one row can be both held and followed; symbol/market ≤64 characters and name ≤256. Under an as-of date: followed identities only, alphabetical, without flags, with a hint that holdings are left out | Memory on; empty list is explicit. Never quantity, cost basis, currency or market value |
| 5 | Loupist tone | One enum per Loupist: `careful`, `balanced` (default), `direct`, chosen when the Loupist is created and changed in Loupist settings. Presentation only; evidence rules never change | Every turn, including Memory off |
| 5b | `chat_surface` | `{surface, hint}`: `bot`, `session`, `scheduled` or `delegated` (`src-tauri/src/chat/surface.rs`), resolved once in setup, frozen in `AgentHostContext` and part of the runtime surface seal; one fixed hint per surface | Every turn, including Memory off |
| 5c | `bot_memory` | `loupist-bot-memory/1`: ≤30 entries (kind, text ≤300 characters, first and last dates, ≤4 basis message ids), the room left and, when due, a review of ≤8 earlier user messages as ≤600-character excerpts, under a fixed not-instructions, not-evidence rule ([Home memory](../ui/home-thread.md#bot-memory)) | Home (`bot` surface) with Memory on; under an as-of date, as it stood then and without a review |
| 6a | Memory active flag and standing-perspective read reference | One flag/reference | Memory on; read reference only after full delivery |
| 6b | Due Journal review pointers | ≤5; judgment ≤1,000 characters, reasoning ≤2,000; check claim ≤2,000, metric ≤200, unit ≤80, date 10; exact-read args | Memory on; excludes frozen evidence, nested earlier-Journal text and read receipts |
| 6c | Lens index, then counts | ≤60 page lines (kind, title ≤200 characters, method summary ≤400 or the first 160 characters of its use-when section; other summaries ≤120, section count, updated time, ≤3 map names), up to 20 methods first, then remaining pages anchored to a followed name first and most recently filed; principles and the perspective have their own blocks; ≤24 Worlds with the title as preview | Memory on; counts disclose truncation |
| 6d | Journal record counts | Four integer aggregates | Memory on |
| 6e | Relevant page and working-note pointers | ≤4 pages (those anchored to World nodes the request names first, then lexical matches) + ≤3 note matches; snippet ≤180 characters; ≤3 later-note pointers per note | Memory on and relevant matches; exact-read required |
| 6f | Selected Memory references | ≤8 references and ≤8 KiB serialized | Memory on and valid user pins |
| 6g | Current Chat working note | One note; title ≤160 bytes, body ≤16,000 bytes, change summary ≤500 bytes, ≤16 prior-note IDs and ≤3 later-note pointers | Memory on, except in Home, which has Home memory (5c); excludes frozen `inputs`; context, not evidence |
| 6h | World catalog counts/hint | Fixed metadata and integer counts | Memory on |
| 6h′ | `worldKinds` in `memory_protocol` | Core type names with one-line meanings, the core relation names, the added types and relations with one-line meanings, and the extension version; ≤1.5 KiB (meanings shrink first). Under an as-of date it shows the vocabulary as of that day, so kinds added later are left out | Memory on |
| 7 | Mentioned connections | Subset of admitted bundled/≤32 custom connections | User mentioned an enabled connection; preloading grants no new authority |
| 8 | As-of date | `loupist-as-of/1`: the cutoff, its end of day, the rule (answer only from what was known by then; tools and Memory already keep to it; say what could not be shown; principles apply as today) and `not_evidence` | The turn has an [as-of date](../analysis/point-in-time.md#point-in-time-answers); independent of Memory |
| 9 | Selected artifact pointer | One validated artifact identity ≤256 bytes, revision, kind and fixed hint | User selected a current artifact in this Chat; independent of Memory |
| 10 | Live time | Fixed schema, UTC timestamp/date and authority flags | Every turn; not evidence |
| After envelope | Current request and attachments | Request ≤128 KiB; ≤4 attachments, ≤20 MiB combined raw bytes; image budget ≤5 MiB; extraction ≤2 MiB per document | Current user turn |
