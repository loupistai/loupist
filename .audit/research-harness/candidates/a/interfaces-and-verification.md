# Candidate A interfaces and verification

These are design signatures, not compilable product changes. Existing ownership and names take precedence when implementing.

```rust
// knowledge/model.rs: domain payloads, bounded at the tool boundary.
enum CheckDisposition { Supported, Mixed, NotSupported, Unresolved }

struct ResearchReviewDraft {
    question: String,
    checks: Vec<ResearchCheckDraft>,
    next_check: Option<String>,
}
struct ResearchCheckDraft {
    question: String,
    disposition: CheckDisposition,
    finding: String,
    evidence: Vec<String>,       // wire readRef/saveRef; parsed at authority boundary
    method_read: Option<String>, // exact Lens readRef
    limitation: String,
}

// Resolved form is internal; callers cannot construct authorized references.
struct ResearchObservation {
    id: ObservationId,           // host derives from turn + check index
    loupist_id: LoupistId,
    turn_id: TurnId,
    recorded_at_ms: i64,
    as_of: Option<Cutoff>,
    question: String,
    disposition: CheckDisposition,
    finding: String,
    evidence: Vec<AdmittedNoteEvidence>,
    method: Option<ExactLensRevision>,
    limitation: String,
}

// commands/tool_executor/authority.rs
impl ToolExecutor {
    async fn resolve_research_review(
        &self,
        policy: &ToolPolicy,
        note: &StagedResearch,
        review: ResearchReviewDraft,
    ) -> Result<Vec<ResearchObservation>, BrokerError> {
        unimplemented!("validate limits and references against this turn's acknowledged note inputs")
    }
}

// knowledge/research.rs, private implementation of the existing save operation.
fn save_research(/* current parameters including optional observations */)
    -> KnowledgeResult<SavedResearch> {
    unimplemented!("persist note, immutable observations, and index under caller's transaction")
}

// knowledge/learning.rs owns both trigger paths; one queue, model, and rate limit.
enum ReviewEvidence {
    WorldClaims(Vec<ClaimId>),
    ResearchObservations(Vec<ObservationId>),
}
fn enqueue_research_review(
    tx: &Transaction<'_>,
    loupist: &str,
    completed_turn: &str,
    now: i64,
) -> KnowledgeResult<Option<String>> {
    unimplemented!("check saving and learning controls, skip as-of, coalesce, wait for World, rate-limit")
}

// Read model derived per exact revision. No model-authored utility number.
struct MethodEvidenceSummary {
    origin: MethodOrigin, // user or autonomous candidate
    observed_investigations: u32,
}

// agent_harness.rs, populated from ontology::merged_schema in Rust.
struct WorldSchemaContext {
    core_version: String,
    extension_version: Option<u32>,
    definitions: Vec<BoundedDefinition>,
    identity_rules: String,
    historical_extension_unavailable: bool,
}

// Future prerequisite if automatic extra research rounds are introduced.
// This is excluded from the first PR, which retains one current Pi turn.
struct TurnWorkBudget {
    started_at: Instant,
    external_tool_starts: u32,
    phase: WorkPhase,
}
enum WorkPhase { Researching, Synthesizing { deadline: Instant }, Exhausted }
impl TurnWorkBudget {
    fn before_tool(&mut self, kind: ToolKind, now: Instant) -> ToolAdmission {
        unimplemented!("allow final note/progress operations while synthesizing; deny fresh research")
    }
    fn next_deadline(&self) -> Instant { unimplemented!() }
}
```

## Ownership map

| Owner | Change | Preserved responsibility |
| --- | --- | --- |
| `agent/skills/research/SKILL.md` and references | Autonomous research loop, disconfirmation, qualified candidate answer, stopping condition, review capture | Method selection and synthesis guidance |
| `agent/tool-catalog.json` | Optional `researchReview` on `note_save` | Single tool schema source; derive generated artifacts through existing tooling |
| `commands/tool_executor/authority.rs` and `tool_executor.rs` | Parse, acknowledge, resolve, and stage observations | Turn-local authorization and receipt ownership |
| `knowledge/model.rs`, `knowledge/research.rs`, and knowledge schema migration | Typed bounded payload; one immutable experience snapshot per completed turn; note stores its ID | Canonical notes and retained inputs |
| `commands/knowledge/learning.rs` | Persist observations and request review in existing transaction | One completed-turn Memory writer |
| `knowledge/learning.rs` and `agent/prompts/method-learning.md` | Consume failure/correction evidence; extend evidence validation; candidate-only publication | Durable admission, rate limit, tool-free inference, principle checks, stale revision proposals |
| `commands/knowledge/chat_reads.rs`, `agent_harness.rs`, `world_extract/ontology.rs` | Bounded derived schema and method status context | Cutoff/privacy filtering and stable context digests |
| `chat_runtime` | No new worker or loop in first PR | Rust-owned worker lifetime and stop precedence |
| Existing Lens and Memory UI components | Candidate label, revision-specific observed-use metadata, source links | Conversation-first interaction and native accessibility |

Do not change ownership of credentials, grants, workspace instructions, or Pi session files. Do not add a second generic orchestration service. Each implementation unit must update its owning documentation through `scripts/guide.py`.

## Verification units

1. Exercise real `note_save` acceptance and completed-turn persistence. A valid original receipt plus exact Lens read produces an observation tied to that revision. A forged receipt, cross-Loupist reference, unacknowledged result, or raw source fetch ID is rejected. Repeating completion does not increment observations twice.
2. Stop after acknowledgment and before completion. The previous working note remains, no observation or review is admitted, and the stopped terminal reply persists. Fail the transaction after resolving observations and verify complete rollback. Existing workspace-file survival remains unchanged.
3. Run the same research in Memory off, read-only Memory, and save-enabled modes. Off sends no personal schema or method metadata. Read-only permits retrieval and forbids learning writes. Toggle saving or learning off during an active attempt and verify publication rechecks settings.
4. Save an as-of investigation with later corrections present in storage. Verify neither later note metadata nor later method revision nor later schema extension leaks into context. The as-of observation does not trigger current learning or update today's asset thesis.
5. Use two committed research attempts with a recurring evidence-backed failure, then finish World jobs. One learning review consumes the bounded observations and can create/refine a Candidate method. A single unrelated news item defers. A stale Lens head produces a proposal. Changed principles invalidate publication. A crashed paid attempt is not retried.
6. Read the same Lens revision in later research and then edit the Lens. The old revision's observed use does not become evidence for the new revision. Model output cannot create an evaluation receipt or a proven label. Paused and removed methods remain excluded.
7. Replace a Chat's working note and delete its old research record through the normal save path. Verify its immutable experience remains readable by the learning worker, including admitted evidence snapshots. Then erase that Loupist's Memory and verify the experiences and review input snapshots are removed through the same user-requested lifecycle. No public API may read another owner's experience.
8. Drive native macOS chat and Lens UI. Ask one broad asset research question; observe one short plan, evidence links, a completed Memory receipt, and a Candidate method link after actual publication. Check keyboard navigation, narrow-window wrapping, reduced motion, and `assertFormLegendsFit` where form legends are touched. Browser preview alone is insufficient.

## Evaluation that can justify the claim

Use the existing `evals/lens` A/B/C/D comparison as the baseline. Create independent training conversations that require the agent to discover a reusable method without an explicit lesson or correction from the user. Export the resulting canonical Lens unedited. Keep held-out questions, grading packets, and outcomes outside runtime context and the workspace. Test D minus B with model, reasoning, tools, and budgets held constant. Report task accuracy, evidence quality, inappropriate transfer, correction behavior, latency, token usage, and interval estimates. Report retention and retrieval separately from answer-quality effects.

Add counterexamples: no reusable pattern, misleading correlation, contradictory dates, one-off company event, weak secondhand summaries, bond-versus-issuer confusion, ETF look-through exposure, currency direction, duplicated sources, and no supported candidate. Include Memory-off and read-only controls. An answer that honestly declines an unsupported candidate should score better than a confident unsupported stock list.

For evidence-backed candidate generation, grade whether the mechanism follows from sources, the exposure identity is correct, counterevidence was tested, and invalidation conditions are observable. Do not score a pleasing narrative as alpha. Prospective return claims require a separately frozen candidate universe, timestamped hypotheses, benchmark and horizon specified before outcomes, data availability controls, transaction-cost assumptions, and a holdout untouched by method selection. A small demo cannot establish this. The PR may claim autonomous evidence retention and method formation if behavior tests pass; claim improved future research only if the held-out comparison supports it.

Run the narrow tests per implementation unit, then the repository handoff gate. This candidate performed read-only source inspection, not tests or live evaluation.

## Storage and review admission contract

Add `knowledge_research_experiences` with `sequence INTEGER PRIMARY KEY`, `experience_id TEXT UNIQUE NOT NULL`, `loupist_id TEXT NOT NULL`, `turn_id TEXT NOT NULL`, `created_at_ms INTEGER NOT NULL`, `as_of TEXT`, `body_json TEXT NOT NULL`, and `body_digest TEXT NOT NULL`. Enforce `UNIQUE(loupist_id, turn_id)`. The body contains the question, bounded checks with host observation IDs, next check, and only the admitted `JournalInputs` snapshot entries referenced by those checks. It does not include the full Chat or note Markdown. Derive an index on `(loupist_id, sequence)` for admission and bounded recent retrieval. Derive counts for exact method revisions by scanning the bounded experience payload initially; introduce a projection only if measurements require it.

The same transaction that replaces the note inserts the experience. Repeating the same turn and digest is idempotent; the same turn with another digest is a conflict. The note stores `experience_id` as a link. No foreign key targets the replaceable `research_id`. Memory erasure and Loupist deletion remove snapshots. Chat deletion follows the current retained-note semantics; it does not delete retained research learning. Sharing must either omit experiences or include only explicitly eligible package content under existing rules, never copy them accidentally through broad table export.

Add `through_experience INTEGER NOT NULL DEFAULT 0` to the existing learning review row. Migrate `world_job_id` to nullable and make `Attempt.world_job_id` an `Option<String>`. Existing review output provenance retains its real World job where present and adds an optional experience range. Do not overload a graph job ID with a research turn ID.

At completed research commit, if saving and learning are enabled and `as_of` is absent, count eligible experience sequences after the latest consumed watermark. At two distinct turns, create a pending review if none exists; otherwise leave the existing pending review to absorb them. The existing six-hour/four-per-day limiter and World-drained condition still control paid admission. When admission succeeds, freeze both `through_event` and `through_experience`. The review input owns immutable copies of the admitted bounded evidence. Paid failure, uncertain completion, deferral, and publication consume that admitted range and do not automatically retry it. New experiences can trigger the next review.

The `publish` validator accepts `evidenceClaimIds` and `evidenceObservationIds`. Both arrays are deduplicated and every ID must belong to the frozen input. Publication requires either at least two eligible claims or observations spanning at least two distinct turns, with at least one retained original Source receipt per cited turn. Store the cited IDs in Lens provenance. The original method document fields stay unchanged. All autonomous publications retain Candidate status, regardless of model-written rationale or disposition.
