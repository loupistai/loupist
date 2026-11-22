# Untrusted text

Third-party text raises a turn tier that pauses selected outbound tools and instruction-file writes, wraps the text the model sees, and limits `web_fetch` to known sites and exact links it has shown the model. `run_python` stays allowed. Reviewed account fields are hidden before a connector result reaches the model.

Third-party text raises a weaker tier of the same `TurnTaint`
(`apps/loupist/src-tauri/src/custom_connections/taint.rs`). Which broker methods each
tier pauses and which results raise a tier are their `ToolMethod::safety` rows
(`broker/tool_class.rs`). After a `web_search`,
`web_fetch`, `dart_filing_read` or bundled OpenBB data result outside the reviewed
numeric-only list (`NUMERIC_ONLY_OPENBB_TOOLS`; profiles, filing documents, transcripts and
news raise it, prices and statements do not), Rust refuses custom-connection calls
and every schedule change, including create, update, status, Run now, delete and
stop, for the rest of the turn with
`untrusted_text_paused`. `run_python` stays allowed on every host, without a card or override.
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
result still raises the stronger outbound pause above, including local schedule
deletion and stopping.

Third-party text that comes back from retained material raises the tier and is
wrapped the same way (`tool_executor/returning.rs`). A `world_read` of a saved
Source, and `world_search` hits on it, do so when a publisher, a search or a custom
connection authored its capture, and for every
[user-provided Source](../knowledge/sources-notes.md#user-provided-sources) (material
pasted into a Chat, `pasted_capture`): the user handed it over, someone else wrote
it. The user's files, text added through Add source and statements, Korea
Investment reads and a shared pack's Sources (below) do not. A capture in the turn
the user pastes the text is the user's message and raises nothing. A saved provider result
counts as numeric only when the dataset it names records `trust: numeric`.
`dataset_query` and `dataset_describe` do so for a dataset whose `trust` is not
`numeric` or `user_file`, including one retained before trust was recorded.
`artifact_read` and `chart_query` apply the same check to a chart's pinned input;
an unavailable input revision fails closed. Artifacts from a reply recorded as
tainted return delimited too. World claim reads and search hits resolve their
canonical evidence spans and use the Source capture's authorship, as Source reads
do. Asset lookup returns bounded structured listing metadata. It keeps the
ordinary retention receipt and leaves same-turn follow and schedule operations available.

Rust records whether either tier was raised in the reply's existing `tool_trace`
JSON. `conversation_memory` and `chat_history` delimit that reply's returned text
or snippets and raise the untrusted-text tier in the new turn. Ordinary history
reads preserve user messages and clean replies. Bot's hand-off card read delimits
the linked session's guidance together with its saved answer and title.
Offsets still address the original saved text. Replies saved before this receipt
was recorded cannot retroactively recover it.

App exports of untrusted datasets and Source captures get host-owned SHA-256
markers under the app's private `export-trust` directory. Renderer-composed CSV
and Markdown exports have no authoritative origin receipt, so they are tagged
conservatively. Native `read` reports its resolved target to Rust, which recognizes
those exact bytes, raises the tier and supplies fresh delimiters for the native
result. The native adapter reports the final text for link recording after it
applies the read limit. Unchanged copies and renamed files are recognized; `dataset_import`
keeps tagged bytes in the third-party class. Workspace captures of tagged bytes
are delimited too and retain tool authorship if saved as a Source. Files exported before tagging,
externally changed bytes and derived files without declared lineage remain
unclassified local files. These tags do not make the host filesystem a sandbox
([datasets](../analysis/datasets.md#data-and-provenance)).

Native `grep` checks every admitted file candidate before searching, including
ones the extension may later omit by glob. Once export tags exist, `read` and
`grep` share a per-call SHA-256 scan budget of 32 MiB across all candidates and a
500 ms deadline. Files already larger than the remaining budget are not read;
each read is at most 64 KiB, and the scan checks the run's cancellation and
deadline between files and reads. A file growing during hashing cannot escape
the byte budget. Budget exhaustion, deadline expiry or cancellation leaves trust
unresolved, so the native result is conservatively delimited as file text and
raises the untrusted-text tier. It never becomes a trusted result merely because
the scan stopped. Small ordinary user files keep their existing tier.

Redirect hops, short private values on already-seen sites and `web_search`
queries are not checked for exfiltration. A hostile publisher can offer many links
and try to make the model choose one based on a private fact, leaking a few bits
per fetch. The same channel already exists for search results.

Once the tier is up, `TurnTaint::web_fetch_target` applies the same rules in
interactive, scheduled and hand-off runs. A search result (by `source_id` or its
exact URL), a URL acquired through a fetch with a site grant, and a URL the user
typed open before the private-text check. The host refuses other URLs that
contain a 24-character run of the turn's principles, perspective, Lens bodies or
notes, or Journal text.

A bare URL or Markdown link in the result text the model received can then open
by exact URL. The model contributes only the choice among links it was shown.
Returned history and derived files can also contain links a model composed.
Opening an exact URL based on a delivered link passes the same private-text
check as opening a composed URL.

The host resolves relative Markdown link targets against the result's page URL
when available and uses the same parsed URL comparison as for other known URLs.
The comparison ignores fragments and normalizes default ports. A trailing slash
on a non-root path and the case of existing percent escapes remain significant.
Markdown decodes character references such as `&amp;`. In a bare URL the same
characters are literal. Only delivered text contributes links. Each piece of a
paged read contributes its own links when delivered. The host records up to 2,000
links per result and 20,000 distinct links per turn, each at most 2,048 bytes.
It silently skips links beyond those limits.

A fetch admitted solely through a delivered link grants the exact requested URL,
each reported redirect hop and the final URL for the rest of the reply. None of those
URLs adds a site. Paging, passage reads and retries of the same page still work.
Links delivered from that page can each be followed by exact URL under the same
rule. A page's alternates and a saved Source's own address also grant no site.

Search result URLs and sites named in the user's message add sites. Fetches
allowed before third-party text, by an exact search result or user-typed URL,
by an official filing or statistics site (`RESEARCH_SITES`), or by the rule for
an already-seen site add the requested and final sites. A fetch uses that site
grant if any of those reasons would admit it without the delivered link.
On a site the turn already saw, a composed URL opens when its path is at most
1,024 bytes and its query and fragment total at most 256 bytes. The path bound
accommodates percent-encoded Korean article slugs. Other composed URLs are
refused with `untrusted_text_paused` and guidance to use `web_search`.

These URL privacy checks run in production builds as well as tests. The app has no current
ownership records or wallet addresses. URLs with numbers follow the same site
and private-text rules.

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

A Korea Investment account read can carry quantities, amounts, prices paid and the
rates derived from them. In every run, interactive or unattended, Rust
(`privacy/account_fields.rs`) replaces each field the operation's reviewed `holding_fields`
list names with `[hidden on this computer]` before the result reaches Pi, a dataset
(a reused one included), a result reference, a chart or an export
([catalog review](../capabilities/data-services.md#maintaining-the-korea-investment-read-catalog)).
This boundary depends only on the reviewed account-read catalog. `account_fields_visible` is the
single decision point for a connection and currently always returns false.
An account operation without a reviewed list is refused before any request
(`account_fields_unreviewed`), and `source_save` refuses an account result
(`account_data_not_saved`). Numbers the user types into a message are left as typed.
