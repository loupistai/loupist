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
a premium or discount. When the sources
do not give a figure, or give it only on another basis, say what is missing and
why rather than mixing bases in one measure. `grid_publish` publishes a table
the user can sort and export, with each cell checked against its source. Use the
available visualize Skill when a chart clarifies the comparison.
