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
3.6.3 to store finance connector API keys in the native macOS
credential store. keyring-rs is available under either the Apache License 2.0
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
application stack includes FastMCP 3.4.11 and `fastmcp-slim` 3.4.11 under
Apache 2.0;
the Model Context Protocol Python SDK 1.30.0, copyright (c) 2024 Anthropic,
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
HTTPX 0.28.1 under BSD-3-Clause; aiohttp 3.14.5 under Apache 2.0 and MIT;
Requests 2.34.2 under Apache 2.0; cryptography 50.0.0 under Apache 2.0 or
BSD-3-Clause; certifi 2026.10.23 under MPL 2.0; curl-cffi 0.16.1 under MIT;
lxml 6.1.2 under BSD-3-Clause; Beautiful Soup 4.15.0 under MIT; yfinance 1.6.0
under Apache 2.0; `pandas-ta-openbb` 0.4.24, finvizfinance 1.3.0, and Nasdaq
Data Link 1.0.4 under MIT. The complete copyright, license, NOTICE, and
bundled-library terms in each installed distribution's `.dist-info` metadata
and license files remain applicable. The bundled
`THIRD_PARTY_LICENSES/python-distributions.json` enumerates every OpenBB runtime
distribution, its exact version, metadata-reported license and retained legal
files; [license_bundle.py](openbb/license_bundle.py) derives that inventory from
the locked runtime closure, excluding development-only packages.

## Korea Investment Open Trading API

Loupist's checked-in read-operation inventory is derived from the
[Korea Investment Open Trading API](https://github.com/koreainvestment/open-trading-api)
repository at commit `b093e42ba32d1df5f5ddad7a71cb715cbc800832`, copyright
(c) 2026 Korea Investment & Securities, under the MIT License. The MIT terms
reproduced above apply. Loupist does not bundle or execute the upstream
MCP server or Python examples.


## Bundled method Skills

The financial and data workflow cards adapt the reviewed revisions recorded in
[Bundled Skill provenance](agent/skills/NOTICE.md):

| Upstream | Reviewed revision | License |
| --- | --- | --- |
| anthropics/financial-services | `574ed3624aebd0418c7e96cd101262f30210ab26` | Apache-2.0 |
| anthropics/knowledge-work-plugins | `bc8f6922cfeea215a202cdcc9f0d299a990889a8` | Apache-2.0 |
| K-Dense-AI/scientific-agent-skills | `593377bb048752abb0152deab9fbb1083a6087df` | MIT |
| microsoft/qlib | `54355232463878d2eebb91fe0ee5fa7fa1f5976c` | MIT |

That notice identifies the adapted files, local changes, copyright notices and
retained license texts. Document workflows and the dividend income, fund
comparison, why-it-moved, goal math and Korean statements guides are original
Loupist instructions. Anthropic’s restricted document Skills are not bundled.
Skill Creator and Skill Installer adapt OpenAI’s public Apache-2.0 workflows;
their license texts are retained in the corresponding local skill folders.

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

## Offline Python runtime

The pinned [WASI Python runtime v0.1.4](https://github.com/ClayGendron/wasi-python/releases/tag/v0.1.4)
bundles CPython 3.15.2 under PSF-2.0 and its historical notices
(`LICENSES/cpython.txt`). Its `MANIFEST` lists the following complete package and
native-library inventory. Build helpers are included here when the manifest lists
them, even when their Python module is not installed in the runtime.

The metadata citations below are paths inside the bundled `lib/python315.zip`;
`LICENSES/` paths are relative to the runtime directory. These files are also in
the pinned runtime archive named in [runtime.json](python-worker/runtime.json).
License names come from each distribution’s Core Metadata and retained license
text; a BSD classifier alone is resolved against that package’s actual license
text. Full copyright, historical and bundled-component terms remain in those
files. This inventory is separate from the OpenBB sidecar closure above.

| Package or library | Version | License | Bundled metadata / license source |
| --- | --- | --- | --- |
| numpy | 2.5.4 | BSD-3-Clause AND 0BSD AND MIT AND Zlib AND CC0-1.0 | `numpy-2.5.4.dist-info/METADATA` (License-Expression); `LICENSES/numpy.txt` |
| pandas | 3.1.0 | BSD-3-Clause | `pandas-3.1.0.dist-info/METADATA` (License-Expression); `LICENSES/pandas.txt` |
| zlib | 1.3.2 | Zlib | `LICENSES/zlib.txt` |
| bzip2 | 1.0.8 | bzip2 license | `LICENSES/bzip2.txt` |
| xz | 5.8.4 | 0BSD (bundled liblzma) | `LICENSES/xz.txt` |
| sqlite-autoconf | 3540100 | Public-domain dedication | `LICENSES/sqlite.txt` |
| zstd | 1.5.7 | BSD-3-Clause (selected runtime terms) | `LICENSES/zstd.txt` |
| python-dateutil | 2.9.0.post0 | Apache-2.0 and BSD-3-Clause (contribution-dependent terms) | `python_dateutil-2.9.0.post0.dist-info/METADATA` (License); `LICENSES/python-dateutil.txt` |
| tzdata | 2026.6 | Apache-2.0 | `tzdata-2026.6.dist-info/METADATA` (License); `LICENSES/tzdata.txt` |
| six | 1.17.0 | MIT | `six-1.17.0.dist-info/METADATA` (License); `LICENSES/six.txt` |
| regex | 2026.9.29 | Apache-2.0 AND CNRI-Python | `regex-2026.9.29.dist-info/METADATA` (License-Expression); `LICENSES/regex.txt` |
| matplotlib | 3.11.2 | Matplotlib license agreement (versions 1.3.0 and later, with historical terms) | `matplotlib-3.11.2.dist-info/METADATA` (License); `LICENSES/matplotlib.txt` |
| pillow | 13.0.0 | MIT-CMU | `pillow-13.0.0.dist-info/METADATA` (License-Expression); `LICENSES/pillow.txt` |
| kiwisolver | 1.5.1 | BSD-3-Clause | `kiwisolver-1.5.1.dist-info/METADATA` (License); `LICENSES/kiwisolver.txt` |
| freetype | 2.14.3 | FTL OR GPL-2.0-or-later | `LICENSES/freetype.txt` |
| harfbuzz | 14.1.0 | Old MIT (with component notices) | `LICENSES/harfbuzz.txt` |
| sheenbidi | 3.0.0 | Apache-2.0 | `LICENSES/sheenbidi.txt` |
| libraqm | 0.10.5 | MIT | `LICENSES/libraqm.txt` |
| pybind11 | 3.1.0 | BSD-3-Clause | `LICENSES/pybind11.txt` |
| cppy | 1.3.1 | BSD-3-Clause | `LICENSES/cppy.txt` |
| cycler | 0.12.1 | BSD-3-Clause | `cycler-0.12.1.dist-info/METADATA` (License); `LICENSES/cycler.txt` |
| pyparsing | 3.3.3 | MIT | `pyparsing-3.3.3.dist-info/METADATA` (License-Expression); `LICENSES/pyparsing.txt` |
| packaging | 26.3 | Apache-2.0 OR BSD-2-Clause | `packaging-26.3.dist-info/METADATA` (License-Expression); `LICENSES/packaging.txt` |
| fonttools | 4.67.0 | MIT | `fonttools-4.67.0.dist-info/METADATA` (License); `LICENSES/fonttools.txt` |
| contourpy | 1.4.0 | BSD-3-Clause | `contourpy-1.4.0.dist-info/METADATA` (License-Expression); `LICENSES/contourpy.txt` |
| qhull | 8.0.2 | Qhull license | `LICENSES/qhull.txt` |
| libjpeg-turbo | 3.2.0 | IJG and BSD-3-Clause; component notices retained | `LICENSES/libjpeg-turbo.txt` |
| libxml2 | 2.15.4 | MIT | `LICENSES/libxml2.txt` |
| libxslt | 1.1.45 | MIT (with libexslt notices) | `LICENSES/libxslt.txt` |
| lxml | 6.1.3 | BSD-3-Clause | `lxml-6.1.3.dist-info/METADATA` (License); `LICENSES/lxml.txt` |
| openpyxl | 3.1.5 | MIT | `openpyxl-3.1.5.dist-info/METADATA` (License); `LICENSES/openpyxl.txt` |
| et-xmlfile | 2.0.0 | MIT | `et_xmlfile-2.0.0.dist-info/METADATA` (License); `LICENSES/et-xmlfile.txt` |
| xlsxwriter | 3.2.9 | BSD-2-Clause | `xlsxwriter-3.2.9.dist-info/METADATA` (License); `LICENSES/xlsxwriter.txt` |
| xlrd | 2.0.2 | BSD-3-Clause and BSD-4-Clause (historical component) | `xlrd-2.0.2.dist-info/METADATA` (License); `LICENSES/xlrd.txt` |
| pypdf | 6.21.4 | BSD-3-Clause | `pypdf-6.21.4.dist-info/METADATA` (License-Expression); `LICENSES/pypdf.txt` |
| beautifulsoup4 | 4.15.0 | MIT | `beautifulsoup4-4.15.0.dist-info/METADATA` (License); `LICENSES/beautifulsoup4.txt` |
| soupsieve | 3.0.4 | MIT | `soupsieve-3.0.4.dist-info/METADATA` (License-Expression); `LICENSES/soupsieve.txt` |
| html5lib | 1.1 | MIT | `html5lib-1.1.dist-info/METADATA` (License); `LICENSES/html5lib.txt` |
| webencodings | 0.6.1 | BSD-3-Clause | `webencodings-0.6.1.dist-info/METADATA` (Classifier); `LICENSES/webencodings.txt` |
| defusedxml | 0.7.1 | PSF-2.0 | `defusedxml-0.7.1.dist-info/METADATA` (License); `LICENSES/defusedxml.txt` |
| xmltodict | 1.0.4 | MIT | `xmltodict-1.0.4.dist-info/METADATA` (License-Expression); `LICENSES/xmltodict.txt` |
| pyyaml | 6.0.3 | MIT | `pyyaml-6.0.3.dist-info/METADATA` (License); `LICENSES/pyyaml.txt` |
| tomli-w | 1.2.0 | MIT | `tomli_w-1.2.0.dist-info/METADATA` (Classifier); `LICENSES/tomli-w.txt` |
| jinja2 | 3.1.6 | BSD-3-Clause | `jinja2-3.1.6.dist-info/METADATA` (Classifier); `LICENSES/jinja2.txt` |
| markupsafe | 3.0.4 | BSD-3-Clause | `markupsafe-3.0.4.dist-info/METADATA` (License-Expression); `LICENSES/markupsafe.txt` |
| tabulate | 0.10.0 | MIT | `tabulate-0.10.0.dist-info/METADATA` (License-Expression); `LICENSES/tabulate.txt` |
| charset-normalizer | 3.5.2 | MIT | `charset_normalizer-3.5.2.dist-info/METADATA` (License); `LICENSES/charset-normalizer.txt` |
| networkx | 3.7 | BSD-3-Clause | `networkx-3.7.dist-info/METADATA` (License-Expression); `LICENSES/networkx.txt` |
| patsy | 1.0.3 | BSD-2-Clause | `patsy-1.0.3.dist-info/METADATA` (License); `LICENSES/patsy.txt` |
| sympy | 1.14.0 | BSD-3-Clause | `sympy-1.14.0.dist-info/METADATA` (License); `LICENSES/sympy.txt` |
| mpmath | 1.3.0 | BSD-3-Clause | `mpmath-1.3.0.dist-info/METADATA` (License); `LICENSES/mpmath.txt` |
| attrs | 26.1.0 | MIT | `attrs-26.1.0.dist-info/METADATA` (License-Expression); `LICENSES/attrs.txt` |
| more-itertools | 11.2.1 | MIT | `more_itertools-11.2.1.dist-info/METADATA` (License-Expression); `LICENSES/more-itertools.txt` |
| toolz | 1.2.0 | BSD-3-Clause | `toolz-1.2.0.dist-info/METADATA` (License-Expression); `LICENSES/toolz.txt` |
| tqdm | 4.70.1 | MPL-2.0 AND MIT | `tqdm-4.70.1.dist-info/METADATA` (License); `LICENSES/tqdm.txt` |
| typing-extensions | 4.16.0 | PSF-2.0 | `typing_extensions-4.16.0.dist-info/METADATA` (License-Expression); `LICENSES/typing-extensions.txt` |
| joblib | 1.6.0 | BSD-3-Clause | `joblib-1.6.0.dist-info/METADATA` (License-Expression); `LICENSES/joblib.txt` |
| cloudpickle | 3.1.2 | BSD-3-Clause | `cloudpickle-3.1.2.dist-info/METADATA` (License); `LICENSES/cloudpickle.txt` |
| threadpoolctl | 3.7.0 | BSD-3-Clause | `threadpoolctl-3.7.0.dist-info/METADATA` (License-Expression); `LICENSES/threadpoolctl.txt` |
| narwhals | 2.27.1 | MIT | `narwhals-2.27.1.dist-info/METADATA` (License-Expression); `LICENSES/narwhals.txt` |
| scipy | 1.18.1 | BSD-3-Clause; bundled-library terms retained | `scipy-1.18.1.dist-info/METADATA` (License); `LICENSES/scipy.txt` |
| openblas | 0.3.34 | BSD-3-Clause | `LICENSES/openblas.txt` |
| scikit-learn | 1.9.1 | BSD-3-Clause | `scikit_learn-1.9.1.dist-info/METADATA` (License-Expression); `LICENSES/scikit-learn.txt` |
| seaborn | 0.13.2 | BSD-3-Clause | `seaborn-0.13.2.dist-info/METADATA` (Classifier); `LICENSES/seaborn.txt` |
| reportlab | 5.0.1 | BSD-3-Clause | `reportlab-5.0.1.dist-info/METADATA` (License); `LICENSES/reportlab.txt` |
| statsmodels | 0.15.0 | BSD-3-Clause | `statsmodels-0.15.0.dist-info/METADATA` (License-Expression); `LICENSES/statsmodels.txt` |
| formulaic | 1.2.2 | MIT | `formulaic-1.2.2.dist-info/METADATA` (License-Expression); `LICENSES/formulaic.txt` |
| interface-meta | 2.0.1 | MIT | `interface_meta-2.0.1.dist-info/METADATA` (License); `LICENSES/interface-meta.txt` |
| wrapt | 2.5.2 | BSD-2-Clause | `wrapt-2.5.2.dist-info/METADATA` (License-Expression); `LICENSES/wrapt.txt` |
| python-docx | 1.2.0 | MIT | `python_docx-1.2.0.dist-info/METADATA` (License); `LICENSES/python-docx.txt` |
| python-pptx | 1.0.2 | MIT | `python_pptx-1.0.2.dist-info/METADATA` (License); `LICENSES/python-pptx.txt` |
| pdfplumber | 0.11.10 | MIT | `pdfplumber-0.11.10.dist-info/METADATA` (Classifier); `LICENSES/pdfplumber.txt` |
| pdfminer-six | 20260107 | MIT | `pdfminer_six-20260107.dist-info/METADATA` (License-Expression); `LICENSES/pdfminer-six.txt` |
| xarray | 2026.9.0 | Apache-2.0 | `xarray-2026.9.0.dist-info/METADATA` (License-Expression); `LICENSES/xarray.txt` |
| drawsvg | 2.4.2 | MIT | `drawsvg-2.4.2.dist-info/METADATA` (Classifier); `LICENSES/drawsvg.txt` |
| pixman | 0.46.4 | MIT | `LICENSES/pixman.txt` |
| libpng | 1.6.59 | libpng-2.0 | `LICENSES/libpng.txt` |
| cairo | 1.18.6 | MPL-1.1 (selected runtime terms) | `LICENSES/cairo.txt` |
| cffi | 2.1.1 | MIT-0 | `LICENSES/cffi.txt` |
| cairocffi | 1.7.1 | BSD-3-Clause | `cairocffi-1.7.1.dist-info/METADATA` (Classifier); `LICENSES/cairocffi.txt` |
| cairosvg | 2.9.1 | LGPL-3.0-or-later | `cairosvg-2.9.1.dist-info/METADATA` (License); `LICENSES/cairosvg.txt` |
| tinycss2 | 1.5.1 | BSD-3-Clause | `tinycss2-1.5.1.dist-info/METADATA` (Classifier); `LICENSES/tinycss2.txt` |
| cssselect2 | 0.10.1 | BSD-3-Clause | `cssselect2-0.10.1.dist-info/METADATA` (Classifier); `LICENSES/cssselect2.txt` |
| imageio | 2.38.1 | BSD-2-Clause | `imageio-2.38.1.dist-info/METADATA` (License-Expression); `LICENSES/imageio.txt` |
| plotly | 7.1.0 | MIT | `plotly-7.1.0.dist-info/METADATA` (License-Expression); `LICENSES/plotly.txt` |

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
