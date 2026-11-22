# Asset pages

## Asset pages

A Loupist keeps one page per catalog asset ID. The page shows the user's name
and reference fields, the price and chart, Journal reasons and lines, the
current read and its moves, and its link to World. The page, its research
and its saved price read belong to that Loupist; another Loupist that follows
the same asset keeps its own ([What you follow](follow.md#what-you-follow)).
The page's Follow reflects and changes this Loupist's follow only.
Unfollowing removes that follow. The page and research remain available by
asset ID.

## Open and match an asset

The renderer sends an asset ID when opening a page. A saved link can fall
back to its exact reference ticker. An unknown asset returns an error, and an
ambiguous ticker returns candidates.

The model's `asset_page_read` and `journal_save.thesis.asset` resolve an ID first,
then an exact reference ticker, an exact name, and a name containing the query.
Multiple matches at the selected step return candidates. Reference fields
describe the listing the user or agent chose. An exchange code and a supplied exchange name have separate fields. A code alone leaves the name unknown.

A read under a historical cutoff keeps the earlier reasons, lines and dated
moves. It leaves out the current read, price and saved call.

### Saved links

A saved asset link resolves its ID, then its exact reference ticker. An unknown
or ambiguous ticker shows the corresponding notice with a route to Following.

## Saved price reads

A price originates in a retained native or bundled read in the current
turn. With Memory saving enabled, the agent calls `price_read_save` with the
asset ID, the receipt's `citation_url`, and JSON Pointers to the selected
fields. For array results it selects the rows first, then applies field
pointers within each row. Rows need parseable dates, preferably ISO. The latest
parsed date or time selects the price, and the same parsing supplies chart days.
The observation keeps the kind identified by the selected provider field, such as
close, adjusted close, last, bid or ask. A generic `price` field leaves the kind unknown.

The host checks that this turn acknowledged the result, that its original
route is eligible for unattended read access, and that the retained payload
contains a finite scalar and a stated date. A canonical decimal string can
supply a scalar. The receipt and dataset preserve the source value. The
tool echoes the staged value. The host holds the extracted price, chart
points and replay arguments in memory throughout the pending save.
It can commit them after the cited dataset receives a newer revision.

When Memory can save, the host applies the reply's follows in the catalog
before it checks the staged reads. It saves each read for the same asset ID
while that asset is followed and records it in the reply's Memory changes.
If that ID is unavailable, the receipt lists the price read under items
that were not kept. A followed asset with no saved read carries
`price_read: "none"` in the followed context and can get its first saved
read on a later reply.

The saved call keeps its route and arguments. History can use a second call
with two designated ISO date arguments and a rolling window. The rest of the
arguments stay as supplied. The chart keeps one point per stated date. A
history read can fill the chart when the source offers it.

## Host reads and refresh

On page open the host repeats the current call with up to 15 minutes of
reuse. Check now asks for a fresh result. Replay uses ordinary tool authority
with the Loupist's captured connection grants and records the returned
provider, value, date, currency and retained dataset reference. The asset page
and Bot use the same stored value. The source chip on the page opens the
provider URL or a receipt with the dataset ID and revision. Currency is empty
when the result omits it.

A failed replay preserves the last successful price and marks the read
`failed`. Losing the connection marks it `needs_connection`. A later turn
can replace a saved read. Check now replays a failed saved read once before
asking the agent for a replacement. A burst of follows from the UI or an
explicit check can start a short delegated price-setup run for a followed asset.
Each Loupist can have one setup in progress per asset. The setup sessions remain
searchable and stay out of the sidebar's recent sessions. Each asset keeps its
latest two: the one that set up, or last tried to set up, its current read, and
the one before it to compare with when a working read breaks and a new setup
fails. Setup runs only for an asset without a working read, so an older session
is an attempt a later one replaced. When a setup run ends, and at startup, older
sessions are deleted through the user's Delete path (`release_sessions` in
`commands/assets/setup.rs`), which reads each session's assets from the JSON
line its request ends with. A session the user wrote in stays, as does one whose
request cannot be read; one with no request set nothing up and goes. Saved reads,
prices and Memory the run saved stay.
If any asset in the run still lacks a ready saved read when setup finishes,
Bot posts the run's reply as one message. A run that fails without a reply
generates a short message in the app language naming the assets still missing
a ready read. A busy Bot receives the message through its normal delivery
pass. Setup sends no desktop notification.
`asset_page_get` and `asset_price_series` accept `storedOnly` for renderer
updates after `assets-changed`. World inspector supplies `entityId` to
`asset_page_get` for a stored thesis read.

## Thesis and World

Reasons and lines are Journal entries. `thesis_update` cites dated evidence
from the reply. The host checks the allowed step and verifies a line's
reported number against the cited result. A crossed line records a move and
can reach Bot as a host post. The user can undo the latest move and unlink a
World entity. See [Thesis](thesis.md).

## Reading the page

The header shows the monogram tile, the name in the interface language (`useAssetNames`:
the saved name, or a Korean listing's Hangul name from World; the page hands that name to
the app header, and the palette lists a followed asset by it, so one name stands for the
asset on the screen), then the reference ticker
and exchange (its supplied name, otherwise its code) when the asset has them, and Following or a Follow action (`asset_follow` by
id). Check now sits on the right and is unavailable offline. Below 1024 px, where the ask
box falls under the main column, an accent "Ask {Loupist}" beside it moves focus to that
box; from 1024 px the box heads the side column and the button is not shown. With a long
name the actions wrap under the name instead of widening the page; the name takes two lines
before it is cut and shows whole under the pointer, and past 28 characters the ask field's
placeholder leaves the name out (its accessible name keeps it).

The headline, price receipt, Following rows and chart values use the same locale-aware
price formatter. A stated currency uses its standard fraction digits. Without a currency,
values of magnitude one or more show up to two decimal places and smaller values
show up to six significant digits. The receipt
names the quote dataset by instrument, provider, available read and observation day. Its Details disclosure contains the selectable dataset ID and revision. When rounding changes the value, it shows the exact stored number under “Stored value”. A stated quote kind appears before the date on the headline
and in the receipt, for example “Close · Nov 19”. A quote without a kind
reads “As of Nov 19”. A stated time adds the viewer's local clock. The provider
and source chip follow the date in the headline. The display currency adds
one "≈" line when the price has a currency (currency design §5). The saved read's state adds
one line, each with one action: `failed` keeps the last price with its date and says the
latest read failed (Try again); `needs_connection` says the read's connection is off for
this Loupist (Open Connections); with no saved read or price for a followed asset the page
offers Check now and Find a price. Opening a
page lets the host replay with 15-minute
reuse; the renderer sends only the asset id. Check now calls `asset_price_check`: a replay
without reuse, with delegated setup for a missing read or a retry that still
fails. The page shows when setup is running, with no action on that line, and Check
now stays unavailable (still focusable) until it settles. When the action pressed in
the price line gives way to a new line, focus moves to that line. An unfollowed asset asks the user to follow it first.
If Check now cannot start setup because there is no AI account, the asset page directs
the user to Settings → AI accounts. Bot offers to connect an account. Only a setup
run that starts can post a missing-price report in Bot.

The chart reads `asset_price_series`: the optional history read's points, else the points
replays stored. It offers the ranges the points can fill (1W to 1Y, opening at three months
for a year) and reads each point by its date and value; fewer than 20 points say so. A
failed read with a price on screen offers a retry.

The main column then holds the thesis history: the Loupist's moves, newest first, each with
its date and sources, and the day the user wrote the thesis (ten rows, then Show all). The
side column holds the ask box (a question goes into Bot's composer with the asset's name and
listing; nothing is sent from here), the thesis card (`AssetThesis.tsx`: the read on its
four-step ladder after a recorded verdict move, otherwise “Not reviewed yet”, with Undo for the Loupist's latest move, `asset_verdict_undo` by move id;
the user's reasons and lines as Journal entries) and World line when the page matched an
entity: its supplier and customer counts, Open, and "Not this company", which unlinks it
(`asset_entity_set` with `entityId: null`).
