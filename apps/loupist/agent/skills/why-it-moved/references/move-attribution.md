# Move attribution

## Timing

A one-day move runs from the previous session's close to that day's close.
Exchange holidays have no row; use the last trading day before the window (for
KRX data, `missing_dates` lists them). News released after a market closes moves
the next session. KRX trades 09:00–15:30 Asia/Seoul; US markets 09:30–16:00 New
York time. A cause must be dated before or during the move; something published
later can explain only later trading.

## Market and sector

Compare with the main index of the listing market (KOSPI or KOSDAQ, S&P 500 or
Nasdaq) and, when one fits, a sector index or fund, over exactly the same dates.
The gap is in percentage points; compute both moves with `percent_change` and
state them side by side rather than doing arithmetic in prose. A move in line
with the market needs little company explanation.

## Kinds of reasons

Results or a preliminary release, guidance, capital raises (유상증자, convertible
bonds), buybacks, ownership reports, index changes, regulatory decisions,
sector news and macro data. Some moves are mechanical: on an ex-dividend date
the price drops by about the dividend, and splits or bonus issues change the
price without changing value. For a small move, compare it with the stock's
usual daily range (`returns_summary` volatility) and say it may be noise.

## Korean investor flows

`kis_read` with `domestic_stock.investor_trade_by_stock_daily` gives net buying
by foreign investors, institutions and individuals for each day (values in
millions of won); `domestic_stock.daily_short_sale` gives short-sale volume.
Flows show who traded, not why. They are final after the close.

## Wording

Keep three registers apart: "reported" (a dated source says it), your reading
("this may reflect…", labelled as inference) and unexplained. Say what you could
not check, such as the market comparison or news behind a paywall. Do not
predict the next move.
