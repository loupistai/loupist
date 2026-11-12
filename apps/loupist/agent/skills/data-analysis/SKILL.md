---
name: data-analysis
description: Profile, clean, join, transform, or audit financial datasets and reproducible local analyses. Use for multi-step data work or data-quality problems; skip a simple calculation over complete supplied values.
---

# Dataset analysis

Locate actual files or exact app dataset revisions. Establish row meaning,
keys, units, time coverage, and the requested output before transforming data.

Prefer the no-runtime path: `calculate` for growth, returns, drawdown, ratios
and moving averages over explicit values or a dataset column, then the market
data connection's local `quantitative_*` and `technical_*` tools. Use
`run_python` for transformations they cannot do, starting from `dataset_export`
files; its description states where it can read and write.

Read [data validation](references/validation.md) for profiling, joins,
deduplication, aggregation, or pre-delivery quality review. Apply checks that
can change this answer rather than profiling every column by default.

Keep raw inputs intact and transformations rerunnable. Preserve decimal
precision for monetary interchange, use explicit missing-value rules, and
record sampling seeds. Use the app dataset export/import contracts when
results must retain a pinned revision for later charts.

Deliver the requested result, useful file or dataset references, and quality
issues affecting interpretation. A successful script proves execution;
declared lineage alone does not prove origin or historical availability.
