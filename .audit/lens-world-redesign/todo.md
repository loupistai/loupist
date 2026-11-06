# Lens·World memory and harness redesign

Goal: the Loupist updates Lens (LLM wiki) and World (GraphRAG) by itself while it does
the user's tasks and scheduled work, so research quality and non-obvious connections
improve with use. Bot stays the low-effort entry; research sessions stay the expert space.

## figure-it-out
- [x] Read the Principles section of the poteto-mode skill.
- [x] Phase A: Frame. Definition of done as a falsifiable predicate, scope quantified, rigor level.
  - Done when, on the integrated branch: (1) `check.sh rust` and `check.sh web` pass (baseline main: 1137+2 Rust tests green); (2) behavior tests exist and pass at real boundaries for: lens_file commits with the reply and a stopped turn files nothing; disjoint section edits merge, overlapping ones are skipped and logged; forgotten sections cannot be re-added by the Loupist; ambiguous followed symbols match nothing; vocabulary changes apply, are evidence-gated for review, and undo; research_open creates one session and one delegated run per request key and posts back to the Bot; a new two/three-hop link to a followed entity is detected once; the envelope carries chat_surface, the page index, anchored pointers and worldKinds; (3) the renderer shows filed pages under an answer with Forget/Undo, Pages grouped by kind, Map Notes, and Bot cards (hand-off, link, vocabulary), verified in vitest and a browser preview capture; (4) native WKWebView not claimed unless run.
  - Scope: ~6 units, ~25 engineering hours across 3-4 agents; blockers: disk (25 GB free), shared files across units.
  - Rigor: high. Schema v6 and tool contracts are one-way for user data; every unit ends in tests at the commit/broker boundary.
- [x] Phase B: Design the workflow. Atomic units, riskiest-unknown-first, verification harness before work, architect for one-way doors, fan-out plan, written phase list.
- [x] Phase C: Run the loop. Hypothesis, smallest change, measure on the real artifact, keep or revert.
- [x] Phase D: Keep the audit trail (show-me-your-work).
- [x] Phase E: Verify and hand back (native WKWebView and paid model runs not exercised; see follow-ups.md) against the Phase A predicate on the real product.

## Opening a PR
- [x] Worktree off main.
- [x] Commits small and ordered.
- [x] /deslop over the diff before commit, /no-comments before review.
- [x] Title, description, commit bodies via /technical-writing then /unslop.
- [x] Forge resolved (gh unless origin resolves).
- [ ] Open ready, not draft. Post the URL.

## Decisions
- 2026-11-06 user: do not reference PR 36 (codex/autonomous-research-harness). Base on main and design independently.
- 2026-11-06 user: unreleased, no backward compatibility needed.
- 2026-11-06 user: keep the World graph view (viral); graph interaction and evidence display may change.
- 2026-11-06 user agreed: model ontology proposals auto-apply after validation, visible and undoable.
- 2026-11-06 correction: the user did NOT agree to auto-apply; it remains the orchestrator's default, open to change.

## Throughput checkpoint
- [x] Blocking first steps: design contract (`.audit/lens-world-redesign/design.md`) committed on the branch; Rust baseline 1137+2 passed on main.
- [x] Independent workstreams: wave 1 = Lens core + lens_file (fable), World vocabulary + identity (sonnet), Bot surface + hand-off (fable), each in its own worktree and COW cargo target; wave 2 = renderer UI (sonnet) after Lens DTOs, discovery (sonnet) after Lens + World merge; wave 3 = integration, docs, full gate, native check (parent).
- [x] Shared mutable state: tool-catalog.json, broker.rs, agent_harness.rs, SYSTEM.md, completion.rs, knowledge/schema.rs are touched by several units; each unit edits only its own entries, the parent merges serially and resolves hunks. knowledge/schema.rs: Lens unit owns Lens tables, World unit owns schema-change table; both bump to 6.
- [x] Smallest safe decomposition: three Rust owners at once is the CPU/RAM/disk ceiling (8 cores, 16 GB, 25 GB free); UI and discovery wait for their inputs.
