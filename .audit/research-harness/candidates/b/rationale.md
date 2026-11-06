# Candidate B: learn from completed research in the existing loop

## Problem

The user wants to ask ordinary financial questions and have Loupist research, build its World, develop reusable methods, and produce better later investigations. Teaching is optional. The present Source-to-World-to-Lens path retains methods from new claims, but its review input lacks research execution and failure evidence. A second agent does not repair that missing input. The initial implementation should make one research loop complete, preserve its provenance, and let the existing bounded learning worker use that experience. An investment idea remains a hypothesis until a separate outcome evaluation supports it.

## Usage (caller's view)

The user asks, "Which less obvious businesses benefit if grid connection delays persist? Check what could make that wrong." No setup, ontology editor, research-mode selector, or method teaching is required. Loupist discovers relevant saved relationships and methods, reads their evidence, fills important gaps, and tests the strongest alternative explanation. It answers with candidate exposures, the proposed causal chain, dated evidence, a disconfirming condition, and material coverage gaps. "No supported candidate yet" is a valid answer.

The runtime caller still submits the existing Chat intent. Within that turn, Pi retains useful originals with `source_save`, then calls the existing `note_save` with its updated working note and a new optional `completion` object. Rust commits a separate immutable research report alongside the mutable working note and reply. A report records checked claims, considered or applied Lens revisions, counterevidence, unresolved questions, and candidate opportunities. It does not claim that a method helped merely because the model says so. See [usage and interfaces](sketch.md).

After commit, Source processing builds World. A report with eligible evidence can also trigger the existing method review queue, even if it does not produce five new World claims. The review may defer, create a candidate method, or refine an existing method. Later research discovers that method through the current Lens catalog and exact-read path. The receipt says "Saved research" and, when a review publishes, "New method · Not evaluated". Learning never extends the active Chat into an unattended research loop.

## Shape

Keep one Pi execution and one canonical Memory writer per Chat. Add a typed `ResearchCompletion` to the existing note tool and research persistence owner. Store the completion as its own `knowledge_research_records` body with no `workingNote` field. The existing working-note replacement deletes earlier note bodies, so a completion must never be embedded only inside a working note. Freeze report inputs using the same receipt-copy routines already used by `SaveResearch`. This is Model the Domain and Separate Before Serializing Shared State applied to two different retention lifetimes.

Extend `knowledge/learning.rs` rather than adding another learning service. Its input receives bounded report checks and pinned method revisions alongside new World claims. Its output cites typed evidence keys which Rust resolves to supplied inputs. A saved method remains unvalidated. A model assessment, host delivery count, successful transaction, and independently scored improvement are distinct facts. No production tool can assert evaluation success or "proven alpha".

Expose bounded World schema semantics in the Memory envelope, plus an exact `world_schema_read` tool for the complete current schema. Include the schema digest in a research report. Preserve the current issuer/instrument model: ordinary company shares are represented by Organization and ticker attributes; other instruments use Asset. An issuer relationship alone does not identify a tradable instrument or security. Resolve any candidate's instrument through current typed asset identity before giving instrument-specific figures. Defer a core ontology migration to work with its own compatibility and extraction evaluation.

The new public choices are only research content and the existing privacy, cutoff, and Stop controls. Rust hides receipt resolution, immutable report creation, review admission, and provenance. This interface is deeper than exposing separate start, stage, review, promote, and commit commands. Boundary Discipline puts untrusted completion validation at tool admission and the final commit. Laziness Protocol keeps learning and persistence in their current owners. Experience First keeps the extra machinery out of the composer.

## Synthesis decision

This is independent candidate B. The parent architect selects the base and records cross-candidate synthesis. B recommends the existing single Pi loop as the initial implementation. The concrete one-child alternative in [the comparison](sketch.md#one-child-alternative) is a later experiment, not an automatic fallback.

## Tradeoffs accepted

- We accept serial research in exchange for keeping evidence, cancellation, and Memory commits under one established owner.
- We accept reports of model-observed research failures as candidate learning material in exchange for obtaining execution context now. Such reports cannot establish measured utility.
- We accept that World extraction completes after the answer in exchange for preserving transactional retention and keeping the answer independent of background inference latency.
- We accept a small immutable completion record in addition to the working note because the note's destructive replacement cannot preserve a stable learning basis.
- We accept conservative method badges in exchange for avoiding an unsupported claim that learned means effective.

## Alternatives considered

An explicit parent plus one read-only counterevidence child can search independently. It hides parallel execution from the user, but introduces a second tool authority, cancellation tree, model-slot admission, and evidence-delivery boundary. Implementing it before a paired evaluation would spend complexity without evidence that it improves answers. Its concrete contract is specified in the sketch so evaluation can compare whole designs.

Prompt-only changes are smaller but leave no typed connection between a method revision, what was checked, and the evidence behind a correction. A generic autonomous planner, worker fleet, graph editor, or strategy optimizer expands the product before the basic research-to-learning path exists.

## Implementation reconciliation

Design only. No product code changed. Accepting this candidate does not authorize dropping its immutable-report or evidence-delivery contracts. If implementation attaches completion data solely to replaceable working notes, that is a structural deviation that must be resolved before handoff.

## Open questions and risks

Can the held-out Lens comparison show transfer from autonomous experience rather than only retention? Which questions need independent search enough to repay the child worker's extra cost? Those are experiments, not questions for the sleeping user. The initial release makes no improvement or alpha claim while the answers remain unknown.

## Next implementation step

Extend `note_save` with an optional bounded completion, persist that completion separately from the working note in the existing finalization transaction, and prove that a later note update cannot erase it.
