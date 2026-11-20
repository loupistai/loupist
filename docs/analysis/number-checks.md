# Number checks

A number chip is a host check of a figure the answer used against a pinned input. The chip does not make the figure advice.

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
results. A link to a result the turn did not retain is skipped and renders as
"Source isn't available in this reply". The answer stores at most 200 checks as
`number_checks` ([turn authority](../runtime/chat-turns.md#turn-authority-and-finalization));
later links stay numbered citations. Host-composed and failed replies have none.
Model text cannot set a status, and the renderer never rechecks. Each check names
its link by `link_index`, the link's place in that order. The chat renderer and
export count the stored answer's links the same way and show a check only on the
link at its index, and only when the evidence and text still agree. A repeated
figure keeps each occurrence's own check, and a link past the bound has none.

A link's text is a number when it is, within 64 bytes, an optional sign (`+`, `-`,
`−`) or accounting negative (`(1,234)` with grouped digits, `△1,234`; `▲` is a rise), a
currency symbol, `US$`-style prefix or ISO code, digits with `,` grouping and one
decimal point, then at most one unit: `%`, points (`pp`, `%p`, `% points`), `bp`, a
multiple (`x` glued to the number, `×`, `times`, `배`), an English scale (`thousand`,
`mn`, `million`, `bn`, `tn`; `k`, `m`, `b`, `t` only glued to the number or after a
currency), Korean scales including compounds in decreasing order (`1조 2,300억원`),
and a trailing currency word or code or "per share". Words, dates, tickers, a bare
year from 1900 to 2100 and a bare integer of one to three digits (a numbered
citation) are not numbers.

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

For structured `data` checks, an unsigned positive
figure may match the magnitude of a negative source value: the surrounding words
can state the fall or distance below. The checker first tries the ordinary signed
match, then the negative magnitude with the same unit and precision rules. An
explicit plus/minus sign, accounting parentheses or triangle mark must agree with
the source. This exception never applies to passages, derived steps or
`calculate` results; text states a sign only with the number or in a word right
after it ([Number matching](#number-matching)).

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
in the same unit and scale; `rounded` when reaching it takes a scale, percent↔fraction
or bp↔pp conversion (the check keeps the value as the evidence prints it, for a
number inside a text field as printed there);
`differs` only for a calculation with no value or input in range (the check keeps
the nearest value as the result displays it); otherwise `not_found`. When the
excerpt of a passage does not hold the number, the full retained result decides the
status from its content fields only (`content`, `text`, `passage`, `passages`,
`body`, `markdown`, `excerpt`, `value`, `values`), never its metadata such as status,
sizes or offsets. A matched or rounded fallback check keeps `context` from the
content field holding the first match: at most 240 characters of `text` around the
number, with end-exclusive UTF-8 byte offsets `start` and `end` inside that text.
It has no excerpt `span`; excerpt matches keep their span and no context. Older
stored checks without context still load, and data checks and `not_found` checks
have none. Context records where the existing verdict matched; it does not change
which numbers the host accepts. A matched or rounded check
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
text field (`"−1.20"`) as in a figure; `(1,234)` and `△1,234` are negatives; and
`1조 2,345억원` is one amount. Reading
is bounded to 50,000 candidates and 1 MiB of text per result.

A word of decrease right after an unsigned number and its unit, with at most two
spaces between, states a fall: Korean `감소`, `줄었`/`줄어`/`줄고`/`줄인`/`줄여`,
`하락`, `떨어`, `낮아`, `낮은`, `축소`, `급감` and `급락` (`9% 줄었고`,
`14억원 감소`), or English `lower`, `less`, `below`, `fall`, `falls`, and `decline`,
`decrease` or `drop` with their `-s` and past forms (`9% lower`, `a 9% decline`). A figure
with its own sign (`+`, `-`, `−`, a triangle mark or accounting brackets) reads
that number as negative: `−9%` matches `약 9% 줄었고` and `+9%` does not. An
unsigned figure still matches the magnitude, since its own words state the
direction. A cited `calculate` input reads the same way: `-9` matches, `9` matches
the magnitude and `"+9"` does not. A particle, a line break or any other word between
(`52억원으로 … 줄었다`, `248달러보다 낮아졌다`, `9% 이상 줄었다`) states nothing,
a number printed with its own sign keeps it, and words of increase need no rule:
an unsigned number already reads as a rise.

A plain number in text also takes a unit the text states apart from it, and
keeps its plain reading beside it. A row label's bracketed unit
(`5. 배당금총액(원) 6,000,000,000`, `시가배당율(%) 보통주식 1.4`) reaches the first
run of numbers after it, with at most three label words between, and ends at a
line break, `;`, a sentence end or the first word after those numbers. A
table's unit (`(단위: 백만원)`, `단위 : 원`, `(Unit: KRW million)`,
`(in KRW millions)`) reaches until a blank line, a sentence end or the next
table's unit. A currency prefix also takes the stated amount scale. Filing headers
such as `(Millions, except per share amounts)` scale identifiable monetary rows
such as net revenue while leaving per-share and percentage cells unscaled. A list
of units such as `(단위: 원, %)` ends the preceding table without placing a scale. A unit in words or brackets right after one number (`184.6이고 단위는 십억 원`,
`300 (원)`) describes that number only. Only a bare number inherits: an item's
number (`4.` before a label), a number glued to a word (`18기`, `1,820명`) and a
number with its own unit never do. A bracket naming no unit (`(주)`, `(연결)`), a
lone letter (`(x)`) or a number (`(USD 10)`) is not a label.

A unit's number multiplies its scale (`KRW 100 million` = 1e8, `10억원` = 1e9,
`KRW 1,000` = 1e3). Any other number in an amount's unit, or a number in a
share or count unit, makes the unit ambiguous; what follows `per` or `/` is a
denominator and never scales the amount.

For `dcf_grid` and `reverse_dcf`, numeric grid cells use the grid's unit and
WACC and terminal growth axes use percentages. Unsolved cells supply no number.
`reverse_dcf` also verifies the formula's implied enterprise value from the
retained price, shares and net debt.

For a link to a `calculate convert` result, the sole candidate is the converted
output in base units of the target currency.
An ISO code, recognized prefixed dollar sign, unambiguous currency symbol or
currency word for a different currency gives `differs` regardless of the digits.
Bare `$` and `¥`, dollar words such as “dollars” and “달러”, and links without a
currency marker are checked on magnitude and scale. The general matcher for
links to other results stays blind to currency identity.

A line reading on an [asset page](../knowledge/thesis.md#thesis-and-the-read) is
accepted only when the observed number, normalized to the line's unit, equals a
candidate exactly at the precision the evidence prints. A percent never matches a
bare fraction, no ×/÷100 conversion is tried, and a line unit that would need a
guess is refused. Only originals count: a filing, data read or fetched page, or a
`calculate` result over one, whose computed value also confirms a number. Search
snippets, Memory reads and the model's own text never do.

## Code

- Number checks: `src-tauri/src/knowledge/numbers.rs`, `src-tauri/src/finance_data/percent_fields.rs`, `src-tauri/src/commands/tool_executor/number_checks.rs`, `src-tauri/src/commands/tool_executor/calculation.rs`, `src-tauri/src/domain/answer_checks.rs`, `src/chat/numberChecks.ts`, `src/components/chat/NumberChip.tsx`
