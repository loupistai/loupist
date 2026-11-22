# What you follow

Each Loupist follows its own assets. The app catalog (`assets` in
`catalog.sqlite3`) keeps one identity per asset for every Loupist: an ID, a
name, a creation time and optional reference fields (FIGI, ticker, exchange
and security type, with their origin). The host records the listing labels as
supplied. `asset_follows` records which Loupist follows which asset and since
when; the catalog creates it when it opens. Every command, tool and run reads
the follows of the Loupist it acts for. A new Loupist follows nothing until
its New Loupist starters or the user follow something. Removing a Loupist
removes its follows and keeps the catalog.

What hangs off an asset is per Loupist and lives in its folder: the asset
page, its Journal reasons and lines, the read and its moves, the saved price
read, the stored price and chart points (see [Asset pages](asset-pages.md)).
Two Loupists that follow the same listing share its ID, name and reference,
and keep their own follow time, page, thesis and prices. A reference that one
Loupist's follow adds to a name-only asset is the catalog's, so every Loupist
reads it.

Typing in Following filters the local list. A submitted lookup sends its
query to OpenFIGI under the Asset lookup connection. Picking a candidate
follows the listing with its reference fields. The host reuses a matching
FIGI first, then adds the reference to a same-name asset that has none, or
creates an asset. Following by name alone reuses an exact name match
regardless of its reference, or creates a name-only asset. Exact name
matching trims whitespace and ignores case. When several assets share a
name, a name-only follow prefers one this Loupist already follows, then the
lowest ID. The agent follows by name through `loupist_setup`. An asset can
stay followed without a matching listing or saved price.

Unfollowing removes this Loupist's follow and leaves the ID, name and its
research. A later follow that matches the asset follows the same ID again
from that time. The host emits `assets-changed` when a follow or stored
price changes; each surface reads its own Loupist's list again.

Each Loupist's World receives immutable “What you follow” Sources with
provider `loupist:followed`, listing that Loupist's follows by name with
reference tickers when present. Each Source gets an ordinary World job.
Source synthesis runs when the Loupist opens and after a short burst of
follows. Unfollowing leaves earlier Sources. Re-following an asset that
appears in an earlier Source adds no duplicate Source.

## Following search

Following is a search field and the selected Loupist's followed assets (`assets_list`), in one column under
the shell header, which is the page's only title (`src/components/briefings/FollowingPage.tsx`).
The list is read again on its own follows and removals and on every `assets-changed`. A row is
the asset's one name (`useAssetNames` over the saved name), its listing reference
(`TICKER · exchange · security type`, or "Name only" for an asset followed by name), and a
visible Stop following (`asset_unfollow` by `assetId` at once, no confirmation; a failure keeps
the row with a retry). The name opens the asset page by `assetId`. Rows sort by name. The row shows the latest stored price when available. The list reads
these prices together from `watchlist_quotes` and uses the asset page’s price formatter. The field takes focus only when nothing is followed or when Bot's "Add assets to
follow" opened the page.

Typing replaces the list with options and sends nothing: followed matches (a case-insensitive
substring of name or ticker) show at once, then "Search listings for '{text}'" and, last,
"Follow '{text}' as typed". When the trimmed text matches a followed asset's name
exactly, ignoring case, the last option says "Already followed" and opens its
page. Enter (with no option active) or Search listings sends the
submitted text once to OpenFIGI through `asset_lookup`; a second Enter on the same answered
text sends nothing, and no request runs on typing, focus, page open or offline. Exact ticker
matches come first. For a name, common stocks, depositary receipts and ETFs precede derivatives
and debt. Regional listings with the same OpenFIGI composite identity share one result. When
OpenFIGI returns the composite listing, it represents the group. Different issuers and country
listings stay separate. Each result shows its name and `ticker · exchange · security type`; a
readable exchange name takes precedence over the code in search results, followed rows
and the command palette. When a response omits the name, the host consults the bundled
OpenFIGI equity exchange directory. Unknown codes and codes shared by multiple venues
keep the name unset. Following localizes known security types. Following, the command
palette, the asset header, its price Source dialog and the followed asset's World
inspector use the exchange directory's ISO code to display country labels in the
interface language. Provider venue names and unknown security types display as supplied.
Following preserves the host’s lookup order. A listing already
shown among local results by FIGI appears there once. Picking a candidate calls
`asset_follow_name` with the candidate's name and its FIGI, ticker, exchange code, available exchange name and security type
as the reference; Follow as typed calls it with the trimmed text and no reference. With an AI account, the host can start
one short delegated run per burst of follows to find each new asset's saved read. The field
clears, the followed row comes into view and
the follow is announced; a failure keeps the query and results with a retry. Down and Up move
the active option with focus kept in the field, Enter takes it, Escape clears the active option
and then the query; input-method composition keeps its Enter. One line under the options says
where listings came from (and that the text was sent to OpenFIGI), that none matched (with an
action that drafts a follow request in Bot's composer when the name is available to follow),
or why the search did not run: offline,
Asset lookup switched off for the Loupist (`connection_unavailable`), a saved OpenFIGI key
that was rejected (`credential_rejected`) or cannot be read (`credential_locked`,
`credential_unavailable`), each with Open Connections and no Try again,
OpenFIGI asking to wait (`rate_limited`) or another failure (with Try again). The field's
accessible description says what typing and Enter do; a long name ends in an ellipsis and
shows whole under the pointer. A cleared or
changed field drops a late answer.
