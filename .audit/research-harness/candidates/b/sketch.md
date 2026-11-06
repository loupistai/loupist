# Candidate B usage and implementation contract

## Existing runtime ownership

`agent_harness.rs` materializes bundled Skills and bounded host context. `commands/chat_runtime/` launches one warm or cold Pi process and streams native tools and Rust broker calls. The runtime CWD is private. Native file and Bash tools target the trusted workspace with host permissions. Automatic project settings and JavaScript extensions remain disabled.

`commands/knowledge/chat_reads.rs` builds discovery context and applies cutoff and set-aside rules. Search and supplied pointers are not read receipts. Exact deliveries bind the delivered bytes and revision to the consuming turn. `commands/chat_runtime/completion.rs` reserves the FIFO Memory writer before dropping the Chat reader, then commits the completed reply, staged Sources, Lens changes, research, receipts, citations, and artifacts through one host transaction. Failed and cancelled turns do not publish staged Memory. Native workspace changes survive.

`commands/knowledge/learning.rs` retains Sources and queues World processing during that transaction. `knowledge/learning.rs` watches added or disputed World claims, waits for World jobs to drain, and performs a tool-free review at most once per six hours and four times per rolling day. Publication currently needs two supplied claim IDs. The prompt asks for reusable reasoning, but the input has focus, principles, method summaries, and claims rather than method execution outcomes. Learned methods already enter later research through Lens discovery and exact reads.

`knowledge/research.rs` is the correct report owner. It already freezes Source, World, and Lens inputs in `ResearchBody`, and provides observations, interpretation, assumptions, and evaluation fields. Its `workingNote` variant keeps one current note per Chat by deleting previous working-note rows. Content-free `runs/log.rs` diagnostics must remain content-free. It is unsuitable for research reports or learning evidence.

## Caller usage first

These are illustrative wire calls, not additional user controls.

### A completed research question

```json
{
  "tool": "note_save",
  "workingNote": {
    "title": "Grid delays and equipment substitution",
    "markdown": "Current synthesis, dated evidence, counterevidence, and open checks.",
    "changeSummary": "Compared order conversion with backlog claims.",
    "expectedResearchId": "rsh_current_note"
  },
  "sourceReads": ["saved_original_a", "read_original_b"],
  "worldReads": ["read_relationship_chain"],
  "lensReads": ["read_method_revision"],
  "completion": {
    "question": "Who could benefit if connection delays persist?",
    "checks": [{
      "question": "Do reported orders convert into cash?",
      "assessment": "contradicted",
      "observation": "Backlog rose while dated cash receipts fell under the disclosed definition.",
      "sourceReads": ["saved_original_a", "read_original_b"],
      "lensRead": "read_method_revision"
    }],
    "counterevidence": "The alternate supplier also faces qualification delays.",
    "gaps": ["No independently disclosed market-share series."],
    "learning": {
      "reason": "The existing backlog method missed cash conversion and qualification lead time.",
      "checkIndexes": [0]
    },
    "candidates": [{
      "subject": "Company A",
      "assetIdentity": null,
      "hypothesis": "Its qualified equipment could substitute for a constrained part.",
      "mechanism": "Substitution matters only where technical qualification is complete.",
      "falsifier": "Customer qualification fails or order conversion weakens.",
      "sourceReads": ["saved_original_a", "read_original_b"],
      "limitations": "Business exposure is supported; instrument-specific valuation remains unverified."
    }]
  }
}
```

The returned response remains staged and `durable: false`. Rust, not Pi, attaches the run, turn, profile, date, schema digest, exact evidence snapshots, and cited Lens revision. A later completed reply returns the committed report ID. The report's `assessment` is a model observation even when its cited facts validate. A candidate's `subject` is not a trade instruction or an assertion of investment merit.

### Later research

The existing `lens_search` finds the method. `lens_read` returns the complete current method with an explicit `not_evaluated` assessment. Pi can mark the revision applied, considered, or revise it through `lens_update` after exact reading. A `conversation_memory` report selector supplies the earlier completion when its failure or evidence can change the answer. Search excerpts and report title pointers alone grant no evidence authority.

```json
{"tool":"conversation_memory","researchId":"rsh_completed_research"}
```

This read uses the existing Loupist, Memory, ownership, and cutoff checks. It returns the report plus its frozen inputs. Originals remain distinct from the agent's interpretation.

### Private or dated research

The caller submits the same Chat intent with the existing flags and as-of date. Memory off supplies no report pointers or World extension; write tools refuse. Read-only Memory allows exact reads and candidate analysis, but refuses report, Source, and Lens writes. A dated turn uses only eligible evidence and methods. It may retain its dated working note under current behavior, but the new report cannot trigger autonomous method learning in this initial release. The host sets that eligibility, never the model.

## Domain sketch

The following signatures are pseudocode for proposed additions. They reuse current IDs, `LensDocument`, `KnowledgeWrite`, `SaveResearch`, and receipt-copy code. They do not create a second database or exported generic agent framework.

```rust
// Tool adapter parses bounded JSON into this draft. References are still
// untrusted identifiers until the host binds them to this turn's deliveries.
struct ResearchCompletionDraft {
    question: String,                     // 1..500 characters
    checks: Vec<ResearchCheckDraft>,       // 0..8
    counterevidence: String,              // <=2000
    gaps: Vec<String>,                    // <=8, each <=300
    candidates: Vec<OpportunityDraft>,    // <=5
    learning: Option<LearningReasonDraft>,
}

struct ResearchCheckDraft {
    question: String,
    assessment: ModelCheckAssessment,
    observation: String,
    source_reads: Vec<SourceReadRef>,     // 1..3 for non-unresolved checks
    lens_read: Option<LensReadRef>,
}

enum ModelCheckAssessment { Supported, Contradicted, Unresolved }

struct OpportunityDraft {
    subject: String,
    asset_identity: Option<ExistingAssetIdentity>,
    hypothesis: String,
    mechanism: String,
    falsifier: String,
    source_reads: Vec<SourceReadRef>,
    limitations: String,
}

struct LearningReasonDraft {
    reason: String,                        // <=500
    check_indexes: Vec<usize>,             // index into this report, <=8
}

// Host creates this from acknowledged inputs. Every field is bound to the
// completed turn. Native files are never silently treated as retained originals.
struct ResearchCompletion {
    report_id: ResearchId,
    origin: CompletedTurnIdentity,
    recorded_at: Timestamp,
    research_cutoff: Option<Cutoff>,
    schema_digest: Option<Digest>,
    body: ResearchCompletionDraft,
    inputs: FrozenResearchInputs,
    learning_eligibility: LearningEligibility,
}

enum LearningEligibility {
    Eligible { evidence: NonEmpty<CompletionEvidenceKey> },
    Excluded { reason: LearningExclusion },
}

enum LearningExclusion {
    MemoryDisabled, LearningDisabled, HistoricalTurn, NoReusableReason,
    InsufficientOriginals, UntrustedResultPolicy,
}

// knowledge/research.rs owns exact binding, immutable report insertion and
// retrieval. Existing working-note persistence remains in the same transaction.
fn save_research(
    write: &KnowledgeWrite,
    request: SaveResearch,                 // adds optional completion draft
) -> Result<SavedResearch>;                // adds optional completion report ID

fn read_completion(
    tx: &Transaction,
    scope: AuthorizedMemoryRead,
    id: ResearchId,
) -> Result<ResearchCompletion>;

// learning.rs owns all admission, inference input and method publication.
fn enqueue_completed_research(
    tx: &Transaction,
    report: &ResearchCompletion,
    now: Timestamp,
) -> Result<Option<ReviewId>>;

enum LearningEvidence {
    WorldClaim(ExistingClaimInput),
    ResearchCheck(FrozenCheckInput),
}

struct ReviewInput {
    // Existing focus, all active principles and method summaries remain.
    principles: Vec<String>,
    methods: Vec<ExistingMethodSummary>,
    evidence: Vec<LearningEvidence>,
}

struct MethodAssessment {
    status: MethodAssessmentStatus,
    // First implementation only emits NotEvaluated. A later independent eval
    // integration must bind a result to an exact revision and frozen cohort.
}
enum MethodAssessmentStatus { NotEvaluated }
```

The `SourceReadRef` and related draft reference wrappers are parsed at the broker boundary. The public app bridge keeps typed Chat intent. It never exposes SQLite rows, review payloads, provider process handles, or mutation authority.

## Persistence and learning contract

Keep `note_save`'s current required working note, add the optional completion, and let a substantive research finish save both in one call. The host allows at most one completion per turn. Repeated identical staging returns its existing staged identity; a changed draft replaces only the uncommitted draft. Commit retries must return the already committed completion rather than insert a second one. The current completed-message recovery path supplies the idempotency boundary. The report's source and method snapshots are separate from the working note's replaceable row.

`save_research` inserts the existing note and a second research row for its completion in one transaction. The completion row has no `workingNote` object, so the current replacement query cannot erase it. Add a typed body discriminator for report reads, or an optional `completion` field with strict mutually exclusive body validation. Do not infer report type from a Markdown heading. A report may reference the current note as navigation, but never requires that row to remain available.

Reports are research interpretations, not Sources or World factual claims. No report is fed into Source extraction as an original. A learning review can consume a report check and its frozen exact inputs. Its output cites supplied evidence keys. Rust resolves those keys, verifies ownership and scope, and checks that the candidate's basis includes at least two distinct supplied evidence items. For report-triggered publication, require at least two distinct retained originals across that basis. This proves a basis was supplied, not that publishers were independent or that the proposed method works.

Reuse one pending review per Loupist, one inference-call budget, World-drain admission, six-hour cooldown, daily cap, principle checks, revision conflict handling, and uncertain-attempt recovery. A report can wake that queue without waiting for five new World claims. Queue progress needs a report watermark in addition to the existing World growth watermark. Snapshot admitted report IDs in the review input and mark them consumed only as that attempt settles. A failed paid attempt is not retried automatically; later new evidence can create a new bounded review with the previous failure visible.

The same method publication owner handles both claim-triggered and report-triggered reviews. Existing claim-only publication remains available. A report-triggered review can create a method when research exposes a repeatable pattern, or refine a supplied method when a checked assumption fails. A model saying "this was helpful" never changes method assessment status. Revisions reset any future evaluation badge because evidence for an earlier revision cannot prove the changed one.

The first release labels all methods as not independently evaluated. Origin remains separately user-taught, agent-derived, or shared according to existing provenance. Do not turn origin, `applied`, delivery counts, or the two-evidence publication gate into quality scores. If origin cannot be determined precisely for an old record, preserve unknown origin instead of guessing.

## Schema awareness and asset coverage

Add `worldSchema` to the existing Memory envelope with extension version, digest, core entity names and short meanings, active extension names, and relation signatures. Bound the summary and report when it is partial. Add `world_schema_read` under Memory read permission to return the complete schema in bounded pages from the same schema owner. It is definition context, not a factual source receipt. No new schema mutation tool is needed for this initial package.

Under a cutoff, omit an extension whose `updated_at_ms` is later than the cutoff because the current schema has no historical revision contract. The bundled core semantics remain available through the World Skill. With Memory off, omit the per-Loupist extension entirely. Schema digests enter the context envelope digest so a changed schema invalidates stale warm delivery.

Do not promise global financial coverage merely because the ontology has Asset. Test equity exposure, ETF constituents versus manager, bond issuer versus issue, currency pair direction, commodity contract versus spot series, and crypto asset versus exchange. Candidate reports may name an exposure while `assetIdentity` remains unresolved. A security-specific price, valuation, yield, or expected-return comparison requires a resolved existing identity and dated data. Unsupported data coverage is a reported gap.

## Research behavior and user experience

Update the research Skill and Lens learning instructions around the existing loop. The agent identifies the question and cutoff, discovers relevant World and Lens, exact-reads evidence, names the result that could overturn its current view, and investigates the important missing link. It does not execute a fixed menu for every question. Before completing substantial research, it saves useful originals and a completion when Memory permits. It may propose a new method through the review path without user teaching.

A candidate opportunity needs an exposure or instrument, mechanism, dated supporting evidence, the strongest found counterevidence, a falsifier, and the remaining data gaps. A chain of graph edges is a search lead. It becomes an answer claim only after relevant originals are read. Novelty means "a connection found in this investigation" unless a benchmark actually establishes novelty. Expected excess return and proven alpha require separate data and evaluation.

The visible answer leads with the finding. Existing progress labels can say "Checking the alternative explanation" and "Saving research". The existing receipt opens the working note and includes "2 checks saved" when the host committed a report. Background World work retains "Reviewing sources…". A learned-method publication links to its current document with "Not evaluated" beside the title. No graph-building wizard, extra research dashboard, self-improvement score, or subagent roster is added.

Use the current rail and Lens reader for this information. Changes need English and Korean copy, reduced-motion behavior, keyboard access, narrow-width layout, and native WKWebView inspection. Do not use a new fieldset grid for receipt details without measuring legends in WKWebView.

## Boundedness and cancellation

The initial package adds no autonomous re-entry, no worker that starts a new Chat, and no loop that runs until a return target is met. The research Skill has a stopping condition tied to answering the user's question and naming material gaps. Existing provider, native-tool, and worker controls remain intact. The review's existing hard inference and time budgets remain the only background learning execution.

If this task also introduces a new long-research mode, it must carry a host-enforced elapsed-time and acquisition budget, expose the remaining budget to Pi, reserve time for synthesis, and stop rather than restart itself when exhausted. Do not claim that the current provider-idle watchdog is such a total-cost budget. The present design does not need that additional mode to provide autonomous method learning.

Cancel before the existing completion claim publishes neither report nor note nor staged Source nor Lens. Cancel after completion has been claimed follows the current linearized completion contract. The result must never say "learned" for a staged-only report. Workspace files retain their existing immediate behavior. A disabled learning preference before background publication defers rather than publishes. Revoked Memory or a custom result's Don't save taint suppresses every report and learning admission, including drafts staged earlier in the turn.

## One-child alternative

An alternative initial design gives the parent a single `research_delegate` tool for a counterevidence question. Rust admits at most one child per parent turn, with no nesting. The parent supplies a bounded question, cutoff, a subset of its evidence, and the role `counterevidence`. Rust inherits the locked execution model, principles, set-aside list, grants, and privacy restrictions. The child gets only approved read-only broker tools, no native file or Bash tools, no Memory writes, no scheduling, no grants, and no user controls. The child cannot acquire access the parent lacks.

The child has its own private runtime directory, capture, process registration, and a fixed acquisition and inference budget. A reasonable evaluation preset is one child, eight acquisitions, 120 seconds, and a 4,000-token result ceiling. These are proposed experiment settings, not claims about existing limits. It consumes a real model worker slot. Slot exhaustion returns an explicit unavailable result and the parent continues serially; it never starts an unregistered process. No shared mutable note or Source staging queue exists between parent and child.

Return a bounded finding plus selected exact evidence excerpts, dates, origin handles, and coverage. Rust issues parent-turn receipts only after Pi acknowledges that result and only for the exact original bytes actually delivered in it. A summary saying "the child read X" grants no receipt for X. If the evidence cannot fit, return navigation handles and require the parent to exact-read the original through existing tools. Never transplant a child's receipt IDs into a parent turn.

All child trust taints propagate conservatively to the parent. Existing site and custom-connector approvals can only be satisfied by parent-authorized grants; the child returns a blocked lead rather than opening a second approval UI. Rust cancellation first stops both processes, waits for confirmed child settlement, then permits the parent's finalization transaction. A child failure yields a visible research gap, never fabricated counterevidence. Only the parent stages durable Memory.

This alternative is structurally viable, but compare it against the serial version on frozen questions with the same total acquisition, token, model, and elapsed-time budgets. Measure whether extra independent search finds stronger counterevidence and improves the final answer. Do not compare an unconstrained pair of agents with a constrained single agent and call the difference an architecture gain.

## File ownership and implementation units

1. `agent/tool-catalog.json`, `commands/tool_executor/authority.rs`, `knowledge/model.rs`, and `knowledge/research.rs` gain completion admission, receipt binding, independent report persistence, exact reads, and idempotency. Preserve the current `note_save` behavior when completion is absent.
2. `commands/knowledge/learning.rs` and `knowledge/learning.rs` gain report admission and a unified evidence input. Extend the current learning review persistence and queue rather than introducing a second scheduler. `agent/prompts/method-learning.md` receives supported failures and candidate-only wording.
3. `commands/knowledge/chat_reads.rs`, `agent_harness.rs`, the World schema command owner, and `agent/tool-catalog.json` gain bounded schema discovery and exact definition reads. Current schema validation and mutation ownership stays in Rust.
4. `agent/skills/research/SKILL.md` and `agent/skills/lens/SKILL.md` explain autonomous research, evidence-backed completion, candidate opportunities, and later reuse. Update the existing owning docs in `docs/agent/context-delivery.md`, `docs/knowledge/lens.md`, `docs/knowledge/memory.md`, and `docs/ui/chat-composer.md` with actual shipped behavior.
5. The current Memory receipt, Lens reader, and localized strings gain report-check counts and unvalidated-method wording. Update the actual owning components after following current references, rather than duplicating them in a new workspace.

No generated schemas, build output, credentials, or native permissions change. The deliverable is a finite new path through the current product, not a general research platform.
