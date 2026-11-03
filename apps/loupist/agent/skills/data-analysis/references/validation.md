# Dataset profiling and validation

Adapted from Anthropic knowledge-work-plugins, explore-data and validate-data
(Apache-2.0). See [upstream provenance](../../NOTICE.md).

## Grain and coverage

Establish what a row represents, candidate keys, row count, schema, date range,
and expected update cadence. Profile the columns used by the question: types,
missingness, key uniqueness, distributions, and plausible bounds. A warning
threshold is a question-specific heuristic, not proof of an error.

Distinguish observation time, publication time, and ingestion time. Align partial
versus complete periods, fiscal calendars, currencies, and adjustment basis.
Disclose sampled checks and retain exact counts where the task requires them.

## Joins, duplicates, and aggregation

State the join keys, intended cardinality, and unmatched-row policy. Compare
row counts, key coverage, and relevant totals before and after joining. Diagnose
many-to-many expansion before aggregating. Counting distinct ids can fix a count
but does not repair inflated monetary sums.

Duplicate-looking records may be valid repeated events; use the data semantics
or the user's rule to decide what to remove. Preserve a record of exclusions.
Do not replace missing values with zero or drop missing assets silently.

For combined rates or margins, aggregate compatible numerators and denominators
rather than averaging already aggregated ratios without appropriate weights.
Test zero denominators and distinguish undefined from zero. Inspect segment
mix when an aggregate trend reverses its subgroup trends.

## Reproduction and delivery

Keep raw files unchanged, record transformations and relevant parameters, and
verify output counts, join coverage, and a representative calculation. Cross-check
a consequential total by a second aggregation or trace it to source records.
Do not delete genuine extremes merely to obtain a cleaner result.

Report the answer and the specific quality limitations that can change it.
A full quality report is useful when requested or necessary, not a prerequisite
for every calculation. For monetary exchange, use exact decimals and decimal
strings where binary floating-point serialization would lose required precision.
