# Results before and after

Before and after is a host comparison of captured figures. The scheduled task that prepares captures is in [Scheduled runs](../runtime/scheduled-runs.md#results-before-and-after).

## Results before and after

For each results event of a followed equity ([records](results.md#results-events)),
the app computes every difference and revision itself (`knowledge/earnings/diff.rs`) when
the card is read; the model never sends one, and there is no beat or miss label. Two
values compare only with the same measure and period, the same basis when both state one,
and the same unit and currency; otherwise the difference reads "Not comparable" with
the reason (basis, period or unit). Against an expected range the reported value is
above, within or below, with the change from the top or bottom when outside; against a
point it is the change from that point. A change is a percent change for amounts and a
percentage-point difference for percentages. New guidance is compared by midpoint with
earlier guidance for the same period from this asset's other events: New, Raised,
Lowered or Kept. A final value that differs from the preliminary one by more than half a
unit of its last printed digit is a revision. Each computed number carries its formula
and inputs (a percent change goes through `calculate`'s `percent_change`) and shows as a
number chip. Price reaction runs from the last recorded close before the report to the
first after it ("Waiting for the next close" until then).

The [results task](../runtime/scheduled-runs.md#results-before-and-after) captures without a model.
Before: at 18:00 in the listing's time zone the day before the report date (14 days
before a legal deadline), or at once for a date first seen a day or less away; it
records closes and, for a non-Korean equity, consensus revenue and EPS for the quarter
only from FMP or Intrinio (otherwise the card says a key is needed) and the price-implied
growth: a `reverse_dcf` over the last annual cash-flow, balance and income statements
(WACC 8–10%, terminal growth 2–3%, five years) at a close on or before the day before the
report and at most a week older. A before-capture that starts on or after the report
date runs under `as_of` = report date − 1 and leaves consensus out ("Consensus wasn't
captured before the report"). Each capture before the report date replaces the host's
earlier expected rows; after it, expected rows are locked. When every attempted
before-capture read fails, Rust records the failure and allows at most six attempts
for that event, waiting 1, 2, 4, 8 and 16 hours between failures. The counter and next
retry time survive restart; after the sixth failure the event stops retrying. A new
earnings event starts a new budget. After: 30 minutes after the
release is seen, reported revenue, operating income, net income and EPS from a statement
read once a provider has the quarter (`fmp`, `intrinio`, `polygon`, `tiingo`, `tmx` or
`sec`; Korea `dart_financials`), retried hourly up to six times; the same runs with stage
`final` after the quarterly or annual report. Korean preliminary figures, company
guidance, a broker's estimate and the call's tone come from a reply instead.

`earnings_snapshot_save` (`commands/tool_executor/earnings.rs`) records them for one
event (`eventId`, or the one around today), staged until the reply commits:

- Refused under an as-of date (`earnings_as_of`) and without both Memory flags
  (`earnings_private`); the asset page and event must exist.
- A call saves 1–12 rows or a tone. Each row cites 1–3 `citation_url`s of this turn,
  and every value must be printed in a cited source in the given unit and scale (1, 1e3,
  1e4, 1e6, 1e8, 1e9 or 1e12; none for a percent), checked like a number chip
  (`earnings_value_not_found`).
- `expected` rows (`guidance`, `consensus`, `broker_estimate`) cite no source dated on or
  after the report date (`earnings_expected_late`) and are written only before it
  (`earnings_expected_locked`); consensus comes only from FMP or Intrinio results
  (`earnings_consensus_provider`), a broker's estimate only from `kis_read`
  (`earnings_broker_source`), and `implied` only from the host (`earnings_implied_host`).
- `reported` rows (stage `preliminary` by default, or `final`) and `guidance_new` rows
  (which name their period) need at least one cited source dated on or after the report
  date (`earnings_reported_early`, `earnings_evidence_undated`); one reported row per
  measure, basis, period and stage (`earnings_reported_exists`).
- An optional tone is one sentence of at most 200 characters with 1–2 quotes that appear
  exactly in a cited call transcript, read or retained as a Source (`earnings_tone_source`,
  `earnings_tone_span`).

At commit Rust leaves out a row the event no longer accepts (expectations locked, or the
reported row already exists) and stores the rest with origin `chat`, or `briefing` for
an unattended run.

### Results events

Around each results date of a followed equity, `knowledge/earnings.rs` keeps one event
per asset and calendar-quarter period ("Q3 2026"; id `ern_` + 16 hex digits of
sha256(`asset:period`)) in `knowledge_earnings_events`: the report date and its source
(`calendar`, `ir_notice` or `legal_deadline`), status (`upcoming`, `before_captured`,
`reported`, `final`), the release and final filings, capture times and flags.
`knowledge_earnings_rows` holds the values: side (`expected`, `reported` or
`guidance_new`), a measure key (`revenue`, `operatingIncome`, `operatingMargin`,
`netIncome`, `eps`, `fcf`) or `label:` and the run's own words, basis, period, a point or
a low–high range as base-unit decimal strings, unit, currency, kind (`guidance`,
`consensus`, `broker_estimate`, `implied`, `reported`), stage (`preliminary`, `final`),
provider, evidence, fact date, the cutoff of a late capture and origin (`host`, `chat`,
`briefing`). `knowledge_earnings_tone` keeps one call-tone sentence with its quotes, and
`knowledge_earnings_notices` the Home messages owed. Every row cites its evidence;
[differences and revisions](results.md#results-before-and-after) are computed when the
card is read and never stored from model text. Holdings never enter these tables.

Events follow the page's observations without a model. An announced calendar date
creates or moves its period's event; a Korean equity always has its next legal-deadline
event (45 days after a quarter or half, 90 after the year), labelled "due by", never
expected; an IR notice (기업설명회(IR)개최) filed in the last 60 days is read once with
`dart_filing_read` for its date. An IR date outranks a calendar date, which outranks a
legal deadline, and a date moves only before results are reported. The release is a
Korean 잠정실적 disclosure, or an 8-K or 6-K with item 2.02 (from three days before to
ten after the date) or any 8-K within a day of it; the final report is the Korean quarterly,
half-year or annual report for the period (with the 감사보고서 as an extra step for a year
end) or a 10-Q, 10-K, 20-F or 40-F filed after the release within 120 days of the period
end. The first matching release and final report stay attached when a pass sees
multiple filings; the first matching audit filing also stays attached within and
across passes, and attaching a release preserves current audit and capture flags.
At a glance's "Next earnings" falls back to the next event, a legal deadline marked
`dueBy`; the morning message's "Coming up" strip lists such a deadline as "Results due (filing deadline)".

## Code

- Results before and after: `src-tauri/src/knowledge/earnings.rs`, `src-tauri/src/commands/earnings.rs`, `src-tauri/src/commands/tool_executor/earnings.rs`, `src-tauri/src/scheduled_tasks/`, `src/components/assets/EarningsCard.tsx`, `src/components/chat/`, `src/chat/earningsLinks.ts`, `src/contracts/earnings.ts`, `agent/prompts/earnings.md`
