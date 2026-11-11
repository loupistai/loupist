# Map and Lens

The Map draws the observed World. Lens highlights names the host already recorded with `applies_to` and does not invent neighbors. What the Loupist knows, including Reset, is [Memory](../knowledge/memory.md#what-the-loupist-knows).

## Map and Library

Map's primary surface is the observed World: entities, claims and the evidence
behind them, from the `knowledge_map` projection. The status line names World and joins its parts (scope count, weekly count, disagreements, updating) with one " · " separator. On a narrow map it wraps inside the visible canvas, so each part stays whole.
World search, layout and replay use the drawable map items. The header's Panel kind selector opens Lens (research principles and methods) or World beside the current chat. The Bot's sidebar contextual menu opens scheduled-task history. Lens shows its title on one row under the panel's own heading, with no Back button of its own (the panel's Close is the way out); a page title that is read after opening takes focus without a ring. This release leaves pack Share off that row ([This release](../knowledge/sharing.md#this-release)). Linked principles and methods open the shared reader. Corrections go to the Bot with the item attached. A newly added relationship can open directly from an answer in the World panel. The Map's Share control exports the image or growth replay.

Under **Filter**, **Highlight Lens applications** highlights the entities the host already recorded with
`applies_to` and lists those names. A Lens with no such entity stays unlinked:
the Map says it has not been applied and does not invent neighbors or dim the
World. A new Loupist can have only its seeded standing Lens in the projection;
the Map still shows first-map guidance and Add source until it has a drawable
World node. The filtered-results message is for existing World nodes hidden by
filters. Fit, Replay, Export and List view stay unavailable in that empty state.
The projection also carries watchlist and holding markers. `applies_to`
weights count turns, not correctness or causal influence. Per-kind
total/shown/truncated counts make the bounded projection explicit. The standing
perspective is the base view a Loupist starts from. In the application filter and the
Lens detail, the Pages list and the page reader, the host's untouched title "Outlook"
and the English default heading of a suggested section key (Beliefs, Focus, ...) show
in the interface language (`knowledge/hostDefaults.ts`). A title or heading someone
chose is kept, and the title stays marked as the base perspective. The seeded placeholder sentence is not
presented as something the Loupist learned.

Entities draw in eight categories, in legend and filter order: Company (World kind
Organization), Person, Product (Product), Technology (Technology), Industry
(Segment), Market & asset (Asset), Event & metric (Event, Metric) and Other (Place,
Policy, Entity and any other kind). Korean labels are 기업, 인물, 제품, 기술, 산업, 시장·자산,
사건·지표 and 기타. A Metric
stays in Event & metric, draws as a small faded dot, takes a canvas label only
when selected or hovered, and does not hide a neighbour's name; name chips outside the Map (the morning message, the chat rail, the palette) use the same faded color; the number itself
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

The Library drawer contains Sources and Notes (Journal and working notes).
On a map narrower than 720px, including the default 460px side panel, the Library covers the canvas instead of leaving a strip too narrow to read. Closing it brings the map back.
The source and note filters stay at the top of the list while it scrolls.
Entity/claim inspectors and Source and note readers open alongside the map.
The inspector's close control stays in the corner while the facts scroll. On a map narrower than 900px the inspector is a bottom sheet, and that control stays inside the sheet.
The application filter acts on the World canvas. It highlights the entities a
Lens was applied to and names them. It does not draw the Lens, and it does not
treat a claim line as an application. When every applied name is outside the
current scope, the Map says so and can show the whole map, instead of dimming
every visible name.
Export stays visible in the toolbar; Library, Replay, Fit and List view live in More. Scope collapses to its menu when those fixed controls leave too little room.
Search, filters, a list fallback and Fit support navigation; camera/selection and
Library state are retained per Loupist. Use in Chat attaches a draft reference;
opening a reader does not itself deliver evidence to the model. Sources can be
added as Text, File or Link; link-only saves do not fetch the page.

Maps with more than 60 names open around one focal company: the remembered
center, else the first watched, a held, or the best-connected company. The scope
is Direct links, Links of links (two hops, bounded at 150 names: the focal, its first ring, then second-ring
names by degree) or All. Searching or picking a name outside the drawing
re-centers; plain selection keeps the center. With the canvas focused, arrow keys
walk the focal's neighbors clockwise, Enter selects, Shift+Enter re-centers,
Backspace steps back (ten steps) and 1, 2 and 0 set the scope. On a scope or focal change, names leaving fade out and names entering fade in over 120 ms (instant with reduced motion); leaving names take no clicks while they fade. A drawing of at most
30 names labels every name where space allows. Label priority is the focal name,
then watched and held names, then the ten best-connected names; these stay named
when zoomed out and may lie across their own spokes.
For the 1,000-name All view, keep relationship lookup for label placement indexed
by endpoint and reuse selection and learned-neighbor sets until their inputs change.
The native frame probe targets p95 at or below 33 ms without dropping those labels.

What's new, replay and export are in [What changed](../knowledge/what-changed.md#on-the-map).

Chat's workspace includes a mini map, with its counts labelled ("4 names · 3 relationships"), and a route to the World panel. The answer's
receipt line counts names and relationships added by that message (growth events
with its `originMessageId`); clicking either segment selects and highlights them
on the mini map. World growth events carry a chat id only when the published
Source was saved from a Chat turn; a Source added on the Map stays unattributed,
and the receipt counts only events for that message.
While a reply runs, a step that read World nodes shows "· n on the map" (the host
attaches the node ids of acknowledged World reads to the step, at most 32), and the
mini map glows those nodes until the run ends; names stay outside the glow.

A small filled warning dot at a node's upper right marks an entity touched by a
disputed claim, separate from selection and from the watch or hold ring; the legend
shows a Sources disagree row while one is drawn, and a row for the highlighted
names while a Lens is applied, plus the red dashed conflict line when drawn. The inspector lists those conflicting
reports above Key facts, with source titles, dates and exact Source actions.
Reports that share a relation and the same role-labelled endpoints are one group,
because a World claim has no conflict-pair id. Explain the disagreement under them opens
a new topic Chat whose composer holds the entity, up to three disputed relations as pins
and a question, never sent. When the workspace covers the chat, that action closes
the cover so the draft is on screen. A `world_read` whose view holds disputed claims adds
`sources_disagree` to its result (the claim count, up to eight entity ids and a hint to
show the difference with a [conflict card](../analysis/grids.md#grid-cells) instead of averaging or
picking one). Perspectives, principles and methods appear in the application filter,
including ones with no `applies_to` edge. The legend lists at most six rows
built from the kinds actually drawn, ranked by how many drawn names each covers,
with one shared ring row (dashed: watchlist, solid: holdings) and, while a Lens
highlight is on, one row for the highlighted names; the
least-drawn rows fold under More.
Below 900×600 the legend floats over the map without shrinking the fit. It starts open on a person's first Map
visit at any window size, and a collapse is remembered app-wide. The stats chip carries the conflicting count.
A link card in the Bot opens the Map with `view.highlightEntityIds` set to the
path's nodes, seed first, so the same highlight that marks disputed names marks the
seed, each middle name and the far end; nothing else on the canvas changes.

## Code

- Map / replay / export: `src/components/map/`, `src-tauri/src/knowledge/map.rs`, `src-tauri/src/commands/export_file.rs`
