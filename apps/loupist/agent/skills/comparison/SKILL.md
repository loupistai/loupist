---
name: comparison
description: Build a peer-company comparison or trading-comparables analysis, including metric alignment, peer selection, and valuation distributions. Use for substantive comparisons; skip a single-company lookup or a simple ratio.
---

# Peer comparison

Start from the comparison the user needs: operating performance, growth, cash
generation, or market valuation. Select metrics and peers for that question.
Read [peer analysis](references/peer-analysis.md) when building a comparable
company table, applying peer multiples, or reconciling inconsistent metrics.

Fix the universe and common measurement basis before ranking. Explain
exclusions and material differences in business mix, geography, scale, and
capital structure. Keep unmatched observations visible rather than silently
dropping an inconvenient company.

Preserve source definitions and normalization steps separately from derived
metrics so a reader can reproduce the ranking. Compute margins, growth and
multiples with `calculate` so each carries its formula and inputs. A peer
median is context, not proof that the subject deserves that multiple.

Return the aligned comparison, important exceptions, and evidence explaining
a premium or discount. Publish the comparison table with `grid_publish`: one row
per asset, one column per measure with its period and basis, each value cell
citing the result it came from (a `calculate` result for margins, growth and
multiples). Leave a value the sources do not give, or one on another basis,
empty with its reason rather than mixing bases in one column; say in a note
what is missing. Use the available visualize Skill when a chart clarifies the
comparison.

When two sources give different figures for the same company, measure and
period, do not average or quietly choose: publish a `grid_publish` conflict
card and say which figure the comparison uses and why.
