# Watchers

A watcher checks a saved condition on a cadence and posts an alert to the home Thread. It does not notify the operating system.

### Watchers

`knowledge/watch.rs` stores each Loupist's watchers: `knowledge_watch_settings` (a switch
per asset and kind, guarded by the asset's revision in `knowledge_watch_assets`),
`knowledge_watch_state` (the pause and the last check), `knowledge_watch_routes` (per
asset and route: the last check, `ok`, `failed` or `needs_connection`, the connection and
whether the starting point is set) and `knowledge_watch_events` (kind, a natural key
unique per asset and kind, fact date, facts of at most 2 KiB, evidence, when it was seen
and where it was delivered: `message`, `morning` or `baseline`). No holdings quantity or
value enters them.

Equity kinds are `filings`, `results`, `insiders`, `holders`, `flows` and `dividends`;
filings, results and dividends start on, the others off; flows apply only to Korean
stocks, dividends only to an asset that paid in the last two years. Class kinds
(`knowledge/watch/class.rs`) are `cryptoMove` (≥10% in a day or ≥20% in 7 days),
`drawdown` (crossing −20/−30/−50% below the 90-day high), `depeg` (a USD stablecoin
closing outside 0.98–1.02; price kinds do not apply to stablecoins), `tvl` (DefiLlama
value locked down ≥20% in 7 days, for coins mapped to a DefiLlama chain or protocol),
`futuresMove` (front contract ≥5% in 7 days), `positioning` (CFTC managed-money net
position changes sign, for 17 mapped roots) and `oilInventory` (a new EIA weekly report,
crude only). They are read from the route's rows, only the latest complete day or report
can fire, and a level already crossed at the point before does not fire again. Class
kinds start on only in a Loupist made from that class's template, otherwise off; a first
explicit choice turns off only the same class's other kinds. Class alerts have no
observation row; a price read still records the page's closes.
Detection reads structured rows and dated observations, never model text, and one filing
fires at most one alert: 8-K, 10-Q, 10-K, 6-K, 20-F, DEF 14A and S-1 (amendments
included) or a Korean filing are New filings; a Schedule 13D or 13G, or a 대량보유 report,
is a big holder crossing 5%; Forms 3, 4 and 5 are read only through the insider route,
where three distinct open-market buyers (Form 4 code P) within 14 days make insider
buying; three Korean 임원ㆍ주요주주 reports within 14 days fire the same kind with the
buy side unchecked. Flows fire when foreign plus institution net buying over the last ten
trading days has the opposite sign to the ten before and is at least 1% of those days'
traded value. Dividends fire on a declared dividend or record date; results fire when an
announced or IR date is within seven days (again if it moves), never for a legal
deadline. A route's first check sets the starting point (`baseline`) and fires nothing.

Asset observations include `insiders` (a purchase cluster) and `flows` (a turn in
buying), which What changed ranks with results rows; the `holders` kind is reserved and
nothing records it yet. `erase_loupist_private` deletes the results and watch tables with
the asset tables.

### Local watchers

While the app runs, `commands/watch.rs` checks what each Loupist follows against data
sources only, with no model call; nothing runs while the app is closed. A loop started
with the scheduler ticks every minute, and the first scheduled check waits two minutes
after launch. A Loupist is checked after watch consent and while its watchers are running. Existing active briefings retain their earlier opt-in on first access. New explicit watch requests enable the chosen asset and kind. Creating a new briefing leaves watch consent unchanged.
[Watch kinds](watchers.md#watchers) map to routes: filings (holders, and every
Korean kind except flows; OpenDART, or SEC filings through OpenBB), insider trades
(`sec`, `fmp` or `intrinio` only), the earnings calendar (`nasdaq`, `fmp` or `tmx`; one
read for every US asset), dividends, Korean investor flows (KIS), prices
(`crypto_price_historical` for the coin's USD pair, `derivatives_futures_historical` for a
futures root), value locked (`defillama_read`), CFTC positioning (`cftc_cot`, `cftc`
provider only) and EIA's weekly petroleum report (`eia` only; one read for every oil
asset). An active results task also needs filings, and the calendar for US assets.
Filings and insiders are due hourly, prices and value locked every six hours, the
calendar, dividends, positioning and the petroleum report once a day after 06:00 local
time, and flows on KRX trading days (published holidays skipped) after 18:30 Seoul time.

Reads go through each Loupist's own executor (`AppToolExecutor::for_host`: its captured
grants and the unattended allowlist). One read per route and asset serves every Loupist
that wants it: the first one holding the grant reads, the others get a copy in their own
dataset store, and a Loupist without the grant records `needs_connection`. The app makes
at most 60 reads per rolling hour across all Loupists (`MAX_READS_PER_HOUR`); a pass
reads the least recently checked routes first and defers the rest. A failed read keeps
the last good state and names the connection that did not answer. Check now
(`watch_check_now`) checks every enabled kind of one Loupist once, ignoring cadence but
not the budget, one check at a time.

A check records dated observations on the asset pages and turns new ones of the kinds
that are on into alerts; a route's first check only sets its starting point. The first
check after launch, or the day's first check, hands its alerts to the morning message;
later ones post in the [home Thread](../ui/home-thread.md). No OS notification is sent for a watch
event. Watch for on the asset page and Schedule › Watching use `watch_settings_list`,
`watch_setting_set` (guarded by the asset's revision; turning a kind on needs consent and
a kind that applies), `watch_pause_set`, `watch_check_now`, `watch_events_list` and
`watch_events_seen`; Rust emits `watch-changed` after each change.

## Code

- Watchers: `src-tauri/src/knowledge/watch.rs`, `src-tauri/src/commands/watch.rs`, `src-tauri/src/chat/home.rs`, `src/components/assets/WatchFor.tsx`, `src/components/briefings/`, `src/components/chat/ThreadWatch.tsx`, `src/contracts/watch.ts`
