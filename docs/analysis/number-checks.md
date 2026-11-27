# Number checks

A number chip is a host check of a figure the answer used against a pinned input. The chip does not make the figure advice.

A figure warning names the document it checked in the popover and receipt. If the document has no title, the warning uses its hostname when available. Calculator and Python checks name the calculation result.

## Number checks

`SYSTEM.md` has the agent write each material number as a link whose text is only
the number with its unit and whose target is the `citation_url` it came from
([prompt rule](../agent/tools.md#tool-contracts-and-turn-preparation)). When the
reply commits, the host normalizes the link-target aliases
`citation_url=#evidence-<id>` and `citation_url:#evidence-<id>` to `#evidence-<id>`
before checking or storing the answer. Only valid evidence link destinations are
changed; code spans, fenced or indented code, labels and titles stay verbatim.
Reference-link definitions are normalized too. The stored answer supplies copy,
export and completed delivery, so all see the same plain targets.
`commands/tool_executor/number_checks.rs` then reads the answer's
`[text](#evidence-…)` links outside code, in order, and checks each one whose whole
text reads as one number against the result it links, from this turn's retained
results. A number link that names neither a retained result nor an evidence item
of this reply is resolved or reported, never left to read like a citation. When
exactly one retained id of this reply is within two character edits of the written
id, the link is inline and the number check against that result does not fail
(`matched` or `rounded`), the host rebinds the stored answer's link to that id and
records the written id as `repairedFrom`; the reader sees an ordinary citation.
Two near candidates, a failing check, a reference-style link or a third edit keep
the link as written, with status `unlinked`. An `unlinked` number is drawn as a
figure to check: Activity lists it under the figures checked, the reply's receipt
counts it, and its line reads "No source is linked to this number in this reply."
The next reply's envelope carries `previous_reply_unlinked_figures` with each such
number and the link it wrote, so the model can read the source again and cite it.
A link that is not a number keeps rendering as "Source unavailable in this reply".
A link to a read made in an earlier reply is not citable and is `unlinked` too. The answer stores at most 200 checks as
`number_checks` ([turn authority](../runtime/chat-turns.md#turn-authority-and-finalization));
later links stay numbered citations. Host-composed and failed replies have none.
Model text cannot set a status, and the renderer never rechecks. Each check names
its link by `link_index`, the link's place in that order. The chat renderer and
export count the stored answer's links the same way and show a check only on the
link at its index, and only when the evidence and text still agree. A repeated
figure keeps each occurrence's own check, and a link past the bound has none.

On read, the host rechecks saved unresolved numbers against the cited excerpt
or its pinned dataset revision. If a truncated web excerpt still leaves a warning,
the host can also use the exact web result saved by the same reply. The chat and
reply provenance, result ID, digest and excerpt prefix must agree. A truncated
`world_read` excerpt is checked against the saved Source span its exact read
returned, as the live check read it, when that span is the excerpt's JSON
continuation. A match outside the excerpt carries context from the saved result. The answer, receipt, grid and
export receive the same projected result. Verified checks and checks with
unavailable evidence retain their saved results. The in-memory cache uses checker
version, reply or artifact identity, and evidence digests. Projections that consult
a saved web result check its availability on each read. Replay uses the existing
per-turn data budget and never writes the message, artifact, Memory or a source.

A link's text is a number when it is, within 64 bytes, an optional sign (`+`, `-`,
`−`) or accounting negative (`(1,234)` with grouped digits, `△1,234`; `▲` is a rise), a
currency symbol, `US$`-style prefix or ISO code, digits with `,` grouping and one
decimal point, then at most one unit: `%`, points (`pp`, `%p`, `percentage points`,
`퍼센트포인트`, `百分点`, `パーセントポイント`), basis points (`bp`, `bps`,
`basis points`, `베이시스포인트`, `基點`, `ベーシスポイント`), a
multiple (`x` glued to the number, `×`, `times`, `배`), an English scale (`thousand`,
`mn`, `million`, `bn`, `tn`; `k`, `m`, `b`, `t` only glued to the number or after a
currency), Korean scales including compounds in decreasing order (`1조 2,300억원`),
and a trailing currency word or code or "per share". Words, dates, tickers, a bare
year from 1900 to 2100 and a bare integer of one to three digits (a numbered
citation) are not numbers. A unit stated for its table cell or sentence makes such
an integer a figure, except a reference marker: a bare integer that trails text in
its cell or clause with only spacing or punctuation after it (`숫자 없음 [2]`) stays
a citation. Read-time replay drops a stored unresolved check of such a link.

While the answer streams, a complete currency link stays underlined as one figure. When both amounts in a stored warning state different currencies, the warning names them.

The linked result decides the check's kind and candidates:

| Kind | Linked result | Candidates |
| --- | --- | --- |
| `calculated` | `calculate` | Its values and inputs, including DCF grid cells and rate axes; a unitless value is tried in the printed scale and in base units |
| `data` | An `fx_rate` or a read with a `dataset_ref` (OpenBB, DART, KRX, KIS, World Bank) | Every number in the retained payload, with its JSON Pointer |
| `step` | `dataset_query`, `dataset_describe`, or another derived or workspace-file result | Every number in the retained payload |
| `passage` | Any other read, such as a web page or filing text | Numbers printed in the evidence's retained excerpt, with byte ranges |

A link to text the user pasted (a `pasted_capture` result, or a `world_read` of a
[user-provided Source](../knowledge/sources-notes.md#user-provided-sources)) checks as a
`passage` against that text; the chip names its source "User-provided", because a
match shows the figure is what the user gave, not that its original says it.
A link directly to an `fx_rate` citation checks as `data` without a dataset.
`to_per_from` and the per-euro legs are raw numbers; the source date and revision
supply no numeric candidates. Currency recognition and FX-pair classification
share the 32-code list in `finance_data/fx.rs`, including CZK, HUF, PLN, RON, ISK,
TRY, IDR, ILS, MYR, PHP and THB. Those eleven additions require uppercase in
amount text and text-inferred currency pairs; the previous 21 codes retain their
case-insensitive recognition. Existing currency words keep their prior behavior.
Lowercase `try`, `ron` and `php` leave neighboring figures plain. Uppercase
`PHP 8.2` remains an amount even in a passage about a software version; decimal
syntax alone cannot distinguish that version from an amount.

For structured `data` checks other than signed FRED observations, an unsigned positive
figure may match the magnitude of a negative source value: the surrounding words
can state the fall or distance below. The checker first tries the ordinary signed
match, then the negative magnitude with the same unit and precision rules. An
explicit plus/minus sign, accounting parentheses or triangle mark must agree with
the source. This exception never applies to passages, derived steps or
`calculate` results; text states a sign only with the number or in adjacent financial wording ([Number matching](#number-matching)),
and a row or field states it by its nature
([Outflows stated as amounts](#outflows-stated-as-amounts)).

For a FRED series read, the host checks the dated observation in the named series
column. OpenBB supplies its units at `extra.results_metadata.<series>.units`.
A stated scale governs conversion. When the series has no unit metadata, the
printed figure can add a unit only if its signed numeric value agrees exactly
with the observation. The checker ignores dates, metadata and the MCP text echo.
In a filing table, a blank line between a monetary section label and its numbered
subheading can occur during PDF extraction. The table's stated scale still applies
to subsequent monetary rows, including cells with a dollar sign.

A `run_python` step prints what the agent's own code computed, often a percent or
point change as a bare number, in the figure's unit (`'qoq_pp': -1.374…`) or as a
fraction (`print(52/675)` → `0.0770…`). A percent or percentage-point figure linked
to one reads each bare number in its `stdout`, never its exit code or other fields,
both ways: as printed in the figure's
own unit, `matched` (`−1.37%p` matches `-1.374…`, `9.8%` matches `9.777…`), and as
a fraction ×100, `rounded` (`7.70%` matches `0.0770…`, `−1.37%p` matches
`-0.01374…`). The exact reading wins when both apply. A number the output prints
with a unit keeps it, and other steps keep the units their payload states.

The printed number must lie within half a unit of its last shown digit (`8.4%`
within ±0.05%, `4,523억` within ±0.5억). The status is `matched` for a candidate
in the same unit and scale or an exact conversion of a stated monetary scale.
An exact conversion carries `scale_conversion` with the source and answer powers
of ten and the available currency. Both it and `rounded` keep the value as the
evidence prints it. A monetary conversion that changes the displayed precision,
a percent↔fraction or bp↔pp conversion, and a conversion inferred from the
digits use `rounded`. A verified literal passage selection takes priority
when equal figures occur elsewhere in its context.
The popover and its accessible description identify an exact conversion as a
source match and name the scale shown in the answer. Approximation keeps the
rounded label.
`differs` only for a calculation with no value or input in range (the check keeps
the nearest value as the result displays it); otherwise `not_found`. When the
excerpt of a passage does not hold the number, the full retained result decides the
status from its content fields only (`content`, `text`, `passage`, `passages`,
`body`, `markdown`, `excerpt`, `value`, `values`), never its metadata such as status,
sizes or offsets. A matched or rounded fallback check keeps `context` from the
content field holding the first match: at most 240 characters of `text` around the
number, with end-exclusive UTF-8 byte offsets `start` and `end` inside that text.
It has no excerpt `span`; excerpt matches keep their span and no context. Older
stored checks without context still load. Data checks use a pointer. A matched or rounded check
keeps its data cell's pointer with up to four fields of the row (descriptive fields
such as company, concept, period and basis, then the matched cell) or its passage
span; a calculated check keeps the operation id (`percent_change`, `reverse_dcf`, ...), the
host's English title, up to eight formula lines, up to twelve inputs
with their label, unit, source and status, and a DCF grid's size. The popover and a
`calculate` source name the operation by its id in the reader's language; answers saved
before the id was kept show the English title. The popover names and
prints each input from these and never reads them from the formula or the evidence (an
input without a unit prints as its bare number). On a short window the popover stays above the composer and
scrolls, with its actions pinned. Surfaces are listed in
[Where each item appears](surfaces.md#where-each-item-appears).

For a structured observation, the popover displays the host's readable row and
marks the observed value. In older field-based rows, only named amounts and
matched quantities use digit grouping. Quotes keep their measurement-specific
formatting. Dates, periods and codes retain their text. The evidence action opens
the retained record.

### Number matching

`knowledge/numbers.rs` reads the numbers in a result for number checks, cited
`calculate` inputs and asset-page line readings. Each candidate keeps the unit the
evidence states (`%`, `%p`/`pp`, `bp`, `x`/`배`, a scale word such as million or 억
with a currency, or a structured row's `unit`, `scale` or `currency` field) and its
place: a byte range in text or a JSON Pointer. Provider fields documented as
percents (`finance_data/percent_fields.rs`: KRX `FLUC_RT`, KIS `*_ctrt`, `*_rate`,
`*_rt`, `rate` and `n_rate`, and `change_percent`, `*_percent`, `*_pct` and `*_pc`)
read as percents. The 13 net-purchase fields in the KIS daily investor rows
(`domestic_stock.investor_trade_by_stock_daily`, `output2`) use millions of won.
The read manifest lists their names. The [filings reference](../../apps/loupist/agent/skills/filings/references/korea-statements.md#investor-flows)
supplies their unit. Other unlabelled structured numbers, including `*_ratio` fields, are raw values. Fields that name, date, order or locate a row are never candidates:
identifiers and codes (`rcept_no`, `corp_code`, `symbol`, `*_cd`, `*_no`, `*_id`),
dates and times (`date`, `bsns_year`, `*_dd`, `*_at`, `createdAtMs`), offsets,
sequence, rank and page fields, revisions, versions, digests, URLs and status. In
text, dates, identifiers and times (`2026-03-18`, `10-K`, `Q3`), digit codes with a
leading zero (`005930`) and six-digit tickers in brackets or with an exchange suffix
(`(990221)`, `990221.KS`) are not numbers; a hyphen after a digit or letter is not a
sign; currency-code prefixes, including glued FX pairs (`USDKRW`), must begin at a
word boundary (`fraud 12` has no AUD amount) and precede the number by at most two
spaces without a line break; `−` (U+2212) is a minus in text and in a provider's
text field (`"−1.20"`) as in a figure, as are `–`, `‒`, `﹣` and full-width `－`
touching the number; a minus after a grouped or decimal amount (`1,234-`) also
negates it; `(1,234)` and `△1,234` are negatives; `1조 2,345억원`, `86億5,000万円`
and `1兆2,000億円` (兆 is 10^12) are each one amount; `TEUR 84.312` is thousands of
euros printed with a decimal comma; a unit line in words (`(R$ milhões)`,
`(em R$ milhões)`, `(Mio. EUR)`, `(₹ in crore)`, `(GBX)`) states its scale, and
`(R$ mil)` states none, since `mil` is a thousand or a million by language.
The filled triangle `▲` is a negative only in Japanese-context text, where
statements print negatives with either triangle: with kana within 512 bytes
before it, `有形固定資産の取得による支出 ▲510,803` reads −510,803. Elsewhere `▲`
marks a rise, as in a Korean price table: `전일 대비 ▲1,200원` reads +1,200원.
Answer text keeps `▲` as a rise. Reading is bounded to 50,000 candidates and
1 MiB of text per result.

Percentage points and basis points share a quantity. The checker converts
100 basis points to one percentage point and compares at the answer's printed
precision. For example, a tab-delimited `−265 bp` in a PDF table verifies
`−2.7%p` as rounded. Percent levels and relative percent changes keep their own
unit.

A numeric link can inherit a scale explicitly stated in its Markdown table
caption, column header, row label or sentence. Column units take priority over
a general caption, so a percent column keeps its own unit. An SEC XBRL fact's
`val` is in the series' base unit. For example, 383,285 in USD millions and
383.3 in USD billions both verify against 383285000000 USD at the precision
shown. Without the scale declaration the bare 383,285 remains unverified.
Equal printed digits can verify when one side leaves scale unstated. The
matched check names that side in `unstated_scale`. The popover and the chip's
accessible description say whether the cited passage or the answer leaves out
the unit. The result uses the verified style and stays out of the figures to
check. Calculation inputs require agreement in their supplied units. A table's
financial section can be an otherwise empty pipe row. Its subsequent totals keep
the declared unit while percentage columns, per-share values and share counts
keep theirs.
A cited excerpt can inherit the same table caption from the adjoining retained
window of its own capture.

For a monetary figure linked to a passage or to `run_python` stdout, compare the
signed source number at the precision the answer shows. A stated scale governs
the conversion, including program labels such as `USDm` and the monetary rows
under a filing's “Net sales by category” or “Net sales by reportable segment”
heading. Saved web results in `world_read` can be JSON inside the Source text.
The checker reads the decoded content with its original line breaks. Share counts
cannot support a monetary figure, and per-share rows cannot support an aggregate
expressed in millions or 억.

Korean magnitude words `만`, `억` and `조` scale the attached currency.
In a compound such as `$1조 5,407억`, the smaller component sets the displayed
precision. A TTM amount printed by the source is a passage check. If the amount
is only a sum of quarterly values, the answer can cite a retained calculation
that supplies the sum.

A row label that names a per-share figure (`EPS`, `per share`, `주당`,
`1株当たり`, `每股`) keeps the answer's figure in its own unit. A table caption
such as `단위는 십억 달러입니다` scales the table's other rows, not `$0.22`
in a `희석 EPS` row.

When the passage or stdout states no scale, compare the digits as printed. For
an answer with a magnitude word, align the significant digits by moving the
decimal point. Accept the reading only when the signed value agrees at the
answer's displayed precision. Monetary midpoints round away from zero. Thus `342.56달러` matches a bare `342.56`,
`1,450.97억 달러` can match `145,097` in an excerpt missing its caption, and
`504억 달러` matches `op USDm 50368.7433`. A stated “in millions” makes
`5,040` equal to 50.4억, so it cannot verify 504억. The chip tests membership
in the cited material. A match alone does not identify a company, period or row.

For a passage, a signed figure must agree with the sign the source prints or the
direction its wording gives the unsigned number
([Direction words](#direction-words)).

An unsigned answer can cite the magnitude of a decline stated in words. A
positive source number without a direction cue supports a positive or unsigned
answer. The answer's own wording counts the same way: an unsigned figure whose
clause names a loss (`net loss`, `순손실`), says it flowed out (`outflow`,
`유출`) or carries a decline or outflow cue touching it (`loss of`, `감소`,
`$1.2bn outflow`, `1,022,379백만 엔 유출`) can cite a negative source value of the
same magnitude. An explicitly printed sign takes precedence over wording. Receipts keep
the original UTF-8 evidence range even for full-width digits, punctuation and
signs (`－４．８１％`). Amounts can carry `元` or `円`, and `万`/`萬` and `亿`/`億`
scale by ten thousand and one hundred million respectively. `元` alone does not
identify a specific dollar or yuan currency.

### Direction words

`knowledge/numbers/direction.rs` reads the direction of each unsigned positive
figure in a passage, and of an answer's own unsigned figure. The host recognizes
common financial phrases in English, Korean, simplified and traditional Chinese,
Japanese, German, French and Spanish, in both directions. For example,
`loss of $70`, `9% 줄었고`, `1.58% 내린`, `跌幅4.81%`, `70円安`, `3%減`,
`fiel um 4.81 %`, `baisse de 4.81 %` and `cayó un 4.81%` state declines, and
`1.58% 오른`, `climbed 4.11%`, `4.11% higher`, `上涨4.11%`, `3.31%値上がり` and
`3%増` state rises. The market-report verbs cover each language's ordinary
forms: Korean 내리다 (내린, 내려, 내렸, 내리며, 내림), 급락, 폭락, 약세, 빠지다,
밀리다, 뒷걸음, 후퇴, 하향 and 오르다, 올리다, 뛰다, 급등, 폭등, 강세, 반등,
상향; English slid, tumbled, plunged, lost, shed, sank, eased and climbed,
jumped, surged, soared, advanced, added, rallied, rebounded, each with its
present forms where common; Japanese 値下がり, 下げ, 低下, 減, 反落, 急落,
続落 and 値上がり, 上げ, 増, 反発, 急騰, 続伸, 高騰; Chinese 下滑, 回落, 走低,
下挫, 降, 降幅 and 涨, 升, 升幅, 上升, 攀升, 回升, 走高, 反弹. A rise is evidence
as much as a fall: `+1.58%` matches `1.58% 오른`, and `-1.58%` against it is a
sign conflict.

A direction word binds to the figure it governs:

- It touches the figure, its unit or an index's point unit (`포인트`, `points`,
  `ポイント`, `点`), allowing up to two spaces each. A clause boundary, a line
  break or intervening prose ends its reach, and a word never reaches back into
  an earlier clause or sentence. In `下跌70元，跌幅4.81%` each change gets its own
  direction.
- A word that introduces a number belongs to the number after it (`下跌2%`,
  `declined by 2%`), and `to`, `from`, `至` and `到` introduce a level. A Korean
  word follows the change it states, so in `1.58% 내린 635.45` or
  `3.31% 하락 4,708.54` the rate takes the word and the level after it takes no
  direction. In `코스피는 3.31% 내린 4,708.54에, 코스닥은 1.58% 오른 645.45에`
  each rate takes its own word.
- A figure in brackets right after another figure, with only a unit or the first
  figure's own word between, restates the same change: `200.14포인트(4.11%) 내린`,
  `down 200.14 points (4.11%)`, `200.14ポイント(4.11%)安`, `下跌200.14点(4.11%)` and
  `200.14 points, or 4.11%, lower` give both figures one direction, from the
  word before the first or after the bracket or comma. A bracketed rate after a
  name is a level (`점유율(25%) 하락` gives 25% no direction).
- The word after the last rate of a list governs every rate before it that has
  no word of its own: `삼성전자 3.06%·SK하이닉스 2.52% 하락`,
  `Samsung 3.06% and SK Hynix 2.52% lower`, `각각 3.06%, 2.52% 내렸다`. Items are
  joined by `·`, `・`, `、`, a comma, `/`, `&`, `and`, `및`, `和`, `及` or `与`, and
  each item's name is at most three words with no digits; a Latin name is
  capitalized (`SK Hynix`, not `with stocks`). Only percents, percentage points and
  basis points form a list, and a rate introduced by a word (`at 3.5%`, `내린 3.5%`) is
  not an item. A word before a list's first figure reaches only that figure.
- A bound word between keeps the figure bare: `3% 넘게 하락해` and `9% 이상 줄었다`
  cite unsigned.

A comparison such as `not down 4.81%` or `跌幅收窄至4.81%` still names a decline
of that magnitude. A matched check whose minus sign comes from a decline in the
source's words records `sourceSign: "fall"`. Stored answers are re-evaluated with
these rules when read, so a flag from an earlier build clears without a new
answer.

### Outflows stated as amounts

Prose states spending and deductions as positive amounts; statements and data
providers print them negative or in brackets. An unsigned answer figure matches a
negative or bracketed source value of the same magnitude when the source's own row
label or field name is an outflow by nature, unless the answer's own clause names
a line of another nature. A generic `value`, `val` or `amount` field takes its
row's `label`, `line_item`, `metric`, `concept`, `account_nm`, `item` or `name`.
The answer's line is its table row label, or else the nearest clause before the
figure, within its sentence, that names a line; links and clause punctuation
separate clauses. The status is the one the same digits would get with the source's
sign (`rounded` where the scale changes the shown precision). The check records
`sourceSign: "outflow"`. For example, `설비투자는 $4.73bn` matches
`Capital expenditures (4,732)` under `($ in millions …)`, and `$5.80bn` matches a
dataset's `capital_expenditure: -5796000000`. A row whose label pairs an expense
with its opposite (`법인세비용(수익)`, `금융수익(비용)`, `Income tax expense (benefit)`)
counts the same way: its negative amount is the opposite item, so `124억원의 법인세수익`
matches `법인세비용(수익) (12,359,747,436)`. `Net income (loss)` and
`Other (expense) income, net` stay signed.

A table whose unit heading lists `%` beside another unit (`(단위 : 백만원, %)`, as in
DART's provisional-results notices) reads its decimal cells as rates as well as plain
numbers, so `-25.4%` matches the bare `-25.4` of its 증감율 column; its whole numbers
are not rates. A statement fact's `row`, `ordinal` and `scale` are positions and
multipliers, never figures.

The row-nature list lives in `knowledge/numbers/direction.rs` (`LINE_NATURES`).
A row's label is the text before its first number, after the last sentence end,
whatever the figure's unit. Every negative notation above (`(1,906,811)`,
`△510,803`, `-1,234`, `1,234-`) is the same negative. Phrases are lowercased,
with whitespace and `_` read as one space (`Capital\texpenditures`,
`capital_expenditure`). Latin phrases match whole words and CJK phrases match
inside words. The first group with a phrase in the label decides:

| Order | Nature | Meaning | Phrases (abridged) |
| --- | --- | --- | --- |
| 1 | Signed | Either sign; the sign is the meaning | change, increase, decrease, growth, margin, provided by, (used in), (loss), (gain), gain, (expense), (income), (benefit), YoY, variance; 증감, 증가, 감소, 변동, 이익률, 손익; 增减, 变动, 增加, 减少, 利润率, 损益, 同比; 増減, 増加, 利益率, 前年比 |
| 2 | Outflow | Spending or a deduction | capital expenditure(s), capex, expenditure, spending, outflow, purchase(s) of, payment(s) of/for/to, repayment, repurchase, buyback, dividends paid, taxes paid, interest paid, cash used in, investments in property, additions to, acquisitions, expense(s), cost of, costs, research and development, SG&A; 설비투자, 자본적 지출, 현금유출, 지출, 취득 (유형자산의취득), 유형자산 투자, 자사주 매입, 배당금 지급, 상환, 매출원가, 판매비와관리비, 비용; 资本支出, 购建固定资产, 支付, 购买, 偿还债务, 分配股利, 回购, 现金流出, 营业成本, 支出; 設備投資, 取得 (有形固定資産の取得による支出), 購入, 支払, 借入金の返済, 売上原価 |
| 3 | Loss | A loss named as such | net loss, operating loss, loss, deficit; 순손실, 영업손실, 손실, 적자; 净亏损, 亏损, 赤字; 純損失, 営業損失, 損失 |
| 4 | Signed | A line whose sign carries its meaning | net income, operating income, income from operations, EBIT(DA), free cash flow, operating cash flow, cash flow, cash from operations, EPS, net debt, net cash, income, profit, earnings, return; 순이익, 영업이익, 이익, 잉여현금흐름, 현금흐름; 净利润, 营业利润, 自由现金流, 现金流, 每股收益; 純利益, 営業利益, キャッシュフロー |
| 5 | Other | Never spending | revenue, sales, assets, liabilities, equity, cash and cash equivalents, deliveries, production, debt, price, shares, dividend; 매출, 자산, 부채, 자본, 주가, 배당; 营收, 收入, 资产, 负债, 股价; 売上, 収益, 資産, 株価 |

Order 1 lets `Net cash provided by (used in) investing activities`,
`Change in capital expenditures` and `Other (expense) income, net` take either
sign, while `Net cash used in investing activities` and `Income taxes paid` are
outflows. Rows of orders 1, 3 and 4 never take the exception: an unsigned positive
figure against `Net income (loss) (1,234)`, `Free cash flow (1,092)` or a negative
growth rate stays a warning, while `net loss of $1.23bn`, `−$1.23bn` or
`declined 4.8%` matches it ([Number matching](#number-matching)). An answer clause
that names another line, such as `Revenue was $4.73bn` or a `| 희석 EPS |` row,
never matches an outflow row of the same magnitude.

A known boundary is left for a deliberate decision: the working-capital rows of a
cash-flow statement. The statement prints an increase in an asset as a negative
cash effect, and an answer that states the increase stays a warning. A stored
research answer wrote `혼다는 금융채권 증가 904,344백만 엔` against Honda's
`資産及び負債の増減` row `金融サービスに係る債権 △1,454,357 △904,344`, and
`운용리스자산 690,110백만 엔` against `オペレーティング・リース資産12,661 △690,110`.
Neither row label names an outflow, and the answer states no fall.

### Choosing among candidates

Each candidate tier is tried in order, and the first candidate in the best tier
supports the figure:

1. The figure in the source's own sign and stated scale: exact, an exact scale
   conversion, equal digits with one scale unstated, then rounded.
2. The same, read through a sign the source states apart from its digits: an
   outflow row, or a fall stated in words for a signed figure.
3. A scale inferred by aligning the digits, a sign the source states first.

A literal passage selection is tried before the rest of the window. When the
figure sits in a Markdown table row of the answer, candidates are ordered by how
many of the row's other figures stand at the same column offsets in the
candidate's source row (numbers separated only by spacing, `|` or a placeholder
dash on one line). That order picks among candidates of the same tier, so the
reproduced row wins over an equal value in another row or period. It never makes
a lower tier win.

A shared alias table reads ISO codes, words and symbols before or after a
figure on either side of the check. A scale can follow the currency within
the same quantity, as in `20 USD million`, or precede it, as in
`20 million USD`. An interposed currency does not extend the scale across
a comma, conjunction or line break. For example, `Common shares: KRW 372`
verifies `372원` and `372 won`. A share-class label describes the row, while
a share-count label such as `Shares outstanding` still identifies a count.
Different explicit currencies give `incompatible_unit`. A shared sign
can use the currency on the same figure or in its governing unit. Without
that context, `$` is compatible with the supported dollar currencies, `¥`
with yen or yuan, and `元` with yen, yuan or the supported Chinese-language
dollar currencies. Unspecified currency still agrees with a currency amount
where the scale and numeric reading allow it.

A plain number in text also takes a unit the text states apart from it, and
keeps its plain reading beside it. A row label's bracketed unit
(`5. 배당금총액(원) 6,000,000,000`, `시가배당율(%) 보통주식 1.4`) reaches the first
run of numbers after it, with at most three label words between, and ends at a
line break, `;`, a sentence end or the first word after those numbers. In a
year-by-year list such as `Revenue (USD millions): 2024 245,122; 2023 211,915`,
the years label the values and the unit continues across the separator. A
table's unit (`(단위: 백만원)`, `단위 : 원`, `(Unit: KRW million)`,
`(in KRW millions)`) reaches until a blank line, a sentence end or the next
table's unit. A currency prefix also takes the stated amount scale. Filing headers
such as `(Millions, except per share amounts)` scale identifiable monetary rows
such as net revenue while leaving per-share and percentage cells unscaled. The same
rule applies to a filing that states a separate share-count scale and to a caption
immediately above its Markdown table. A filing caption ending in “(dollars in millions):”
sets the scale of the pipe-separated rows that immediately follow it. A list
of units such as `(단위: 원, %)` ends the preceding table without placing a scale. A unit in words or brackets right after one number (`184.6이고 단위는 십억 원`,
`300 (원)`) describes that number only. Only a bare number inherits: an item's
number (`4.` before a label), a number glued to a word (`18기`, `1,820명`) and a
number with its own unit never do. A bracket naming no unit (`(주)`, `(연결)`), a
lone letter (`(x)`) or a number (`(USD 10)`) is not a label.

A unit's number multiplies its scale (`KRW 100 million` = 1e8, `10억원` = 1e9,
`KRW 1,000` = 1e3). Any other number in an amount's unit, or a number in a
share or count unit, makes the unit ambiguous; what follows `per` or `/` is a
denominator and never scales the amount.

Chart and dataset query checks compare cells in the returned row window. Each
cell takes the unit of its own column from column metadata, its display or axis
label, or its semantic type. Dates and identifiers are dimensions. Labels and
query metadata themselves supply no amounts.

An exact digit match with a scale stated on only one side is marked
`unstated_scale` on the other side. This applies to passages, structured
observations, chart and dataset reads, calculation results and number checks in
grids. A scale conflict requires differing stated scales or values separated
by the exact factor of the one scale that is stated.

For `dcf_grid` and `reverse_dcf`, numeric grid cells use the grid's unit and
WACC and terminal growth axes use percentages. Unsolved cells supply no number.
`reverse_dcf` also verifies the formula's implied enterprise value from the
retained price, shares and net debt.

A `calculate` value keeps the unit it states. A `ratio` stated per something
(`USD per share`) is an amount in that unit, so `3.46달러` matches
`345.82 / 100 = 3.4582 USD per share`; a ratio of two amounts is a multiple (`x`)
or, with `as_percent`, a percentage ([Calculation](calculation.md#calculation-1)).
A stored ratio result that states a multiple stays a multiple on read: results
saved before the unit was kept print `345.82 / 100 = 3.46x`, so `3.46달러` stays
`differs`. Replay never reads a unit back from tool-call arguments.

For a link to a `calculate convert` result, the sole candidate is the converted
output in base units of the target currency.
An ISO code, recognized prefixed dollar sign, unambiguous currency symbol or
currency word for a different currency gives `differs` regardless of the digits.
Bare `$` and `¥`, dollar words such as “dollars” and “달러”, and links without a
currency marker are checked on magnitude and scale.

A line reading on an [asset page](../knowledge/thesis.md#thesis-and-the-read) is
accepted only when the observed number, normalized to the line's unit, equals a
candidate exactly at the precision the evidence prints. A percent never matches a
bare fraction, no ×/÷100 conversion is tried, and a line unit that would need a
guess is refused. Only originals count: a filing, data read or fetched page, or a
`calculate` result over one, whose computed value also confirms a number. Search
snippets, Memory reads and the model's own text never do.

### When a number is missing

A new numeric `not_found` check records `comparisonReason`. The host picks a
comparison only after it has finished verifying the citation. It first looks for
the same digits with an opposite sign, a conflicting stated scale, or an
incompatible unit such as a share count. It also recognizes adjacent transposed
digits or a single changed digit within 2% of the answer. These comparisons
require at least three digits at the answer's precision. On the line of a
selected literal passage or in a short labelled result with a single value, the host
can also offer the nearest value within 15%. Equally plausible values that differ in amount or stated unit leave
the comparison open.

With a candidate, `nearest` and `comparedValue` carry the figure as printed.
`span` points into the excerpt, `context` locates the original number in other
retained passage text, and a structured result or step uses its JSON `pointer`
(for Python, `/stdout`). The diagnostic reason is `digits_differ`,
`scale_conflict`, `sign_conflict` or `incompatible_unit`.
`nearestSourceUnit` identifies a unit the evidence states. `nearestSourceScale`
is the stated power of ten (6 for millions). An inferred scale stays absent.
When no candidate qualifies, `no_candidate` carries no comparison value or
location. On replay, a retained passage with no candidate also carries
`comparisonReplay` with reason `no_candidate` and the searched passage
(`retained_passage` and the Source title). If the evidence itself is unavailable,
the replay reason is `evidence_unavailable`.
These diagnostics are projected at read time from the evidence the host can
still inspect. They do not change the saved answer.

Opening a flagged figure names the Source or calculation result, then shows the
answer’s figure beside the host’s printed candidate and its stated unit and
scale. The sentence explains the recorded difference. A missing candidate has
only the answer’s figure and names the saved passage when the host supplies its
scope. The popover and the receipt distinguish a figure absent from the saved
passage from one whose evidence is no longer stored. When the evidence is gone,
the receipt and popover omit the source action. The receipt’s count opens a list
of all flagged figures.
Each comparison opens its own retained source passage, structured field or
calculation step. Bot opens the selected calculation output in Activity. A
research session expands the step in its timeline. If a saved excerpt ends
before a structured field, the reader shows the recorded path and printed
value from the receipt alongside that excerpt. A located passage opens in Evidence even when an ordinary
citation to that web page would open the browser.

## Code

- Number checks: `src-tauri/src/knowledge/numbers.rs`, `src-tauri/src/finance_data/percent_fields.rs`, `src-tauri/src/commands/tool_executor/number_checks.rs`, `src-tauri/src/commands/tool_executor/number_checks/diagnostics.rs`, `src-tauri/src/commands/tool_executor/calculation.rs`, `src-tauri/src/domain/answer_checks.rs`, `src/chat/numberChecks.ts`, `src/components/chat/NumberChip.tsx`, `src/components/chat/FigureWarnings.tsx`
