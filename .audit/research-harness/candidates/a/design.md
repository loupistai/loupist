# Candidate A: research feedback inside the existing turn

## Problem

The user wants to ask a research question and receive a defensible answer while Loupist retains useful evidence and develops methods for later work. Teaching methods must be optional. Existing code already supports this delivery path, but its learning reviewer sees new World claims without seeing what a research attempt checked, failed to resolve, or corrected. The smallest coherent shipment extends that path with structured, receipt-backed observations and honest method status. It does not create an agent framework or promise financial alpha.

## Usage, caller's view

The user writes "Which less obvious businesses benefit if grid spending accelerates?" in the existing composer. The agent discovers retained context, reads relevant methods, states a short plan, researches the strongest candidates, and tries to disprove them. It retains decisive originals and a working note when saving is enabled. The final answer names candidate exposures, the causal mechanism, evidence dates, contrary evidence, and an observable invalidation condition. "No supported candidate" is a successful result.

The existing calls remain the main interface:

```js
// Pi uses the existing tool after exact reads and source_save acknowledgments.
note_save({
  workingNote: {
    title: "Grid investment: component bottlenecks",
    markdown: "...complete investigation and qualified candidates...",
    changeSummary: "Checked demand transmission and attempted disconfirmation.",
    expectedResearchId: null
  },
  sourceReads: ["save_primary", "read_counterevidence"],
  lensReads: ["read_method"],
  researchReview: {
    question: "Does grid demand reach these suppliers before capacity expands?",
    checks: [{
      question: "Does backlog convert to cash rather than inventory?",
      disposition: "mixed",
      finding: "Orders grew, but the retained filing also reports slower collections.",
      evidence: ["save_primary", "read_counterevidence"],
      methodRead: "read_method",
      limitation: "Customer concentration and next-quarter collections remain unknown."
    }],
    nextCheck: "Compare cash conversion after the next results release."
  }
});
```

`researchReview` is optional for ordinary factual notes. It describes research evidence and a model's assessment. It cannot set success scores, verified alpha, method confidence, or learned status. Reference strings above stand for acknowledged references returned by this turn, not caller-created IDs.

The Rust completion caller performs one operation through the current Memory transaction:

```rust
persist_saved_turn(/* existing turn, sources, reads, staged research, lenses */)?;
// This now also retains the validated researchReview and admits a learning
// review under existing settings, rate limits, and the World-drained rule.
```

A later chat still discovers methods through the existing catalog and exact-reads the chosen Lens. The compact catalog adds "Candidate method; observed in 2 investigations" as metadata. The method's procedure remains the complete operational document. A saved review does not become a standing instruction.

## Current architecture and constraints

The current `agent_harness.rs` admits bounded bundled and workspace Skills. `agent/skills/research/SKILL.md` already asks for a question, cutoff, stopping condition, relevant methods, and counterevidence. It does not require user teaching. Its current synthesis requirement does not produce structured research feedback.

`commands/chat_runtime/finalize.rs` stops Pi, the broker, and MCP before terminal completion. `completion.rs` takes the writer lease and invokes the completed-turn Memory transaction. Staged Sources, notes, and Lenses become durable together with the completed reply. Stopped turns preserve prior notes. Native file writes survive cancellation.

`commands/tool_executor/authority.rs` validates note references and current-note identity. `StagedResearch` currently stores one working note and acknowledged source/world/lens references. `knowledge/research.rs::ResearchBody` retains the note and its inputs. This is the natural home for feedback. A parallel research database or user Journal is not needed. The Journal records the user's judgment, so an agent-generated candidate must never create one.

`knowledge/learning.rs::Input` currently contains focus, principles, method summaries, and claims. Five added/disputed claims enqueue a review. Reviews wait for World jobs to drain, run at most once per six hours and four times daily, and get one tool-free inference attempt. Publication needs two input claim IDs, respects principles, and handles stale Lens heads as proposals. This establishes provenance, not improved quality. Keep this worker and extend its input and validated evidence alternatives.

`commands/knowledge/chat_reads.rs` filters paused and removed Memory and as-of context. It provides relevant pointers but does not turn discovery into exact-read authority. `world_extract/ontology.rs` generates the extraction ontology from core definitions and the extension. Ordinary equity is presently represented by Organization; other instruments use Asset. That ontology is not explicitly included in Chat's Memory context.

`chat_runtime/watchdog.rs` bounds provider and tool silence. It is not an overall work-duration or spend cap. A more autonomous research policy must not describe this as a total budget.

## Shape

Keep one Pi process and one existing Rust turn owner. Add a bounded `ResearchReviewDraft` to `note_save`, resolve its references at the same authority boundary, and store the validated form in the existing research payload. Feed completed reviews to the existing learning worker along with relevant World evidence. This follows Model the Domain: the disposition and references are typed data, rather than prose fields that every consumer reparses.

The public interface adds one optional field to an existing write tool. It hides acknowledgment validation, provenance resolution, as-of checks, transactionality, and background admission. React only renders the returned read model. The public interface does not expose internal worker phases.

Use a small bounded list of checks, not a general experiment schema. Each check has one question, evidence references, a finding, a disposition, an optional exact method revision, and a limitation. Accept 1–6 checks, a 500-character question, 1,000-character finding, 500-character limitation, and at most 8 references per check. Reject any reference absent from the enclosing note's admitted inputs. A check marked unresolved may have no evidence, but it must state the missing evidence. Other dispositions require at least one evidence reference. None of these statuses verifies the finding's truth.

Store a host observation identity from `turn_id + check_index`. Resolve method read references to exact `record_id + revision_id`; never increment the current Lens revision's observation count when an old revision was used. Repeating completion must not duplicate feedback. Inspection of `research.rs:370` confirms that saving a replacement working note deletes its previous research record. Therefore add one immutable `knowledge_research_experiences` snapshot per completed turn. It owns the structured review and copies the bounded admitted evidence and exact method documents using the existing `JournalInputs` snapshot shape. The ordinary note remains the sole user-facing research document. Its payload stores only the experience ID, not another copy of the feedback. Experiences do not depend on a mutable research ID or an originating Chat remaining present.

Admission uses the existing learning queue and rate limit. Two new completed research experiences from distinct turns admit one pending review even when they did not create five new graph claims. One experience waits without a paid call. Existing claim growth can still admit a review independently. Use an experience sequence watermark separate from the existing growth-event watermark. The pending review coalesces both through the moment of admission, waits for World work to drain, and can run without a World job. Make `world_job_id` nullable rather than inventing a fake graph job. Bound its total serialized input to the existing 12,000 characters. Preserve all active principles first, then include at most four recent research experiences and relevant claim evidence. Overflow defers rather than silently dropping principles. Do not replay a paid or uncertain call.

Learning output adds `evidenceObservationIds` beside the existing `evidenceClaimIds`. Create and refine both require either two distinct claim IDs under the existing rule, or observation IDs from two distinct committed turns with at least one original Source receipt in each cited turn. IDs must be in the frozen input. Both alternatives produce only candidate methods. Do not add a one-observation publication exception in the initial slice. The validator checks provenance, ownership, revision, source eligibility, current controls, and absence of invalid IDs. It cannot validate the financial reasoning from structural fields.

Keep observed use separate from utility. `Candidate` means an autonomous method has a retained provenance trail. `Observed in N investigations` counts exact-revision applications with retained reviews. The first PR adds no evaluation receipt store and no promotion API. Its external held-out evaluation report supplies any measured quality claim. A future product `Evaluated` label would require a host-imported receipt tied to the exact revision, frozen dataset, protocol, comparator, sample size, effect, uncertainty, and date. The model must never write that receipt. Do not introduce a label named `Proven`. Manual methods can be labeled "Added by you" without suggesting measured effectiveness.

Laziness Protocol changes the implementation choice here: extend `StagedResearch`, `ResearchBody`, existing review admission, and the existing Lens read model. Do not add an alternative note tool, alternate finalizer, new job scheduler, score-learning system, or second agent event protocol.

## Schema context and broad assets

Derive a bounded `WorldSchemaContext` from the same `ontology::merged_schema` used by extraction. Include the core definition version, admitted extension version, type names and short descriptions, relation signatures, and identity notes in the Memory turn envelope. Do not hand-maintain a second ontology in a prompt. Changes participate in the context digest so a warm Pi session cannot retain a stale schema unnoticed.

Schema definitions are vocabulary, not financial evidence and not read receipts. Memory off omits the personal extension. Under as-of, omit an extension updated after the cutoff and say that its historic version is unavailable; do not claim that today's extension existed then. Core definitions may be current application vocabulary, with that distinction explicit. Keep the existing proposal UI and validators for schema mutations. New evidence already extends World automatically. A custom ontology is not a prerequisite to research.

Prompt and acceptance cases must distinguish an issuer from a specific equity listing, bond maturity, ETF, currency pair, or commodity contract. Reuse `assets::AssetIdentity` where a known app asset is named. A World Organization alone does not identify the exposure's currency, exchange, seniority, or maturity. For assets the app cannot identify, keep an explicit qualified identity in the research note and mark the missing identifier. Do not invent a ticker or force the answer into an equity-only screen. A full ordinary-share ontology migration is a separate, evidence-backed change and is not required to ship this feedback loop.

## Completion, cancellation, privacy, and cost

Research observations share the completed-turn transaction with the note. Failure, Stop, a conflicting note identity, or a rejected receipt leaves no partial feedback or learning admission. Existing files still survive cancellation. Graph jobs begin only after committed Sources. The agent must not wait inside the turn for those new graph jobs to finish.

Memory off permits ephemeral research and a normal answer, with no hidden retention of observations, candidates, methods, or Sources. Read-only Memory permits discovery and exact reads but no observation writes or background learning from the turn. Setting changes and shared-content taint are rechecked at commit. As-of observations retain the cutoff, and later context cannot expose them before their recorded date. An as-of research run does not update today's asset thesis or trigger current-context method learning. Existing principles retain their documented current-day semantics.

Tool outputs, shared Lenses, sources, and review text remain untrusted data. Preserve the host-only credentials and grants path. Do not copy holdings into review input. Reuse the identity-only projection when asset names are needed. The tool-free learning model receives only bounded admitted material, not the workspace, other Loupists, full conversation history, or tool access.

For the first PR preserve the existing explicit user turn, Stop, tool timeouts, and bounded learning inference. Do not add automatic research restarts, background searches, child agents, or open-ended task continuation. State that existing watchdogs are silence limits, not spend caps. If implementation introduces repeated research rounds beyond the current Pi turn, it must first add a host total-work deadline counted across compactions and native tools. That is a prerequisite to expanded autonomy, not an extra framework to include in this slice. Do not advertise a precise currency cap; provider usage can arrive late. Preserve no automatic paid retry.

## First PR boundary and observable acceptance

Ship optional structured experience on `note_save`, its immutable snapshot, research-only admission into the existing learning worker, bounded derived schema context, Candidate copy in existing Lens views, and research Skill guidance that autonomously retains decisive evidence and reports checks. Keep the existing Pi turn, graph extraction, note reader, and finalizer. Do not ship delegation, an ontology migration, a backtest system, an evaluation-import API, automatic monitoring, or a new task scheduler.

Acceptance requires an end-to-end sequence through the production runner. Run two research conversations with no user lesson. Their completed experiences must cause one candidate method review without five new World claims. Start a fresh Chat with only the retained Sources and unedited learned Lens, use read-only Memory, and ask a held-out question that requires that method. Observe the exact Lens read, correct application to new evidence, a supported answer with a counterexample check, and zero Memory writes. A matched question outside the method's applicability must not use it indiscriminately. This proves autonomous formation and later use. The wider D-minus-B experiment decides whether the method improves answer quality.

## User experience

The composer stays conversational. Do not add a setup wizard, agent selection, schema form, or mandatory method lesson. Use the existing short plan and progress view with plain labels such as "Check who receives the spending" and "Test the strongest counterargument". Show one answer, not transcripts from roles.

The answer leads with the finding and separates qualified hypotheses from established source facts. Each candidate includes the exposure identity, mechanism, dated evidence, competing explanation, and what would change the conclusion. An expandable "Research basis" shows exact sources and methods used. This can reuse the existing Memory receipt affordance.

After commit, the existing Memory change row links to the note and Sources. A learned-method announcement says "Saved a candidate method" and links to Lens. Lens adds Candidate and observed-use metadata, plus a short explanation that observed use is not a quality score. Avoid an intelligence meter, win rate, or ranking by number of methods. Do not announce World additions until World publication actually occurs.

## Alternatives considered

Explicit bounded delegation is structurally viable only as one parent-owned `research_delegate` operation with read-only workers, a shared total budget, independent state directories, and one final writer. Each worker needs a fresh private Pi CWD, bounded instructions, inherited grants narrowed to read operations, no nested delegation, and a typed report that cannot mint parent receipts. A report must contain acknowledged evidence deliveries that the parent re-admits before use. Parent cancellation must stop every child and quarantine uncertain processes before finalization.

That design hides task splitting, but exposes a larger authority and lifecycle contract to the runtime. Reusing the same trusted workspace lets native worker tools write despite a nominal read-only broker policy. It would require narrower native tool admission or separated workspaces, plus handling independent cache seals and evidence ownership. There is no demonstrated quality or latency gain here that pays for that change in the first shipment. Separate Before Serializing Shared State rules out letting children write shared Memory or the same mutable research files.

A prompt-only change is smaller but cannot establish receipt-backed feedback, revision-specific method use, or truthful status. It also cannot enforce total work bounds. It loses because the user asked for autonomous learning that affects subsequent work, not just longer answers.

## Synthesis decision

This is candidate A. Parent synthesis has not selected a base. Recommended base is the current single Pi loop with typed research feedback and no delegation in this PR.

## Tradeoffs accepted

- We accept serial research in exchange for preserving the current authority and commit model.
- We accept model-assessed observations in exchange for retaining useful failure and correction evidence, while declining to treat those assessments as causal proof of method utility.
- We accept candidate methods before outcome evaluation in exchange for learning without user teaching, while keeping their status and evaluation limits visible.
- We accept existing graph retrieval and schema storage in exchange for a small delivery. This proposal does not claim a new GraphRAG algorithm or automatic ontology optimization.

## Implementation reconciliation

No implementation has occurred. Do not treat the sketch as shipped behavior. Changes to review publication criteria, budget enforcement, or as-of learning admission require an explicit update to this file before implementation handoff.

## Questions and risks

Can the runtime enforce tool-start limits for native Pi tools before execution, rather than merely notice the event after it starts? Implementation must answer this from the Pi adapter. If it cannot, describe the bound as an elapsed-time cap and retain per-tool timeouts rather than claiming an exact call ceiling.

Does every existing Memory erase/export/share path account for the new experience snapshots? Implementation must cover these lifecycle paths explicitly; a new learning table must not silently outlive the user's Memory erasure request or expose another Loupist's inputs.

Can a changed method outperform the previous version on held-out questions without the training cases or notes leaking? The existing Lens experiment is the decision gate, not a model's favorable self-review.

## Next implementation step

Add receipt-validated `ResearchReviewDraft` to the existing `note_save` path and prove completion, rejection, cancellation, and read-only behavior before changing learning admission.
