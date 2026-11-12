# Tool catalog

Static tool names, routes and evidence rules live in the catalog and `SYSTEM.md`. Per-turn Memory and asset tools are in [Turn tools](turn-tools.md).

## Tool contracts and turn preparation

`agent/tool-catalog.json` owns static names, broker routes, Memory permission
classes, component membership, schemas and descriptions. Its `progress` metadata
is for Chat presentation, not model input. Each tool step in `ChatProgress` carries the
host's `toolName`; the renderer recognizes steps by it (a `chart_publish` step reserves
a chart card, a `grid_publish` step a table card) and shows a step whose label is that
tool's catalog wording in the interface language from `toolProgress` in the Chat locale
files. The English entries must equal the catalog, so a tool that gains progress labels
needs both locales; labels the host composes itself (`Opened {connection}`, `Reading
{host}`) stay as sent. Rust derives the permitted surface;
service handlers independently validate arguments, ownership and evidence.
Host-handled `as_of` treats null as absent and rejects other non-string values;
non-null optional `world_read` revision, span and center-node IDs must be bounded
strings.
The runtime profile advertises a native finance tool only when its vendor's exact
capability is enabled: enabling public wallets alone does not expose DefiLlama.
Each execution checks the Chat's live database binding without loading its
transcript. Dataset file/query work and Lens search run on blocking workers;
captured results reuse their measured byte size through reservation and delivery.
Native finance and bundled MCP reads share one dataset-retention writer, keeping
their existing provenance metadata and reuse rules.
An active tool with a stale source, wrong Chat or artifact kind, or missing host
context names the field and a repair, rather than suggesting a capability load.
Missing provider setup names that provider. Web refusals distinguish URL policy
from a scanned document with no text layer.
Internal storage failures and free-form finance diagnostics return host-owned
repair messages rather than raw errors, URLs or private paths. Native finance
results refuse an echoed API key before capture or retention; bundled MCP also
refuses credentials, app-data paths and private scratch paths in results and tool
inventories. Ambiguous DART resolution errors list only validated corporate codes,
with names available through the lookup result. Schedule receipts
report that a run failed without copying its stored diagnostic. Third-party result
warnings, exported dataset column names and saved results quotes retain the
third-party-text boundary; receipt IDs and citation URLs remain host fields.
Warning receipts keep the producer's existing trust class, including trusted
workspace and reviewed numeric results. Artifact-list and conversation-search
titles inherit the saved reply's third-party-text marker, as their bodies do.
Journal saves and reviews share one per-turn reservation, released for an
undelivered call. Acknowledged web pages preserve all issued pagination offsets
even when deliveries finish out of order.
`schedule_set_status` accepts active or paused; only a conflicting task mutation
suggests rereading its saved revision.
What Rust decides per broker method is one row of
`apps/loupist/src-tauri/src/broker/tool_class.rs`: `ToolMethod::safety` (whether
the custom-connection and third-party-text tiers pause it, whether its result is
third-party text, its Don't-save and credential checks, unattended access, shared-pack
delimiting) and `ToolMethod::producer` (run-result runtime, provenance, saved-Source
author, dataset retention and `max_age`). Neither match has a wildcard, so a new
method does not compile until both rows are chosen, and `tool_class/tests.rs` pins
the reviewed classification and checks the columns against the catalog. The Pi
extension registers schemas/routes. Anchor every schema `pattern` at both ends: xAI
constrains Grok's arguments to a pattern as a whole-value match, so the open-ended
`^#evidence-` once let it emit only `#evidence-` and every cited calculation was refused.
A citation field's pattern is the exact host form, `^#evidence-ev_[0-9a-f]{32}$`. MCP tools are discovered dynamically; bundled
OpenBB cards (descriptions and per-run pruned schemas,
[Data services](../capabilities/data-services.md#agent-facing-data-services)) are host-generated in
`apps/loupist/src-tauri/src/commands/tool_executor/mcp_host/openbb.rs`,
not taken from upstream docstrings, so edit them there. The capability index
(`formatRuntimeIndex` in `agent/loupist-extension.mjs`) names each enabled
connection and built-in pack with a purpose cut at a sentence or word, appends
the activation sentence only to the component Rust marks `bundled`, and replaces
the list of off connections with one fixed sentence that points at
`connection_setup`, shown only when that tool is in the turn.

Native `read`, `write`, `edit`, `ls`, `find`, and `grep` keep Pi's tool descriptions. [SYSTEM.md](../../apps/loupist/agent/SYSTEM.md) states the permission rule once, under "Authority and local work". File tools are restricted to admitted workspace, scratch and attachment paths. Private app data, credentials and git metadata are refused. `run_python` is the code execution tool and stays allowed after third-party text. [Native tool permissions](../runtime/permissions.md) owns the complete boundary. `registerWorkspaceTools` in `agent/workspace-tools.mjs` does not append that sentence to each tool. It still calls the Rust `authorize` hook before every execution, wraps untrusted text with the host boundary, and passes the workspace as the tool cwd while Pi's runtime directory stays private. The two-case form for an investment view is in that same file under "Investment judgment" and in the [Journal Skill](../../apps/loupist/agent/skills/journal/SKILL.md): two compact points, for and against, each a dated observation, then its next check (metric and timing), then what would strengthen or weaken it. The system's dated, linked per-principle checklist still applies. A proposed chart level's observed anchor is date, measure, and value; the [Visualize Skill](../../apps/loupist/agent/skills/visualize/SKILL.md) says "last day" or the chart's overall range does not date that value.

`web_search` follows the [routing setting](../capabilities/data-services.md#agent-facing-data-services)
in Rust. Model-native search runs as a Pi support command through
`agent/loupist-native-search.mjs` and the adapters in `agent/native-search/`:
`openai` (a ChatGPT sign-in or an OpenAI key) and the debug-only `openai-codex`
client share the Responses hosted-search request in `codex.mjs`; `anthropic.mjs`
and `xai.mjs` have their own. Rust offers a provider's search only when the
[provider manifest](../runtime/providers.md#provider-manifest) gives it
`native_search`, and a test keeps that list and the adapters the same. Every
adapter needs a resolvable credential before any request.

`calculate` (`calculations` pack) is the host arithmetic tool. Its contract is in
[Calculation](../analysis/calculation.md#calculation). `SYSTEM.md` sends common arithmetic to
`calculate` before `run_python`. The financial Skills route numeric work through
`calculate`, then OpenBB's local `quantitative_*`, `technical_*` and
`econometrics_*` tools, then `dataset_export` plus `run_python` for what those
cannot do.

A `dcf_grid` or `reverse_dcf` result shows as a [sensitivity card](../analysis/calculation.md#sensitivity-cards)
when the answer links it alone in a paragraph with the company name as the link text;
the `calculate` description and the `valuation` Skill ask for that form. For what a price
implies, the valuation Skill gives `reverse_dcf` the dated price and shares with their
sources and, when the data allows, a `cagr` of the same cash-flow measure over the last
five fiscal years as `reference_growth_pct` (cited in `input_sources`, its period in
`labels.reference_growth_pct`); it never presents one cell as the answer or as a target
price. `dividend-income` asks for the same card with the dividend `cagr` as reference.
The bundled catalog in `agent_harness.rs` lists the four retail Skills
([list](../capabilities/skills.md#skills-management)) first among the optional
workflows, so the Skills group under `/` shows them right after the fixed procedures,
each with its `skillBlurb` line from the Chat locale files.

`SYSTEM.md` ("Evidence and citations") has the agent write each material number as a
link whose text is only the number with its unit or currency and whose target is the
exact `citation_url` it came from: a data result or passage for a figure read from a
source (for a web page, a `web_fetch` passage whose `find` phrase contains the
number), a `calculate` result for a derived figure, with each sourced input named in
`input_sources`. Arithmetic never happens in prose; the first mention in a paragraph
and every material table cell are linked; years, dates, counts and the user's own
assumptions stay plain. Rust [checks those links](../analysis/number-checks.md#number-checks) when the
reply commits, and the [evaluation](../../apps/loupist/evals/README.md#agent-evals)
assertion `numbers_linked` measures them.

For a local original, native `read` returns the file text; `workspace_capture`
returns the host's `citation_url` alongside that text, outside any third-party
delimiter. `source_save` stages that captured result and does not create another
citation. The [citation scenarios](../../apps/loupist/evals/agent/suites/citations-v1.json)
exercise both Chat attachments and workspace files through the headless runner;
their scripted counterparts check delivery, Source retention and the linked
answer's number check. A native journey must count both numbered citations and
checked number links as inline evidence, independently of the Source footer.

## Code

- Prompt / tools: `agent/SYSTEM.md`, `agent/tool-catalog.json`, `src-tauri/src/broker.rs`
