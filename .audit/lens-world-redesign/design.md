# Lens·World Memory, harness, Bot and sessions: synthesized design

This is the contract every implementation unit follows. Candidate packages are in `candidates/` (opus, fable, grok, codex); the cross-judge verdict and the synthesis decision are at the end. Paths are under `apps/loupist/` unless they start with `docs/`.

## Goal

Loupist grows its own Memory while it does the user's chat tasks and scheduled runs, so financial research gets better and the Loupist notices connections a general agent would not. The user never manages Memory. Lens stays an LLM wiki of interlinked Markdown pages. World stays a GraphRAG graph extracted from retained Sources and drawn on the Map (the Map is the product's viral surface and stays). The Bot is effortless; research sessions are the expert space. The app is unreleased: bump `KNOWLEDGE_SCHEMA_VERSION` to 6, no migration, delete replaced paths.

"Learns" always names an observable record: a `knowledge_lens_log` row, a page section with a basis, a `knowledge_schema_changes` row, a `knowledge_connections` row, a `chat_handoffs` row. No claim of measured quality without an eval.

## 1. Lens: keyed-section wiki pages, one write language

### Kinds

`LensKind = principle | perspective | method | subject | idea`, stored once in `knowledge_lens_metadata.kind`. The payload never repeats the kind. `knowledge_standing_selection` and payload `$.lensKind` are deleted; a partial unique index allows one active perspective per Loupist.

- **principle**: the user's rule. Body unchanged: statement, why, example, origin. Written only by `principle_save` and bridge edits.
- **perspective** (shown as Outlook / 관점): the Loupist's standing view. A page.
- **method** (Methods / 분석 방법): a reusable way of checking something. A page.
- **subject** (Topics / 주제): what the Loupist knows about one thing: a company, asset, sector, value chain, technology or science topic, macro theme. A page. The compounding unit.
- **idea** (Ideas / 아이디어): a connection or thesis the Loupist has not verified, with a falsifier. A page plus `IdeaState`.

### Page shape (`src-tauri/src/knowledge/lens_page.rs`, new, pure, no SQL)

```rust
pub struct Page {
    pub summary: String,              // ≤ 400 chars, one paragraph; index line uses it
    pub sections: Vec<PageSection>,   // ≤ 16, display order
}

pub struct PageSection {
    pub key: SectionKey,              // slug [a-z0-9-]{1,48}, stable identity inside the page
    pub heading: String,              // ≤ 80 chars, free text in the user's language
    pub markdown: String,             // ≤ 2,000 chars; links only lens:rec_… and world:<node id>
    pub basis: Basis,
    pub origin: Origin,               // user | loupist | learning | shared
    pub state: SectionState,          // current | contested { since_ms, claim_ids }
    pub updated_at_ms: i64,
}

pub struct Basis {
    pub refs: Vec<BasisRef>,          // ≤ 8
    pub uncertainty: Option<String>,  // ≤ 300 chars; required when any ref is a Page ref
}

pub enum BasisRef {
    Claim { claim_id: String },                                  // a World claim this turn read
    Span { record_id: String, revision_digest: String, span_id: String }, // a retained Source span read or saved this turn
    UserSaid { message_id: String },                              // host-set: the user message of the filing turn
    Page { record_id: String, section_key: Option<String> },      // another Lens page (synthesis, not evidence)
}

pub enum IdeaStatus { Open, Supported, Weakened, Refuted, Retired }
pub struct IdeaState { pub status: IdeaStatus, pub registered_at_ms: i64, pub status_at_ms: i64 }

pub enum LensBody {
    Principle(PrincipleBody),
    Perspective(Page),
    Method(Page),
    Subject(Page),
    Idea(Page, IdeaState),
}
```

Basis rule (one function, `fn basis_allowed(kind, &Basis) -> bool`): subject and idea sections need at least one ref; a section whose refs include a `Page` ref, or carry no `Claim`/`Span`/`UserSaid`, is synthesis and must carry `uncertainty`. Method and perspective sections may have empty basis. The reader derives the chip from the basis: Source/claim chips for evidence refs, "you said" for `UserSaid`, "Loupist's view" for synthesis. Agent prose never becomes World data.

Suggested keys per kind live in one table (`fn suggested_sections(kind)`) used by the tool description and the reader's ordering hint, not as a closed set: subject `overview, facts, value-chain, drivers, risks, views, open-questions`; method `use-when, steps, checks, limits`; idea `claim, mechanism, evidence-for, evidence-against, would-break, checks`; perspective `beliefs, priorities, avoid`.

### Ops and the fold

```rust
pub enum PageTarget {
    Existing { record_id: String },
    /// Idempotent create: resolves to the active page with the same (kind, title_fold),
    /// or for subject the same primary anchor, before minting a record.
    Create { kind: PageKind, title: String, anchors: Vec<AnchorInput> },
}
pub enum AnchorInput { Node { node_id: String }, Name { name: String } }

pub enum LensOp {
    /// Add or replace a section. `expected` is the digest from the exact read;
    /// None means "add only" (the key must be absent).
    Put { key: SectionKey, heading: String, markdown: String, basis: Basis, expected: Option<String> },
    Retire { key: SectionKey, expected: String, why: String },
    Summary { text: String },
    IdeaStatus { status: IdeaStatus, why: String },
}
pub struct PageDelta { pub target: PageTarget, pub ops: Vec<LensOp> }   // ≤ 12 ops

pub enum OpOutcome {
    Added { key }, Replaced { key }, Retired { key }, Summarized, StatusSet(IdeaStatus),
    AlreadyKnown { key },        // identical content
    SkippedStale { key },        // digest mismatch or key missing; text kept in the log
    RefusedForgotten { key },    // the user retired this key; only origin user may re-add
    RefusedBasis { key }, RefusedFull, RefusedKind,
}

pub fn apply(body: &LensBody, ops: &[LensOp], ctx: &ApplyContext) -> KnowledgeResult<(LensBody, Vec<OpOutcome>)>;
pub fn section_digest(section: &PageSection) -> String;      // sha256 over key, heading, markdown, basis
pub fn render_markdown(title: &str, body: &LensBody, with_keys: bool) -> String;
pub fn links(body: &LensBody) -> (BTreeSet<String> /*lens records*/, BTreeSet<String> /*world nodes*/);
```

`apply` never fails on one bad op; each op gets an outcome. Disjoint section edits from two actors both land; an edit of the same section after it changed is `SkippedStale` and logged, never a user-facing proposal.

### Storage (`knowledge/schema.rs`, version 6) and the single writer (`knowledge/lens.rs`)

- `knowledge_lens_metadata`: `kind` CHECK in the five kinds; title, title_fold, principle statement fold, archived, learned, updated. Unique active (kind, title_fold) for pages; unique active principle statement; one active perspective.
- `knowledge_lens_log` (append-only, replaces `knowledge_lens_changes`): op_id, loupist_id, record_id, op JSON, outcome, before_digest, after_digest, origin (`turn | user | learning | import`), chat_id, message_id, created_at_ms, undoes_op_id.
- `knowledge_lens_anchors` (replaces `knowledge_lens_applications`): record_id, role (`subject | mention | applied`), node_id nullable, name_fold, turn_id nullable, created_at_ms. `subject` from create anchors, `mention` derived from `world:` links at publish, `applied` from final-answer `lens:` links. A name anchor without a node stays pending and resolves after a World publication.
- Revisions: keep one current revision row per record (head); history is the log. As-of: a section whose `updated_at_ms` is after the cutoff is hidden; a page created after the cutoff is hidden.
- Deleted: `knowledge_lens_drafts`, `knowledge_lens_changes`, `knowledge_lens_applications`, `knowledge_standing_selection`, proposal functions and the stale-head proposal path.

`KnowledgeWrite::file(loupist, origin: FilingOrigin, deltas, at_ms) -> Vec<FiledPage>` is the only page writer: resolve targets (idempotent create), fold with `apply`, publish one revision per touched page, append one log row per op (refused and skipped included), rebuild links and mention anchors, emit one growth event per page (`lens_created` / `lens_revised`). `undo_op(op_id)` appends the inverse op and refuses when a later op changed that section. `learned_digest(message_ids)` derives what a message filed from the log. A create with a taken title merges into that page; nothing is dropped.

Sharing exports only portable kinds (principle, perspective, method); subject and idea pages stay local. Memory reset removes Loupist-origin pages and sections, keeps user-origin ones.

## 2. Harness: file in the turn

### Tools (`agent/tool-catalog.json`)

- `lens_file` (new, `memory_write`, component `memory-save`, unattended-allowed): `{ pages: [{ page: {recordId} | {create: {kind: subject|idea|method|perspective, title, about?: [{nodeId}|{name}]}}, ops: [...] }], rationale }`. Basis refs in the call are the turn's handles: `{ world: readRef, claimId }`, `{ source: readRef | saveRef, spanId? }`, `{ said: true }`, `{ lens: recordId, key? }`. Rust resolves them against acknowledged reads and same-turn saves at staging and refuses unknown handles by field. Budgets: ≤2 calls, ≤8 pages, ≤40 ops per turn. Credential-like text refuses with `sensitive_memory`. `said` is refused in unattended runs. Replaces `lens_update`.
- `principle_save` unchanged in authority, moved into `memory-save`. The `lens-learning` component is deleted so one `capability_load memory-save` exposes every write.
- `note_save` stays, described as "this Chat's progress and user-stated facts for this Chat; durable conclusions go to `lens_file`".
- `lens_search` gains `kinds` filter; `lens_read` returns sections with keys, digests, basis chips, anchors, links and backlinks, and `readRef`.

### Turn commit

`commit_completed_turn` commits staged filings through `KnowledgeWrite::file` with `FilingOrigin::Turn{chat_id, message_id, user_message_id, unattended}` in the same transaction as the reply. Stopped or failed turns file nothing. The answer's `memory_update` lists filed pages with outcomes.

### Envelope (`agent_harness.rs`, `commands/knowledge/chat_reads.rs`)

| Block | Content | Bound |
| --- | --- | --- |
| `chat_surface` (new, every turn, Memory on or off) | `{surface: bot|session|scheduled|delegated, hint}` fixed hint text per surface | fixed |
| 6c `index` (replaces newest-40 catalog) | page lines: kind, title, summary ≤120, section count, updated, ≤3 map names; pages anchored to followed names first, then recently filed, then most used; counts disclose truncation | ≤60 lines |
| 6e pointers | pages anchored to World nodes named in the request (resolved through the World index), then lexical | ≤4 pages + existing ≤3 notes |
| `worldKinds` (new) | core and extension kinds and relations with one-line meanings, extension version | ≤1.5 KiB |

Pointers stay discovery; exact `lens_read` still creates receipts. As-of rules apply to every new block.

### SYSTEM.md rules (Memory section)

With saving on: before substantive research, check the index and pointers and read relevant pages. When the turn establishes something durable (a dated fact with its source, a mechanism, a value-chain link, a contradiction, a reusable check), file it with `lens_file` before replying: revise the page you read (section digests) or create one; cite the turn's reads; mark synthesis with its uncertainty. Light questions, chatter and one-offs file nothing. Never say something was saved unless the tool staged it.

## 3. World: broader core, one vocabulary source, changes that apply with undo

### Core (`knowledge/world_extract/ontology.rs` is the single list)

- New types: `Technology` (a science, technique, process, material or standard that more than one organization can use: EUV lithography, hybrid bonding, GLP-1 agonism, proof-of-stake), `Segment` (an industry, market segment or value-chain stage: advanced packaging, Seoul office leasing, L2 sequencing).
- `Asset` covers instruments and holdings of any class: `asset_class` takes exactly `AssetClass::ALL` (equity, crypto, fund, commodity, rates_fx, real_asset, prediction, private) rendered from the enum, plus an `instrument` field (index, bond, etf, token, pair, property, deal, other). Real estate and private deals are `Asset` with class `real_asset` / `private`.
- New relations: `PART_OF` (component or stage composition), `REQUIRES` (input dependency: a product, technology or segment needs another), `OPERATES_IN` (organization in a segment), `ISSUES` (organization issues an asset: share class, token, bond, fund unit), `OWNS` (organization owns an asset or real asset).
- `world_schema.rs` derives its core type and relation names from `ontology.rs`; the hand-copied lists are deleted.
- Map categories gain Technology and Industry (Segment). Korean labels added.

### Followed identity (`knowledge/map.rs`, `knowledge/assets/mod.rs`)

One resolver `resolve_followed(item, nodes) -> IdentityMatch { Exact(node_id) | Ambiguous(Vec) | Unmatched }`. A symbol hit counts only when the node's stated exchange/market maps to the item's market (same normalized code or same `exchange_time_zone`); a node with no stated market still matches when it is the only symbol hit. Several hits with no deciding market → `Ambiguous`, which matches nothing and never picks the smallest id. Map markers, asset pages, upcoming events, conflict lookups and Lens pointers use this resolver.

### Vocabulary changes (`knowledge/world_schema.rs`)

`knowledge_schema_changes` (append-only: change_id, loupist_id, before_json, after_json, producer `preset|chat|review|undo`, evidence_json, origin chat/message, created_at_ms) replaces `knowledge_schema_proposals`. The live extension is the latest `after_json`. Every change applies at once after `validate_extension` (limits stay 4 types, 6 relations, 3 fields) and writes a `schema_extended` / `schema_reduced` growth event.

Producers:
1. **Preset**: the template's curated seed applies at creation keyed by `LoupistTemplate`, not by comparing the focus text. Semiconductors, dividends and macro keep their seeds; no new seed ships without the template seed grader.
2. **Chat**: `world_schema_update` (`memory_write`, interactive turns only, ≤1 per turn, staged, commits with the reply). The user's sentence is the approval.
3. **Review**: the existing background review may only promote what the World already shows: a relation from an open relation name with ≥6 claims across ≥3 Sources, a type covering ≥5 untyped names. The host counts; an unsupported proposal is refused.

The model-written seed for a custom focus is deleted. Undo appends a change restoring the previous extension when nothing later touched the same kinds, else removes just that kind. Extraction never reclassifies existing nodes.

## 4. Bot and research sessions

### Surface (`src-tauri/src/chat/surface.rs`)

`ChatSurface { Bot, Session, Scheduled, Delegated { handoff_id } }`, resolved once in setup, frozen in `AgentHostContext`, part of the cache seal. It owns the schedule-tool filter, the `research_open` filter, the `chat_surface` envelope block and the after-commit hook (deliver runs into the Bot, launch a hand-off). The scattered `is_bot` checks (`setup.rs:62-76`, `chat_runtime.rs:509-517`, `MessageSchedules.tsx:23`) move into it.

### Hand-off (`src-tauri/src/chat/handoff.rs`, `store/schema.rs` owns `chat_handoffs`)

`research_open { title, question, brief, pins: [{lens: recordId} | {world: nodeId}], requestKey }`: Bot only, ≤1 per turn, never in scheduled or delegated runs. It stages; at commit the same transaction inserts `chat_handoffs(loupist_id, request_key UNIQUE, chat_id, status created|running|done|failed|stopped, run_id, created_at_ms)` and the session Chat with its first user message (question + brief + pins). After commit, the host starts one delegated run through the existing run coordinator with the unattended allowlist and the Loupist's chat model. A `created` row without a run (crash between commit and launch) is launched by the next delivery pass; that is a first send, not a retry. A run that started and stopped is reported, never rerun. The finished run reaches the Bot through `deliver_finished_runs` as a `handoff` record with its learned digest. SYSTEM.md: in the Bot answer directly and briefly from Memory and a few reads; call `research_open` when the user asks for depth or the request needs multi-step work; otherwise offer it. In a session work in depth. Both file what they establish.

### Bot posts

- `handoff`: "Researched: {title} · Open · Learned n · m ideas".
- `delivery` (scheduled) gains the run message's learned digest.
- `link` (from discovery, ≤3 per day): "New link: A ↔ C through B · Look into it · Not useful". Look into it submits the same hand-off through the bridge (`research_handoff_start`), so there is one way to delegate.
- `vocabulary`: "Your map now also tracks {plain label} · Undo".
- The Bot title renders as the localized Bot label everywhere (MemoryPage history and Used in included); perspective receipts are no longer dropped (`src/chat/receipt.ts:61-62`).

## 5. Discovery after every World publication (`knowledge/world_followups.rs`, `knowledge/connections.rs`)

Runs in the same transaction as `growth::world_diff` (`knowledge/world.rs`), no model call:
1. **Contested sections**: sections whose `Claim` basis became disputed, withdrawn or removed get `SectionState::Contested` and one growth event per page.
2. **Anchor resolution**: pending name anchors resolve by canonical name or alias.
3. **Link detection** (`connections::detect`, pure): paths of 2 or 3 hops from a seed (a followed entity via `resolve_followed`, or an entity an open idea is about) through a newly added claim to an entity the seed could not reach within 2 hops before. Score: crosses map categories, low-degree middle nodes (hub penalty, degree ≤40), support count. One candidate per (seed, far end) ever.

`knowledge_connections(connection_id, loupist_id, seed_node_id, far_node_id, path_json, score, status new|shown|opened|dismissed|filed, created_at_ms, status_at_ms)`. At most 3 shown per day; the rest stay `new`. The Map highlights the path; the inspector separates "What the sources say" from "What this could mean". An idea page filed about a link marks it `filed`.

## 6. UX

- **Under every answer**: one line, "Learned n · Map +n names · n ideas · Used n pages". Expanding lists each filed page with added, replaced and retired sections. Each section has Fix (opens the Bot composer with the section pinned and "Correct this:") and Forget (retire by user, Undo). Nothing waits for approval.
- **Lens panel / "What {Loupist} knows"**: tabs About you, Pages (grouped Topics, Ideas, Methods, Your principles, Outlook, with search), History (from the log: "Filed after: {chat}", "Forgot by you", "Undone"). A page reads as Markdown sections with basis chips and a contested marker; Map names are chips that open the Map there.
- **Map** stays the graph. Entity inspector gains Notes (pages anchored to it, with summaries) and the claim inspector gains Cited in. A connection highlights its path.
- **Loupist settings → What the map tracks**: kinds and links in plain words, recent changes with Undo, Remove. No Apply/Dismiss.
- **Onboarding** keeps preset cards (templates) plus the free-text focus; every preset applies its curated tracking at creation. Copy never says ontology, schema or entity type (string test over i18n).

## 7. Deleted

`lens_update` and its staging, `knowledge_lens_drafts` and proposal UI and bridge entries, `knowledge_standing_selection`, payload `lensKind`, `knowledge_lens_changes`, `knowledge_lens_applications`, `knowledge_schema_proposals` and Apply/Dismiss UI and commands, the model custom-focus seed and `evals/world/seed-grade.mjs` with its corpus and oracle, the focus-equality seed gate, hand-copied core lists, the `lens-learning` component, the silent `continue` on a taken title, scattered `is_bot` checks.

Method learning (`knowledge/learning.rs`) stays for now but publishes through `KnowledgeWrite::file` as a method page (`FilingOrigin::Learning`), mapping its six fields to sections `use-when`, `steps`, `checks`, `limits` plus summary. A research-aware background compile job replaces it later.

## Later (out of this change)

Background compile job over completed research turns (replaces method learning; needs faithfulness evals); deleting working notes (Journal `research_id` dependency); model lint (contradictions, summary rewrite, merges); communities as topic pages; PPR or model-explained discovery; temporal-holdout evaluation of links; issuer/instrument/listing identity layer (FIGI/LEI/DTI); re-extraction under a new kind; sharing subject and idea pages; utility ranking from Keep/Forget/applied counts; paid `compounding-v1` suite runs.

## Synthesis decision

Cross-judge (Fable, other family than the Opus parent) scored A=opus highest on compounding, generality and Bot/sessions and recommended it as the base, trimmed to about a day. Base: **opus** (one write language, pure fold, append-only log, idempotent creation, forgotten-text refusal, `ChatSurface`, idempotent `research_open` with a delegated run, template-keyed presets, evidence-gated review, deterministic post-seal discovery).

Grafts:
- **codex**: keyed Markdown sections with free headings and typed basis instead of opus's fixed 18-section item enum, so pages stay a Markdown wiki (the user's fixed decision) while keeping per-section provenance and conflict-free merge; the `Exact | Ambiguous | Unmatched` identity resolver shared by every consumer.
- **fable**: the reasoned auto-apply argument (approval nobody can give is a silent never) and the i18n string test that bans ontology jargon; Bot title and perspective receipt fixes; `title_taken` never dropped.
- **grok**: the minimal-core instinct (we add 2 types, not opus's 3: `RealAsset` folds into `Asset` with `asset_class real_asset`), forward-only vocabulary changes, and `Ambiguous` matching nothing.

Rejected: grok's deterministic projector as the only writer (pages become chronicles, never current synthesis, and fresh `source_save` turns cannot file); grok's open-only hand-off (gives the work back to the user); fable's whole-page rewrite (prompt-only compounding, stale heads become proposals); codex's identity keys on every node and registry inversion (multi-day); opus's deletion of working notes and the compile job in this change (pushed to Later; both judges flagged day-fit).

Orca seats (codex `gpt-6-astra @xhigh`, grok `grok-4.7 @xhigh`) read the Orca-managed main checkout at `5046408`, the same commit as this worktree.

## 8. Bridge and host-post contracts (Rust DTOs serialize camelCase exactly like this)

```ts
// src/contracts/lens.ts (new). Rust owner: Lens core unit.
export type LensKind = "principle" | "perspective" | "method" | "subject" | "idea";
export type IdeaStatus = "open" | "supported" | "weakened" | "refuted" | "retired";
export type BasisRefView =
  | { kind: "claim"; claimId: string; label?: string }
  | { kind: "span"; recordId: string; spanId: string; title?: string }
  | { kind: "said"; messageId: string; chatId?: string }
  | { kind: "page"; recordId: string; sectionKey?: string; title?: string };
export interface SectionView {
  key: string; heading: string; markdown: string; digest: string;
  basis: { refs: BasisRefView[]; uncertainty?: string };
  chip: "source" | "said" | "view" | "none";      // derived by Rust from the basis
  origin: "user" | "loupist" | "learning" | "shared";
  contested: boolean; contestedClaimIds: string[]; updatedAtMs: number;
}
export type LensBodyView =
  | { kind: "principle"; statement: string; why: string; example: string; origin: string }
  | { kind: "perspective" | "method" | "subject"; summary: string; sections: SectionView[] }
  | { kind: "idea"; summary: string; sections: SectionView[]; idea: { status: IdeaStatus; registeredAtMs: number; statusAtMs: number } };
export interface AnchorView { role: "subject" | "mention" | "applied"; nodeId: string | null; name: string }
export interface LensLinkView { recordId: string; title: string; kind: LensKind }
export interface LensLogEntryView { opId: string; op: "put" | "retire" | "summary" | "idea_status"; outcome: string; key?: string; origin: "turn" | "user" | "learning" | "import"; chatId?: string; messageId?: string; createdAtMs: number; undoesOpId?: string }
export interface PageView {
  recordId: string; revisionId: string; kind: LensKind; title: string;
  archived: boolean; paused: boolean; learned: boolean;
  body: LensBodyView; anchors: AnchorView[]; links: LensLinkView[]; backlinks: LensLinkView[];
  history: LensLogEntryView[];   // newest first, ≤ 20
}
export interface LearnedPage { recordId: string; title: string; kind: LensKind; created: boolean; outcomes: Array<{ opId: string; outcome: string; key?: string }> }
export interface LearnedDigest { messageId: string; pages: LearnedPage[] }
```

Bridge commands (names are Rust command names; `tauriBridge.ts` camelCases them):
- Lens core: `knowledge_lens_read {loupistId, recordId} → PageView` (replaces the old six-field view); `knowledge_catalog` summaries carry `lensKind: LensKind` and `summary`; `knowledge_lens_pages_for_node {loupistId, nodeId} → Array<{recordId, title, kind, summary}>`; `memory_section_forget {loupistId, recordId, key} → PageView`; `memory_op_undo {loupistId, opId} → PageView`; `memory_learned {loupistId, messageIds} → LearnedDigest[]`. Deleted: `knowledge_lens_proposals`, `knowledge_lens_proposal_dismiss`, `knowledge_lens_changes`.
- World unit: `world_schema_get` view gains `changes: Array<{changeId, producer: "preset"|"chat"|"review"|"undo", summary, addedTypes: string[], addedRelations: string[], removedTypes: string[], removedRelations: string[], createdAtMs, undoable: boolean}>` (newest first, ≤20) and loses `proposal`; `world_schema_undo {loupistId, changeId} → WorldSchemaView`. Deleted: `world_schema_apply`, `world_schema_dismiss`.
- Discovery unit: `knowledge_connections {loupistId} → Array<{connectionId, seedNodeId, farNodeId, seedName, farName, path: Array<{nodeId, name, relation, claimId}>, score, status, createdAtMs}>`; `knowledge_connection_dismiss {loupistId, connectionId}`.
- Bot unit: `research_handoff_start {loupistId, title, question, pins: Array<{lens: string} | {world: string}>, requestKey} → {chatId, handoffId}`.

Host-post records on `ChatMessage` (`src/contracts/chat.ts`), each optional:
- `handoff?: { handoffId: string; chatId: string; title: string; status: "running" | "done" | "failed" | "stopped"; learned?: LearnedDigest }` (Bot unit)
- `link?: { connectionId: string; seedName: string; farName: string; path: Array<{ name: string; relation: string }> }` (Discovery unit)
- `vocabulary?: { changeId: string; producer: "preset" | "chat" | "review"; labels: string[] }` (World unit; `labels` are plain-language display labels, never type names)
- existing `delivery` gains `learned?: LearnedDigest` (Bot unit)
