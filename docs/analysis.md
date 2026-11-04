# Financial analysis

Chat can query data, calculate, write Python or JavaScript, and publish saved charts. Native Pi file and Bash tools work directly in the Loupist's selected local workspace, using installed runtimes, packages and network access with the user's host permissions. Research scripts, downloaded data and exported results remain ordinary files.

The host [`calculate`](analysis/calculation.md#calculation) tool owns common finance arithmetic; local
scripts own custom calculation and rerun behavior. For scripts, the prompt directs
the agent to check runtimes, install missing dependencies only in the project or
user environment, save reproducible inputs/code/outputs, and verify calculated
values with local code. Review results against those instructions; execution
alone does not establish their correctness.
App services own dataset snapshots, charts, connector credentials and canonical
Memory writes.

## Find a topic

- [Where each item appears](analysis/surfaces.md#where-each-item-appears)
- [Cards outside the reply](analysis/cards.md)
- [Data and provenance](analysis/datasets.md#data-and-provenance)
- [Provenance and cutoffs](analysis/provenance.md)
- [Calculation](analysis/calculation.md#calculation)
- [Sensitivity cards](analysis/calculation.md#sensitivity-cards)
- [Number checks](analysis/number-checks.md#number-checks)
- [Number matching](analysis/number-checks.md#number-matching)
- [Grid cells](analysis/grids.md#grid-cells)
- [Results before and after](analysis/results.md#results-before-and-after)
- [Korean asset rows](analysis/korea.md#korean-asset-rows)
- [Point-in-time answers](analysis/point-in-time.md#point-in-time-answers)
- [Execution and history](analysis/calculation.md#execution-and-history)
- [Visualizations](analysis/charts.md#visualizations)
- [Evaluation](analysis/charts.md#evaluation)
