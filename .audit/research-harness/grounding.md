# Research harness grounding

User goal: a simple conversational financial research agent that autonomously builds World and Lens, improves later investigations, and finds evidence-backed non-obvious hypotheses. User teaching is optional. Broad asset support and UX/design are first-class. User authorized autonomous implementation, commit, push and PR; no merge or release requested.

Latest main baseline is 65261d98453959f8a27a241e547c0e87210d1d28. Prior investigation used an older main; recheck touched code. Existing user's checkout and untracked nested loupist are untouched.

Known paths: agent_harness.rs, commands/chat_runtime/, commands/knowledge/chat_reads.rs; knowledge/learning.rs and agent/prompts/method-learning.md; knowledge/world_schema.rs, world_extract/ontology.rs; agent/tool-catalog.json; UI LensWorkspace and Lens panel, chat context/progress; evals/lens and evals/world.

Previous evidence: learning input includes focus/principles/methods/new claims, not observed research outcomes. New/disputed World claims queue a tool-free method review. At least two input claim IDs required to publish a method. This is evidence of retention, not utility or alpha. Current World schema affects extraction, absent explicit schema definition/revision in Chat. Current core folds ordinary shares into Organization; shared asset identity and issuer/instrument semantics remain important. User-facing ontology editor is not the primary product.

Primary research read in this task: GraphRAG https://arxiv.org/abs/2404.16130 shows improvements on global sensemaking, not financial alpha. ACE https://arxiv.org/abs/2510.04618 and MemRL https://arxiv.org/abs/2601.03192 support examining execution feedback and utility, not blindly promoting model self-evaluation. Backtest overfitting https://www.davidhbailey.com/dhbpapers/backtest-prob.pdf informs fair future-outcome checks.

Candidate design must compare extending the current single Pi loop vs explicit bounded multi-agent research. No infinite loop or tool-permission expansion. Consider cancellation/commit, Memory off/read-only/as-of, local-first ownership, evidence receipts, UI simplicity, cost limits, and testable behavior. Existing run and note records should be reused where they fit. No alpha claims from merely saving a Lens.

Task-local model choice: explicit high effort for design/review because this host does not expose active-turn effort. Parent uses available role models per platform mapping without changing user/global settings, under the user's autonomous execution instruction.
