# Untrusted text

Third-party text raises a turn tier that pauses outbound tools, takes commands offline, wraps the text the model sees, and keeps `web_fetch` to known sites. Holdings quantities stay on this computer.

Third-party text raises a weaker tier of the same `TurnTaint`
(`apps/loupist/src-tauri/src/custom_connections/taint.rs`). After a `web_search`,
`web_fetch`, `dart_filing_read` or bundled OpenBB data result outside the reviewed
numeric-only list (`NUMERIC_ONLY_OPENBB_TOOLS`; profiles, filing documents, transcripts and
news raise it, prices and statements do not), Rust refuses custom-connection calls
and schedule create, update, status and Run now for the rest of the turn with
`untrusted_text_paused`. Native Bash keeps running inside the OS sandbox with the
network off and the Loupist folder mounted read-only except its existing
subfolders, `outputs/` and scratch, so a command cannot touch the files a later
reply loads as instructions; where no verified sandbox exists it is refused.
Native `write` and `edit` refuse an instruction file, and a file with another
name on disk (a hard link), for the reply; native file tools stay inside the
Loupist folder in every reply
([Native tool permissions](permissions.md)). The refused step
carries its `refusal` in Chat progress (`untrusted_text_paused`, `protected_path` or
`outside_workspace`), set from the broker's own trace of that call
(`ToolTraceCapture::refused`), never from model-facing text, and the Chat labels the
step and explains it with no override in the step. Workspace file tools, datasets,
charts, Memory and market data stay allowed, and web reads stay allowed within the
site rule below. The tier rises before the result is delivered, so parallel calls are
checked against it.
The model's copy of that text, and of custom-connection results, is wrapped in
`<<untrusted id=… source="…">>` … `<<end untrusted id=…>>` with a random
per-delivery id; retained captures keep the original bytes. A custom-connection
result still raises the stronger outbound pause above.

Third-party text that comes back from retained material raises the tier and is
wrapped the same way (`tool_executor/returning.rs`). A `world_read` of a saved
Source, and `world_search` hits on it, do so when a publisher, a search or a custom
connection authored its capture; the user's files, pasted text and statements, Korea
Investment reads and a shared pack's Sources (below) do not. A saved provider result
counts as numeric only when the dataset it names records `trust: numeric`.
`dataset_query` and `dataset_describe` do so for a dataset whose `trust` is not
`numeric` or `user_file`, including one retained before trust was recorded.
`artifact_read` and `chart_query` apply the same check to a chart's pinned input;
an unavailable input revision fails closed. Artifacts from a reply recorded as
tainted return delimited too. World claim reads and search hits resolve their
canonical evidence spans and use the Source capture's authorship, as Source reads
do. Asset-page observation rows and watch alerts, and a briefing's `assetDeltas`
and watch alerts, are delimited; a briefing starts at the untrusted-text tier
before its first tool call. Observation rows are conservative because even a
numeric observation can carry a publisher's filing or source title.

Rust records whether either tier was raised in the reply's existing `tool_trace`
JSON. `conversation_memory` and `chat_history` delimit that reply's returned text
or snippets and raise the untrusted-text tier in the new turn; user messages and
clean replies remain unchanged. Offsets still address the original saved text.
Replies saved before this receipt was recorded cannot retroactively recover it.

App exports of untrusted datasets and Source captures get host-owned SHA-256
markers under the app's private `export-trust` directory. Renderer-composed CSV
and Markdown exports have no authoritative origin receipt, so they are tagged
conservatively. Native `read` reports its resolved target to Rust, which recognizes
those exact bytes, raises the tier and supplies fresh delimiters for the native
result. Unchanged copies and renamed files are recognized; `dataset_import`
keeps tagged bytes in the third-party class. Workspace captures of tagged bytes
are delimited too and retain tool authorship if saved as a Source. Files exported before tagging,
externally changed bytes and derived files without declared lineage remain
unclassified local files. These tags do not make the host filesystem a sandbox
([datasets](../analysis/datasets.md#data-and-provenance)).

The web site rule below is unchanged: redirect hops, short private values on
already-seen sites and `web_search` queries are not checked for exfiltration.

Once the tier is up, `web_fetch` keeps to sites the turn already knows
(`TurnTaint::web_fetch_target`). A search result (by `source_id` or its exact URL), a
page already read and a URL the user typed always open. A URL carrying a held
quantity, cost or value from the latest holdings snapshot (whole numbers of four
digits or more, years excepted), or a 24-character run of the turn's principles,
perspective, Lens bodies or notes and Journal text, is refused on any site. Official
filing and statistics sites (`RESEARCH_SITES`) open. A site seen this turn (a search
result's or fetched page's host, or a site named in the user's message) opens when its
path is at most 1024 bytes and its query and fragment total at most 256 bytes. The
path bound is wider because a percent-encoded Korean article slug is long. Anything
else is refused with `untrusted_text_paused` and a `next` that says to find the page
with `web_search` and fetch the result, whose exact URL then opens. Links and text
inside a page, its alternates and a saved Source's own site never count as known.

These URL privacy checks run in production builds as well as tests. Debug-only
prompt and tool-result canary logging is separate from this outbound gate.

Text from a [shared Loupist](../knowledge/sharing.md#sharing) is third-party text too
(`sharing/delivery.rs`). In the turn's Memory context and Memory tool results
(principle lines, a shared perspective, catalog titles and summaries, relevant-method
pointers, `lens_search` and `lens_read`, and the names, relationships, summaries and
reference Sources in `world_read` and `world_search`) it arrives inside `<<shared id=… from="{pack}">>` …
`<<end shared id=…>>`, with an id the pack author cannot know, and a locked `SYSTEM.md`
rule explains it. A Lens item accepted despite a scan finding raises the untrusted-text
tier wherever it is delivered. A clean accepted item raises it only when its full body
arrives through `lens_read`; its principle line, perspective, catalog entry, pointers and
`lens_search` hit pause nothing, so a Loupist made from a pack stays usable. Names,
relationships, summaries and reference Sources a pack added raise it in every World read
or search that returns them, including a pack Source read. When the lookup of shared
records fails, every Memory delivery is wrapped and raises the tier. The user's own
Memory never raises it.

Holdings reach a model only as identities (symbol, market, name and a held flag; see
[context bounds](../agent/context.md#context-assembly-and-bounds)). A Korea Investment
account read is the one tool result that carries quantities, amounts, prices paid and the
rates derived from them. In every run, interactive or unattended, Rust
(`privacy/holdings.rs`) replaces each field the operation's reviewed `holding_fields` list
names with `[hidden on this computer]` before the result reaches Pi, a dataset (a reused
one included), a result reference, a chart or an export
([catalog review](../capabilities/data-services.md#maintaining-the-korea-investment-read-catalog)).
An account operation without a reviewed list is refused before any request
(`account_fields_unreviewed`), and `source_save` refuses an account result
(`account_data_not_saved`). Numbers the user types into a message are left as typed.
Debug builds also scan each briefing prompt and every delivered tool result for the
latest holdings snapshot's quantity, cost and value strings (four digits or more) and log
a hit.
