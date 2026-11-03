# Earnings and statement review

Adapted from Anthropic financial-services, earnings-analysis and its workflow,
and knowledge-work-plugins, financial-statements (Apache-2.0).
See [upstream provenance](../../NOTICE.md).

## Evidence and comparison basis

Establish the fiscal quarter, release date, document version, and cutoff.
Read the requested earnings release and filing. Use a transcript or supplemental
presentation when it can explain an important change. A release's reporting
period is different from its publication time.

Build a compact actual / prior comparable / prior guidance / consensus table
only for material metrics. Each comparison needs a consistent definition.
Consensus must be dated before the announcement to establish a surprise.
A later estimate cannot become the pre-release expectation. If the comparator
is unavailable, report the change you can establish instead of inventing a beat.

## Results snapshot (before and after)

For an asset the user follows, the app keeps one snapshot per results date.
Before the report date, record company guidance first, then consensus with its
provider and date (FMP or Intrinio only), with `earnings_snapshot_save` side
`expected`. After the release, record the reported figures (stage `preliminary`
for a release or 잠정실적, `final` for the quarterly or annual report) and any
guidance for a later period (`guidance_new`). Cite the evidence that prints each
value in its unit and scale. Never invent a comparison or write a difference:
the app computes differences, ranges and revisions and shows them on the card.

## Explain what changed

Separate revenue changes from price, volume, mix, acquisitions, and currency
when supported. Separate operating margin movement from tax, financing, and
one-time items. For EPS, reconcile net income and the weighted diluted share
count. Percentage points and percent changes are different measures.

Compare updated guidance with the previous guidance on the same basis. Preserve
ranges, withdrawn guidance, and changes in definition. Separate management
explanations from a causal conclusion supported by the data.

## Statement and earnings-quality checks

Reconcile assets with liabilities plus equity, cash opening balance plus
cash-flow movements with closing cash (including FX where reported), and
income/retained-earnings movements with dividends and other disclosed changes.
Use financial-sector-specific layouts where appropriate. Investigate rather
than automatically reject a reconciliation gap.

Inspect accruals versus operating cash, receivables and inventory movements,
capitalized costs, recurring adjustments, dilution, and changing segment
boundaries when they affect the question. Compare like periods; do not silently
mix restated and originally reported numbers or consolidated and separate data.

Conclude with decisive changes, unresolved items, and which prior thesis
assumptions need revision. Follow the requested length and format. An earnings
review does not require DOCX, a fixed chart count, or materials published
within three months of today.
