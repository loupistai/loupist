# Repository guide

`python3 scripts/guide.py <words>` searches titles and paths, then section text if needed.
Open one result with `python3 scripts/guide.py 'path#heading'`. For an
implementation term, use `rg -n '<term>' docs apps/loupist/src apps/loupist/src-tauri/src`.

| Work | Start here |
| --- | --- |
| Sidebar, panels, composer, citations, map, schedule | [UI pages](ui/) |
| Storage, turns, providers, scheduling, untrusted input | [Architecture](architecture.md), [Runtime](runtime/) |
| Sources, World, Lens, Journal, sharing | [Knowledge and Memory](knowledge-model.md), [Knowledge](knowledge/) |
| Datasets, calculations, numbers, charts | [Financial analysis](analysis.md), [Analysis](analysis/) |
| Product agent prompts, tool contracts, Skills, MCP | [Agent](agent/), [Capabilities](capabilities/) |
| Development decisions, tool entry points, session handoff | [Repository guidance](dev/repository-guidance.md) |
| Checks, setup, diagnostics, visual review | [Development map](development.md), [Development](dev/), [native tools](../apps/loupist/e2e/README.md#which-tool), [evals](../apps/loupist/evals/README.md) |
| Distribution, support and product language | [CI and releases](ci-cd.md), [Release qualification](release.md), [installation](../installation.md), [positioning](positioning.md) |
