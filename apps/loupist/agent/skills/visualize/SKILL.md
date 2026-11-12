---
name: visualize
description: Create or revise an in-app chart, Mermaid diagram, or image export when requested or useful to the answer.
---

# Visualizations in Loupist

## Output routes

| Need | App route |
| --- | --- |
| Prices, volume, indicators, or drawings | `chart_publish` with `view.kind: "candlestick"`; real OHLC columns, time, symbol, and interval are required. |
| Trends, categories, or numeric relationships | `chart_publish` with `line`, `area`, `bar`, or `scatter`. |
| A compact process or relationship diagram | A fenced Mermaid block in the reply. |
| Recompute assumptions | `run_python` with matplotlib, save a PNG under `/workspace/outputs/`, then revise the chart with `chart_publish`. |
| Unsupported forms or requested exports | `run_python` writing a file under `/workspace/outputs/`. |

App charts do not execute arbitrary HTML, JavaScript, Vega specifications or
custom slider controls. `chart_publish` is the in-app chart. A matplotlib figure
is a file: save it as PNG under `/workspace/outputs/`. Bundled fonts do not cover
Korean, Japanese or Chinese labels: use `chart_publish` for those, or label the
figure in Latin script and say so. Files previews CSV/TSV as tables and raster
formats as images; HTML and SVG open as source.

## Publish and revise

Follow the `chart_publish` contract for data binding and
revision arguments. Use `chart_query` when exact stored values are needed.
Include the period and units in the chart title or note. Inline rows must be
a small inspected table or an explicit hypothetical.

Label position, entry, stop, or target levels with their source or analytical basis.
For each proposed level, state its observed anchor in the answer as date,
measure, and value, then distinguish the proposed level. Date every anchor
explicitly; "last day" or the chart's overall range does not date that value.

Preserve studies, custom series, and drawings the user did not ask to change.
A successful publish confirms storage, not visual inspection or the truth of
inputs. State when you could not inspect the rendered result.
