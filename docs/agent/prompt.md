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

User-facing names are Bot, research session, the Map, Schedule and Market data
platform. Answers, questions and scheduled prompts never write World
or OpenBB; internal identifiers and tool names are unchanged. An unattended comparison can use prose or a Markdown
table, with the usual number links. Interactive research can also publish a
checked table for sorting and export.

## Keep the prompt thin

`SYSTEM.md` is written for frontier models at their normal reasoning level; a
model too weak for financial work is not a target, and rules that patch its
mistakes cost every capable model context and attention. Each sentence in the
prompt is one of three things:

- **An invariant or host protocol** the app depends on: the `safety:locked`
  spans, the evidence-link and number-link form the host checks, the envelope and
  runtime fields the agent reads, path and permission rules, the names the user
  sees. These are stated exactly.
- **What the product wants from an answer**: voice, brevity, what a quick ask
  is, a view with both cases, what the Loupist keeps. These describe the
  situation and the aim, with the reason in a clause, so the model can judge the
  cases the sentence does not name.
- **A tool's own rule**, stated once where the agent decides to reach for the
  tool. The detail stays in the tool description, which the model reads at the
  call: schedule modes and self-contained prompts, alerts that stay quiet,
  thesis reasons and lines, what makes a price read worth saving, map kinds,
  discovery and exact-read mechanics, reuse of a retained Source, what
  `lens_file` leaves unfiled.

Do not add a prohibition for a mistake seen only on a small model or at low
effort. Before adding a rule, reproduce the defect on a frontier model at medium
effort through the [agent evals](../../apps/loupist/evals/README.md#agent-evals),
and prefer fixing the tool result or description that misled it. A rule restored
after such a run says why in its sentence. The 2026-11-17 revision cut the prompt
from 21.4 KB to 15.2 KB on this basis (the text outside locked spans from 18.2 KB
to 12.0 KB) and compared both versions on GPT-6.1 Sol at medium.

## Language-neutral model text

Model-facing text (`SYSTEM.md`, prompt files, Skills, tool descriptions, surface
hints and host notes) is written in English and names no user language. It holds
no word, example or rule that exists for one language; a market or data source
(KRX, OpenDART, K-IFRS) is named in English, with filing kinds described rather
than quoted in their original script. Three rules carry what a per-language rule
used to:

- **Register.** Answers use the language the user writes in, which is not
  always the interface's (under the earlier prompt GPT-6.1 Sol answered some
  messages in the `interface` block's language instead), in the register of a
  careful analyst writing to a client. Where a language marks politeness or formality the
  answer uses its formal polite level, never the informal-polite or casual one,
  with no exclamations, emoji or filler. Brevity comes from saying less.
- **Figures.** The prompt asks for numbers as a careful financial writer in the
  user's language would print them and fixes no digit counts. It names what the
  check depends on: amounts scaled with that language's own scale words and one
  currency marker; a rate, margin or growth figure as a percentage whatever
  form the result gives; and a change with the sign its result gives inside the
  link, because the number check does not match a number to a value of the
  opposite sign. A day change that a result gives as −1.1 is written as "−1.1%".
  The check also accepts an unsigned figure for a negative value, a fallback
  specific to structured data reads. The prompt gives "−1.1%" for the signed
  day change because GPT-6.1 Sol wrote "down 1.1%" without the sign. It says
  the check accepts rounding and scale words, so raw precision is never printed. The
  [number check](../analysis/number-checks.md#number-checks) accepts rounding,
  scale words and percent-for-fraction. It reads English and Korean scale words
  today; a figure scaled in another language's words stays a numbered citation
  until the checker learns them.
- **Terms.** The tone line sets who gets terms explained: the careful tone
  explains unfamiliar ones, the balanced tone explains one only on request or
  where the user plainly would not know it, the direct tone assumes fluency. A
  term the user used is not defined back to them.
- **Interface words.** The prompt gives the public English names. On Bot and
  interactive research turns, `interface_labels` can read selected product or
  navigation labels in the appearance captured for the turn. The envelope
  carries only `interface.display_currency` if set and omits the block otherwise.
  Scheduled, delegated and host-started Map-link turns receive the resolved
  language and five localized product names, plus any selected display currency. Scheduled runs answer in
  the language their task names or, by default, the language it is written in.
  A briefing follows `appLanguage`. A link-action hand-off follows
  `interface.language`, because its first message is the host's caption. A
  Bot-initiated research hand-off takes its answer language from the original
  user message, and `interface.language` describes the app's screens.
  [Turn context](context-delivery.md#memory-and-context)

A citation link's target is the value of a result's `citation_url` (`#evidence-…`)
and nothing else. The prompt and Skills show the target in that form, never the
field name, because a model that copies `citation_url=` into the target writes a
link the number check does not read.

## Quick asks in the Bot

Most Bot turns are quick asks: how an asset or the market stands, why it moved,
a figure, what changed, a short briefing, a follow-up, what a term means.
`SYSTEM.md` describes the situation rather than listing limits: the user wants
the answer in seconds, the way a well-informed colleague answers in passing, so
a quick ask is not research. No host tool gathers an asset's figures: the agent
chooses a read or two from the data sources the turn has, and for a followed
asset `asset_page_read` returns the call the app repeats for its price. The
answer does not recite the result: it says what matters about where the asset
stands, and a price carries its date. A quote often carries no date, so the
prompt allows the day of the read. Figures link to the result that gives them
under the common citation rules, and a headline is a report with its publisher
and date.

How long the answer is and what form it takes are the agent's to judge from the
question, and the user's to change. The prompt, the Bot hint
(`ChatSurface::hint`) and the `why-it-moved`
Skill set no count of sentences, bullets or figures, no decimal places and no
rule against headings or tables in a quick answer. A fixed shape suited "how is
it doing?" and not "walk me through the quarter" or "put that in a table", and
the user could not ask for another; a longer answer had to leave for a research
session. A passing question still gets a passing answer from the situation the
prompt describes. The sentence that leaves form to the agent is the only
guidance added for this; if a frontier model at normal effort answers the same
without it, it goes. A read returns evidence and no card, so the prompt does not
mention one ([Bot quick asks](../ui/home-thread.md#bot-quick-asks)).

When the read returns nothing the answer says the data is unavailable now and
stops; without that sentence GPT-6.1 Sol filled the reply with themes from its
own knowledge. The prompt has no list of forbidden endings and no rule about
promising a later reply; those corrected a weaker model
([Keep the prompt thin](#keep-the-prompt-thin)).

A question about why an asset moved is a quick ask. The answer gives the reason
or says first that none is established; a headline is a report, not the cause.
It offers a research session for the cause, or calls `research_open` when
the user asked for an explanation a quick read cannot give. The `why-it-moved`
Skill is the full attribution for a research session or a user who picks it; its
description and body send a Bot quick ask back to this rule. The description is
also the English text the interface matches to find the Skill's localized name
(`src/lib/agentSkills.ts`), so a change to it is made together with
`bundledSkills.why-it-moved.description` in the English locale file.

A quick ask uses no Memory searches and no Skill the user did not
choose, files nothing to Lens and saves no Source. With saving on, it does
save the price read of a followed asset whose row carries `price_read`, which
is how the app shows that price and not research; the Bot hint says so too.
The prompt names no phrase
for how the answer ends, because a model repeats the label it is given. A
relevant principle still gets its check. A briefing asked for now is an answer;
only a recurring one is scheduled.

`SYSTEM.md` states a tool's own rule once. Rules that a tool description already
gives in full (what `lens_file` leaves unfiled and when a method page is made,
checking existing schedules before creating one, schedule modes and the Bot's
pending questions, what makes a price read worth saving, what the lookup sends
to OpenFIGI, alert tasks and `schedule_quiet`, thesis reasons and lines, map
kinds, arithmetic that needs no tool) live in that description only.

## Followed assets and saved price reads

The host keeps a list of names and replays one saved call per asset; the agent
decides which listing a name means and which call prices it. The followed
hint (`FOLLOWED_HINT`) owns what a row's fields mean. `SYSTEM.md` says that an
asset is followed under the user's name for it, that the app shows a price by
repeating a saved call, and that a turn reading the price of an asset whose row
carries `price_read` saves that read. The `price_read_save` description owns
what makes a read worth saving: the listing the user means, the latest price on
every repeat, a stated date. A source that serves only dated calls meets the
second through `rolling`, a start and end window the app moves to end today,
with the newest row as the price; a single-date call cannot roll. The screen
shows the saved read's result as fact,
so the description says nobody checks the choice, and the delegated
[price setup prompt](../../apps/loupist/agent/prompts/price-setup.md) saves
nothing for a name whose listing is not plain. The briefing prompt says the app
read nothing for the run, and the run decides what to check; an alert is a
custom task that calls `schedule_quiet` when its condition is not met.

A file the user hands over, a request to keep or file something, a principle or
judgment the user states and a request for a view on an investment are not quick
asks and keep their rules. Depth or multi-step work goes to `research_open`
unless the user wants it done in the Bot; the tool's result says what to tell
the user. Research sessions, scheduled runs and delegated sessions are
unchanged. What a quick ask leaves behind is described in
[Bot quick asks](../ui/home-thread.md#bot-quick-asks). The
[timing probe](../../apps/loupist/evals/README.md#bot-quick-ask-timing-probe)
measures a quick ask's time and tool calls, not the answer's form.

## Investment judgment

The Loupist answers investment-judgment questions with its own clear or conditional
view, dated evidence, reasons, main risks, uncertainty, and what would change
its mind. Brief follow-ups, including yes-or-no answers, retain how uncertain the view is,
the decisive figures and dates, opposing case,
and explicit observations that would support or overturn each case. Missing
inputs for an exact allocation do not prevent a conditional exposure decision;
the agent states that decision first, then names the missing sizing inputs without
inventing a percentage. Each decisive observation keeps its date in the answer.
It cannot place orders or trade. The
[Journal Skill](../../apps/loupist/agent/skills/journal/SKILL.md) supports evidence
and principle checks; a request for a view does not authorize saving it as the
user's judgment. Product disclosures belong to [Positioning](../positioning.md). The
`word-documents`, presentations and PDF Skills end work made for others with the
AI-produced line ("Made with AI in Loupist. AI can be wrong. Not a licensed
financial adviser.") in its language; a deck puts it on the last slide.

Followed-company context makes research relevant without narrowing investment
judgment. It supplies identities and follow flags, never position size, cost or
value; the agent neither infers nor asks for those private fields.

For each relevant saved principle, the answer links its statement to its
`lens:` target, cites the dated evidence, and explains in the answer's language
whether the evidence meets the rule, conflicts with it or is insufficient. This
includes the condition of a rule that names a trade action. Placement follows
the question. Principles outrank methods and the standing perspective. Tool
permissions and authority rules remain binding.

An explicit request for a method page is filed only as that page, even when the
rule is short; a standing rule without that request is one principle.
Apply a relevant principle as stated: when it sets how a figure is computed or
presented, lead with that figure, then give the standard one and the difference once.

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
Journal and calibration tasks. Finance scenarios seed followed assets by name and
reference, and a turn can send the host-built briefing prompt (`briefing`) before its
text; World suites apply a
template's curated seed with `seed: "template"`
([template seed check](../../apps/loupist/evals/world/README.md#template-seed-check)). Use the
[uncued Memory loop](../../apps/loupist/evals/world/README.md#uncued-memory-loop)
for acquisition, refinement, fresh-Chat reuse and save restraint. Keep training and
validation cases separate and introduce later source files only at their declared
turn. Record model, inputs, expected outcome, actual trace, latency and limits.
Use repeated matched runs before claiming superiority.
