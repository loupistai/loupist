# Charts and evaluation

Charts draw pinned series. Evaluation entry points are the guides linked below, not a second copy of the cases.

## Visualizations

The bundled `visualize` method selects a visual form when it helps the user
compare, inspect a pattern, understand relationships, or explore assumptions.
It is a fixed app procedure for publishing and revising charts. Optional financial
workflows provide their own analytical references; those references load only for
the relevant operation. Small facts and short comparisons can remain prose or
tables. Optional PDF, Word, spreadsheet, and presentation workflows handle local
file extraction and authoring; they do not add native editors or bundled office
runtimes. Workbook recalculation and visual rendering require available local
tools, and any unverified boundary is reported.

| Question or deliverable | Existing route |
| --- | --- |
| Prices, volume and technical studies | `chart_publish` with a financial view; KLineCharts supports indicators, custom numeric series in shared auxiliary panes, drawing overlays and local zoom |
| Trends, rankings and relationships in tabular data | `chart_publish` with an analytic line, area, bar or scatter view, rendered through Flint and Vega-Lite |
| A sortable, exportable asset comparison or a card comparing conflicting sources | `grid_publish` with layout `assets` or `conflict` ([grid cells](grids.md#grid-cells)) |
| A compact process, relationship or explanatory diagram | A Mermaid block in Chat or a Markdown artifact |
| Change assumptions | Rerun a saved local script with explicit inputs and parameters, then import a new result revision |
| A custom chart, report or shareable export | Native workspace Python or JavaScript with saved inputs, code and output files |

App charts render inline in the answer as cards (Open in side panel, Expand, Copy data, Ask). Open in side panel opens that card's revision in the side panel, the same live chart the panel already shows (range, export, and the panel header). The control is absent while the card is expanded and is not drawn on the panel's own chart. Publishing a chart still does not open the panel by itself. A table card opens the same way. Copy data, the data view's rows and Save as CSV write each number with the digits it has: a fraction a provider stored as a 32-bit float prints as its shortest 32-bit decimal (225.3, not 225.3000030517578); integers and other values print exactly as stored. Drawing tools and range selection stay in the panel's chart view, reachable from saved work and from Open in side panel. The panel's chart fits the default bottom dock without scrolling: study panes take a quarter of its height, a short chart shows its legend on hover, and price-axis labels the last-price tag would cover are dropped.
Analytic axes and legends use the dataset's readable column labels. A value axis
counts large amounts at the scales answers print them, in the interface language
(`chartFormat.ts`, `amountScale`): 조, 억 or 만 in Korean ("300조"), tn, bn or m in
English ("300tn"); below 1,000만 or 1m, and for a column whose semantic type is a
price, a percentage, a ratio, a rate or a score, digits stay plain with their
decimals. Tick precision is that of a step for twice the ticks the axis asks for,
trailing zeros dropped, so adjacent ticks read apart and distinct small values do not
all display as zero. The currency a value column names (its id, "revenue_krw", or
its label, "매출(원)") appears once, in the value axis title ("Revenue (KRW)",
"매출 (원)"), or for folded series as "In KRW" / "단위: 원" when their labels do not
say it. Bar labels use the same units ("302.2조"), and hovering a mark gives the
exact stored value ("302조 2,312억 원", "₩302,231,360,000,000"). An explicitly
scaled currency column keeps its own unit across ticks, value labels, tooltips and
spoken mark descriptions. For example, a plotted 40 is read as
"Revenue (USD millions): 40". A category or time
axis never formats a period as a quantity: a date prints in the app's date format
("Jan 31", "1월 31일"), a string such as "2024Q3" or "FY2025" as given, and a numeric
x column is a category (one band per value, printed as given) when its semantic type
names a period, date, category or identifier, its column kind is a string or date,
or, with no type saying it is a quantity, every value is an integer from 1800 to
2200, a year. The panel's chart, the answer's card and Save as PNG draw the same
view, so they agree, and a language change draws the chart again. Legends
wrap below the plot as the panel changes width, keeping room for the data;
display labels do not change series identity or stored values.
The accessible SVG describes axes and data marks in the interface language, using
the same readable column labels and values. A continuous axis describes its displayed range.
Multi-series bar charts group bars side by side at each x value with a shared
zero baseline. Multi-series area charts overlay translucent fills and solid
lines. The value axis fits the individual values. For parts of a total, set
`view.stack` to `true` on a bar or area view to stack the series and fit their
sum. Saved charts that omit the field use grouped bars or overlaid areas.
When grouped bars get crowded, the chart keeps the bars and tooltips while
skipping overlapping category labels and omitting numbers that cannot fit.
Financial charts can attach a selected date range to a follow-up question. Theme changes restyle the open chart while retaining local drawings and zoom.
Position, entry, stop and target overlays name their source or analytical basis.
A chart's caption, like a table's notes, may name its sources by `#evidence-…`
target: the card draws each as a citation (its number in the answer, or the source's
name), an unresolved one reads "source", and table exports write the source's name,
never the raw id.
The agent preserves user-supplied values; when asked to choose levels, it labels
them as an analytical proposal with its evidence and assumptions, never an order
or the user's own Journal judgment.
`chart_publish` can select rows from a current-turn result or a pinned dataset
revision without transcribing full data through the model. Presentation changes
read the current artifact and reuse its dataset. The host persists the data,
view and lineage when the Chat turn completes.

Artifact history retains the newest 20 revisions per chart, table or document,
subject to a 32 MiB cap on serialized history per artifact (including chart data
and table evidence). Older revisions are pruned first; a single revision over the
cap is refused. The existing current-record storage is additional to that cap.
Each revision keeps its title, payload, source Chat and answer message, publication
time, and chart data snapshot or table evidence. Replaced source datasets do not
change a retained chart. History begins with the current version when an existing
artifact is first used; revisions overwritten before history was enabled cannot
be recovered.

`artifact_versions` lists retained revisions newest first. `chat_artifact_read`
and the model's `artifact_read` accept an optional `revision_id` (the artifact's
revision number); omission reads current content. Historical `chart_query` reads
must follow an exact `artifact_read`. `artifact_restore` requires the current
revision the caller saw and appends the selected content as a new current revision;
it keeps the original producer and records `restored_from_revision_id`. Existing
answers and retained revisions remain unchanged. Ask on an older chart selects
that revision; edits require the current revision to have been read or published
in this turn. Model reads resolve delivered drafts before storage, including their
dataset and lineage; an explicit earlier revision still reads retained history.
A reply may publish four distinct artifacts. Revising a delivered draft replaces
it at the same revision number without using another slot, and only the final
draft is stored when the reply completes. The first revision of a stored artifact
increments its revision number once.

`chart_query` pages the current draft or stored chart data at up to 200 rows and 32 KiB per window,
using `next_offset` for continuation. Ask Loupist selects an exact artifact revision;
opening, scrolling or copying its data does not add it to model context.

The Files reader previews supported raster images, SVG, CSV/TSV tables and Markdown. A
complete SVG read (within the reader's 256 KiB bound) previews as an image, with a
Preview and Source toggle: an `<img>` whose source is a `data:image/svg+xml` URL of the
file's text (`workspaceSvgPreview` in `components/workspace/WorkspaceFileContext.ts`),
so its scripts do not run and its external references do not load; its markup is never
inserted into the page, an iframe or an object. A file that fails to draw shows the image
error state. A local HTML file, or an SVG too large to read whole, remains source text in
the app; its reader explains how to view the finished output and offers **Show file** to
reveal it in the file manager.
Opening it in a browser is a separate action. The app chart schema does not accept arbitrary
HTML, JavaScript, Vega specifications or custom slider controls. A chart or image
does not become independent evidence, and screenshot-derived prices are not exact
historical market data.

## Evaluation

The [evaluation runner](../../apps/loupist/evals/README.md) owns the held-out
tasks, Memory conditions, comparison setup and scoring. It measures correctness,
completion, latency and cost across all attempts, including failures and retries;
unknown measurements remain missing. Local execution, authority, persistence
and UI checks run separately from paid model evaluations.

Numerical correctness and a completed reply do not establish original-document
retention or a measured speed improvement. Record those outcomes separately in
evaluation reports.
