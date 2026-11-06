# External research (as of 2026-11-06)

Two web research passes. Links are as reported by the research agents; "(secondary)" marks press or blog coverage. Abstract-only reads are flagged.

## A. Finance agents, alpha discovery, identity, memory UX

### Financial research agents
- Open frameworks simulate a desk more than they accumulate knowledge. TradingAgents (https://arxiv.org/abs/2412.20138, repo https://github.com/TauricResearch/TradingAgents) appends each decision to a markdown memory log, later settles it against realized outcome and writes a one-paragraph reflection the next run reads. FinMem (https://arxiv.org/abs/2311.13743) ranks layered memories by recency, relevance, importance. FinCon updates "investment beliefs" by self-critique.
- FinRobot V2 (https://github.com/AI4Finance-Foundation/FinRobot): numbers are code-calculated with provenance; the LLM narrates.
- R&D-Agent(Q) (https://arxiv.org/abs/2505.15155) loops research/development with a bandit choosing direction. AlphaAgent (https://arxiv.org/abs/2502.16789) penalizes complexity, checks hypothesis-factor alignment, enforces originality against existing alphas.
- Graph memory: FinDKG (https://arxiv.org/abs/2407.10909) dynamic graph of LLM-extracted quadruples; GraphRAG claims/covariates carry status and time bounds (https://microsoft.github.io/graphrag/index/default_dataflow); Zep/Graphiti (https://arxiv.org/abs/2501.13956) bi-temporal edges, invalidation without deletion.
- Benchmarks: retrieval precision and freshness are the bottleneck. FinanceBench (81% wrong/refused for GPT-4-Turbo+RAG), Finance Agent Benchmark (https://arxiv.org/abs/2508.00828; best ~64% on Vals v1.1), FinSearchComp (https://arxiv.org/abs/2509.13160; no-search models score 0 on time-sensitive tasks), FinDeepResearch (https://arxiv.org/abs/2510.13936; best 37.9/100, weakest at precise multilingual retrieval).
- Commercial: AlphaSense Deep Research with sentence-level citations, one-click and scheduled workflow agents; Bloomberg ASKB (Feb 2026); OpenBB Workspace agents cite widgets used. No commercial product found that shows users an agent-maintained knowledge graph across tasks (absence finding).

### Knowledge-graph alpha and validation
- Economic links: Cohen & Frazzini customer momentum ~1.55%/month, 78% of links cross industries (https://www.aqr.com/Insights/Research/Journal-Article/Economic-Links-and-Predictable-Returns). Technology links via patents (Lee, Sun, Wang & Zhang, JFE 2019) ~117bp/month EW, strongest where attention is low.
- Many "new" links are rediscoveries: shared analyst coverage subsumes industry, customer/supplier, geographic and technology momentum (Ali & Hirshleifer, JFE 2020). Hoberg-Phillips TNIC peers (https://hobergphillips.tuck.dartmouth.edu/).
- 2026 LLM-graph papers are early, small samples (abstracts only): https://arxiv.org/abs/2604.19476, https://arxiv.org/abs/2607.15640 (88% of sampled extracted edges confirmed; refuted edges in the lowest-weight tail), https://arxiv.org/abs/2607.10932 (simulation only).
- Overfitting controls: t > 3.0 for new factors (Harvey-Liu-Zhu https://nber.org/papers/w20592), Deflated Sharpe Ratio needs the count of ideas tried (https://papers.ssrn.com/abstract=2460551), post-publication decay (McLean-Pontiff).
- LLM risks: look-ahead leakage from pretraining (https://www.nber.org/papers/w35247); memory poisoning via queries alone, MINJA 98.2% (https://arxiv.org/abs/2503.03704).

### Identity across asset classes
- Issuer vs instrument vs listing: FIGI share-class / composite / exchange levels; FIBO `Listing` is an exchange catalog entry (https://github.com/edmcouncil/fibo); GLEIF LEI-ISIN mapping and Level 2 parents.
- Crypto: ISO 24165 DTI (token) vs DLI (ledger). Real estate: RESO Data Dictionary (residential listings); no open REIT-to-property ontology found.
- Science to value chain: ETO chip supply-chain dataset (https://eto.tech/dataset-docs/chipexplorer) models stage → input (process, material, tool, design) → provider with `goes_into`, `is_type_of`, and share provided by year.

### Memory UX
- ChatGPT memory: saved memories + chat history reference; temporary chats skip memory; project-only memory; Pulse (https://openai.com/blog/introducing-chatgpt-pulse) does overnight research and delivers a few morning cards with feedback/curate, expiring unless saved.
- Claude memory (https://claude.com/blog/memory): one viewable/editable summary per project, correctable by chat, incognito skips memory.
- Grok: memories viewable/removable in settings; @grok in a reply explains the post in context (TechCrunch, secondary).
- NotebookLM: user decides sources; a Deep Research report can be imported as a source; notes keep citations. Perplexity Tasks run inside a Space. Notion 3.0 agent memory as pages. Mem 2.0 resurfaces notes proactively.
- Karpathy LLM Wiki (https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f, April 2026): raw sources → LLM-compiled interlinked pages, schema file sets the conventions; operations ingest, query (file good answers back), lint (contradictions, uncited claims, stale pages, orphans).
- Recurring patterns: memory as a visible, editable artifact; scoped to a container; off-the-record mode; proactive feed with feedback; handoff by moving a thread into a container with its own instructions and files.

### Onboarding
- Products configure with natural-language instructions plus a few typed fields and templates (GPT builder conversational + fields; Gemini premade Gems; AlphaSense prebuilt agents). None asks users to pick ontology classes; progressive disclosure.

### Recommendations from pass A
- P0: claim as the unit of World with event time, ingest time, status, author; identity layer (issuer / instrument / listing / venue / real asset) before the graph; Lens as an LLM wiki over World with lint on the briefing schedule and file-back of good answers; a "What I learned" diff card after every task and scheduled run with Keep / Fix / Forget; gate Memory writes (agent claims need a retained source span; instruction-like source text never becomes memory); numbers computed in code; Bot → "Open as research session" carrying summary, entities and relevant Lens pages, results return through the same diff card; onboarding as one conversational question plus preset chips, never ontology terms.
- P1: connection-to-hypothesis cards (mechanism, link type, expected sign and lag, falsifier, baselines, registration date, forward tracking, count of ideas tried); value-chain templates in the ETO shape.
- Defer: GNN/community propagation signals, full FIBO reasoning, DeFi ontologies, debate agents as Bot default, fine-tuning.
- Avoid: trading-simulation framing, backtests without point-in-time controls, ontology terms in UI, silent memory writes.

## B. Memory methods, graphs, ontology induction, discovery

### LLM wiki
- Karpathy LLM Wiki (gist above, 2026-04-04): raw sources (immutable), LLM-owned Markdown wiki, schema file. Ingest updates summary, entity and concept pages, then index and log (one source can touch 10–15 pages). Query answers with citations and files good answers back as pages. Lint: contradictions, stale claims, orphans, missing pages and cross-references, data gaps. `index.md` (catalog, one line per page, read first) and append-only `log.md`. Index navigation holds to ~100 sources / hundreds of pages; beyond that, search (BM25 + vectors + rerank).
- Derivatives (secondary): typed pages (entity, concept, source, synthesis, question), required frontmatter, validation after every write, `confidence`/`status` fields, lint that cross-references point both ways.
- Failure modes: untyped links cannot say supports/contradicts/supersedes; errors get baked into pages later work builds on, reader drifts one or two steps from the source.
- Convergence: Letta context repositories (git-backed Markdown, file tree always in prompt, background reflection/defragmentation into 15–25 focused files; https://www.letta.com/blog/context-repositories). Claude memory tool docs advise size caps, expiring unread files, path validation. Agent Skills use progressive disclosure.

### GraphRAG and alternatives
- Microsoft GraphRAG: Leiden communities, bottom-up community reports, map-reduce global answers; extraction ~75% of indexing cost (https://arxiv.org/abs/2404.16130). LazyGraphRAG: no up-front summaries, query-time relevance budget, ~0.1% indexing cost (MSR blog; open-source availability unverified).
- HippoRAG 2 (https://arxiv.org/abs/2502.14802): OpenIE triples + passage nodes + Personalized PageRank; +7 points on associative multi-hop, no index-time summaries, local-friendly.
- LightRAG: local/global dual retrieval, incremental insert; default stores for small scale only.
- Graphiti/Zep: bi-temporal edges, invalidation not deletion, hybrid BM25+vector+BFS, label-propagation communities with incremental update. Since 0.30.1: 0.31 makes combined node+edge extraction the default, separate timestamp resolution, episode batching (latest v0.31.2).
- Cognee ontology grounding: `annotate` keeps unmatched entities, `strict` drops them.
- GraphRAG-Bench (https://arxiv.org/abs/2506.05690): graphs win on complex reasoning and summarization; plain RAG wins on simple fact lookup. Mem0 v3 dropped typed relations for entity linking + add-only temporal facts.

### Ontology induction
- EDC (https://arxiv.org/abs/2404.03868): extract openly, define schema, canonicalize via embedding retrieval + LLM verification. AutoSchemaKG (https://arxiv.org/abs/2505.23628) induces schema with extraction. DIAL-KG (https://arxiv.org/abs/2603.20059) induces schema only from validated knowledge.
- Product pattern: Graphiti custom types + edge map, unmapped pairs keep free-text predicates, changes apply to new data only; Zep default types with `set_ontology` overrides. Presets plus overrides.
- Finance KGs: FinReflectKG (S&P 100 10-Ks, best 64.8% rule compliance; https://arxiv.org/abs/2508.17906); Helicase attaches uncertainty to supply-chain facts.

### Self-improving memory
- When to write: Letta sleep-time agents edit memory asynchronously, cleaner than in-turn writes (https://www.letta.com/blog/sleep-time-compute). Mem0 moved to single-pass add-only writes.
- Bloat and collapse: ACE (https://arxiv.org/abs/2510.04618) itemized bullets with IDs and helpful/harmful counters; Curator emits deltas merged by non-LLM code; avoids brevity bias and context collapse; finance gains shrank or reversed without reliable labels. Dynamic Cheatsheet, A-MEM (Zettelkasten notes whose neighbors evolve).
- Utility feedback: MemRL (intent, experience, Q) with similarity recall then Q rerank, update only memories actually used; ReasoningBank distills from successes and failures; MemOS carries provenance and version metadata.
- Contamination: MINJA query-only injection 98.2%; do not weight agent-generated facts equally with user-stated or sourced ones; governance and rollback are under-researched (https://arxiv.org/abs/2606.30306).
- Evaluation: LongMemEval (knowledge updates, temporal reasoning, abstention), LoCoMo, MemoryAgentBench (test-time learning, selective forgetting).

### Discovering connections
- Communities (Leiden / label propagation) with summaries. SciAgents (https://arxiv.org/abs/2409.05556): sampled paths between concepts with random waypoints, an ontologist explains each relation, hypothesis written and critiqued; novelty self-scored.
- Link prediction (FinDKG GNN) needs training data and scale. AI co-scientist uses generation/reflection/Elo tournaments; validation came from experiments.
- Validating novelty: temporal holdout (BioVerge freezes the KB at T and scores hypotheses against later literature); gather supporting and contrasting evidence. Novel to the graph is not novel to the world.

### Recommendations from pass B
- P0: one provenance spine (World edges carry source span, extraction time, valid time, schema version, origin tier source/user/agent-inferred; Lens factual sentences cite claim IDs or spans; agent-inferred content never becomes a source fact). Lens as versioned Markdown with index and log, typed links, revisions that can roll back, scheduled lint (broken/one-way links, orphans, uncited claims, claims whose World edge was invalidated). Split write policy: in-turn writes are deltas, a background job merges under size caps; never rewrite whole pages blindly. Presets as overlays on the core in annotate mode; induced type candidates surface as suggestions in chat; lazy re-extraction for watched entities.
- P1: communities become Lens topic pages, built lazily; watchlist-seeded discovery (PPR over valid edges, sampled paths between watched entities, surprise = cross-community, rare co-occurrence) written as `unverified` hypothesis pages; temporal-holdout evaluation; utility counters from explicit signals only (user accept/reject, cited in a final answer, outcomes resolved later).
- Defer or avoid: GNN link prediction, full global map-reduce, tournaments, auto-promoting induced schema (pass B) vs agent-era auto-apply (pass A and the user's direction) is a real tension to resolve in design; server-based graph stores; strict ontology filtering; untyped links; monolithic memory files.
