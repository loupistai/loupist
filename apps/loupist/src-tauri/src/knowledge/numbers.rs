//! Finds a number among a piece of evidence's numbers (finance-show-the-math.md
//! §7.3). Two callers share it. The asset page's line readings (`find`) accept
//! the model's observed value only when an original result prints exactly that
//! quantity, at the evidence's printed precision, in the unit of the user's
//! line. Number chips under answers (`parse_printed`, `check`) read a link's
//! whole text as one number and compare it with the linked evidence within half
//! a unit of the answer's last printed digit.
//!
//! Each candidate keeps the unit the evidence states (a `%` or `bp` suffix, a
//! scale word such as million or 억, a currency, or a structured row's `unit`
//! field) and where it sits (a byte range in a passage, a JSON Pointer in a
//! structured result). Dates, identifiers and times (`2026-03-18`, `10-K`,
//! `Q3`) are not numbers, a `-` right after a digit or letter is never a sign,
//! and Korean compounds (`1조 2,345억`) read as one amount. For a line reading a
//! percent never matches a bare fraction and no ×/÷100 guess is made; a number
//! chip reports such a conversion as `Rounded`.
use serde_json::{Map, Value};

/// Candidates read from one payload; bounds the work on large datasets.
const MAX_CANDIDATES: usize = 50_000;
const MAX_TEXT_BYTES: usize = 1024 * 1024;

/// What a line's unit, or a printed number, measures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    /// A count, level or ratio with no stated unit.
    Plain,
    /// A structured row's value with no stated unit: raw base units.
    Raw,
    Percent,
    /// Percentage points (`%p`, `pp`).
    Points,
    BasisPoints,
    /// A multiple (`x`, `배`).
    Multiple,
    /// An amount in base units times this scale (`억원` = 1e8, `USD bn` = 1e9).
    Amount(f64),
}

/// A unit word the matcher cannot place without guessing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmbiguousUnit(pub String);

const CURRENCY_CODES: &[&str] = &[
    "usd", "krw", "eur", "jpy", "gbp", "cny", "cnh", "chf", "aud", "cad", "hkd", "sgd", "nzd",
    "sek", "nok", "dkk", "inr", "twd", "mxn", "brl", "zar",
];
const CURRENCY_WORDS: &[&str] = &[
    "won", "dollar", "dollars", "euro", "euros", "yen", "yuan", "pound", "pounds",
];
const CURRENCY_SYMBOLS: &[char] = &['$', '₩', '€', '£', '¥', '￦'];
/// Korean currency suffixes after an optional scale (`억원`, `백만달러`).
const KOREAN_CURRENCY: &[&str] = &["원", "달러", "엔", "위안", "유로"];
/// Korean scales, longest first so `천억` wins over `억`.
const KOREAN_SCALES: &[(&str, f64)] = &[
    ("천억", 1e11),
    ("백억", 1e10),
    ("십억", 1e9),
    ("천만", 1e7),
    ("백만", 1e6),
    ("십만", 1e5),
    ("조", 1e12),
    ("억", 1e8),
    ("만", 1e4),
    ("천", 1e3),
];

/// The ISO 4217 currency a unit names ("USD millions", "₩", "억원", "KRW bn"),
/// when it names exactly one.
pub fn currency_code(unit: &str) -> Option<String> {
    const PREFIXED_DOLLARS: &[(&str, &str)] = &[
        ("us$", "USD"),
        ("hk$", "HKD"),
        ("nz$", "NZD"),
        ("a$", "AUD"),
        ("c$", "CAD"),
        ("s$", "SGD"),
    ];
    const SYMBOLS: &[(char, &str)] = &[
        ('$', "USD"),
        ('₩', "KRW"),
        ('￦', "KRW"),
        ('€', "EUR"),
        ('£', "GBP"),
        ('¥', "JPY"),
    ];
    const WORDS: &[(&str, &str)] = &[
        ("won", "KRW"),
        ("dollar", "USD"),
        ("dollars", "USD"),
        ("euro", "EUR"),
        ("euros", "EUR"),
        ("yen", "JPY"),
        ("yuan", "CNY"),
        ("pound", "GBP"),
        ("pounds", "GBP"),
    ];
    const KOREAN: &[(&str, &str)] = &[
        ("원", "KRW"),
        ("달러", "USD"),
        ("엔", "JPY"),
        ("위안", "CNY"),
        ("유로", "EUR"),
    ];
    let mut rest = unit.trim().to_lowercase();
    let mut found: Vec<String> = Vec::new();
    let mut add = |code: String| {
        if !found.contains(&code) {
            found.push(code);
        }
    };
    for (prefix, code) in PREFIXED_DOLLARS {
        if rest.contains(prefix) {
            add((*code).to_owned());
            rest = rest.replace(prefix, " ");
        }
    }
    for (symbol, code) in SYMBOLS {
        if rest.contains(*symbol) {
            add((*code).to_owned());
        }
    }
    for word in rest
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        if CURRENCY_CODES.contains(&word) {
            add(word.to_ascii_uppercase());
        } else if let Some((_, code)) = WORDS.iter().find(|(name, _)| *name == word) {
            add((*code).to_owned());
        } else if let Some((_, code)) = KOREAN.iter().find(|(suffix, _)| word.ends_with(suffix)) {
            add((*code).to_owned());
        }
    }
    (found.len() == 1).then(|| found.remove(0))
}

fn english_scale(word: &str) -> Option<f64> {
    match word {
        "thousand" | "thousands" => Some(1e3),
        "million" | "millions" | "mn" | "mm" | "mil" => Some(1e6),
        "billion" | "billions" | "bn" => Some(1e9),
        "trillion" | "trillions" | "tn" => Some(1e12),
        _ => None,
    }
}

/// Single-letter scales are read only next to a currency or a digit.
fn letter_scale(word: &str) -> Option<f64> {
    match word {
        "k" => Some(1e3),
        "m" => Some(1e6),
        "b" => Some(1e9),
        "t" => Some(1e12),
        _ => None,
    }
}

fn korean_amount(text: &str) -> Option<f64> {
    let mut rest = text;
    let mut currency = false;
    for suffix in KOREAN_CURRENCY {
        if let Some(stripped) = rest.strip_suffix(suffix) {
            rest = stripped;
            currency = true;
            break;
        }
    }
    if rest.is_empty() {
        return currency.then_some(1.0);
    }
    KOREAN_SCALES
        .iter()
        .find(|(word, _)| *word == rest)
        .map(|(_, scale)| *scale)
}

impl Unit {
    /// The unit of a user's line (`check.unit`). Unknown words (`stores`,
    /// `days`) are counts; a lone scale letter or a percent mixed with an
    /// amount is ambiguous and refused.
    pub fn of_line(unit: Option<&str>) -> Result<Self, AmbiguousUnit> {
        let Some(raw) = unit.map(str::trim).filter(|unit| !unit.is_empty()) else {
            return Ok(Self::Plain);
        };
        let text = raw.to_lowercase();
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        match compact.as_str() {
            "%" | "％" | "percent" | "pct" | "percentage" | "퍼센트" => {
                return Ok(Self::Percent)
            }
            "%p" | "%pt" | "%pts" | "pp" | "ppt" | "ppts" | "percentagepoint"
            | "percentagepoints" | "%포인트" | "퍼센트포인트" => return Ok(Self::Points),
            "bp" | "bps" | "basispoint" | "basispoints" | "베이시스포인트" => {
                return Ok(Self::BasisPoints)
            }
            "x" | "times" | "multiple" | "배" => return Ok(Self::Multiple),
            _ => {}
        }
        if let Some(scale) = korean_amount(&compact) {
            return Ok(Self::Amount(scale));
        }
        let ambiguous = || AmbiguousUnit(raw.to_owned());
        let mut currency = text.chars().any(|c| CURRENCY_SYMBOLS.contains(&c));
        let mut scale: Option<f64> = None;
        let mut letters: Option<f64> = None;
        let mut percent = text.contains('%');
        for word in text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
        {
            if CURRENCY_CODES.contains(&word) || CURRENCY_WORDS.contains(&word) {
                currency = true;
            } else if let Some(found) = english_scale(word) {
                if scale.replace(found).is_some_and(|known| known != found) {
                    return Err(ambiguous());
                }
            } else if let Some(found) = letter_scale(word) {
                letters = Some(found);
            } else if let Some(found) = korean_amount(word) {
                currency |= found == 1.0;
                if found != 1.0 && scale.replace(found).is_some_and(|known| known != found) {
                    return Err(ambiguous());
                }
            } else if matches!(word, "percent" | "pct") {
                percent = true;
            }
        }
        // "$m" and "KRW bn" read as scales; a bare "m" could be metres or minutes.
        if let Some(found) = letters {
            if !currency || scale.is_some_and(|known| known != found) {
                return Err(ambiguous());
            }
            scale = Some(found);
        }
        match (percent, currency || scale.is_some()) {
            // "% of bn" mixes a share with an amount.
            (true, true) => Err(ambiguous()),
            // "% of revenue", "% points".
            (true, false) if compact.contains("point") || compact.contains("포인트") => {
                Ok(Self::Points)
            }
            (true, false) => Ok(Self::Percent),
            (false, true) => Ok(Self::Amount(scale.unwrap_or(1.0))),
            (false, false) => Ok(Self::Plain),
        }
    }

    /// The observed value in the quantity candidates are compared in.
    fn normalized(self, value: f64) -> f64 {
        match self {
            Self::Amount(scale) => value * scale,
            Self::BasisPoints => value / 100.0,
            _ => value,
        }
    }

    /// A candidate's value in the line's quantity, when the units are compatible.
    fn compare(line: Self, candidate: Self, value: f64) -> Option<f64> {
        match (line, candidate) {
            (Self::Percent, Self::Percent) => Some(value),
            (Self::Points | Self::BasisPoints, Self::Points) => Some(value),
            (Self::Points | Self::BasisPoints, Self::BasisPoints) => Some(value / 100.0),
            (Self::Multiple, Self::Multiple | Self::Plain | Self::Raw) => Some(value),
            (Self::Plain, Self::Plain | Self::Raw | Self::Multiple) => Some(value),
            (Self::Amount(_), Self::Amount(scale)) => Some(value * scale),
            // Structured rows carry base units (DART and OpenBB report raw values).
            (Self::Amount(_), Self::Raw) => Some(value),
            _ => None,
        }
    }
}

/// Where a candidate sits in its evidence: UTF-8 byte offsets in a passage,
/// or the JSON Pointer of a structured value (a text field's own pointer when
/// the number is printed inside it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    Span { start: usize, end: usize },
    Pointer(String),
}

/// One number printed in the evidence, with the unit printed beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub value: f64,
    pub unit: Unit,
    pub place: Place,
}

/// A Korean scale and currency printed after a number, before any particle
/// (`4,523억원으로`, `3천만`, `300원`), with the bytes they take and whether
/// the scale stood alone (so `1조 2,345억` can join into one amount).
fn korean_suffix(word: &str) -> Option<(f64, usize, bool)> {
    let currency = |rest: &str| {
        KOREAN_CURRENCY
            .iter()
            .find(|unit| rest.starts_with(**unit))
            .map(|unit| unit.len())
    };
    for (scale_word, scale) in KOREAN_SCALES {
        if let Some(rest) = word.strip_prefix(scale_word) {
            if let Some(length) = currency(rest) {
                return Some((*scale, scale_word.len() + length, false));
            }
            // A bare scale counts only when nothing but a particle follows.
            if rest.chars().count() <= 1 {
                return Some((*scale, scale_word.len(), true));
            }
        }
    }
    currency(word).map(|length| (1.0, length, false))
}

/// What a printed number states right after it: its unit, the bytes of
/// `rest` the unit takes, and the Korean scale when one stood alone.
fn suffix_unit(rest: &str) -> (Unit, usize, Option<f64>) {
    // ASCII lowercase keeps byte offsets; the unit words below are ASCII or Korean.
    let lower = rest.to_ascii_lowercase();
    let trimmed = lower.trim_start_matches([' ', '\u{a0}']);
    let gap = lower.len() - trimmed.len();
    let word: String = trimmed
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '%' | '％'))
        .collect();
    for phrase in [
        "percentage points",
        "percentage point",
        "% points",
        "% point",
    ] {
        if trimmed.starts_with(phrase) {
            return (Unit::Points, gap + phrase.len(), None);
        }
    }
    for prefix in ["%포인트", "%pts", "%pt", "%p"] {
        if word.starts_with(prefix) {
            return (Unit::Points, gap + prefix.len(), None);
        }
    }
    if word == "pp" {
        return (Unit::Points, gap + 2, None);
    }
    if let Some(first) = word.chars().next().filter(|c| matches!(c, '%' | '％')) {
        return (Unit::Percent, gap + first.len_utf8(), None);
    }
    if matches!(word.as_str(), "percent" | "pct") {
        return (Unit::Percent, gap + word.len(), None);
    }
    for phrase in ["basis points", "basis point"] {
        if trimmed.starts_with(phrase) {
            return (Unit::BasisPoints, gap + phrase.len(), None);
        }
    }
    if matches!(word.as_str(), "bp" | "bps") {
        return (Unit::BasisPoints, gap + word.len(), None);
    }
    let multiple = word == "배"
        || (word.starts_with('배') && word.chars().count() == 2 && !word.starts_with("배당"));
    if (gap == 0 && word == "x") || multiple || word == "times" {
        let length = if multiple { "배".len() } else { word.len() };
        return (Unit::Multiple, gap + length, None);
    }
    if let Some(scale) = english_scale(&word) {
        return (Unit::Amount(scale), gap + word.len(), None);
    }
    if gap == 0 {
        if let Some(scale) = letter_scale(&word) {
            return (Unit::Amount(scale), word.len(), None);
        }
    }
    if let Some((scale, length, bare)) = korean_suffix(&word) {
        return (Unit::Amount(scale), gap + length, bare.then_some(scale));
    }
    if CURRENCY_CODES.contains(&word.as_str()) || CURRENCY_WORDS.contains(&word.as_str()) {
        return (Unit::Amount(1.0), gap + word.len(), None);
    }
    (Unit::Plain, 0, None)
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// A number found in text before Korean compounds are joined.
struct Scanned {
    value: f64,
    unit: Unit,
    start: usize,
    end: usize,
    /// The Korean scale when it stood alone (`1조`), so a smaller one may follow.
    bare_scale: Option<f64>,
}

/// Numbers written in `text`, each with the unit printed next to it and its
/// byte range (sign, currency symbol and unit included). Digits group with
/// `,` and take one decimal point. A run joined by `-`, `/` or `:` (dates,
/// identifiers, times) or with a second `.` (`2026.03.18`) is skipped, as is a
/// number glued to letters (`Q3`, `FY2026`, `10-K`). Korean compounds
/// (`1조 2,345억원`) are one amount in the smallest scale.
fn text_numbers(text: &str, out: &mut Vec<Candidate>) {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let byte = |index: usize| chars.get(index).map_or(text.len(), |(offset, _)| *offset);
    let mut scanned: Vec<Scanned> = Vec::new();
    let mut index = 0;
    while index < chars.len() && out.len() + scanned.len() < MAX_CANDIDATES {
        let (_, c) = chars[index];
        if !c.is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        let previous = start.checked_sub(1).map(|i| chars[i].1);
        let before = start.checked_sub(2).map(|i| chars[i].1);
        // Scan the whole run, joiners included, so a date is skipped as one token.
        let mut end = start;
        let mut joined = false;
        let mut points = 0;
        while end < chars.len() {
            let c = chars[end].1;
            let next_digit = chars.get(end + 1).is_some_and(|(_, n)| n.is_ascii_digit());
            if c.is_ascii_digit() || (c == ',' && next_digit) {
                end += 1;
            } else if c == '.' && next_digit {
                points += 1;
                end += 1;
            } else if matches!(c, '-' | '/' | ':') && next_digit {
                joined = true;
                end += 1;
            } else {
                break;
            }
        }
        let following = chars.get(end).map(|(_, c)| *c);
        let after = chars.get(end + 1).map(|(_, c)| *c);
        index = end;
        let glued_before = previous.is_some_and(is_word_char)
            || (matches!(previous, Some('-' | '/' | '.' | ':'))
                && before.is_some_and(is_word_char));
        let identifier_after = matches!(following, Some('-' | '_'))
            && after.is_some_and(|c| c.is_alphabetic())
            || (following.is_some_and(|c| c.is_ascii_alphabetic())
                && after.is_some_and(|c| c.is_ascii_digit()));
        if joined || points > 1 || glued_before || identifier_after {
            continue;
        }
        let run: String = chars[start..end].iter().map(|(_, c)| *c).collect();
        let grouped = run.contains(',');
        // Codes, not quantities: a leading zero (`005930`), or a six-digit
        // ticker in brackets or with an exchange suffix (`(990221)`, `990221.KS`).
        let ticker = run.len() == 6
            && run.bytes().all(|b| b.is_ascii_digit())
            && ((previous == Some('(') && following == Some(')'))
                || (following == Some('.') && after.is_some_and(|c| c.is_ascii_uppercase())));
        if code_like(&run) || ticker {
            continue;
        }
        let digits: String = run.chars().filter(|c| *c != ',').collect();
        let Ok(mut value) = digits.parse::<f64>() else {
            continue;
        };
        let mut span_start = byte(start);
        let mut signed = false;
        // A sign only after a space, an opening bracket or the start of text.
        if matches!(previous, Some('-' | '−'))
            && !before.is_some_and(|c| c.is_alphanumeric() || matches!(c, ')' | ']' | '%'))
        {
            value = -value;
            span_start = byte(start - 1);
            signed = true;
        } else if let Some(mark) = previous.and_then(triangle_sign) {
            // Korean statements print a loss as `△1,234`; `▲` marks a rise.
            value *= mark;
            span_start = byte(start - 1);
            signed = true;
        }
        let rest_start = byte(end);
        let (mut unit, length, bare_scale) = suffix_unit(&text[rest_start..]);
        // A currency before the number makes it an amount ("$4.5bn", "₩41,200").
        let prefix: String = chars[start.saturating_sub(5)..start]
            .iter()
            .map(|(_, c)| *c)
            .collect::<String>()
            .to_lowercase();
        let symbol = prefix
            .trim_end_matches(['-', '−', ' '])
            .ends_with(|c: char| CURRENCY_SYMBOLS.contains(&c));
        let prefixed = symbol
            || CURRENCY_CODES
                .iter()
                .any(|code| prefix.trim_end().ends_with(code));
        if prefixed && matches!(unit, Unit::Plain) {
            unit = Unit::Amount(1.0);
        }
        let mut lead = start;
        if symbol {
            // Mark the symbol with the number (`₩41,200`, `-$3.1bn`).
            let mut at = start;
            while at > 0 && start - at < 5 {
                at -= 1;
                if CURRENCY_SYMBOLS.contains(&chars[at].1) {
                    span_start = span_start.min(byte(at));
                    lead = at;
                    break;
                }
            }
        }
        let mut span_end = rest_start + length;
        // An accounting negative: a grouped amount wholly in brackets
        // (`(1,234)`, `(₩1,234)`, `(1,234억원)`), never a share or a multiple.
        if !signed
            && grouped
            && matches!(unit, Unit::Plain | Unit::Amount(_))
            && lead > 0
            && chars[lead - 1].1 == '('
            && text[span_end..].starts_with(')')
        {
            value = -value;
            span_start = byte(lead - 1);
            span_end += 1;
        }
        scanned.push(Scanned {
            value,
            unit,
            start: span_start,
            end: span_end,
            bare_scale,
        });
    }
    // Join `1조 2,345억원`: a bare larger scale followed by a smaller Korean amount.
    let mut joined: Vec<Scanned> = Vec::with_capacity(scanned.len());
    for next in scanned {
        if let Some(last) = joined.last_mut() {
            let between = text.get(last.end..next.start).unwrap_or("x");
            if let (Some(large), Unit::Amount(small)) = (last.bare_scale, next.unit) {
                if small < large
                    && small > 1.0
                    && between.chars().all(char::is_whitespace)
                    && between.chars().count() <= 1
                    && last.value >= 0.0
                    && next.value >= 0.0
                {
                    last.value = last.value * large / small + next.value;
                    last.unit = Unit::Amount(small);
                    last.end = next.end;
                    last.bare_scale = next.bare_scale;
                    continue;
                }
            }
        }
        joined.push(next);
    }
    out.extend(joined.into_iter().map(|found| Candidate {
        value: found.value,
        unit: found.unit,
        place: Place::Span {
            start: found.start,
            end: found.end,
        },
    }));
}

/// The unit a structured row states for its numeric fields.
fn row_unit(row: &Map<String, Value>) -> Option<Unit> {
    let scale = row
        .get("scale")
        .and_then(|value| match value {
            Value::Number(number) => number.as_f64(),
            Value::String(text) => text.trim().parse().ok(),
            _ => None,
        })
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(1.0);
    if let Some(unit) = row.get("unit").and_then(Value::as_str) {
        // "KRW/share" is a per-share amount in base units.
        let unit = unit.split('/').next().unwrap_or(unit);
        return Some(match Unit::of_line(Some(unit)) {
            Ok(Unit::Amount(found)) => Unit::Amount(found * scale),
            Ok(Unit::Plain) | Err(_) => return None,
            Ok(other) => other,
        });
    }
    row.get("currency")
        .and_then(Value::as_str)
        .is_some_and(|code| CURRENCY_CODES.contains(&code.trim().to_lowercase().as_str()))
        .then_some(Unit::Amount(scale))
}

/// A code written with digits: a leading zero (`005930`, `00126380`).
fn code_like(text: &str) -> bool {
    text.len() >= 2 && text.starts_with('0') && text.bytes().all(|b| b.is_ascii_digit())
}

/// The sign a triangle mark gives the number after it: `△`, `▽` and `▼` mark
/// a negative (Korean statements print a loss as `△1,234`), `▲` a rise.
fn triangle_sign(mark: char) -> Option<f64> {
    match mark {
        '△' | '▽' | '▼' => Some(-1.0),
        '▲' => Some(1.0),
        _ => None,
    }
}

/// A field whose whole value is one number (`"41,200"`, `"-3.1"`, the
/// accounting negatives `"(1,234)"` and `"△1,234"`); dates, identifiers with
/// inner hyphens and codes with a leading zero are not.
fn field_number(text: &str) -> Option<f64> {
    let text = text.trim();
    // A bracketed or marked number carries its sign in the mark, never a `-`.
    let (body, sign, marked) =
        if let Some(inner) = text.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
            (inner.trim(), -1.0, true)
        } else if let Some(sign) = text.chars().next().and_then(triangle_sign) {
            let mark = text.chars().next().map_or(0, char::len_utf8);
            (text[mark..].trim_start(), sign, true)
        } else {
            (text, 1.0, false)
        };
    let digits = if marked {
        body
    } else {
        body.strip_prefix('-').unwrap_or(body)
    };
    if digits.is_empty()
        || code_like(digits)
        || !digits.starts_with(|c: char| c.is_ascii_digit())
        || !digits
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ',' | '.'))
        || digits.matches('.').count() > 1
    {
        return None;
    }
    body.replace(',', "")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(|value| value * sign)
}

/// Fields that name, date, order or locate a row rather than measure it:
/// codes (`ISU_SRT_CD`, `mksc_shrn_iscd`, `pdno`), filing and receipt numbers
/// (`rcept_no`), dates and times (`BAS_DD`, `filed_at`, `stck_cntg_hour`),
/// ids, offsets and revisions (`dataset_id`, `match_offset`, `revision`).
/// Their values are never numbers to match.
fn identifier_key(key: &str) -> bool {
    const EXACT: &[&str] = &[
        "id",
        "ids",
        "code",
        "symbol",
        "symb",
        "ticker",
        "isin",
        "cusip",
        "cik",
        "figi",
        "lei",
        "pdno",
        "date",
        "year",
        "quarter",
        "period",
        "time",
        "timestamp",
        "revision",
        "offset",
        "ord",
        "seq",
        "page",
        "index",
        "rank",
        "version",
        "digest",
        "hash",
        "url",
        "uri",
        "status",
    ];
    const SUFFIXES: &[&str] = &[
        "_cd",
        "_no",
        "_dd",
        "_dt",
        "_at",
        "_id",
        "_ids",
        "_code",
        "iscd",
        "pdno",
        "_date",
        "_time",
        "_hour",
        "_tm",
        "ymd",
        "yymm",
        "_ym",
        "_year",
        "_yr",
        "_offset",
        "_seq",
        "_ord",
        "_rank",
        "_index",
        "_idx",
        "_page",
        "_version",
        "_ver",
        "_revision",
        "_digest",
        "_hash",
        "_ref",
        "_url",
        "_ms",
    ];
    // camelCase names (`createdAtMs`, `matchOffset`, `datasetId`).
    const CAMEL: &[&str] = &[
        "Id", "Ids", "At", "Ms", "Date", "Time", "Offset", "Code", "No", "Ref", "Url", "Digest",
        "Revision",
    ];
    let lower = key.to_ascii_lowercase();
    EXACT.contains(&lower.as_str())
        || lower.starts_with("isu_")
        || SUFFIXES.iter().any(|suffix| lower.ends_with(suffix))
        || CAMEL.iter().any(|suffix| {
            key.strip_suffix(suffix)
                .and_then(|head| head.chars().next_back())
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// A field's stated percent unit, from the providers' documented fields.
fn key_unit(key: &str) -> Option<Unit> {
    crate::finance_data::is_percent_field(key).then_some(Unit::Percent)
}

fn pointer_token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn collect(
    value: &Value,
    unit: Option<Unit>,
    pointer: &mut String,
    out: &mut Vec<Candidate>,
    text_budget: &mut usize,
) {
    if out.len() >= MAX_CANDIDATES {
        return;
    }
    match value {
        Value::Number(number) => {
            if let Some(value) = number.as_f64().filter(|value| value.is_finite()) {
                out.push(Candidate {
                    value,
                    unit: unit.unwrap_or(Unit::Raw),
                    place: Place::Pointer(pointer.clone()),
                });
            }
        }
        Value::String(text) => {
            let bytes = text.len().min(*text_budget);
            if bytes == 0 {
                return;
            }
            *text_budget -= bytes;
            let mut end = bytes;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let first = out.len();
            text_numbers(&text[..end], out);
            for candidate in &mut out[first..] {
                candidate.place = Place::Pointer(pointer.clone());
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let length = pointer.len();
                pointer.push('/');
                pointer.push_str(&index.to_string());
                collect(item, None, pointer, out, text_budget);
                pointer.truncate(length);
            }
        }
        Value::Object(map) => {
            let row = row_unit(map);
            for (key, item) in map {
                if matches!(key.as_str(), "unit" | "scale" | "currency") || identifier_key(key) {
                    continue;
                }
                let length = pointer.len();
                pointer.push('/');
                pointer.push_str(&pointer_token(key));
                collect_field(key, item, row, pointer, out, text_budget);
                pointer.truncate(length);
            }
        }
        _ => {}
    }
}

/// One field of a row (`pointer` already names it), in the unit its key or
/// row states.
fn collect_field(
    key: &str,
    item: &Value,
    row: Option<Unit>,
    pointer: &mut String,
    out: &mut Vec<Candidate>,
    text_budget: &mut usize,
) {
    // Providers often send row values as strings ("41,200").
    if let Some(value) = item.as_str().and_then(field_number) {
        if out.len() < MAX_CANDIDATES {
            out.push(Candidate {
                value,
                unit: key_unit(key).or(row).unwrap_or(Unit::Raw),
                place: Place::Pointer(pointer.clone()),
            });
        }
    } else {
        let unit = if item.is_number() {
            key_unit(key).or(row)
        } else {
            None
        };
        collect(item, unit, pointer, out, text_budget);
    }
}

/// Every number in a structured payload (bounded), each with its JSON Pointer.
/// Identifier, date and position fields are skipped.
pub fn data_candidates(payload: &Value) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut budget = MAX_TEXT_BYTES;
    collect(payload, None, &mut String::new(), &mut out, &mut budget);
    out
}

/// Fields that carry a result's content (a page's text, a passage, a row's
/// value) rather than describe it (status, sizes, offsets, retrieval times).
const CONTENT_KEYS: &[&str] = &[
    "content", "text", "passage", "passages", "body", "markdown", "excerpt", "value", "values",
];

fn collect_content(
    value: &Value,
    pointer: &mut String,
    out: &mut Vec<Candidate>,
    text_budget: &mut usize,
) {
    if out.len() >= MAX_CANDIDATES {
        return;
    }
    match value {
        Value::Object(map) => {
            let row = row_unit(map);
            for (key, item) in map {
                if identifier_key(key) {
                    continue;
                }
                let length = pointer.len();
                pointer.push('/');
                pointer.push_str(&pointer_token(key));
                if CONTENT_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
                    collect_field(key, item, row, pointer, out, text_budget);
                } else if item.is_object() || item.is_array() {
                    collect_content(item, pointer, out, text_budget);
                }
                pointer.truncate(length);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let length = pointer.len();
                pointer.push('/');
                pointer.push_str(&index.to_string());
                collect_content(item, pointer, out, text_budget);
                pointer.truncate(length);
            }
        }
        // A result that is only text is all content.
        Value::String(_) if pointer.is_empty() => {
            collect(value, None, pointer, out, text_budget);
        }
        _ => {}
    }
}

/// Numbers in a result's content fields only (bounded), never its metadata:
/// a page's `content`, a passage's `text`, a row's `value`.
pub fn content_candidates(payload: &Value) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut budget = MAX_TEXT_BYTES;
    collect_content(payload, &mut String::new(), &mut out, &mut budget);
    out
}

/// Every number printed in a passage, each with its byte range.
pub fn passage_candidates(text: &str) -> Vec<Candidate> {
    let mut end = text.len().min(MAX_TEXT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = Vec::new();
    text_numbers(&text[..end], &mut out);
    out
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// The evidence value (as printed) that equals `observed`, read in the line's
/// `unit`, or `None`. The match is exact: the evidence's printed precision.
pub fn find(observed: f64, unit: Unit, payload: &Value) -> Option<f64> {
    if !observed.is_finite() {
        return None;
    }
    let target = unit.normalized(observed);
    data_candidates(payload).into_iter().find_map(|candidate| {
        let value = Unit::compare(unit, candidate.unit, candidate.value)?;
        same(value, target).then_some(candidate.value)
    })
}

/// A number as an answer prints it: the value in its printed unit and half a
/// unit of its last printed digit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Printed {
    pub value: f64,
    pub unit: Unit,
    pub precision: f64,
}

/// Leading digits with `,` grouping and one decimal point: the value, the
/// number of decimals, whether it was grouped, and the bytes it takes. A run
/// that continues as a date or identifier (`2026-03-18`, `2026.03.18`) is not
/// a number.
fn leading_number(text: &str) -> Option<(f64, usize, bool, usize)> {
    let bytes = text.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut end = 0;
    let mut decimals: Option<usize> = None;
    let mut grouped = false;
    while end < bytes.len() {
        let byte = bytes[end];
        let next_digit = bytes.get(end + 1).is_some_and(u8::is_ascii_digit);
        if byte.is_ascii_digit() {
            if let Some(count) = &mut decimals {
                *count += 1;
            }
            end += 1;
        } else if byte == b',' && next_digit && decimals.is_none() {
            grouped = true;
            end += 1;
        } else if byte == b'.' && next_digit && decimals.is_none() {
            decimals = Some(0);
            end += 1;
        } else if matches!(byte, b'.' | b'-' | b'/' | b':') && next_digit {
            return None;
        } else {
            break;
        }
    }
    let value = text[..end].replace(',', "").parse::<f64>().ok()?;
    Some((value, decimals.unwrap_or(0), grouped, end))
}

fn strip_sign(text: &str) -> (&str, bool) {
    if let Some(rest) = text.strip_prefix(['-', '−', '–']) {
        (rest, true)
    } else {
        (text.strip_prefix('+').unwrap_or(text), false)
    }
}

/// A currency written before the number: a symbol (`$`, `₩`), a prefixed
/// dollar (`US$`, `HK$`) or an ISO code (`KRW 452.3bn`).
fn strip_currency(text: &str) -> Option<&str> {
    if let Some(rest) = text.strip_prefix(CURRENCY_SYMBOLS) {
        return Some(rest);
    }
    let letters = text.bytes().take_while(u8::is_ascii_alphabetic).count();
    if (1..=2).contains(&letters) && text.as_bytes().get(letters) == Some(&b'$') {
        return Some(&text[letters + 1..]);
    }
    if letters == 3 {
        let code = text[..3].to_ascii_lowercase();
        let after = &text[3..];
        if CURRENCY_CODES.contains(&code.as_str())
            && after.starts_with(|c: char| c == ' ' || c.is_ascii_digit() || c == '-')
        {
            return Some(after);
        }
    }
    None
}

/// What may follow an amount: nothing, a currency word or code, or "per share".
fn amount_tail(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    let text = text
        .strip_suffix("per share")
        .or_else(|| text.strip_suffix("a share"))
        .or_else(|| text.strip_suffix("/share"))
        .unwrap_or(&text)
        .trim();
    text.is_empty()
        || CURRENCY_CODES.contains(&text)
        || CURRENCY_WORDS.contains(&text)
        || KOREAN_CURRENCY.contains(&text)
}

/// Reads an answer's link text as one number with its unit (`8.4%`,
/// `₩452.3bn`, `4,523억 원`, `1조 2,300억원`, `-1.2pp`, `31.2x`, `$1.5 billion`,
/// `34,560,000`). Anything else is not a number: words, dates, tickers, a
/// year (`2026`) or a bare one-to-three-digit integer such as a numbered
/// citation (`1`).
pub fn parse_printed(text: &str) -> Option<Printed> {
    let normalized: String = text
        .chars()
        .map(|c| {
            if matches!(c, '\u{a0}' | '\u{202f}' | '\u{2009}') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let text = normalized.trim();
    if text.is_empty() || text.len() > 64 {
        return None;
    }
    // Accounting negatives: a grouped amount in brackets (`(1,234)`) or after
    // `△`; `▲` marks a rise.
    let (text, bracketed) = match text.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        Some(inner) => (inner.trim(), inner.contains(',')),
        None => (text, false),
    };
    let (text, marked) = match text.chars().next().and_then(triangle_sign) {
        Some(sign) => (
            text[text.chars().next().map_or(0, char::len_utf8)..].trim_start(),
            sign < 0.0,
        ),
        None => (text, false),
    };
    let (mut rest, mut negative) = strip_sign(text);
    negative |= bracketed || marked;
    let currency = match strip_currency(rest) {
        Some(after) => {
            let (after, sign) = strip_sign(after.trim_start());
            negative |= sign;
            rest = after.trim_start();
            true
        }
        None => false,
    };
    let (first, mut decimals, grouped, length) = leading_number(rest)?;
    rest = &rest[length..];
    // Korean scales add up in strictly decreasing order (`1조 2,300억원`).
    let mut total = 0.0;
    let mut scale: Option<f64> = None;
    let mut component = first;
    loop {
        let after = rest.trim_start();
        let Some((word, found)) = KOREAN_SCALES
            .iter()
            .find(|(word, _)| after.starts_with(word))
        else {
            break;
        };
        if scale.is_some_and(|previous| *found >= previous) {
            return None;
        }
        total += component * found;
        scale = Some(*found);
        rest = &after[word.len()..];
        let next = rest.trim_start();
        let Some((value, places, _, length)) = leading_number(next) else {
            break;
        };
        // The next component must carry its own, smaller scale.
        let tail = next[length..].trim_start();
        if !KOREAN_SCALES.iter().any(|(word, _)| tail.starts_with(word)) {
            return None;
        }
        component = value;
        decimals = places;
        rest = &next[length..];
    }
    let precision = 0.5 * 10f64.powi(-(decimals as i32));
    let sign = if negative { -1.0 } else { 1.0 };
    if let Some(scale) = scale {
        if !amount_tail(rest) {
            return None;
        }
        return Some(Printed {
            value: sign * total / scale,
            unit: Unit::Amount(scale),
            precision,
        });
    }
    let value = sign * first;
    let gap = rest.len() != rest.trim_start().len();
    let lower = rest.trim().to_ascii_lowercase();
    let unit = match lower.as_str() {
        "" if currency => Unit::Amount(1.0),
        "" => {
            // Years and small counts are labels or numbered citations.
            let digits = text.trim_start_matches(['-', '−', '–', '+']);
            if decimals == 0
                && !grouped
                && (digits.len() <= 3 || (digits.len() == 4 && (1900.0..=2100.0).contains(&first)))
            {
                return None;
            }
            Unit::Plain
        }
        "%" | "％" | "percent" | "pct" if !currency => Unit::Percent,
        "%p" | "%pt" | "%pts" | "pp" | "ppt" | "ppts" | "percentage point"
        | "percentage points" | "% point" | "% points" | "%포인트"
            if !currency =>
        {
            Unit::Points
        }
        "bp" | "bps" | "basis point" | "basis points" if !currency => Unit::BasisPoints,
        "x" if !gap && !currency => Unit::Multiple,
        "×" | "times" | "배" if !currency => Unit::Multiple,
        _ => {
            let (word, remainder) = lower.split_once(' ').unwrap_or((lower.as_str(), ""));
            let letter = (currency || !gap).then(|| letter_scale(word)).flatten();
            if let Some(found) = english_scale(word).or(letter) {
                if !amount_tail(remainder) {
                    return None;
                }
                Unit::Amount(found)
            } else if amount_tail(&lower) {
                Unit::Amount(1.0)
            } else {
                return None;
            }
        }
    };
    Some(Printed {
        value,
        unit,
        precision,
    })
}

/// How a printed number stands against its evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Within half a unit of the printed last digit, at the same scale.
    Matched,
    /// Within precision after a scale or percent↔fraction conversion.
    Rounded,
    /// A calculation produced no value within precision.
    Differs,
    NotFound,
}

/// A candidate's value read in the printed unit, and whether reading it took a
/// scale or percent↔fraction conversion. A calculation's plain value may be
/// in the printed scale or in base units, so both are tried.
fn readings(printed: Unit, candidate: Unit, value: f64, calculated: bool) -> Vec<(f64, bool)> {
    use Unit::*;
    match (printed, candidate) {
        (Percent, Percent) | (Points, Points) | (BasisPoints, BasisPoints) => vec![(value, false)],
        (Percent, Plain | Raw) => vec![(value * 100.0, true)],
        (Points, BasisPoints) => vec![(value / 100.0, true)],
        (BasisPoints, Points) => vec![(value * 100.0, true)],
        (Multiple | Plain, Multiple | Plain | Raw) => vec![(value, false)],
        (Amount(a), Amount(b)) if same(a, b) => vec![(value, false)],
        (Amount(a), Amount(b)) => vec![(value * b / a, true)],
        (Amount(a), Raw) if same(a, 1.0) => vec![(value, false)],
        (Amount(a), Raw) => vec![(value / a, true)],
        (Amount(a), Plain) if calculated => vec![(value, false), (value / a, true)],
        _ => Vec::new(),
    }
}

/// Checks a printed number against evidence candidates. Returns the verdict
/// and the index of the candidate it rests on: the match, or for `Differs` the
/// nearest compatible value. `calculated` candidates come from a `calculate`
/// result (values first, then inputs).
pub fn check(
    printed: &Printed,
    candidates: &[Candidate],
    calculated: bool,
) -> (Verdict, Option<usize>) {
    let mut rounded = None;
    let mut nearest: Option<(f64, usize)> = None;
    for (index, candidate) in candidates.iter().enumerate() {
        for (value, converted) in
            readings(printed.unit, candidate.unit, candidate.value, calculated)
        {
            if !value.is_finite() {
                continue;
            }
            let gap = (value - printed.value).abs();
            let slack = 1e-9 * value.abs().max(printed.value.abs()).max(1.0);
            if gap <= printed.precision + slack {
                if !converted {
                    return (Verdict::Matched, Some(index));
                }
                rounded.get_or_insert(index);
            }
            let relative = gap / printed.value.abs().max(1e-12);
            if nearest.is_none_or(|(best, _)| relative < best) {
                nearest = Some((relative, index));
            }
        }
    }
    if let Some(index) = rounded {
        return (Verdict::Rounded, Some(index));
    }
    if calculated {
        let fallback = (!candidates.is_empty()).then_some(0);
        return (
            Verdict::Differs,
            nearest.map(|(_, index)| index).or(fallback),
        );
    }
    (Verdict::NotFound, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn percent(observed: f64, payload: &Value) -> Option<f64> {
        find(observed, Unit::Percent, payload)
    }

    #[test]
    fn dates_identifiers_and_hyphens_are_never_numbers_or_signs() {
        let filing = json!({"filings": [{"filed_at": "2026-03-18", "rcept_no": "20260318000123",
            "title": "Form 10-K for FY2026 Q3, filed 2026.03.18 at 09:30 (ref ev_31-18)"}]});
        for observed in [-3.0, 3.0, -18.0, 18.0, 10.0, 2026.0, 31.0, 9.0, 30.0] {
            assert_eq!(find(observed, Unit::Plain, &filing), None, "{observed}");
        }
        // A real sign after a space or bracket still counts.
        assert_eq!(
            percent(-3.1, &json!("shares fell -3.1% (−2.0%) on the day")),
            Some(-3.1)
        );
        assert_eq!(percent(-2.0, &json!("(−2.0%)")), Some(-2.0));
        assert_eq!(percent(3.1, &json!("shares fell -3.1%")), None);
    }

    #[test]
    fn units_are_normalized_to_the_line_and_never_guessed() {
        let text = json!(
            "Largest customer 27.4% of revenue; margin fell 1.2%p; spread 35bp; \
            revenue ₩4,523억원, cash $1.5 billion; P/E 31.2x; ratio 0.274"
        );
        assert_eq!(percent(27.4, &text), Some(27.4));
        // A fraction is not a percent, and no ×/÷100 match is made.
        assert_eq!(percent(0.274, &text), None);
        assert_eq!(find(27.4, Unit::Plain, &text), None);
        assert_eq!(find(1.2, Unit::Points, &text), Some(1.2));
        assert_eq!(find(120.0, Unit::BasisPoints, &text), Some(1.2));
        assert_eq!(find(0.35, Unit::Points, &text), Some(35.0));
        let eok = Unit::of_line(Some("억원")).unwrap();
        assert_eq!(find(4523.0, eok, &text), Some(4523.0));
        let trillions = Unit::of_line(Some("조원")).unwrap();
        assert_eq!(find(0.4523, trillions, &text), Some(4523.0));
        assert_eq!(find(4523.0, Unit::Amount(1.0), &text), None);
        let usd_m = Unit::of_line(Some("USD m")).unwrap();
        assert_eq!(find(1500.0, usd_m, &text), Some(1.5));
        assert_eq!(
            find(31.2, Unit::of_line(Some("x")).unwrap(), &text),
            Some(31.2)
        );
        // A structured row's own unit and scale decide.
        let rows = json!({"facts": [
            {"canonical_concept": "revenue", "value": 452_300_000_000_i64, "unit": "KRW", "scale": "1"},
            {"label": "customer share", "value": 0.274},
            {"share_pct": 27.4}
        ]});
        assert_eq!(find(4523.0, eok, &rows), Some(452_300_000_000.0));
        assert_eq!(percent(27.4, &rows), Some(27.4));
        assert_eq!(percent(0.274, &rows), None);
        assert_eq!(find(0.274, Unit::Plain, &rows), Some(0.274));
        let won = Unit::of_line(Some("원")).unwrap();
        let bars = json!({"bars": [{"date": "2026-10-28", "close": "41,200"}]});
        assert_eq!(find(41_200.0, won, &bars), Some(41_200.0));
        assert_eq!(find(28.0, Unit::Plain, &bars), None);
        let korean = json!("3분기 매출은 4,523억원으로 늘었고 영업이익률은 14.8%였다.");
        assert_eq!(find(4523.0, eok, &korean), Some(4523.0));
        assert_eq!(percent(14.8, &korean), Some(14.8));
        assert_eq!(Unit::of_line(Some("% of revenue")).unwrap(), Unit::Percent);
    }

    #[test]
    fn matches_are_exact_at_the_printed_precision() {
        let text = json!("Operating margin was 14.8% on revenue of ₩1,234,500 million.");
        assert_eq!(percent(14.8, &text), Some(14.8));
        assert_eq!(percent(14.80, &text), Some(14.8));
        assert_eq!(percent(14.83, &text), None);
        assert_eq!(percent(15.0, &text), None);
        assert_eq!(
            find(
                1_234_500.0,
                Unit::of_line(Some("KRW million")).unwrap(),
                &text
            ),
            Some(1_234_500.0)
        );
        assert_eq!(find(1_234_500.0, Unit::Plain, &text), None);
    }

    #[test]
    fn ambiguous_line_units_are_refused() {
        for unit in ["m", "b", "% of bn", "percent of KRW"] {
            assert!(Unit::of_line(Some(unit)).is_err(), "{unit}");
        }
        assert_eq!(Unit::of_line(Some("stores")), Ok(Unit::Plain));
        assert_eq!(Unit::of_line(None), Ok(Unit::Plain));
        assert_eq!(Unit::of_line(Some("$m")), Ok(Unit::Amount(1e6)));
        assert_eq!(Unit::of_line(Some("KRW bn")), Ok(Unit::Amount(1e9)));
        assert_eq!(Unit::of_line(Some("백만원")), Ok(Unit::Amount(1e6)));
        assert_eq!(Unit::of_line(Some("%")), Ok(Unit::Percent));
    }

    fn printed(text: &str) -> Printed {
        parse_printed(text).unwrap_or_else(|| panic!("{text} should parse"))
    }

    fn verdict(text: &str, candidates: &[Candidate], calculated: bool) -> Verdict {
        check(&printed(text), candidates, calculated).0
    }

    #[test]
    fn link_text_parses_as_one_number_with_its_unit() {
        let cases: &[(&str, f64, Unit, f64)] = &[
            ("8.4%", 8.4, Unit::Percent, 0.05),
            ("₩452.3bn", 452.3, Unit::Amount(1e9), 0.05),
            ("KRW 452.3 billion", 452.3, Unit::Amount(1e9), 0.05),
            ("$1.5 billion", 1.5, Unit::Amount(1e9), 0.05),
            ("US$12.30", 12.3, Unit::Amount(1.0), 0.005),
            ("-$3.1bn", -3.1, Unit::Amount(1e9), 0.05),
            ("−1.2pp", -1.2, Unit::Points, 0.05),
            ("1.2%p", 1.2, Unit::Points, 0.05),
            ("35bp", 35.0, Unit::BasisPoints, 0.5),
            ("31.2x", 31.2, Unit::Multiple, 0.05),
            ("1.8배", 1.8, Unit::Multiple, 0.05),
            ("34,560,000", 34_560_000.0, Unit::Plain, 0.5),
            ("0.274", 0.274, Unit::Plain, 0.0005),
            ("4,523억 원", 4523.0, Unit::Amount(1e8), 0.5),
            ("381억원", 381.0, Unit::Amount(1e8), 0.5),
            ("41,200원", 41_200.0, Unit::Amount(1.0), 0.5),
            ("₩1,250 per share", 1250.0, Unit::Amount(1.0), 0.5),
            ("$4.5m", 4.5, Unit::Amount(1e6), 0.05),
        ];
        for (text, value, unit, precision) in cases {
            let found = printed(text);
            assert!(same(found.value, *value), "{text}: {}", found.value);
            assert_eq!(found.unit, *unit, "{text}");
            assert!(
                same(found.precision, *precision),
                "{text}: {}",
                found.precision
            );
        }
        // Korean compounds read as one amount in the smallest scale.
        let compound = printed("1조 2,300억원");
        assert_eq!(compound.unit, Unit::Amount(1e8));
        assert!(same(compound.value, 12_300.0));
        assert!(same(compound.precision, 0.5));
        // Years, dates, numbered citations, identifiers and words are not numbers.
        for text in [
            "1",
            "12",
            "2026",
            "FY2025",
            "Q3",
            "2026-03-18",
            "2026.03.18",
            "10-K",
            "three",
            "8.4% margin",
            "1조 2300",
            "",
            "4.5 m",
        ] {
            assert_eq!(parse_printed(text), None, "{text}");
        }
    }

    #[test]
    fn checks_compare_within_half_the_printed_digit() {
        let passage = passage_candidates(
            "Operating margin was 8.42% on revenue of ₩452.3bn; the year before, 9.1%.",
        );
        assert_eq!(verdict("8.4%", &passage, false), Verdict::Matched);
        assert_eq!(verdict("8.42%", &passage, false), Verdict::Matched);
        assert_eq!(verdict("₩452.3bn", &passage, false), Verdict::Matched);
        assert_eq!(verdict("₩452bn", &passage, false), Verdict::Matched);
        assert_eq!(verdict("8.5%", &passage, false), Verdict::NotFound);
        // A passage never matches on a guessed scale or a bare number.
        assert_eq!(verdict("452.3", &passage, false), Verdict::NotFound);
        // The rounding edge is half a unit of the last printed digit.
        let edge = passage_candidates("margin 8.45%");
        assert_eq!(verdict("8.4%", &edge, false), Verdict::Matched);
        let past = passage_candidates("margin 8.4501%");
        assert_eq!(verdict("8.4%", &past, false), Verdict::NotFound);
        // The span marks the printed number with its unit.
        let text = "Revenue was ₩452.3bn.";
        let (found, index) = check(&printed("₩452.3bn"), &passage_candidates(text), false);
        assert_eq!(found, Verdict::Matched);
        let Place::Span { start, end } = passage_candidates(text)[index.unwrap()].place.clone()
        else {
            panic!("passage candidates carry spans");
        };
        assert_eq!(&text[start..end], "₩452.3bn");
    }

    #[test]
    fn data_rows_match_after_scale_with_their_pointer() {
        let rows = json!({"facts": [
            {"canonical_concept": "operating_income", "value": "38100000000", "unit": "KRW", "scale": "1"},
            {"canonical_concept": "revenue", "value": 452_312_000_000_i64, "unit": "KRW", "scale": "1"},
        ]});
        let candidates = data_candidates(&rows);
        let (found, index) = check(&printed("₩452.3bn"), &candidates, false);
        assert_eq!(found, Verdict::Rounded);
        assert_eq!(
            candidates[index.unwrap()].place,
            Place::Pointer("/facts/1/value".into())
        );
        assert_eq!(verdict("4,523억 원", &candidates, false), Verdict::Rounded);
        assert_eq!(verdict("381억 원", &candidates, false), Verdict::Rounded);
        assert_eq!(verdict("4,600억 원", &candidates, false), Verdict::NotFound);
        // A Korean passage compound and grouping.
        let korean = passage_candidates("연결 매출액은 1조 2,345억원, 영업이익은 381억원이었다.");
        assert_eq!(verdict("1조 2,345억 원", &korean, false), Verdict::Matched);
        assert_eq!(verdict("381억원", &korean, false), Verdict::Matched);
        assert_eq!(verdict("₩1.2345tn", &korean, false), Verdict::Rounded);
    }

    #[test]
    fn calculations_report_rounding_and_the_nearest_value_when_they_differ() {
        let calc = vec![
            Candidate {
                value: 8.417_329,
                unit: Unit::Percent,
                place: Place::Pointer("/values/0".into()),
            },
            Candidate {
                value: 38_100_000_000.0,
                unit: Unit::Plain,
                place: Place::Pointer("/inputs/0".into()),
            },
            Candidate {
                value: 0.084_173,
                unit: Unit::Plain,
                place: Place::Pointer("/inputs/9".into()),
            },
        ];
        assert_eq!(
            check(&printed("8.4%"), &calc, true),
            (Verdict::Matched, Some(0))
        );
        assert_eq!(
            check(&printed("8.42%"), &calc, true),
            (Verdict::Matched, Some(0))
        );
        // A plain input in base units reads in the printed scale.
        assert_eq!(
            check(&printed("₩38.1bn"), &calc, true),
            (Verdict::Rounded, Some(1))
        );
        // A fraction becomes a percent only as a rounded reading.
        let fraction = [calc[2].clone()];
        assert_eq!(check(&printed("8.4%"), &fraction, true).0, Verdict::Rounded);
        // Nothing within precision: the calculation differs and names its nearest value.
        assert_eq!(
            check(&printed("9.1%"), &calc, true),
            (Verdict::Differs, Some(0))
        );
        assert_eq!(
            check(&printed("9.1%"), &calc[..1], false).0,
            Verdict::NotFound
        );
        // Negative numbers keep their sign.
        let change = passage_candidates("shares fell -3.1% on the day");
        assert_eq!(verdict("-3.1%", &change, false), Verdict::Matched);
        assert_eq!(verdict("3.1%", &change, false), Verdict::NotFound);
    }
    #[test]
    fn identifiers_dates_offsets_and_codes_are_never_candidates() {
        let rows = json!({
            "dataset_ref": {"dataset_id": "ds_1", "revision": 3},
            "match_offset": 1234,
            "revision": 7,
            "OutBlock_1": [{"BAS_DD": "20260318", "ISU_CD": "KR7990221002", "ISU_SRT_CD": "990221",
                "TDD_CLSPRC": "41,200", "FLUC_RT": "-1.20"}],
            "output": {"mksc_shrn_iscd": "990221", "stck_cntg_hour": "153000", "stck_prpr": "41200",
                "pdno": "005930"},
            "facts": [{"rcept_no": "20260318000123", "corp_code": "00990221", "account_id": "ifrs_Revenue",
                "createdAtMs": 1_773_000_000_000_i64, "ord": 12, "thstrm_amount": "452312000000"}],
            "code": "005930"
        });
        let values: Vec<f64> = data_candidates(&rows)
            .into_iter()
            .map(|candidate| candidate.value)
            .collect();
        for identifier in [
            3.0,
            1234.0,
            7.0,
            20_260_318.0,
            990_221.0,
            153_000.0,
            5930.0,
            20_260_318_000_123.0,
            990_221.0,
            1_773_000_000_000.0,
            12.0,
        ] {
            assert!(!values.contains(&identifier), "{identifier} in {values:?}");
        }
        assert!(values.contains(&41_200.0));
        assert!(values.contains(&452_312_000_000.0));
        // A value field that is a code is not a number either.
        assert_eq!(field_number("005930"), None);
        // In text, a bracketed or suffixed six-digit ticker is a code.
        let text = passage_candidates("Solsup Foods (990221) and 990221.KS closed at ₩41,200");
        let values: Vec<f64> = text.iter().map(|candidate| candidate.value).collect();
        assert_eq!(values, [41_200.0]);
    }

    #[test]
    fn accounting_negatives_read_with_their_sign() {
        assert_eq!(field_number("(1,234)"), Some(-1234.0));
        assert_eq!(field_number("△1,234"), Some(-1234.0));
        assert_eq!(field_number("▲1,234"), Some(1234.0));
        assert_eq!(field_number("(-1,234)"), None);
        let text = "영업이익 △1,234억원, 순이익 (5,678)백만원 vs (12.5%) and (2026)";
        let found = passage_candidates(text);
        let read: Vec<(f64, &str)> = found
            .iter()
            .map(|candidate| {
                let Place::Span { start, end } = candidate.place else {
                    unreachable!()
                };
                (candidate.value, &text[start..end])
            })
            .collect();
        assert_eq!(
            read,
            [
                (-1234.0, "△1,234억원"),
                (-5678.0, "(5,678)"),
                (12.5, "12.5%"),
                (2026.0, "2026"),
            ]
        );
        // A bracketed share or an ungrouped year is not an accounting negative.
        assert_eq!(verdict("-1,234억원", &found, false), Verdict::Matched);
        assert_eq!(
            verdict("(1,234)", &passage_candidates("loss of (1,234)"), false),
            Verdict::Matched
        );
        assert_eq!(
            verdict("△1,234", &passage_candidates("(1,234)"), false),
            Verdict::Matched
        );
        assert_eq!(
            verdict("1,234", &passage_candidates("▲1,234"), false),
            Verdict::Matched
        );
        let rows = json!({"facts": [{"thstrm_amount": "(5,678)"}]});
        assert_eq!(find(-5678.0, Unit::Plain, &rows), Some(-5678.0));
    }

    #[test]
    fn content_candidates_skip_a_results_metadata() {
        let page = json!({"status": 200, "content_bytes": 45231, "page_count": 12,
            "retrieved_at": "2026-03-18T00:00:00Z", "content": "Revenue rose to 4,523 units.",
            "rows": [{"value": "17", "page": 9}]});
        let values: Vec<f64> = content_candidates(&page)
            .into_iter()
            .map(|candidate| candidate.value)
            .collect();
        assert_eq!(values, [4523.0, 17.0]);
        assert!(data_candidates(&page)
            .iter()
            .any(|candidate| candidate.value == 45231.0));
    }

    #[test]
    fn provider_percent_fields_are_percents_not_fractions() {
        let krx = json!({"rows": [{"FLUC_RT": "-1.20", "TDD_CLSPRC": "41,200"}]});
        let found = data_candidates(&krx);
        assert_eq!(verdict("-1.20%", &found, false), Verdict::Matched);
        // Never read as a fraction ×100.
        assert_eq!(verdict("-120%", &found, false), Verdict::NotFound);
        let kis = json!({"output": {"prdy_ctrt": "5.20", "evlu_pfls_rt": "12.3"}});
        assert_eq!(
            verdict("5.2%", &data_candidates(&kis), false),
            Verdict::Matched
        );
        assert_eq!(find(12.3, Unit::Percent, &kis), Some(12.3));
    }
}
