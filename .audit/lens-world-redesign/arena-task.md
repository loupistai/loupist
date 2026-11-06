# Design task: Lens·World Memory, agent harness, Bot and research sessions

## The user's goal
Loupist is a local-first, global, general-purpose financial and investment research agent app. It never trades. Its differentiator is that the agent updates its own Memory while it does the user's tasks and scheduled tasks, so it gets smarter with use, and the user does not have to manage Memory. Memory means **Lens** and **World** (the loupe metaphor: World is what the Loupist examines, Lens is how it looks).

"Smarter" means that with use these improve:
- the quality of financial and investment research;
- discovery of connections and new alpha that the market, or at least a general-purpose agent app, does not see.

## Scope (redesign and implement together)
1. Lens and World design.
2. User-facing UI/UX.
3. How the agent connects to Memory, and the harness.
4. How the Bot and research sessions interact.

## Fixed decisions
- Lens stays an **LLM wiki** (agent-maintained, interlinked Markdown pages). World stays **GraphRAG** (entity/claim graph extracted from retained Sources).
- The Bot and research sessions stay separate. The Bot should be usable without thought, like the Grok bot: ask anything and it handles it. A research session is the space for expert work.
- The app is unreleased. No backward compatibility, no migrations: bump `KNOWLEDGE_SCHEMA_VERSION`, reshape tables and delete obsolete paths freely.
- Rust owns credentials, persistence, Memory transactions and worker lifetime. React submits typed intent through `apps/loupist/src/bridge/tauriBridge.ts`. Model tool writes stage during the turn and commit with the completed reply; failed or stopped turns save nothing. Tool contracts live in `apps/loupist/agent/tool-catalog.json`. Keep the read/write permission model, the per-turn broker budgets, the as-of cutoff rules and the shared-content wrapping.
- No automatic paid retries. Background model work runs on the existing isolated, tool-free helper inference path with rate limits.
- English and Korean UI. Product copy avoids ontology jargon.

## Requirements
- Generality: World must hold any asset class (equities, crypto, real estate, alternatives and more) and knowledge outside market data. Example: semiconductor process science matters when analysing upstream and downstream value chains.
- Ontology onboarding: ordinary users do not know what an ontology is. A non-binding idea: ordinary users pick a preset, power users shape the ontology by talking to the agent. A better direction is welcome.
- Agent-era UX: people rarely configure or write documents themselves; agents do, and people ask agents in chat. Design how chat flows into Lens and World, and how the user views what has accumulated.
- Review the Bot/session interaction and improve it.
- Fix wrong behavior found along the way.

## Grounding (read these first)
- `.audit/lens-world-redesign/grounding.md`: current code facts with file:line pointers. Paths are under `apps/loupist/`.
- `.audit/lens-world-redesign/research.md`: current external methods and open-source systems with links.
- Owning docs: `docs/knowledge/worlds.md`, `docs/knowledge/lens.md`, `docs/knowledge/memory.md`, `docs/agent/context.md`, `docs/agent/context-delivery.md`, `docs/agent/turn-tools.md`, `docs/ui/home-thread.md`, `docs/ui/map-lens.md`.
- Read the code you rely on. Grounding notes are pointers, not proof.

## Problems the grounding found
1. Memory grows only through agent judgment per turn. Research conclusions land in a per-Chat working note that is replaced and never compounds. The LLM-wiki "file the answer back" operation is missing.
2. Background method learning never sees research; it needs five new World claims. Its output publishes straight to Lens.
3. Lens has no subject pages (concepts, sectors, value chains, theses). Lens bodies cannot point at World entities. Kind is stored three ways.
4. The World schema is invisible to the Chat agent and cannot be shaped through chat. Model-written ontology proposals wait for a user who cannot judge them. The core lacks types for technology/science knowledge, real assets and private assets; the asset-class vocabulary disagrees with `AssetClass`.
5. Nothing surfaces new multi-hop connections between what the user follows and new knowledge.
6. The model is never told it is the Bot. The Bot cannot hand deep work to a research session. The Bot never shows what the Loupist learned.
7. Bugs: followed-asset matching ignores market (`knowledge/map.rs:189-194`, `knowledge/assets/mod.rs:1562-1563`); MemoryPage prints "Thread"; perspective receipts dropped; Lens title collision silently drops a create.

## Deliverable
Write a candidate design package in your output directory:
- `design.md` shaped per the architect rationale template (Problem, Usage, Shape, Tradeoffs accepted, Alternatives considered, Open questions and risks, Next implementation step). Write the caller's usage first: the model's tool calls in a real research turn and a scheduled run, and the user's view in the Bot, a research session and the Memory views.
- `types.md`: Rust and TypeScript type sketches and function signatures for the new or changed data shapes, tool contracts (JSON schema sketches for `tool-catalog.json`), SQLite tables, and the module map. Bodies are `todo!()` / pseudocode.
- `plan.md`: the implementation split into small, independently verifiable units in dependency order. For each: files, the behavior test that proves it (real boundary: Rust test at the broker or commit path, vitest with the mock bridge, headless eval contract), and what it deletes. Mark which units can run in parallel without touching the same files. Keep the whole plan buildable in roughly one working day by a few agents; push anything larger to an explicit "later" list with the reason.

Constraints on the design itself:
- Smallest surface that meets the goal. Prefer extending one existing path over adding a second way to do the same thing. Delete what the new shape replaces.
- Name the data shape first and choose its organizing structure (typed model, registry, state machine) instead of scattered conditionals.
- Every claim that the Loupist "learns" must be checkable: say what observable record proves it and what test or eval would show it.
- Do not edit repository files outside your output directory.

## Later user decisions
- Keep the World as a visible graph (the Map); it is the product's viral point. Graph interaction and how evidence and materials are shown may change.
- Not decided by the user: whether model-proposed ontology changes apply automatically. The orchestrator's default is to apply them after host validation, visibly and reversibly. Argue for the best shape.
