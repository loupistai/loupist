---
name: visualize
description: Create or revise an in-app chart, Mermaid diagram, or visual export when requested or useful to the answer. Charts inside a Word file, workbook, or slide deck follow that file Skill.
---

# Visualizations in Loupist

## Output routes

| Need | App route |
| --- | --- |
| Prices, volume, indicators, or drawings | `chart_publish` with `view.kind: "candlestick"`; real OHLC columns, time, symbol, and interval are required. |
| Trends, categories, or numeric relationships | `chart_publish` with `line`, `area`, `bar`, or `scatter`. |
| A compact process or relationship diagram | A fenced Mermaid block in the reply. |
| Recompute assumptions | Rerun a local model, import its result, and revise the chart. |
| Unsupported forms or requested exports | Local code and a workspace file. |

App charts do not execute arbitrary HTML, JavaScript, Vega specifications, or
custom slider controls. Files previews CSV/TSV as tables and supported raster
formats as images. HTML and SVG open as source; deliver a self-contained export
and explain how to view it in a browser when needed.

## Publish and revise

Load `charts` and follow the `chart_publish` contract for data binding and
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
