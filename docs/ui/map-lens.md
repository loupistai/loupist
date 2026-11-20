# Map and Lens

The Map draws the observed World. Lens highlights names the host already recorded with `applies_to` and does not invent neighbors. What the Loupist knows, including Reset, is [Memory](../knowledge/memory.md#what-the-loupist-knows).

## Map and Library

Map's primary surface is the observed World: entities, claims and the evidence
behind them, from the `knowledge_map` projection. The status line names World and joins its parts (scope count, weekly count, disagreements, updating) with one " · " separator. On a narrow map it wraps inside the visible canvas, so each part stays whole.
World search, layout and replay use the drawable map items. The sidebar's Lens and Map rows (⌘3, ⌘2) open Lens (research principles and methods) or the Map as full pages in the main area, each under its own page header ("Lens", "Map"); a conversation keeps its own map in its side panel ([workspace panel](workspace-panel.md)). Deep links, the palette's names and principles, What's new and Bot cards open the same pages, and the Map's selection, focal name and scope carry over through the Loupist's map navigation and its remembered center and scope. The Bot's sidebar contextual menu opens scheduled-task history. Lens shows its title on one row under the page header, with no Back button of its own; a page title that is read after opening takes focus without a ring. In a wide Lens page the rule/method/history list and its document stay in separate, independently scrolling columns; a long list cannot take away the document's height. In a compact Lens page, opening an item replaces the list, and Back restores it. Compact section tabs retain their full accessible names when the visible labels are shortened. This release leaves pack Share off that row ([This release](../knowledge/sharing.md#this-release)). Linked principles and methods open the shared reader. Corrections go to the Bot with the item attached. A newly added relationship can open directly from an answer on the Map page; a relationship whose two ends are one thing (same node or a shared name) is not offered.

Under **Filter**, **Highlight Lens applications** highlights the entities the host already recorded with
`applies_to` and lists those names. A Lens with no such entity stays unlinked:
the Map says it has not been applied and does not invent neighbors or dim the
World. A new Loupist can have only its seeded standing Lens in the projection;
the Map still shows first-map guidance and Add source until it has a drawable
World node. The filtered-results message is for existing World nodes hidden by
filters. Fit, Replay, Export and List view stay unavailable in that empty state.
The projection also marks each name the user follows (`watched`), drawn as a dashed ring. `applies_to`
weights count turns, not correctness or causal influence. Per-kind
total/shown/truncated counts make the bounded projection explicit. The standing
perspective is the base view a Loupist starts from; its title shows as written and is
marked as the base perspective in the application filter. In the Lens detail, the Pages
list and the page reader, the English default heading of a suggested section key
(Beliefs, Focus, ...) shows in the interface language (`knowledge/hostDefaults.ts`). A
heading someone chose is kept. The seeded placeholder sentence is not
presented as something the Loupist learned.

Entities draw in eight categories, in legend and filter order: Company (World kind
Organization), Person, Product (Product), Technology (Technology), Industry
(Segment), Market & asset (Asset), Event & metric (Event, Metric) and Other (Place,
Policy, Entity and any other kind). Korean labels are 기업, 인물, 제품, 기술, 산업, 시장·자산,
사건·지표 and 기타. A Metric
stays in Event & metric, draws as a small faded dot, takes a canvas label only
when selected or hovered, and does not hide a neighbour's name; name chips outside the Map (the activity sheet, the palette) use the same faded color; the number itself
stays in the subject's Key facts. Rust adds up to four non-Latin aliases to each
entity as `localNames`. When the UI language is Korean,
the Map shows the first Hangul one on the canvas, the tooltip, search, the list and the
inspector, with the English canonical name under the inspector heading. Search
matches the canonical name and every local name; "Ask about this" pins the canonical
name. Core relation names have curated labels; other relation names stay readable
English.

### Notes on a name

An entity's inspector lists **Notes**: the active Lens pages anchored to it, from
`knowledge_lens_pages_for_node`, each with its title, kind and summary. A note opens
the page in the Lens reader. The Lens kinds draw as Lens nodes (`lens_perspective`,
`lens_method`, `lens_principle`, `lens_subject`, `lens_idea`) with `applies_to` edges from
their anchors. The legend still shows one Lens row. What's new names a new Topic or
Idea beside new Methods and Rules.

### What the map tracks

Loupist settings list, under World processing and in plain words, the kinds of things and links the map tracks
beyond the basics, with Stop tracking on each kind, and the twenty most recent
changes. Each change says who made it (starting setup, you, a background review),
what it added or removed, and offers Undo while it still changes something. There
is no Apply or Dismiss: changes already apply
([how](../knowledge/worlds.md#presets-changes-and-undo)). The same words appear in
the Bot's "Your map now also tracks ..." message. Copy never says ontology, schema or
entity type.

Company and market entities offer a Relationships view in the inspector beside Facts:
claims grouped by relation, one row per counterpart ranked by weight, then degree,
then name, six per group then "+n more", line weight by claim weight and dashed when
disputed; Lens entries applied to the name (`applies_to`) close Other on one "Lenses applied" line with dotted rules in the method colour; pointing at a counterpart or Lens entry (hover or focus) widens its line on the canvas to 2.5 px (a Lens line stays dotted in the method colour); clicking a counterpart walks there and Back retraces. Clicking a Lens opens its document.

The Library drawer has three tabs: Sources, Journal (your entries) and Notes (the Loupist's research notes). A Journal selection, a Journal change in a chat receipt and the palette's "New Journal entry" open the Journal tab; the palette action also opens its new-entry form (`requestLibrary` in `components/map/libraryState.ts`).
On a map narrower than 720px the Library covers the canvas instead of leaving a strip too narrow to read. Closing it brings the map back.
The source and note filters stay at the top of the list while it scrolls.
Entity/claim inspectors and Source and note readers open alongside the map.
The inspector's close control stays in the corner while the facts scroll. On a map narrower than 900px the inspector is a bottom sheet, and that control stays inside the sheet. An entity's name leads its inspector: the kind and Following sit above it in the side column and follow it on the sheet's first line.
The application filter acts on the World canvas. It highlights the entities a
Lens was applied to and names them. It does not draw the Lens, and it does not
treat a claim line as an application. When every applied name is outside the
current scope, the Map says so and can show the whole map, instead of dimming
every visible name.
Library is a toolbar button. **More actions** contains Replay and List view, followed by **Export image (PNG)** and **Export replay video** under the Export label. Fit sits on the canvas beside the legend. List view is a checked item that keeps its name, as the conversation map's pressed List view button does. Escape in search closes an open result list first; with none open it reaches the Map. Closing an inspector returns focus to where the name was chosen (the canvas, a list row, search); a name opened from elsewhere returns focus to search. When the row leaves too little room (or the Map is 900px or narrower), the Scope segment folds into the Around menu, whose first section holds the three scope choices, and the Around button reads just the focal name ("TSMC ▾"). With no names yet, What's new, Filter and More are disabled and the empty Map offers Ask in Chat and Add source.
Search, filters, a list fallback and Fit support navigation; camera/selection and
Library state are retained per Loupist. Opening the Map, or drawing its first names,
frames every shown name in the area the inspector, status pill and legend leave free,
each dot whole and the outermost 24 px from that area's edge;
a selected name is the centre of that framing as soon as it is drawn, with the zoom
chosen so the rest still fits around it. A camera saved for the scope returns only
while at least half the shown names sit in that area at no less than half the fitted
zoom; otherwise the Map frames again. The camera moves only once the canvas element
has the measured size, and until the person pans, zooms or selects, the opening view
or a Fit is framed again whenever the canvas or its free area changes size, so it is
framed for the size the page settles at. The opening view also waits, for about two
seconds at most, until the renderer has given every shown name a position, and framing
leaves out a name without one: a single such name made the whole camera NaN in WebKit,
which drew only lines in the top-left corner. A later selection only pans.
The Library, What's new, a bottom-sheet inspector or a reader sheet covers part of the
canvas; `MapCanvas` measures what each covers into `--map-covered-*` and `--map-overlay-bottom`
on the stage, and the status pill, key strip, Fit, hover tooltip and filtered-results message
sit in what is left (`e2e/map-layout.mjs` checks this in preview captures and the native
smoke). A panel counts where it lands, not where its slide-in draws it, since nothing resizes
when it arrives. A camera move made while another still eases ends after it, so a reframe for
a panel opened during a Fit is not undone when the Fit ends. With no names there is no canvas
to measure, so the empty Map's message moves beside the Library by the Library's width. Use in Chat attaches a draft reference;
opening a reader does not itself deliver evidence to the model. Sources can be
added as Text, File or Link; link-only saves do not fetch the page.

Maps with more than 60 names open around one focal company: the remembered
center, else the best-connected followed company, else the best-connected company. The scope
is Direct links, Links of links (two hops, bounded at 150 names: the focal, its first ring, then second-ring
names by degree) or All. Turning on Direct links centers the Map on the entity whose details are open.
In Direct links, opening a search result centers it even when it is already visible.
Selecting a visible node keeps the center. Picking a new center or opening a name outside the drawing re-centers the Map. With the canvas focused, arrow keys
walk the focal's neighbors clockwise, Enter selects, Shift+Enter re-centers,
Backspace steps back (ten steps) and 1, 2 and 0 set the scope. On a scope or focal change, names leaving fade out and names entering fade in over 120 ms (instant with reduced motion); leaving names take no clicks while they fade. A drawing of at most
30 names labels every name where space allows. Label priority is the focal name,
then followed names, then the ten best-connected names; these stay named
when zoomed out and may lie across their own spokes. A Journal ring keeps its
ordinary place in that order; at a pane edge its name also tries the side toward
the drawing and shortens before it is dropped (`components/map/labels.ts`).
For the 1,000-name All view, keep relationship lookup for label placement indexed
by endpoint and reuse selection and learned-neighbor sets until their inputs change.
The native frame probe targets p95 at or below 33 ms without dropping those labels.

The canvas reads like a graph view. Each name is a round dot in its kind's colour (the
legend's tokens), sized by how many relationships it has; zooming in spreads the drawing
out, and a dot grows on screen to at most 1.5 times its radius (`DOT_ZOOM_CAP` in
`components/map/labels.ts`). Relationship lines stay one screen pixel wide and quiet.
Pointing at or selecting a name keeps it, its lines and its neighbours bright and dims the
rest, easing over 160 ms; the pointed dot grows slightly and glows. Names fade in with
zoom: the focal name, followed names, the ten best-connected names, a selection and its
neighbours stay named, the next best-connected names fade in between 0.6 and 0.85 times
zoom and every other name between 1.2 and 1.6 times, so labels never form a wall. Opening
a Map, names spread out from the middle of their drawing and settle into place with a
spring over 700 ms; a name that arrives in an update grows out of a name it links to. In
the dark theme the best-connected names, and a lit neighbourhood, carry a soft glow.
Reduced motion draws the settled Map and changes focus without easing. Between transitions
the canvas draws only on demand, so when the last running transition (settle, fade, glow,
pulse or replay) ends it draws one more frame: the canvas never rests on a frame from
part-way through, whichever of its timers or frames comes first. The layout itself
is computed once and frozen (`layout.ts`), so names keep their places across updates and
exports. A name pushes only on names within 200 layout units, so names without
relationships sit at the drawing's rim instead of in a far ring that Fit would have to
include. A claim from a name to itself (a value reported about it, `REPORTS`) is no
relationship (`isDrawnRelation` in `knowledge/mapItems.ts`): the layout and the canvas
leave it out, and so do the Map's and the chat Map's relationship counts and the answer
receipt's (it still marks its name as new). As a layout link of length zero it put that
name at NaN, so its dot was not drawn and, in the dark theme, its glow threw and took the
canvas down.

What's new, replay and export are in [What changed](../knowledge/growth.md#on-the-map).

Chat's workspace includes a mini map, with its counts labelled ("4 names · 3 relationships"), and "Open full map", which opens the Map page. The answer's
receipt line counts names and relationships added by that message (growth events
with its `originMessageId`); clicking either segment selects and highlights them
on the mini map. World growth events carry a chat id only when the published
Source was saved from a Chat turn; a Source added on the Map stays unattributed,
and the receipt counts only events for that message.
While a reply runs, a step that read World nodes shows "· n on the map" (the host
attaches the node ids of acknowledged World reads to the step, at most 32), and the
mini map glows those nodes until the run ends; names stay outside the glow.

A small filled warning dot at a node's upper right marks an entity touched by a
disputed claim, separate from selection and from the followed ring; the legend
shows a Sources disagree row while one is drawn, and a row for the highlighted
names while a Lens is applied, plus the red dashed conflict line when drawn. The inspector lists those conflicting
reports above Key facts, with source titles, dates and exact Source actions.
When an entity has a matched asset thesis, Facts also shows the current read and
a link to the asset page. Opening the inspector reads stored research by entity ID.
Reports that share a relation and the same role-labelled endpoints are one group,
because a World claim has no conflict-pair id. Explain the disagreement under them opens
a new topic Chat whose composer holds the entity, up to three disputed relations as pins
and a question, never sent. When the workspace covers the chat, that action closes
the cover so the draft is on screen. A `world_read` whose view holds disputed claims adds
`sources_disagree` to its result (the claim count, up to eight entity ids and a hint to
show the difference with a [conflict card](../analysis/grids.md#grid-cells) instead of averaging or
picking one). Perspectives, principles and methods appear in the application filter,
including ones with no `applies_to` edge. The legend is a key strip across the bottom of the stage, open by default at every size, with a first-visit lead line while the Map hint is unread. It lists at most eight rows
built from the kinds actually drawn, ranked by how many drawn names each covers,
with "Other" last, one row for the plain stated-relationship line, one row for the dashed ring around what you follow and, while a Lens
highlight is on, one row for the highlighted names; the
least-drawn rows fold under More.
Its band is always reserved so no node sits under it; in a narrow pane it hides while a name is selected. A collapse is remembered app-wide. Maps of up to 60 names label every dot that fits. The status pill carries counts only; disagreement uses the disputed-line colour for the line, dot, count and legend. The stats chip carries the conflicting count.
A link card in the Bot opens the Map with `view.highlightEntityIds` set to the
path's nodes, seed first, so the same highlight that marks disputed names marks the
seed, each middle name and the far end; nothing else on the canvas changes.

## Code

- Map / replay / export: `src/components/map/`, `src-tauri/src/knowledge/map.rs`, `src-tauri/src/commands/export_file.rs`
