# Fund checks

## Overlap

`holdings_overlap` adds, for each holding both funds own, the smaller of its two
weights. Map identifiers first: the same company can appear as a ticker, an
ISIN, a local listing or a depositary receipt, and share classes differ. A
provider that returns only the top holdings gives a lower bound; write "at
least". Holdings dated months apart can differ because of rebalancing; name
both dates. Two funds tracking different indexes can still overlap heavily, and
two with the same index can differ in cost and tracking.

## Returns

Use one window for every fund, starting at the later inception, and say how
long it is; under a year is anecdote. Total return includes distributions;
price return does not. A price series from an exchange is usually price return.
Compare in one currency or say which currency each return is in; a currency-
hedged class differs from an unhedged one. Report volatility and the worst
drawdown from `returns_summary` beside the return.

## Cost and structure

The expense ratio is the headline yearly fee; trading spreads and tracking
difference add to it. Korean funds publish 총보수 and a higher 실부담비용; name
which one a figure is. Small or young funds can close or trade thinly.
Leveraged and inverse funds reset daily, so over months their return is not a
multiple of the index return; say so whenever one is compared. Synthetic
(swap-based) funds carry counterparty exposure that physical ones do not.

## The table

Rows are funds. Columns: expense ratio, assets with their date, inception, the
overlap with the other fund (two funds) or with the first fund (more), and the
return over the shared window with its period and basis (price or total). A
value a source does not give stays an empty cell with its reason.
