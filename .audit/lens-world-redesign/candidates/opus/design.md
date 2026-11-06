# Lens·World Memory, harness, Bot and sessions (candidate: opus)

## Problem

The user wants a Loupist that gets better at their research by doing it: every chat
answer, briefing and delegated investigation should leave the Loupist knowing more,
seeing more connections, and using both next time, without the user curating Memory.
Lens stays an LLM wiki, World stays GraphRAG over retained Sources, the Map stays the
visible graph, the Bot stays effortless and research sessions stay the expert space.

Today nothing compounds by construction (grounding, main @ 5046408):

- A research conclusion lands in the per-Chat working note (`note_save`), which the
  next save replaces. There is no "file the answer back" operation.
- Lens is six-field whole documents (`LensDocument`, `knowledge/model.rs:750`). The
  only change path is a full rewrite after an exact read (`lens_update`); a stale
  head becomes a proposal that waits for a user who will not review it. A create with
  a taken title is silently dropped (`commands/knowledge/learning.rs:134`).
- There are no pages about subjects, and a Lens body may not point at a World
  entity (`knowledge/lens.rs:334-390`). Kind is stored three ways.
- Background method learning reads only new World claims (≥5, `knowledge/learning.rs:119`),
  never the research, and publishes straight to Lens.
- The schema is invisible to the Chat agent, cannot be shaped by talking, and model
  proposals wait for an approval ordinary users cannot give. The core has no place
  for technology or science, real assets or industries, and the Asset class prompt
  vocabulary disagrees with `AssetClass`.
- Nothing looks for new multi-hop links between what the user follows and new facts.
- The model is never told it is the Bot; the Bot cannot delegate deep work and never
  shows what was learned.

The shape is non-obvious for three reasons. Three actors write Memory: the chat turn
(in-turn tools, committed with the reply), scheduled and delegated runs (same path,
unattended), and the background helper (tool-free, rate-limited). Whole-document
writes make those actors conflict and force a user-facing proposal queue. And
"learns" must be checkable, so every growth step needs an observable record.

Constraints honored: Rust owns persistence, transactions and worker lifetime; React
submits typed intent through `bridge/tauriBridge.ts`; tool writes stage and commit
with the completed reply; failed or stopped turns save nothing; tool contracts live
in `agent/tool-catalog.json`; read/write permission model, per-turn broker budgets,
as-of cutoff rules and shared-content wrapping are kept; no automatic paid retries;
background model work stays on `dispatch_inference`; English and Korean copy without
ontology terms; `KNOWLEDGE_SCHEMA_VERSION` bumps to 6 with no migration.

## Usage (caller's view)

### A research session turn

The user, in a session titled "HBM4 back-end": "Who supplies SK hynix's TC bonders
now, and does Micron's qualification news change Hanmi's position?"

The envelope says where the model is and what it already knows:

```json
{ "chat_surface": { "surface": "session", "hint": "Research session: work in depth; file what you establish." },
  "memory_protocol": {
    "active": true,
    "index": { "shown": 41, "total": 41, "pages": [
      { "kind": "subject", "title": "Hanmi Semiconductor", "summary": "TC bonder maker; SK hynix is the anchor customer…", "items": 14, "updated": "2026-10-28", "on_map": ["Hanmi Semiconductor"] },
      { "kind": "idea", "title": "Dual-sourcing caps Hanmi's HBM4 share", "status": "open", "items": 5, "updated": "2026-10-30" },
      { "kind": "method", "title": "Equipment share after a second-vendor qualification", "items": 7 } ] },
    "relevant": [ { "kind": "subject", "title": "Hanmi Semiconductor", "why": "names Hanmi Semiconductor on the map", "tool": "lens_read", "readArgs": { "recordId": "rec_hanmi" } } ],
    "chatPages": [ { "title": "HBM4 back-end supply", "kind": "subject", "items": 9, "readArgs": { "recordId": "rec_hbm4be" } } ],
    "worldKinds": { "core": ["Organization", "Person", "Asset", "Product", "Technology", "Segment", "RealAsset", "Place", "Policy", "Event", "Metric"], "added": [ { "name": "Qualification", "about": "A customer's vendor qualification of a tool or part" } ] } } }
```

The model's calls:

```text
lens_read { recordId: "rec_hanmi", status: "applied" }
  → page: summary, items [{ itemId: "itm_7", section: "views", text: "SK hynix single-sources TC bonders from Hanmi…", basis: [{claim}] }, …], readRef "l1"
world_search { query: "Micron TC bonder qualification Hanmi Hanwha", scope: "world" } → readArgs
world_read {...} → readRef "w1" (claims clm_a, clm_b)
web_fetch {...}; workspace_capture {...}; source_save {...} → saveRef "s1"
(answer drafted)
capability_load { id: "memory-save" }
lens_file { pages: [
  { page: { recordId: "rec_hanmi" }, ops: [
      { op: "add", section: "facts", text: "Micron qualified Hanwha Semitech's TC bonder for HBM4 (reported 2026-10-22).", basis: [{ source: "s1" }] },
      { op: "revise", itemId: "itm_7", readRef: "l1",
        text: "SK hynix still single-sources; Micron now dual-sources, so Hanmi's HBM4 share depends on SK hynix volume.",
        basis: [{ world: "w1", claimId: "clm_a" }, { source: "s1" }], why: "Micron qualification" } ] },
  { page: { create: { kind: "idea", title: "Dual-sourcing caps Hanmi's HBM4 share", anchors: ["Hanmi Semiconductor", "Micron Technology", "Hanwha Semitech"] } }, ops: [
      { op: "add", section: "claim", text: "Hanmi's HBM4 bonder share falls below its HBM3E share.", basis: [{ source: "s1" }] },
      { op: "add", section: "would_break", text: "Micron returns to a single vendor, or SK hynix qualifies a second vendor too." },
      { op: "add", section: "checks", text: "Hanmi order disclosures vs SK hynix capex guidance, each quarter." } ] } ] }
  → { staged: 2, outcomes: [added itm_…, revised itm_7, created page + 3 added], note: "Saved only when this reply completes." }
```

The answer completes; Rust commits Sources, page deltas and log rows in the turn's
transaction. Because the turn read evidence and wrote a long answer, Rust queues one
compile job for it. Minutes later the helper model files what the turn established but
the model did not file: here, a method step ("after a customer qualifies a second
vendor, compare the incumbent's order backlog with the customer's capex guidance"),
citing the same evidence. The receipt under the answer updates from "Filing…" to
"Learned 5 things · 2 topics · 1 idea".

A creation whose title or subject is already taken merges into that page; nothing is
dropped. A `revise` on an item someone changed after the read is reported as
"not applied, the page changed since it was read", with its text kept in the log.

### A scheduled briefing run

Same Chat path, unattended allowlist, Memory forced on. The briefing reads
`assetDeltas`, retains a new 10-Q with `source_save`, moves a read with
`thesis_update`, and files with `lens_file` (now on the unattended allowlist in place
of `note_save` and `lens_update`):

```text
lens_file { pages: [ { page: { recordId: "rec_hanmi" }, ops: [
  { op: "add", section: "facts", text: "Q3 orders KRW …bn, up …% y/y (10-Q equivalent, filed 2026-11-05).", basis: [{ source: "s2" }] } ] } ] }
```

`said` basis is refused here (no user in the run). After commit: the compile job runs
for the run's message; World extraction of `s2` publishes; the post-seal pass marks the
Hanmi item whose claim was withdrawn as contested and finds one new link. The Bot gets
one delivery post: "From: Morning briefing · Open · Learned 4 things about 2 topics ·
1 new link".

### The Bot

The user types "is the HBM thing bad for Hanmi?" in the Bot. The envelope says
`surface: "bot"`. The Bot answers in a few lines from the Hanmi page and World, files
nothing new because nothing new was established, and offers depth. When the user says
"dig into it" (or asks for something multi-step in the first place), the Bot calls:

```text
research_open { title: "Hanmi after Micron dual-sourcing",
  question: "Quantify Hanmi's HBM4 TC bonder exposure after Micron qualified Hanwha Semitech…",
  brief: "Known so far: …(with citation URLs)", pins: [ { kind: "lens", recordId: "rec_hanmi" }, { kind: "world", entityId: "ent_hanwha_semitech" } ],
  requestKey: "hanmi-micron-1" }
  → { status: "opens_after_reply", title: "Hanmi after Micron dual-sourcing" }
```

After the Bot reply commits, Rust creates the session, posts the hand-off as its first
message, and starts one delegated run in it (unattended allowlist, the Loupist's chat
model). When the run ends, the Bot gets a post: "Researched: Hanmi after Micron
dual-sourcing · Open · Learned 6 things · 1 idea". The user continues in the session
with full tools, or never opens it.

Other Bot posts: "New link: Hanmi Semiconductor ↔ Hanwha Aerospace, through Hanwha
Semitech (subsidiary) · Look into it · Not useful", and "Your map now also notices
vendor qualifications · Undo".

### The user's views

- **Under every answer** (Bot and sessions): one line, "Learned 5 · Map +4 names · 1
  idea · Used 3 pages". Expanding it lists each page with its added, revised and
  retired items. Each item has Keep, Fix (opens chat with the item pinned and "Correct
  this:") and Forget (undoable). Nothing waits for approval.
- **Lens panel** (Research principles today) becomes the notebook: an index grouped as
  Topics, Ideas, Methods, Your principles and Outlook, with search. A page reads as
  Markdown sections. Each item shows a source chip, a "you said" chip or "Loupist's
  view", and a contested marker when its evidence changed. Names that are on the Map
  are chips that open the Map there. History lists the log ("Filed after: Morning
  briefing", "Forgot by you", "Undone").
- **Map** stays the graph. The entity inspector gains "Notes": the pages about this
  name with their summaries. The claim inspector gains "Cited in". Filter gains
  "Links found" to highlight new link paths. Two new legend categories:
  Technology and Industry.
- **Loupist settings** "What the map tracks" lists the kinds of things and links in
  plain words, recent changes with Undo, and Remove. "Learn from research" replaces
  "Learn methods from new information".
- **Onboarding** keeps preset cards (templates) plus one free-text question. Every
  preset applies its curated tracking at creation. Power users change tracking by
  saying so in chat.

## Shape

### 1. The page model (Lens as an itemized wiki)

A Lens record is one of five kinds, stored once in `knowledge_lens_metadata.kind`:
`principle`, `perspective` (shown as Outlook), `method`, `subject` (Topic) and
`idea`. The head payload parses by that kind into `LensBody`, an enum whose variants
carry their own data: principles keep `PrincipleBody` (statement, why, example);
the other four are a `Page` of a short summary and items; `Idea` also carries
`IdeaState` (status and registration date). Kind is never in the payload and never
in a second table, so it cannot disagree (per type-system-discipline). The
`knowledge_standing_selection` table and payload `$.lensKind` are deleted; one
active perspective per Loupist is a partial unique index.

A page is items, not prose. Each `LensItem` has a stable id, a section from its
kind's fixed set (`PageKind::sections()`), text of at most 500 characters, a typed
`Basis` list, an origin (user, loupist, compile, shared) and dates. Sections carry
their evidence rule (`Section::basis_rule()`): `facts`, `for` and `against` need a
World claim, a Source span or the user's own words; `views`, `workings` and
`mechanism` need some basis, possibly other items; method steps and perspective
beliefs may have none. Agent-inferred text therefore cannot sit where facts sit,
and nothing in Lens ever becomes a World fact (World is written only from Sources).
Caps (24 items per section, 60 per page) force consolidation instead of growth
without bound (ACE).

Pages link to each other with `lens:rec_…` and now to World names with
`world:ent_…`. Anchors (`knowledge_lens_anchors`) record which World names a page
is about (`subject`), mentions (`mention`, derived from item links at publish) and
was applied to (`applied`, from final-answer `lens:` links, replacing
`knowledge_lens_applications`). An anchor given by name before extraction has
published stays pending and resolves after the next World publication.

### 2. One write language, one apply function, one log

Every Lens change is a `PageDelta`: a target (existing record, or create by kind and
title) and a list of `LensOp` (`add`, `revise`, `retire`, `summary`,
`idea_status`). Three producers emit deltas: the `lens_file` tool (staged, committed
with the reply), the compile job (helper output parsed by Rust) and user intent from
the bridge (Forget, Keep, Undo). One pure function, `lens_page::apply`, folds ops into
the current body and returns per-op outcomes; one SQL function, `lens::file`, loads
the head, calls `apply`, publishes the new revision and appends one
`knowledge_lens_log` row per op. Producers never write pages directly (per
model-the-domain and boundary-discipline).

Adds never conflict, so concurrent actors need no lock or proposal queue: each
producer publishes independent deltas and the fold merges them in the folder
transaction (per separate-before-serializing-shared-state). `revise` and `retire`
carry the item digest the actor read; a mismatch yields `skipped_stale` with the text
kept in the log. Creation is idempotent by natural key: `(kind, title_fold)`, and for
subjects also the primary anchor. Re-filing an equal item yields `already_known`; text
the user forgot is refused as `refused_forgotten`, so the compile job cannot bring it
back (per make-operations-idempotent).

`knowledge_lens_log` is append-only and replaces `knowledge_lens_changes` (one row per
record, overwritten). It is the learning record: who filed what, from which turn,
run or job, on what basis. Undo appends an inverse op. As-of reads filter items by
their dates, so a cutoff hides a revised item instead of the whole page.

`knowledge_lens_drafts`, `save_lens_proposal`, `dismiss_lens_proposal` and the
proposal UI are deleted: the delta model has no stale-document state to propose.

### 3. The compile job (sleep-time filing) replaces method learning

After a memory-saving turn commits, Rust queues a compile job when the turn read
evidence (two acknowledged Source or World reads, or one `source_save`) and the answer
is at least 600 characters. The job runs on the existing supervisor queue
(`GraphSupervisor::pump_once`, in the slot `pump_learning` held) through
`dispatch_inference`: one tool-free call. Input: the user message, the answer, the
turn's acknowledged evidence as local handles (`e1`, `e2`; the model never sees IDs),
up to four candidate pages chosen by anchors, the ops the turn already filed, and the
active principle statements. Output: `PageDelta`s whose basis references only those
handles. Rust maps handles to claims and spans, so a cited fact is always one the turn
read. Limits: one running job, one pending job per Chat (later turns join it, up to
three), 24 jobs per Loupist per day, no retry of an uncertain call, a restart marks a
running job uncertain. Learning off, Memory saving off or no helper model: no job.

This deletes the claim-count review (`knowledge/learning.rs`, `method-learning.md`,
`knowledge_learning_reviews`), its six-hour cooldown and the "five new claims" gate.
Methods now grow from research, the place methods are actually exercised.

### 4. After every World publication

`world.rs` already diffs the previous and next snapshot (`growth::world_diff`). A new
pure pass, `world_followups::after_seal`, runs beside it in the same transaction with
the followed identities passed in:

1. **Stale lint.** Items whose basis claim became disputed, withdrawn or was removed
   get `ItemState::Contested` and one `item_contested` growth event per page. No model.
2. **Anchor resolution.** Pending anchors resolve by canonical name or alias; anchors
   whose entity vanished re-resolve by name or turn pending.
3. **Link detection.** `connections::detect` looks for paths of length two or three
   from a seed (followed entity, or a name an open idea is about) through a newly added
   claim to an entity the seed could not reach within two hops before. Score: crosses
   map categories, low-degree middle nodes (hub penalty), support count. At most one
   candidate per (seed, far end) pair, ever; at most three Bot posts per day.

`knowledge_connections` keeps every candidate with its path and status (new, shown,
opened, dismissed, filed). The count of links shown and looked into is printed on idea
pages ("1 of 14 links looked into this month") so a hit reads against how many were
tried. Explanation and hypothesis writing are model work the user starts: "Look into
it" opens a delegated session on the path, whose answer files an idea page.

### 5. World generality and the ontology

The core grows from 8 types and 12 relations to 11 and 15:

- `Technology`: a science, technique, process, material or standard (EUV lithography,
  hybrid bonding, GLP-1 agonism). `Product` stays a commercial offering.
- `Segment`: an industry, market segment or value-chain stage (advanced packaging,
  Seoul office leasing). This is where value chains live (ETO's stage → input → provider).
- `RealAsset`: a specific owned physical asset (building, land, mine, plant, vessel,
  pipeline, data center). `Place` becomes geography only.
- `PART_OF` (stage or component composition), `OWNS` (control or title of a real
  asset or asset), `OPERATES_IN` (organization in a segment). `USES` widens to
  Segment and Technology endpoints and carries the input dependency.
- `Asset.asset_class` takes exactly the `AssetClass` values, rendered into the prompt
  from `AssetClass::ALL`; a new `instrument` field holds index, bond, ETF, token or
  pair. Private deals and funds are `Asset` with class `private`.

`world_schema.rs` stops repeating the core: its type and relation lists derive from
`ontology.rs`. Map categories gain Technology and Industry.

The extension stays small (four types, six relations) and still applies only to
Sources processed afterward, with open names kept for what does not fit (annotate,
not strict). Three producers change it, and every change applies at once after host
validation, appears as a growth event and in the Bot, and can be undone:

1. **Preset.** Each template's curated seed applies at creation, keyed by template,
   not by an equality check on the default focus text (`world_schema.rs:582-593`).
2. **The user, in chat.** "Also keep track of licensing deals" makes the agent call
   `world_schema_update`. The user's sentence is the approval; it commits with the
   reply.
3. **The reviewer.** The background review may only promote what the World already
   shows: an open relation name with at least six claims across three Sources, or
   a set of at least five untyped names with one fitting type. A proposal for a kind
   with no such evidence is rejected by the gate.

The model-written seed for a custom focus is deleted. It guessed a schema before any
evidence, and the broader core plus focus emphasis covers day one.

`knowledge_schema_changes` (append-only: before, after, producer, evidence, origin)
replaces `knowledge_schema_proposals` and its pending state. Undo restores the
previous extension when nothing later touched the same kinds, otherwise removes just
that kind. Nodes already extracted keep their type.

My position on the open choice: auto-apply, gated by evidence, visible and undoable.
An approval queue fails the stated user, who cannot judge ontology, and the current
queue shows it rots. Speculative auto-apply biases extraction before evidence exists.
Evidence-gated promotion of kinds the graph already contains is the only producer
that both runs without the user and cannot invent structure.

### 6. Bot and sessions: one `ChatSurface`

`chat/surface.rs` defines `ChatSurface { Bot, Session, Scheduled, Delegated }`,
resolved once in setup and frozen in `AgentHostContext`. It owns what today is three
scattered `is_bot` checks (`setup.rs:62-76`, `chat_runtime.rs:511-517`,
`MessageSchedules.tsx:23`): the tool filter (schedule tools, `research_open` only in
the Bot), the `chat_surface` envelope block (present with Memory on or off) and the
after-commit hook (deliver runs into the Bot, launch a hand-off).

`research_open` stages a `ResearchHandoff` (title, question, brief, up to eight pins,
request key). The turn's commit transaction inserts only a `chat_handoffs` row keyed by
`(loupist, request_key)`. After commit, `handoff::launch_pending` creates the session
Chat with an id derived from the hand-off (so a repeat finds it), writes its first
message, and starts one delegated run through the run coordinator with the unattended
policy. A crash anywhere in that sequence leaves a `created` row that the next delivery
pass finishes. That is a first send, not a retry. A run that started and stopped is
reported, never rerun. The finished run is delivered into the Bot by the existing
`deliver_finished_runs` path with a `handoff` record. The link card's "Look into it"
submits the same hand-off through the bridge, so there is one way to delegate.

SYSTEM.md gains a short "Bot and research sessions" section: in the Bot answer
directly and briefly, and call `research_open` when the request needs multi-step work
or the user asks for depth. In a session work in depth. Both file what they establish.

### 7. Context delivery

Envelope order is unchanged; contents change:

| Block | Before | After |
| --- | --- | --- |
| new, beside live time | none | `chat_surface`: surface enum and a fixed hint, every turn |
| 6c | standing + newest 40 Lens | `index`: up to 60 page lines (kind, title, ≤120-char summary, item count, updated, up to 3 map names), pages about followed names first, then recently filed, then most used; counts disclose truncation |
| 6e | ≤3 Lens + ≤3 note pointers, lexical | ≤4 page pointers: names in the message resolved through the World index to anchored pages, then lexical |
| 6g | current Chat working note | `chatPages`: up to 6 pointers to pages this Chat filed |
| new 6i | none | `worldKinds`: core names plus extension names with one-line descriptions, ≤1.5 KiB |

Pointers stay discovery, not evidence; exact reads still create receipts.

### 8. Tools and budgets

- `lens_file` replaces `lens_update` and `note_save`. At most 2 calls, 40 ops and 8
  pages per turn. The `memory-save` component absorbs `principle_save` and the new
  `world_schema_update`; the `lens-learning` component is deleted, so one
  `capability_load` exposes every write.
- `principle_save` is unchanged in authority (user-stated only, locked text kept).
- `world_schema_update`: one per turn, Memory write, interactive turns only.
- `research_open`: one per turn, Bot only, never in unattended runs.
- `lens_search` gains `kind` and `changedSince` so "what did you learn this week?" is
  answerable in chat; `lens_read` returns items with ids, anchors and backlinks.
- `conversation_memory` loses note reads; Journal reads stay.

Journal stays the user's record of their own judgments; idea pages are the Loupist's.
`thesis_update` and asset pages are unchanged.

### Interface depth

The model's write surface shrinks from three Lens and note tools with different rules
(`lens_update`, `note_save`, `principle_save`) to `lens_file` plus `principle_save`.
`lens_file` hides staging, basis resolution, idempotent creation, merge, caps, stale
checks and logging; the caller states what it learned and why. The renderer gets
derived views (`memory_learned`, page reads, connection lists) and four intents (keep,
forget, undo, start hand-off); it never computes Memory state.

### What "learns" means, and how each claim is checked

| Claim | Observable record | Test |
| --- | --- | --- |
| A research turn files what it established | `knowledge_lens_log` rows with `origin_message_id` and basis | Rust commit-path test: scripted turn with `lens_file`; stopped turn leaves no rows |
| Research compounds across sessions | a later turn's `knowledge_lens_deliveries` row for a page filed earlier | Rust eval-harness: Chat B's envelope points to the page Chat A filed; paid `compounding-v1` suite grades transfer with the original Sources withdrawn |
| Background filing only cites what the turn read | compile log rows whose basis maps to the turn's receipts | Rust replay: helper output citing an unknown handle is refused whole |
| Methods grow from use | method items with origin `compile` and `lens_read status: applied` counts | compile replay test plus `knowledge_memory_usage` |
| Contradicted knowledge is marked | `item_contested` events and `ItemState::Contested` | Rust: seal a snapshot that withdraws a cited claim |
| New links are found and judged | `knowledge_connections` rows with status history | Rust detector fixture; later a temporal-holdout eval |
| Ontology adapts to evidence | `knowledge_schema_changes` rows with evidence counts | Rust gate test; template seed grader still holds |
| The user's corrections stick | `retire` rows with origin user; refused re-adds | Rust: compile output re-adding forgotten text yields `refused_forgotten` |

None of these claims a measured quality gain. Quality is shown only by paid evals.

### What the design deliberately does not do

It does not rewrite whole pages with a model, run graph algorithms that need training
(GNN link prediction, community summaries), auto-promote speculative schema, let Lens
text become a World fact, call a model when a link is found, run Bash or native file
tools in delegated runs, or keep a second note store.

## Synthesis decision

To be filled by arena.

## Tradeoffs accepted

- We accept a second Lens producer (the compile job, one helper call per qualifying
  turn) in exchange for compounding that does not depend on the chat model's
  discipline. Both producers share one op language and one fold.
- We accept items over free prose in exchange for per-item provenance, conflict-free
  merge and undo. Pages still read as Markdown; summaries can drift from items until
  the later model lint rewrites them.
- We accept losing whole-page optimistic concurrency in exchange for item digests:
  two actors can both add, and only an edit of the same item can collide.
- We accept deleting working notes (a visible behavior) in exchange for one store
  of research knowledge. Chat continuity now rests on the transcript, compaction
  rules and `chatPages`.
- We accept delegated runs on the unattended allowlist (no Bash, no native files)
  in exchange for running without a user present to approve. The user continues in
  the session with full tools.
- We accept auto-applied schema changes that can type some new nodes wrongly until
  undone in exchange for never asking a user to judge ontology. Extraction of
  existing nodes never changes.
- We accept deterministic, structural link detection that cannot explain itself in
  exchange for zero model cost per candidate; explanation costs a model call only
  when the user asks.
- We accept a larger core schema (11 types, 15 relations), which can lower
  per-relation precision, in exchange for holding science, industries and real
  assets without per-Loupist extensions. The template seed grader gates it.

## Alternatives considered

- **Pure Karpathy wiki: free Markdown pages rewritten by the model.** Simple to
  explain and close to the user's wording, but every write needs an exact read and a
  full regeneration, two actors on one page produce proposals nobody reviews, and
  per-fact provenance is impossible, so "learned" cannot be checked. It exposes
  rewrite discipline to every caller and hides nothing.
- **Background-only learning (all writes sleep-time).** Cleaner turns, but "remember
  this" must be confirmed in the reply that the user reads, and user corrections need
  the turn's context. Rejected as the only producer; kept as the second producer.
- **Keep working notes and add file-back on top.** Smallest diff, but leaves two places
  for research conclusions, and the model keeps choosing the one that never compounds.
- **Approval queue for schema (status quo) or speculative auto-apply.** The first rots
  for users who cannot judge ontology; the second biases extraction before evidence.
  Evidence-gated auto-apply with undo replaces both.
- **Store agent interpretation in World (topic annotations, inferred edges).** One
  graph, but it breaks the provenance spine: interpretation would read as a sourced fact
  on the Map. Lens holds interpretation; anchors connect the two.
- **Model-driven discovery (SciAgents-style path sampling with an explainer per path).**
  Richer candidates, but a paid call per candidate in the background and self-scored
  novelty. Deferred behind the deterministic detector and its feedback data.
- **Bot hand-off as a prefilled draft (today's pattern).** No cost risk, but it gives
  the work back to the user, which is what makes the Bot feel like effort.

## Implementation reconciliation

Empty until implementation.

## Open questions and risks

- Should a delegated run start without a second confirmation when the Bot judges depth
  is needed, or only after the user says "dig into it"? This design lets the model call
  `research_open` on an explicit or clearly implied request and tells it to offer
  otherwise. That is a cost and product call for the user.
- Is 24 compile jobs per Loupist per day the right ceiling for helper spend? It is a
  guess sized against `dispatch_inference`'s existing per-call budget.
- Can one agent land the page model (unit 4) in its share of a day? It touches every
  Lens kind enumeration the grounding lists. This is the biggest schedule risk; the plan
  isolates it and keeps sharing exports to methods, principles and Outlook.
- Will the helper decompose answers into faithful items? Basis handles stop invented
  citations, not a wrong paraphrase of a real one. The `compounding-v1` judge must
  score item faithfulness, and Fix/Forget rates are the live signal.
- Deleting working notes changes continuation behavior covered by existing evals; those
  suites must be rerun and their note expectations rewritten, not loosened.
- A larger core schema changes extraction goldens. The template seed grader and the
  World suites decide whether typed share held.

## Next implementation step

Write `knowledge/lens_page.rs` (the `LensBody`, `Page`, `LensItem`, `Section`, `Basis`
and `LensOp` types with a pure `apply` and its unit tests) and bump the schema to 6 with
the single `kind` column, before touching any producer.
