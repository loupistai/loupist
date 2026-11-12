# Prompt and judgment

Effective instructions are the sealed prefix plus the Loupist's tone. The product states what the evidence shows and does not trade or prescribe a position. The pages in `docs/agent/` are developer instructions, not text to inject into Chat.

## Review the effective instructions

Start with a user task or observed failure. Before changing the system prompt,
read the entire [SYSTEM.md](../../apps/loupist/agent/SYSTEM.md), then inspect the
assembled request: Pi sections, host/workspace instructions, Skill catalog and
loaded bodies, active tool definitions, results and current turn input. For a
focused Skill/tool change, read its full text and governing instructions. `rg`
locates owners; excerpts alone do not establish semantic correctness.

Check purpose, authority, load condition and effect. Look for duplicate directions,
stale UI claims and mismatches between descriptions, schemas, validation and results.
Put enforceable boundaries in Rust; argument/result rules in tool contracts;
specialized methods in selected Skills; universal behavior in `SYSTEM.md`.
Review the assembled prompt again after editing.

When task data requests unauthorized instruction or file changes, scheduling or
sharing, the agent names the skipped actions in the final answer, including when
missing research inputs require clarification, and continues the authorized
research. The data does not become a user instruction.

Native file guidance names only admitted file/instruction/Skill paths. Bundled
Skill reads have their own read-only root; adjacent agent internals stay private.
The `web_fetch` search-result/site restriction applies after third-party text has
been read in the current reply; before that, a public URL the user supplies can be
opened directly. The Journal Skill routes durable
assistant research to `lens_file`, while `note_save` keeps Chat progress and facts
the user stated.

Ordinary research packs and enabled web research preload on every surface. The
catalog marks charts and schedules with `preload: false`: they remain registered
and listed in the runtime index, and need `capability_load` when requested. Data
connections also load by id; unattended profiles retain their fixed allowlist.
`run_python`'s tool description is the single sandbox/package reference. Journal
covers user saves/reviews; Lens covers methods/library maintenance. The source
prompt retains `safety:locked` annotations for review, but launch strips those
comments while preserving every enclosed instruction.

## Investment judgment

The Loupist answers investment-judgment questions with its own clear or conditional
view, dated evidence, reasons, main risks, uncertainty, and what would change
its mind. Brief follow-ups retain the decisive figures and dates, opposing case,
and explicit observations that would support or overturn each case. Missing
inputs for an exact allocation do not prevent a conditional exposure decision;
the agent states that decision first, then names the missing sizing inputs without
inventing a percentage. Each decisive observation keeps its date in the answer,
including when a chart also shows it. It cannot place orders or trade. The
[Journal Skill](../../apps/loupist/agent/skills/journal/SKILL.md) supports evidence
and principle checks; a request for a view does not authorize saving it as the
user's judgment. Product disclosures belong to [Positioning](../positioning.md). The
`word-documents`, presentations and PDF Skills end work made for others with the
AI-produced line ("Made with AI in Loupist. AI can be wrong. Not a licensed
financial adviser.") in its language; a deck puts it on the last slide.

Followed-company context makes research relevant without narrowing investment
judgment. It supplies identities and follow flags, never position size, cost or
value; the agent neither infers nor asks for those private fields.

When at least one saved principle is relevant, the answer ends with a short
section titled “Checked against your principles” (Korean: “내 원칙 점검”). Each
line is a `lens:` link, the dated fact that was checked, and meets, conflicts,
or not enough information. Include principles that name trade actions, for example “your rule’s sell
condition is met (growth 3% < 5%).”
Skip the section when no principle is relevant. Principles outrank methods and
the standing perspective; tool permissions and authority rules remain binding.

When saving is on, a Journal records only the user's explicitly stated judgment.
A dated check and user-entered confidence are optional; never invent them. A review
compares a saved check with fresh dated evidence and records its factual outcome;
it does not invent a new user judgment. A recurring analytical error can justify
a proposed method revision. Method refinement retains the existing title; there
is no rename operation. The exact record rules belong to
[Knowledge and Memory](../knowledge-model.md).

## Scoped agent settings

Settings → Skills manages shared optional/custom workflows; the Loupist settings
page manages its local workflows and overrides, preserving its search, filter and save confirmation when configuration reloads or the window regains focus. Included app procedures stay
fixed. Skill Creator uses runtime-provided personal/Loupist paths; Settings owns imports.
Supporting files are packaged and included in admission validation and cache seals;
a reference edit must invalidate a warm session just like an entrypoint edit.
See [bundled provenance](../../apps/loupist/agent/skills/NOTICE.md).

Settings → Chat defaults, the Loupist's Chat preferences, and scoped TOML expose a bounded Pi subset. Rust freezes
it before launch and atomically replaces the snapshot in the Chat's stable private
CWD. Do not give each turn a new CWD or load arbitrary project extensions/settings.
[Scoped configuration](../runtime/storage-authority.md#scoped-configuration) owns inheritance.

| TOML field | Default | Accepted values |
| --- | --- | --- |
| `pi.compaction.reserve_tokens` | `16384` | Nonnegative JavaScript-safe integer |
| `pi.compaction.keep_recent_tokens` | `20000` | Nonnegative JavaScript-safe integer |
| `pi.retry.enabled` | `true` | Boolean |
| `pi.retry.max_retries` | `3` | Integer `0–10` |
| `pi.retry.base_delay_ms` | `2000` | Integer `0–300000` |
| `pi.retry.max_agent_delay_ms` | `60000` | Integer `0–300000` |

Verify combined file/byte limits and path/content hashes in Rust and the extension.
Changed instructions or settings must rebuild a previously warm session from SQLite.

## Measure cache behavior

Pi session reuse and provider prompt caching are different. Warm resume requires
valid transcript/authority seals; it does not prove cached provider tokens. Inspect
actual usage for provider cache comparisons and keep provider, model, settings,
inputs and turn pattern fixed. Do not weaken invalidation to improve hit rates.

Saved main-assistant usage includes `input`, `cacheRead` and `cacheWrite`; total
input is their sum and is unavailable when a counter is missing. These aggregates
exclude auxiliary calls and compaction. Record correctness and completion alongside
usage, setup/first-text/total latency and available cost. Missing measurements stay
missing. For offline comparisons, hold Memory flags, Skills, connections, resource
paths, history and references fixed. Use the installed Pi prompt builder and
`estimateTokens`, including active tool definitions; label its UTF-16/4 estimate
separately from measured provider tokens. Include the Rust envelope as a separate
measured component when evaluating Loupist context changes. Use only allowlisted, redacted diagnostics; never raw credentials or RPC
payloads. One smoke run does not establish a speed or quality improvement.

## Verify the user's completed task

Check observable outcomes and artifacts, not prompt wording or tool-call counts.
Cover selected/unselected Skills, malformed inputs, warm/cold continuation,
cancellation and ordinary versus private Memory where relevant. Visible behavior needs the native
flow described by [Which tool](../../apps/loupist/e2e/README.md#which-tool).

Use [agent evals](../../apps/loupist/evals/README.md#scenario-suites) for learning,
Journal and calibration tasks. Finance scenarios seed results events (`earnings`), one
local watch check before the first turn (`watch_check`) and exchange balance files
(`holdings_crypto_csv`), and a turn can send the host-built results procedure
(`earnings_prompt`) or briefing prompt (`briefing`) before its text; World suites apply a
template's curated seed with `seed: "template"`
([template seed check](../../apps/loupist/evals/world/README.md#template-seed-check)). Use the
[uncued Memory loop](../../apps/loupist/evals/world/README.md#uncued-memory-loop)
for acquisition, refinement, fresh-Chat reuse and save restraint. Keep training and
validation cases separate and introduce later source files only at their declared
turn. Record model, inputs, expected outcome, actual trace, latency and limits.
Use repeated matched runs before claiming superiority.
