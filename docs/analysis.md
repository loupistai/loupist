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

## Where each item appears

Reply timeline, numbers and readers: [Where results appear](analysis/surfaces.md#where-each-item-appears). Asset rows, grids, results and alerts: [Cards outside the reply](analysis/cards.md).

## Data and provenance

Snapshots, trust tags, import and export: [Datasets](analysis/datasets.md#data-and-provenance). Publication time, cutoffs and market reads: [Provenance and cutoffs](analysis/provenance.md).

## Calculation

Host arithmetic and the sensitivity card. [Calculation](analysis/calculation.md#calculation).

### Sensitivity cards

A card states the pinned inputs and the result. It does not recommend a trade. [Calculation](analysis/calculation.md#sensitivity-cards).

## Number checks

Match a figure to its pinned input. [Number checks](analysis/number-checks.md#number-checks).

### Number matching

How a close match, a conflict and a missing input are labeled. [Number checks](analysis/number-checks.md#number-matching).

### Grid cells

Checked tables and disagreeing sources. [Grids and conflict cards](analysis/grids.md#grid-cells).

## Results before and after

The comparison of captured figures. [Results before and after](analysis/results.md#results-before-and-after).

## Korean asset rows

Filings, flows and retail rows. [Korean asset rows](analysis/korea.md#korean-asset-rows).

## Point-in-time answers

The cutoff and the composer date control. [Point-in-time answers](analysis/point-in-time.md#point-in-time-answers).

## Execution and history

File writes stick if a reply is cancelled. Historical simulations need an explicit fill time. [Calculation](analysis/calculation.md#execution-and-history).

## Visualizations

Chart cards and their pinned series. [Charts and evaluation](analysis/charts.md#visualizations).

## Evaluation

Credential-free cases and where they live. [Charts and evaluation](analysis/charts.md#evaluation).
