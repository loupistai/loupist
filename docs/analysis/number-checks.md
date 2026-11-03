# Number checks

A number chip is a host check of a figure the answer used against a pinned input. The chip does not make the figure advice.

## Number checks

`SYSTEM.md` has the agent write each material number as a link whose text is only
the number with its unit and whose target is the `citation_url` it came from
([prompt rule](../agent/tools.md#tool-contracts-and-turn-preparation)). When the
reply commits, `commands/tool_executor/number_checks.rs` reads the answer's
`[text](#evidence-…)` links outside code, in order, and checks each one whose whole
text reads as one number against the result it links, from this turn's retained
results. A link to a result the turn did not retain is skipped and renders as
"Source isn't available in this reply". The answer stores at most 200 checks as
`number_checks` ([turn authority](../runtime/chat-turns.md#turn-authority-and-finalization));
later links stay numbered citations. Host-composed and failed replies have none.
Model text cannot set a status, and the renderer never rechecks.

A link's text is a number when it is, within 64 bytes, an optional sign or
accounting negative (`(1,234)` with grouped digits, `△1,234`; `▲` is a rise), a
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
| `calculated` | `calculate` | Its values in their units, then its inputs; a unitless value is tried in the printed scale and in base units |
| `data` | A read with a `dataset_ref` (OpenBB, DART, KRX, KIS, World Bank) | Every number in the retained payload, with its JSON Pointer |
| `step` | `dataset_query`, `dataset_describe`, or another derived or workspace-file result | Every number in the retained payload |
| `passage` | Any other read, such as a web page or filing text | Numbers printed in the evidence's retained excerpt, with byte ranges |

The printed number must lie within half a unit of its last shown digit (`8.4%`
within ±0.05%, `4,523억` within ±0.5억). The status is `matched` for a candidate
in the same unit and scale; `rounded` when reaching it takes a scale, percent↔fraction
or bp↔pp conversion (the check keeps the value as the evidence prints it);
`differs` only for a calculation with no value or input in range (the check keeps
the nearest value as the result displays it); otherwise `not_found`. When the
excerpt of a passage does not hold the number, the full retained result decides the
status from its content fields only (`content`, `text`, `passage`, `passages`,
`body`, `markdown`, `excerpt`, `value`, `values`), never its metadata such as status,
sizes or offsets, and the check has no span to mark. A matched or rounded check
keeps its data cell's pointer with up to four fields of the row (descriptive fields
such as company, concept, period and basis, then the matched cell) or its passage
span; a calculated check keeps the title, up to eight formula lines, up to twelve inputs
with their label, unit, source and status, and a DCF grid's size; the popover names and
prints each input from these and never reads them from the formula or the evidence (an
input without a unit prints as its bare number). Surfaces are listed in
[Where each item appears](surfaces.md#where-each-item-appears).

### Number matching

`knowledge/numbers.rs` reads the numbers in a result for number checks, cited
`calculate` inputs and asset-page line readings. Each candidate keeps the unit the
evidence states (`%`, `%p`/`pp`, `bp`, `x`/`배`, a scale word such as million or 억
with a currency, or a structured row's `unit`, `scale` or `currency` field) and its
place: a byte range in text or a JSON Pointer. Provider fields documented as
percents (`finance_data/percent_fields.rs`: KRX `FLUC_RT`, KIS `*_ctrt`, `*_rate`,
`*_rt`, `rate` and `n_rate`, and `change_percent`, `*_percent`, `*_pct` and `*_pc`)
read as percents; `*_ratio` fields and other unlabelled structured numbers are raw
values. Fields that name, date, order or locate a row are never candidates:
identifiers and codes (`rcept_no`, `corp_code`, `symbol`, `*_cd`, `*_no`, `*_id`),
dates and times (`date`, `bsns_year`, `*_dd`, `*_at`, `createdAtMs`), offsets,
sequence, rank and page fields, revisions, versions, digests, URLs and status. In
text, dates, identifiers and times (`2026-03-18`, `10-K`, `Q3`), digit codes with a
leading zero (`005930`) and six-digit tickers in brackets or with an exchange suffix
(`(990221)`, `990221.KS`) are not numbers; a hyphen after a digit or letter is not a
sign; `(1,234)` and `△1,234` are negatives; and `1조 2,345억원` is one amount. Reading
is bounded to 50,000 candidates and 1 MiB of text per result.

A line reading on an [asset page](../knowledge/thesis.md#thesis-and-the-read) is
accepted only when the observed number, normalized to the line's unit, equals a
candidate exactly at the precision the evidence prints. A percent never matches a
bare fraction, no ×/÷100 conversion is tried, and a line unit that would need a
guess is refused. Only originals count: a filing, data read or fetched page, or a
`calculate` result over one, whose computed value also confirms a number. Search
snippets, Memory reads and the model's own text never do.

## Code

- Number checks: `src-tauri/src/knowledge/numbers.rs`, `src-tauri/src/finance_data/percent_fields.rs`, `src-tauri/src/commands/tool_executor/number_checks.rs`, `src-tauri/src/commands/tool_executor/calculation.rs`, `src-tauri/src/domain/answer_checks.rs`, `src/chat/numberChecks.ts`, `src/components/chat/NumberChip.tsx`
