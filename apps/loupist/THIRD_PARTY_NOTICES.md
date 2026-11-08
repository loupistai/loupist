# Third-party notices

## Component styles

Loupist retains the component state variants and scrollbar utility from
[shadcn](https://github.com/shadcn-ui/ui) 4.22.1, copyright (c) 2023 shadcn,
under the MIT License. The complete notice is retained in
`src/styles/vendor/shadcn.LICENSE`. The shadcn CLI is not an app dependency.

## Chart rendering

Loupist uses [KLineChart](https://github.com/klinecharts/KLineChart)
10.1.0, copyright (c) 2019 lihu, under the Apache License 2.0. Its upstream
NOTICE, including the TradingView Lightweight Charts attribution recorded by
KLineChart, and the Apache license text are retained with the dependency.

Loupist uses [Flint](https://github.com/microsoft/flint-chart) 0.5.0,
copyright Microsoft Corporation, under the MIT License reproduced below.

Loupist uses Vega 6.3.1, Vega-Lite 6.4.3, and Vega-Embed 7.3.0,
copyright the University of Washington Interactive Data Lab and contributors,
under the BSD 3-Clause License. Their complete license texts are retained with
the dependencies.

## World graph rendering

Loupist uses [react-force-graph-2d](https://github.com/vasturiano/react-force-graph)
1.29.2 and [force-graph](https://github.com/vasturiano/force-graph) 1.52.0,
by Vasco Asturiano, under the MIT License reproduced below. The force-graph
notice is copyright (c) 2018 Vasco Asturiano.

Their MIT-licensed rendering helpers include d3-force-3d 3.0.6,
d3-binarytree 1.0.2, d3-octree 1.1.0, accessor-fn 1.5.3, kapsule 1.16.3,
and react-kapsule 2.6.0 (copyright (c) 2017 Vasco Asturiano);
canvas-color-tracker 1.3.2 and index-array-by 1.4.2 (copyright (c) 2018
Vasco Asturiano); jerrypick 1.1.2 (copyright (c) 2019 Vasco Asturiano);
and float-tooltip 1.7.5 (copyright (c) 2022 Vasco Asturiano).
Their complete license texts are retained with the dependencies.

The graph uses D3 interaction and spatial-index components under the ISC
License, copyright 2010-2021 Mike Bostock:

> Permission to use, copy, modify, and/or distribute this software for any purpose
> with or without fee is hereby granted, provided that the above copyright notice
> and this permission notice appear in all copies.
>
> THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
> REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
> FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
> INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
> OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
> TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
> THIS SOFTWARE.

## Pi coding agent

Loupist bundles [fd](https://github.com/sharkdp/fd) 10.5.0 for offline
workspace file discovery, copyright David Peter and contributors, under the
MIT or Apache License 2.0. Both upstream license texts are included in
`pi-runtime/workspace-tools/`.

Loupist bundles [ripgrep](https://github.com/BurntSushi/ripgrep) 15.2.0 for
offline workspace text search, copyright Andrew Gallant and contributors, under
the MIT License or the Unlicense. Both upstream license texts are included in
`pi-runtime/workspace-tools/`.

Loupist bundles Pi coding agent 1.5.0, copyright (c) 2025 Mario Zechner,
under the MIT License:

> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

## keyring

Loupist uses [keyring-rs](https://github.com/open-source-cooperative/keyring-rs)
3.6.3 to store finance connector API keys in the native macOS and Windows
credential stores. keyring-rs is available under either the Apache License 2.0
or the MIT License. The applicable license texts are distributed with the
crate; the MIT terms reproduced above also apply when that option is selected.

## OpenBB financial-data runtime

Loupist bundles a read-only Python sidecar built from
[OpenBB Platform](https://github.com/OpenBB-finance/OpenBB) 4.7.2 and
`openbb-mcp-server` 1.4.1. The OpenBB repository records copyright (c)
2021-2025 OpenBB Inc. and licenses all files in that repository under the GNU
Affero General Public License v3.0 (`AGPL-3.0-only`). The complete AGPLv3 terms
are distributed as Loupist's top-level `LICENSE`; the sidecar's bundled
`uv.lock` identifies the exact PyPI artifacts and SHA-256 hashes.

The bundled OpenBB core, router, and analysis distributions are `openbb-core`
1.6.13, `openbb-platform-api` 1.3.6, `openbb-commodity` 1.5.2,
`openbb-crypto` 1.6.2, `openbb-currency` 1.6.2, `openbb-derivatives` 1.6.2, `openbb-econometrics` 1.7.2, `openbb-economy`
1.6.2, `openbb-equity` 1.6.2, `openbb-etf` 1.6.2, `openbb-fixedincome`
1.6.2, `openbb-index` 1.6.2, `openbb-news` 1.6.2, `openbb-quantitative`
1.6.2, `openbb-regulators` 1.6.2, and `openbb-technical` 1.6.2.

The bundled OpenBB provider extensions are `openbb-alpha-vantage` 1.6.1,
`openbb-benzinga` 1.6.1, `openbb-biztoc` 1.6.1, `openbb-bls` 1.3.1,
`openbb-cboe` 1.6.1, `openbb-cftc` 1.4.2, `openbb-congress-gov` 1.2.3,
`openbb-deribit` 1.2.1, `openbb-ecb` 1.6.1, `openbb-econdb` 1.5.1,
`openbb-us-eia` 1.3.1, `openbb-famafrench` 1.2.1,
`openbb-federal-reserve` 1.6.2, `openbb-finra` 1.6.1, `openbb-finviz`
1.5.1, `openbb-fmp` 1.6.1, `openbb-fred` 1.6.2,
`openbb-government-us` 1.6.1, `openbb-imf` 2.1.3, `openbb-intrinio`
1.6.1, `openbb-multpl` 1.3.1, `openbb-nasdaq` 1.6.3, `openbb-oecd`
1.6.1, `openbb-sec` 1.6.7, `openbb-seeking-alpha` 1.6.1,
`openbb-stockgrid` 1.6.1, `openbb-tiingo` 1.6.1, `openbb-tmx` 1.5.2,
`openbb-tradier` 1.5.1, `openbb-tradingeconomics` 1.6.1, `openbb-wsj`
1.6.1, and `openbb-yfinance` 1.6.3. Installed Core Metadata declares
`AGPL-3.0-only` for these OpenBB distributions except
`openbb-congress-gov` and `openbb-multpl`. Those two installed wheels provide
no `License`, `License-Expression`, or `License-File` field, so this notice does
not infer a separate package-level license for them; the repository-wide
OpenBB license stated above is the verified source-tree notice.

The sidecar embeds CPython 3.12 under the Python Software Foundation License
Version 2 and its historical license notices, and the PyInstaller 6.22.3
bootloader, copyright (c) 2010-2023 the PyInstaller
Development Team, 2005-2009 Giovanni Bajo, and 2002 McMillan Enterprises,
Inc., under GPLv2-or-later with PyInstaller's bootloader exception. Its MCP and
application stack includes FastMCP 3.4.7 and `fastmcp-slim` 3.4.7 under
Apache 2.0;
the Model Context Protocol Python SDK 1.29.0, copyright (c) 2024 Anthropic,
PBC, under MIT; FastAPI 0.136.3, copyright (c) 2018 Sebastián Ramírez, under
MIT; Starlette 1.6.0, copyright (c) 2018 Encode OSS Ltd, under BSD-3-Clause;
and Pydantic 2.13.4 and `pydantic-core` 2.46.4, copyright (c) 2017-present
Pydantic Services Inc. and contributors, under MIT.

Major scientific dependencies include NumPy 2.5.2, copyright (c)
2005-2025 the NumPy Developers, under BSD-3-Clause with bundled components
under 0BSD, MIT, Zlib, and CC0-1.0; pandas 3.0.5 under BSD-3-Clause; SciPy
1.18.1 under BSD-3-Clause with its bundled-library notices; statsmodels 0.14.6
under BSD; scikit-learn 1.9.0, copyright (c) 2007-2026 the scikit-learn
developers, under BSD-3-Clause; OpenPyXL 3.1.5 under MIT; `arch` 7.2.0 and
`linearmodels` 6.1 under the University of Illinois/NCSA License; and
`exchange-calendars` 4.13.2 under Apache 2.0.

Major networking, security, parsing, and provider-client dependencies include
HTTPX 0.28.1 under BSD-3-Clause; aiohttp 3.14.3 under Apache 2.0 and MIT;
Requests 2.34.2 under Apache 2.0; cryptography 50.0.0 under Apache 2.0 or
BSD-3-Clause; certifi 2026.7.22 under MPL 2.0; curl-cffi 0.16.1 under MIT;
lxml 6.1.2 under BSD-3-Clause; Beautiful Soup 4.15.0 under MIT; yfinance 1.6.0
under Apache 2.0; `pandas-ta-openbb` 0.4.24, finvizfinance 1.3.0, and Nasdaq
Data Link 1.0.4 under MIT. The complete copyright, license, NOTICE, and
bundled-library terms in each installed distribution's `.dist-info` metadata
and license files remain applicable.

## Korea Investment Open Trading API

Loupist's checked-in read-operation inventory is derived from the
[Korea Investment Open Trading API](https://github.com/koreainvestment/open-trading-api)
repository at commit `b093e42ba32d1df5f5ddad7a71cb715cbc800832`, copyright
(c) 2026 Korea Investment & Securities, under the MIT License. The MIT terms
reproduced above apply. Loupist does not bundle or execute the upstream
MCP server or Python examples.


## Always-on method Skills

Short valuation, comparison, historical-rule-test, and filings workflow
cards in `agent/skills/` are adapted from
[Vibe-Trading](https://github.com/HKUDS/Vibe-Trading) skills
`valuation-model`, `financial-statement`, `deep-company-series`,
`factor-research`, `research-discipline`, and `edgar-sec-filings`,
copyright (c) 2026 Vibe-Trading Contributors, under the MIT License.
The MIT terms reproduced above apply. Loupist does not bundle
Vibe-Trading, its brokers, Alpha Zoo, swarms, or trade execution.

## Oh My Pi web adapters

Selected Codex, Anthropic, and xAI hosted-search request/response parsers in
`agent/loupist-native-search.mjs`, plus Wikipedia, Wikidata, and Crossref
URL-matcher and bounded JSON-projection patterns in
`src-tauri/src/web/official.rs`, are ported from
[Oh My Pi](https://github.com/can1357/oh-my-pi) commit
`76a294cb19bfded1e32e2111f1f729129595bf5e`, copyright (c) 2025 Mario
Zechner, 2025-2026 Can Bölük, 2026 Stencil Labs, Inc., and contributors,
under the MIT License. The MIT terms reproduced above apply.
Loupist does not bundle Oh My Pi, merge multiple official API
responses, or adopt its site-handler registry.

## World extraction and local embeddings

The desktop application's in-process World extraction embeds five prompt templates
from graphiti-core 0.30.1 (`extract_nodes.extract_text`,
`extract_nodes.extract_summaries_batch`, `dedupe_nodes.nodes`, `extract_edges.edge`,
`dedupe_edges.resolve_edge`), copyright 2024 Zep Software, Inc., under the Apache
License 2.0. The templates and their notice are kept in
`src-tauri/src/knowledge/world_extract/prompts/`; only placeholder slots were
added.

Local English embeddings use MinishLab potion-base-4M under the MIT License.
The tokenizer originates from BAAI/bge-base-en-v1.5, also MIT. The staged
`embed_assets/NOTICE` records both. The app loads those files and does not
download a model at runtime.

The offline scientific Python closure contains NumPy 2.4.3, pandas 3.0.2,
SciPy 1.18.0, statsmodels 0.14.6, scikit-learn 1.8.0, joblib 1.5.3,
packaging 26.1, patsy 1.0.2, python-dateutil 2.9.0.post0, pytz 2026.1.post1,
six 1.17.0, and threadpoolctl 3.6.0. Their BSD, MIT, Apache, PSF, Zlib, CC0,
and other applicable license notices are retained inside the bundled wheel
archives under their `.dist-info` metadata and license directories.


## Mathematical text rendering

Loupist uses [KaTeX](https://github.com/KaTeX/KaTeX) 0.19.1, including its math
styles and fonts, copyright (c) 2013-2020 Khan Academy and other contributors,
under the MIT License. The MIT terms reproduced above apply. Its complete
original license is retained with the dependency. The renderer loads these
assets only when a response contains mathematical notation.

## Development WebDriver

Development and isolated E2E builds use a repository-local patch of
[tauri-plugin-wdio-webdriver 1.3.0](https://crates.io/crates/tauri-plugin-wdio-webdriver/1.3.0),
copyright (c) 2024 WebdriverIO Community, under the MIT License. Its complete
original license is preserved in `src-tauri/vendor/tauri-plugin-wdio-webdriver/LICENSE`.
The patch resolves current Tauri child Webviews separately from their owner
Window. It is excluded from release builds.
