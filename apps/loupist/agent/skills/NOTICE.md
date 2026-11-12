# Bundled Skill provenance

The optional workflows and their references are adapted for Loupist's local
execution, evidence, and Memory contracts. Upstream attribution and license texts
are retained below. These are curated local resources; upstream updates are reviewed
before incorporation, not fetched or installed during a Chat.

| Source revision | Reviewed material | Local use | License |
| --- | --- | --- | --- |
| [anthropics/financial-services @ 574ed3624aeb](https://github.com/anthropics/financial-services/tree/574ed3624aebd0418c7e96cd101262f30210ab26) | `dcf-model`, `comps-analysis`, `earnings-analysis`, `thesis-tracker` | Valuation model checks; peer alignment; earnings deltas; thesis evidence table | [Apache-2.0](licenses/Apache-2.0-financial-services.txt) |
| [anthropics/knowledge-work-plugins @ bc8f6922cfee](https://github.com/anthropics/knowledge-work-plugins/tree/bc8f6922cfeea215a202cdcc9f0d299a990889a8) | `financial-statements`, `explore-data`, `validate-data`, `statistical-analysis` | Data profiling, join/aggregation validation, statement review | [Apache-2.0](licenses/Apache-2.0-knowledge-work-plugins.txt) |
| [K-Dense-AI/scientific-agent-skills @ 593377bb0487](https://github.com/K-Dense-AI/scientific-agent-skills/tree/593377bb048752abb0152deab9fbb1083a6087df) | `skills/statistical-analysis/SKILL.md`, `skills/statistical-analysis/references/test_selection_guide.md`, `skills/statistical-analysis/references/assumptions_and_diagnostics.md` | Inference design, dependence diagnostics, multiplicity and effect-size reporting | [MIT](licenses/MIT-scientific-agent-skills.txt) |
| [microsoft/qlib @ 543552324638](https://github.com/microsoft/qlib/tree/54355232463878d2eebb91fe0ee5fa7fa1f5976c) | `docs/component/strategy.rst` | Signal/execution/account separation and benchmark/cost-aware portfolio reporting | [MIT](licenses/MIT-qlib.txt) |

Changes from upstream: concise task routing and on-demand references replace
fixed report lengths, chart counts, spreadsheet layouts, staged approval questions,
and provider-specific MCP preferences. The DCF guide uses explicit financing and
cash-flow conventions rather than copying net-debt weights, fixed WACC/tax ranges,
or terminal-value-share targets. Peer analysis does not enforce universal margin
ordering or replace undefined ratios with zero. The original DCF workbook validator
was reviewed but is not bundled: its label heuristics and cached-value checks do
not establish fresh recalculation or model correctness.

The PDF, Word (`word-documents`), spreadsheet, and presentation workflows are
original Loupist instructions limited to verified offline WASI file operations.
They do not include an office renderer, OCR or spreadsheet recalculation.
The following official library documents informed the supported operations:

- [pypdf: PDF text extraction and scanned pages](https://pypdf.readthedocs.io/en/stable/user/extract-text.html)
- [python-docx: paragraphs, runs, and styles](https://python-docx.readthedocs.io/en/latest/user/text.html)
- [openpyxl: formulas are not evaluated](https://openpyxl.readthedocs.io/en/stable/simple_formulae.html)
- [python-pptx: masters, layouts, and slide creation](https://python-pptx.readthedocs.io/en/latest/user/slides.html)
- [Damodaran: valuation definitions and capital weights](https://pages.stern.nyu.edu/~adamodar/New_Home_Page/datafile/variable.htm)

Anthropic's public `skills/pdf`, `skills/docx`, `skills/xlsx`, and `skills/pptx`
were reviewed as product references. Their [per-skill license](https://github.com/anthropics/skills/blob/main/skills/docx/LICENSE.txt)
restricts copying, derivatives, and redistribution; their prompts, scripts, and
assets are not bundled here. The permissively licensed financial/data materials
above are separate repositories with their own retained terms.

The dividend income, fund comparison, why-it-moved and goal math workflows and
the Korean statements reference are original Loupist instructions; they adapt
no upstream material.


The Skill Creator workflow adapts OpenAI's public
[Skill Creator](https://github.com/openai/skills/tree/main/skills/.system/skill-creator).
Its Apache-2.0 license is retained in its local skill folder. Loupist
modifies destinations, host scope, validation, next-turn discovery, and Memory
boundaries; Codex-specific configuration and installer helpers are not bundled.
