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
//!
//! A bare number in text also takes a unit stated apart from it, within its
//! row or table or in words after it (`배당금총액(원)`, `(단위: 백만원)`,
//! `184.6이고 단위는 십억 원`). A word of decrease right after an unsigned
//! number (`9% 줄었고`) states a fall that a figure's own sign must agree with.
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

use crate::finance_data::fx::{is_currency_code, is_currency_marker, CURRENCY_CODES};
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
    let mut found = currency_markers(unit, false);
    (found.len() == 1).then(|| found.remove(0))
}

/// Currency markers in a printed figure that identify a single currency each.
/// Bare dollar and yen signs, and dollar words, are shared by several currencies.
pub(crate) fn unambiguous_currency_markers(text: &str) -> Vec<String> {
    currency_markers(text, true)
}

fn currency_markers(unit: &str, unambiguous: bool) -> Vec<String> {
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
        if (!unambiguous || !matches!(symbol, '$' | '¥')) && rest.contains(*symbol) {
            add((*code).to_owned());
        }
    }
    // A printed figure can start with a code directly against the digits.
    let separator = |c: char| {
        if unambiguous {
            !c.is_alphabetic()
        } else {
            !c.is_alphanumeric()
        }
    };
    for word in unit.split(separator) {
        if is_currency_marker(word) {
            add(word.to_ascii_uppercase());
        }
    }
    for word in rest.split(separator).filter(|word| !word.is_empty()) {
        let marker = WORDS
            .iter()
            .find(|(name, _)| *name == word)
            .or_else(|| KOREAN.iter().find(|(suffix, _)| word.ends_with(suffix)));
        if let Some((_, code)) = marker.filter(|(_, code)| !unambiguous || *code != "USD") {
            add((*code).to_owned());
        }
    }
    found
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

/// The multiplier a unit's number states (`100` in "KRW 100 million"): a
/// whole power of ten, or `None` for any other number.
fn power_of_ten(digits: &str) -> Option<f64> {
    let zeros = digits.strip_prefix('1')?;
    (zeros.len() <= 18 && zeros.bytes().all(|b| b == b'0')).then(|| 10f64.powi(zeros.len() as i32))
}

/// `text` without the `,` that groups digits (`1,000` → `1000`).
fn ungrouped(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .filter(|(index, c)| {
            **c != ','
                || !(index
                    .checked_sub(1)
                    .is_some_and(|i| chars[i].is_ascii_digit())
                    && chars.get(index + 1).is_some_and(char::is_ascii_digit))
        })
        .map(|(_, c)| *c)
        .collect()
}

impl Unit {
    /// The unit of a user's line (`check.unit`). Unknown words (`stores`,
    /// `days`) are counts; a lone scale letter or a percent mixed with an
    /// amount is ambiguous and refused. A number in an amount's unit
    /// multiplies its scale ("KRW 100 million" = 1e8, `10억원` = 1e9); any
    /// number but one whole power of ten for an amount is ambiguous. What
    /// follows `per` or `/` is a denominator and never scales the amount.
    pub fn of_line(unit: Option<&str>) -> Result<Self, AmbiguousUnit> {
        let Some(raw) = unit.map(str::trim).filter(|unit| !unit.is_empty()) else {
            return Ok(Self::Plain);
        };
        let text = ungrouped(&raw.to_lowercase());
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
        let mut currency = text.chars().any(|c| CURRENCY_SYMBOLS.contains(&c))
            || raw
                .split(|c: char| !c.is_alphanumeric())
                .any(is_currency_marker);
        let mut scale: Option<f64> = None;
        let mut letters: Option<f64> = None;
        let mut multiplier: Option<f64> = None;
        let mut percent = text.contains('%');
        let (amount, denominator) = text.split_once('/').unwrap_or((&text, ""));
        let mut per = false;
        let amount_words = amount
            .split(|c: char| !c.is_alphanumeric())
            .map(|word| (word, false));
        let denominator_words = denominator
            .split(|c: char| !c.is_alphanumeric())
            .map(|word| (word, true));
        for (word, after_slash) in amount_words
            .chain(denominator_words)
            .filter(|(word, _)| !word.is_empty())
        {
            per |= after_slash || word == "per";
            let digits = word.bytes().take_while(u8::is_ascii_digit).count();
            let (number, word) = word.split_at(digits);
            // "per 1,000 shares" names a denominator, not a multiplier.
            if !number.is_empty() && !per {
                let found = power_of_ten(number).ok_or_else(ambiguous)?;
                if multiplier.replace(found).is_some() {
                    return Err(ambiguous());
                }
            }
            if word.is_empty() {
                continue;
            }
            if CURRENCY_WORDS.contains(&word) {
                currency = true;
            } else if per {
                continue;
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
        // A multiplier scales an amount; with a share or a count it cannot be placed.
        if let Some(factor) = multiplier {
            if percent || !(currency || scale.is_some()) {
                return Err(ambiguous());
            }
            scale = Some(scale.unwrap_or(1.0) * factor);
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
    /// A word of decrease right after the unsigned number and its unit
    /// states a fall (`9% 줄었고`, `14억원 감소`, `9% lower`): a figure with
    /// its own sign reads the number as negative.
    pub stated_fall: bool,
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
    // A unit is a few short words: reading further would rescan the text
    // after every number.
    let rest = &rest[..rest.char_indices().nth(48).map_or(rest.len(), |(at, _)| at)];
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
    if rest
        .get(gap..gap + word.len())
        .is_some_and(is_currency_marker)
        || CURRENCY_WORDS.contains(&word.as_str())
    {
        return (Unit::Amount(1.0), gap + word.len(), None);
    }
    (Unit::Plain, 0, None)
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Korean words of decrease, as the start of the word after a number
/// (`줄었고`, `감소한`, `하락했다`, `떨어졌다`, `낮아졌다`).
const KOREAN_FALLS: &[&str] = &[
    "감소", "줄었", "줄어", "줄고", "줄인", "줄여", "하락", "떨어", "낮아", "낮은", "축소", "급감",
    "급락",
];
/// English words of decrease after a number (`9% lower`, `a 9% decline`).
const ENGLISH_FALLS: &[&str] = &[
    "lower",
    "less",
    "below",
    "decline",
    "declines",
    "declined",
    "decrease",
    "decreases",
    "decreased",
    "drop",
    "drops",
    "dropped",
    "fall",
    "falls",
];

/// Whether the text right after a number and its unit states a fall: a word
/// of decrease directly after it, or after at most two spaces. A particle or
/// any other word between (`248달러보다 낮아졌다`, `9% 이상 줄었다`) states
/// nothing, and words of increase need no reading: an unsigned number is
/// already a rise.
fn falls_after(rest: &str) -> bool {
    let word = rest.trim_start_matches([' ', '\u{a0}']);
    if rest[..rest.len() - word.len()].chars().count() > 2 {
        return false;
    }
    // Every stem is two syllables; every English word is shorter than 16 letters.
    let korean: String = word.chars().take_while(|c| is_hangul(*c)).take(2).collect();
    if !korean.is_empty() {
        return KOREAN_FALLS.iter().any(|stem| korean.starts_with(stem));
    }
    let english: String = word
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .take(16)
        .collect::<String>()
        .to_ascii_lowercase();
    ENGLISH_FALLS.contains(&english.as_str())
}

/// A number found in text before Korean compounds are joined.
struct Scanned {
    currency_prefix_only: bool,
    value: f64,
    unit: Unit,
    start: usize,
    end: usize,
    /// The Korean scale when it stood alone (`1조`), so a smaller one may follow.
    bare_scale: Option<f64>,
    /// A sign, triangle mark or accounting brackets came with the number.
    signed: bool,
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
        let prefix_text = &text[..byte(start)];
        let before_number = prefix_text.trim_end_matches(' ');
        let gap = prefix_text.len() - before_number.len();
        let boundary = |text: &str| {
            text.chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        };
        let prefixed = symbol
            || (gap <= 2
                && CURRENCY_CODES.iter().any(|code| {
                    let Some(at) = before_number.len().checked_sub(code.len()) else {
                        return false;
                    };
                    before_number.get(at..).is_some_and(|tail| {
                        tail.eq_ignore_ascii_case(code) && is_currency_marker(tail)
                    }) && (boundary(&before_number[..at])
                        || at.checked_sub(3).is_some_and(|pair_at| {
                            before_number.get(pair_at..at).is_some_and(|first| {
                                CURRENCY_CODES.iter().any(|code| {
                                    first.eq_ignore_ascii_case(code) && is_currency_marker(first)
                                })
                            }) && boundary(&before_number[..pair_at])
                        }))
                }));
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
            signed = true;
        }
        scanned.push(Scanned {
            currency_prefix_only: prefixed && length == 0,
            value,
            unit,
            start: span_start,
            end: span_end,
            bare_scale,
            signed,
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
    let stated = stated_units(text, &joined);
    for (found, stated) in joined.into_iter().zip(stated) {
        let place = Place::Span {
            start: found.start,
            end: found.end,
        };
        let stated_fall = !found.signed && found.value > 0.0 && falls_after(&text[found.end..]);
        // A number printed bare keeps its plain reading beside the stated unit.
        let original = (!found.currency_prefix_only || stated.is_none()).then_some(found.unit);
        for unit in original.into_iter().chain(stated) {
            if out.len() < MAX_CANDIDATES {
                out.push(Candidate {
                    value: found.value,
                    unit,
                    place: place.clone(),
                    stated_fall,
                });
            }
        }
    }
}

/// How far a unit stated ahead of its numbers reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reach {
    /// A row label's unit (`배당금총액(원)`): the first run of numbers after
    /// it, before a line break, a sentence end or another label.
    Row,
    /// A table's unit (`(단위: 백만원)`): until a blank line, a sentence end
    /// or the next table's unit.
    Table,
    /// A filing states a scale with explicit exceptions. Read only clearly
    /// labelled monetary rows, leaving per-share and percentage cells alone.
    Filing,
}

/// A unit stated ahead of the numbers it applies to.
struct Heading {
    start: usize,
    end: usize,
    /// `None` for a table whose unit cannot be placed (`(단위: 원, %)`): it
    /// ends the table before it and states nothing.
    unit: Option<Unit>,
    reach: Reach,
}

fn is_hangul(c: char) -> bool {
    ('\u{ac00}'..='\u{d7a3}').contains(&c)
}

/// The rest of a table's unit statement after its marker (`단위: 백만원`,
/// `단위 : 원`, `Unit: KRW million`).
fn table_marker(text: &str) -> Option<&str> {
    // ASCII lowercase keeps byte offsets.
    let lower = text.to_ascii_lowercase();
    ["단위", "units", "unit"].iter().find_map(|marker| {
        let rest = lower.strip_prefix(marker)?.trim_start();
        let after = rest.strip_prefix([':', '：'])?;
        Some(text[text.len() - after.len()..].trim_start())
    })
}

/// The unit a bracket states (`(원)`, `(%)`, `(백만원)`, `(단위: 백만원)`,
/// `(in KRW millions)`) and how far it reaches. A row label names its unit
/// in words: a number, a lone letter (`(x)`, `(a)`) or a list (`(원, %)`)
/// is not one. A table's list of units or other words leaves it unplaced.
fn bracket_unit(content: &str) -> Option<(Option<Unit>, Reach)> {
    let content = content.trim();
    let placed = |text: &str| {
        let unit = (!ungrouped(text).contains([',', '、', '·']))
            .then(|| Unit::of_line(Some(text)).ok())
            .flatten();
        unit.filter(|unit| *unit != Unit::Plain)
    };
    let normalized = content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let declaration = normalized.strip_prefix("in ").unwrap_or(&normalized);
    if let Some((scale, exceptions)) = declaration.split_once(", except ") {
        if matches!(
            exceptions,
            "percentages"
                | "per share data"
                | "per share amounts"
                | "percentages and per share data"
                | "percentages and per share amounts"
        ) {
            return Some((english_scale(scale).map(Unit::Amount), Reach::Filing));
        }
        return Some((None, Reach::Table));
    }
    let table = table_marker(content).or_else(|| {
        let (marker, rest) = content.split_once(char::is_whitespace)?;
        marker
            .eq_ignore_ascii_case("in")
            .then_some(rest.trim_start())
    });
    if let Some(rest) = table {
        return Some((placed(rest), Reach::Table));
    }
    let letter = content.len() == 1 && content.bytes().all(|b| b.is_ascii_alphabetic());
    if content.chars().count() > 16 || content.contains(|c: char| c.is_ascii_digit()) || letter {
        return None;
    }
    placed(content).map(|unit| (Some(unit), Reach::Row))
}

/// The bytes of a Korean scale and currency at the start of `word`
/// (`십억원이다` → `십억원`), and whether a particle follows them.
fn korean_unit(word: &str) -> Option<(usize, bool)> {
    let scale = KOREAN_SCALES
        .iter()
        .find(|(scale, _)| word.starts_with(scale))
        .map_or(0, |(scale, _)| scale.len());
    let length = scale
        + KOREAN_CURRENCY
            .iter()
            .find(|currency| word[scale..].starts_with(*currency))
            .map_or(0, |currency| currency.len());
    let particle = &word[length..];
    (length > 0 && particle.chars().all(is_hangul)).then_some((length, !particle.is_empty()))
}

/// The bytes of `word` that are a unit word (`million`, `KRW`, `%`, `십억`,
/// `1,000원`, `원` in `원이라고`), and whether the unit words end after it (a
/// particle or punctuation follows).
fn unit_word(word: &str) -> Option<(usize, bool)> {
    let body = word.trim_end_matches(|c: char| {
        !c.is_alphanumeric() && !matches!(c, '%' | '％') && !CURRENCY_SYMBOLS.contains(&c)
    });
    let ends = body.len() < word.len();
    // A multiplier, alone or glued to a Korean unit (`1,000원`).
    let digits = body
        .bytes()
        .take_while(|b| b.is_ascii_digit() || *b == b',')
        .count();
    if digits > 0 {
        power_of_ten(&ungrouped(&body[..digits]))?;
        if digits == body.len() {
            return Some((digits, ends));
        }
        let (length, particle) = korean_unit(&body[digits..])?;
        return Some((digits + length, ends || particle));
    }
    let core = body.trim_end_matches(is_hangul);
    if core.is_empty() {
        return korean_unit(body).map(|(length, particle)| (length, ends || particle));
    }
    let lower = core.to_lowercase();
    let unit = english_scale(&lower).is_some()
        || CURRENCY_WORDS.contains(&lower.as_str())
        || is_currency_marker(core)
        || core.chars().all(|c| CURRENCY_SYMBOLS.contains(&c))
        || matches!(
            lower.as_str(),
            "%" | "％" | "percent" | "pct" | "%p" | "pp" | "bp" | "bps" | "of"
        );
    unit.then_some((core.len(), ends || core.len() < body.len()))
}

/// A unit written in words at the start of `text` (`십억 원이라고 알렸다`,
/// `백만원 과 목`): the unit, the bytes through its last unit word, and
/// whether a comma follows it, as in a list of units (`원, 주, %`).
fn unit_words(text: &str) -> Option<(Unit, usize, bool)> {
    // A unit is a few short words; never read further.
    let text = &text[..text.char_indices().nth(40).map_or(text.len(), |(at, _)| at)];
    let mut phrase = String::new();
    let (mut offset, mut consumed, mut list) = (0, 0, false);
    for _ in 0..5 {
        let rest = &text[offset..];
        let start = offset + rest.len() - rest.trim_start().len();
        let word = text[start..]
            .split(char::is_whitespace)
            .next()
            .unwrap_or("");
        let Some((length, ends)) = unit_word(word).filter(|_| !word.is_empty()) else {
            break;
        };
        phrase.push(' ');
        phrase.push_str(&word[..length]);
        consumed = start + length;
        offset = start + word.len();
        if ends {
            list = word[length..].starts_with([',', '、']);
            break;
        }
    }
    match Unit::of_line(Some(&phrase)) {
        Ok(Unit::Plain) | Err(_) => None,
        Ok(unit) => Some((unit, consumed, list)),
    }
}

/// Whether nothing but spacing or a cell separator follows a number: a
/// number glued to a word (`18기`, `1,820명`) has a counter of its own.
fn bare(text: &str, end: usize) -> bool {
    let mut after = text[end..].chars();
    match after.next() {
        None => true,
        Some('.') => after.next().is_none_or(char::is_whitespace),
        Some(c) => c.is_whitespace() || matches!(c, ',' | ';' | '|'),
    }
}

/// An item's number (`4. 시가배당율`, `2) 배당종류`): one or two digits after
/// a space, then `.` or `)` and a space.
fn enumerator(text: &str, number: &Scanned) -> bool {
    let run = &text[number.start..number.end];
    let mut after = text[number.end..].chars();
    run.len() <= 2
        && run.bytes().all(|b| b.is_ascii_digit())
        && text[..number.start]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace)
        && matches!(after.next(), Some('.' | ')'))
        && after.next().is_some_and(char::is_whitespace)
}

/// Whether `gap` ends a sentence: a full stop before a space or the end
/// (`입니다.`, `67,500.`), never a decimal point or an item's number (`4.`).
fn ends_sentence(text: &str, gap: std::ops::Range<usize>) -> bool {
    text[gap.clone()].char_indices().any(|(offset, c)| {
        let at = gap.start + offset;
        if !matches!(c, '.' | '!' | '?' | '。')
            || !text[at + c.len_utf8()..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace)
        {
            return false;
        }
        let before = &text[..at];
        // An abbreviation or PDF dot leader can end a filing row label.
        if c == '.'
            && (before.ends_with("U.S") || before.ends_with("..."))
            && text[at + 1..].trim_start().starts_with(['|', '$'])
        {
            return false;
        }
        let digits = before.len() - before.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        let item = (1..=2).contains(&digits)
            && before[..at - digits]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace);
        !item
            && before
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace())
    })
}

/// Only unambiguous aggregate lines inherit a mixed filing heading. Row labels
/// such as "Basic" under earnings per share and unlabeled columns stay unscaled.
fn filing_amount_label(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    let words = || lower.split(|c: char| !c.is_ascii_alphabetic());
    if lower.contains('%')
        || words().any(|word| {
            matches!(
                word,
                "per" | "margin" | "ratio" | "percent" | "growth" | "rate" | "yield"
            )
        })
    {
        return false;
    }
    words().any(|word| {
        matches!(
            word,
            "revenue"
                | "revenues"
                | "sales"
                | "income"
                | "profit"
                | "expense"
                | "expenses"
                | "cost"
                | "costs"
                | "loss"
                | "losses"
                | "fees"
                | "assets"
                | "liabilities"
                | "incentives"
        )
    })
}

/// Units the text states apart from its plain numbers, one per number:
/// a row label's (`5. 배당금총액(원) 6,000,000,000`), a table's (`(단위:
/// 백만원)`) or words after one number (`184.6이고 단위는 십억 원`, `300
/// (원)`). Words describe only the number before them; a heading reaches
/// only bare numbers within its row or table, never an item's number.
fn stated_units(text: &str, numbers: &[Scanned]) -> Vec<Option<Unit>> {
    let mut stated = vec![None; numbers.len()];
    if numbers.is_empty() {
        return stated;
    }
    let ends: Vec<usize> = numbers.iter().map(|number| number.end).collect();
    // The number whose end precedes `at` with only a short `gap` between,
    // and whether it is plain: a number with its own unit keeps it.
    let described = |at: usize, gap: &dyn Fn(&str) -> bool| {
        let index = ends.partition_point(|end| *end <= at).checked_sub(1)?;
        (at - ends[index] <= 16 && gap(&text[ends[index]..at]))
            .then_some((index, numbers[index].unit == Unit::Plain))
    };
    let spacing = |gap: &str| gap.chars().all(|c| matches!(c, ' ' | '\u{a0}'));
    // A glued particle, a comma or full stop, then spacing (`184.6이고 `).
    let particle = |gap: &str| {
        let rest = gap.trim_start_matches(is_hangul);
        gap.chars().count() - rest.chars().count() <= 3
            && spacing(rest.strip_prefix([',', '.']).unwrap_or(rest))
    };
    let mut headings: Vec<Heading> = Vec::new();
    for (at, open) in text.char_indices() {
        let close = match open {
            '(' => ')',
            '（' => '）',
            '[' => ']',
            _ => continue,
        };
        let inside = at + open.len_utf8();
        let Some(length) = text[inside..]
            .char_indices()
            .take_while(|(offset, _)| *offset <= 64)
            .find(|(_, c)| *c == close || matches!(c, '(' | '（' | '[' | '\n'))
            .filter(|(_, c)| *c == close)
            .map(|(offset, _)| offset)
        else {
            continue;
        };
        let content = &text[inside..inside + length];
        let Some((unit, mut reach)) = bracket_unit(content) else {
            continue;
        };
        if reach == Reach::Row && english_scale(&content.trim().to_ascii_lowercase()).is_some() {
            // "(Millions) | 2024 | 2023" is a table heading; "Revenue (millions)"
            // gives the unit only to that row.
            let before = text[..at].trim_end_matches([' ', '\t', '\u{a0}', '|']);
            if before.is_empty() || before.ends_with('\n') {
                reach = Reach::Table;
            }
        }
        let end = inside + length + close.len_utf8();
        match described(at, &spacing) {
            // `300 (원)` states the unit of the number it follows.
            Some((index, plain)) if reach == Reach::Row => {
                if plain {
                    stated[index] = unit;
                }
            }
            _ => headings.push(Heading {
                start: at,
                end,
                unit,
                reach,
            }),
        }
    }
    let lower = text.to_ascii_lowercase();
    let markers = text
        .match_indices("단위")
        .map(|(at, marker)| (at, marker.len()))
        .chain(lower.match_indices("unit").filter_map(|(at, _)| {
            let length = if lower[at..].starts_with("units") {
                5
            } else {
                4
            };
            let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
            (boundary(lower[..at].chars().next_back())
                && boundary(lower[at + length..].chars().next()))
            .then_some((at, length))
        }))
        .collect::<Vec<_>>();
    // Bracket headings never overlap and are in text order.
    let brackets = headings.len();
    for (at, length) in markers {
        let inside = headings[..brackets].partition_point(|heading| heading.start < at);
        if inside > 0 && at < headings[inside - 1].end {
            continue;
        }
        let after = &text[at + length..];
        let rest = after.trim_start_matches([' ', '\u{a0}']);
        if let Some(phrase) = rest.strip_prefix([':', '：']) {
            // `단위: 백만원` heads a table.
            let phrase_at = text.len() - phrase.len();
            let (unit, end) = match unit_words(phrase) {
                Some((unit, consumed, list)) => ((!list).then_some(unit), phrase_at + consumed),
                None => (None, phrase_at),
            };
            headings.push(Heading {
                start: at,
                end,
                unit,
                reach: Reach::Table,
            });
            continue;
        }
        // `184.6이고 단위는 십억 원`, `184.6, units are KRW billion`.
        let phrase = ["는", "가"]
            .iter()
            .find_map(|word| after.strip_prefix(word))
            .or_else(|| {
                ["is ", "are "]
                    .iter()
                    .find(|word| {
                        rest.as_bytes()
                            .get(..word.len())
                            .is_some_and(|head| head.eq_ignore_ascii_case(word.as_bytes()))
                    })
                    .map(|word| &rest[word.len()..])
            });
        if let (Some(phrase), Some((index, true))) = (phrase, described(at, &particle)) {
            if let Some((unit, _, _)) = unit_words(phrase) {
                stated[index] = Some(unit);
            }
        }
    }
    headings.sort_by_key(|heading| heading.start);
    // The row's unit, with its numbers and the words before its first one.
    let mut row: Option<(Unit, usize, usize)> = None;
    let mut table: Option<Unit> = None;
    let mut mixed_filing = false;
    let mut monetary_row = false;
    let mut headings = headings.into_iter().peekable();
    let mut cursor = 0;
    let close_scopes = |gap: std::ops::Range<usize>,
                        row: &mut Option<(Unit, usize, usize)>,
                        table: &mut Option<Unit>| {
        let between = &text[gap.clone()];
        let lines: Vec<&str> = between.split('\n').collect();
        let blank = lines.len() > 2
            && lines[1..lines.len() - 1]
                .iter()
                .any(|line| line.trim().is_empty());
        if blank || ends_sentence(text, gap) {
            *row = None;
            *table = None;
        } else if between.contains(['\n', ';']) {
            *row = None;
        } else if let Some((_, count, words)) = row {
            let found = between
                .split_whitespace()
                .filter(|word| word.contains(char::is_alphanumeric))
                .count();
            *words += found;
            if (*count > 0 && found > 0) || *words > 3 {
                *row = None;
            }
        }
    };
    for (index, number) in numbers.iter().enumerate() {
        while let Some(heading) = headings.next_if(|heading| heading.start < number.start) {
            close_scopes(
                cursor.min(heading.start)..heading.start,
                &mut row,
                &mut table,
            );
            match heading.reach {
                Reach::Table | Reach::Filing => {
                    mixed_filing = heading.reach == Reach::Filing;
                    monetary_row = false;
                    table = heading.unit;
                    row = None;
                }
                Reach::Row => row = heading.unit.map(|unit| (unit, 0, 0)),
            }
            cursor = cursor.max(heading.end);
        }
        let gap = cursor.min(number.start)..number.start;
        if mixed_filing && table.is_some() {
            let between = &text[gap.clone()];
            let label = between.rsplit_once('\n').map_or(between, |(_, last)| last);
            if between.contains('\n') || label.contains(char::is_alphabetic) {
                monetary_row = filing_amount_label(label);
            }
        }
        close_scopes(gap, &mut row, &mut table);
        cursor = cursor.max(number.end);
        if enumerator(text, number) {
            row = None;
            continue;
        }
        let heading = row.map(|(unit, _, _)| unit).or_else(|| {
            let next_cell = text[number.end..].trim_start_matches([' ', '\t', '\u{a0}', '|']);
            table.filter(|_| !mixed_filing || (monetary_row && !next_cell.starts_with('%')))
        });
        if stated[index].is_none()
            && bare(text, number.end)
            && (number.unit == Unit::Plain
                || (number.currency_prefix_only && matches!(heading, Some(Unit::Amount(_)))))
        {
            stated[index] = heading;
        }
        if let Some((_, count, _)) = &mut row {
            *count += 1;
        }
    }
    stated
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
        .is_some_and(|code| is_currency_code(code.trim().to_lowercase().as_str()))
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

/// A field whose whole value is one number (`"41,200"`, `"-3.1"`, `"−3.1"`,
/// the accounting negatives `"(1,234)"` and `"△1,234"`); dates, identifiers
/// with inner hyphens and codes with a leading zero are not.
fn field_number(text: &str) -> Option<f64> {
    let text = text.trim();
    // A bracketed or marked number carries its sign in the mark, never a `-`.
    let (body, sign) = if let Some(inner) = text.strip_prefix('(').and_then(|t| t.strip_suffix(')'))
    {
        (inner.trim(), -1.0)
    } else if let Some(sign) = text.chars().next().and_then(triangle_sign) {
        let mark = text.chars().next().map_or(0, char::len_utf8);
        (text[mark..].trim_start(), sign)
    } else if let Some(unsigned) = text.strip_prefix(['-', '−']) {
        (unsigned, -1.0)
    } else {
        (text, 1.0)
    };
    if body.is_empty()
        || code_like(body)
        || !body.starts_with(|c: char| c.is_ascii_digit())
        || !body
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ',' | '.'))
        || body.matches('.').count() > 1
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
                    stated_fall: false,
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
                stated_fall: false,
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

/// Daily investor net-purchase fields from the KIS read manifest. The investor flows
/// section in `agent/skills/filings/references/korea-statements.md` gives their unit.
const KIS_DAILY_NET_PURCHASES_KRW_MILLIONS: &[&str] = &[
    "bank_ntby_tr_pbmn",
    "etc_corp_ntby_tr_pbmn",
    "etc_ntby_tr_pbmn",
    "etc_orgt_ntby_tr_pbmn",
    "frgn_ntby_tr_pbmn",
    "fund_ntby_tr_pbmn",
    "insu_ntby_tr_pbmn",
    "ivtr_ntby_tr_pbmn",
    "mrbn_ntby_tr_pbmn",
    "orgn_ntby_tr_pbmn",
    "pe_fund_ntby_tr_pbmn",
    "prsn_ntby_tr_pbmn",
    "scrt_ntby_tr_pbmn",
];

/// Every number in a structured payload (bounded), each with its JSON Pointer.
/// Identifier, date and position fields are skipped.
pub fn data_candidates(payload: &Value) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut budget = MAX_TEXT_BYTES;
    collect(payload, None, &mut String::new(), &mut out, &mut budget);
    if payload["source_id"] == crate::finance_data::KIS_SOURCE_ID
        && payload
            .pointer("/query/operation_id")
            .and_then(Value::as_str)
            == Some("domestic_stock.investor_trade_by_stock_daily")
    {
        for candidate in &mut out {
            let Place::Pointer(path) = &candidate.place else {
                continue;
            };
            let field = path
                .strip_prefix("/data/response/output2/")
                .and_then(|row| row.rsplit('/').next());
            if candidate.unit == Unit::Raw
                && field.is_some_and(|key| KIS_DAILY_NET_PURCHASES_KRW_MILLIONS.contains(&key))
            {
                candidate.unit = Unit::Amount(1e6);
            }
        }
    }
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

/// Candidates read as printed: an amount's number in its own scale (`52`
/// for `52억원`, `21000` for a row in KRW millions), for an input whose unit
/// nothing states.
pub fn as_printed(candidates: &[Candidate]) -> Vec<Candidate> {
    candidates
        .iter()
        .map(|candidate| Candidate {
            unit: match candidate.unit {
                Unit::Amount(_) => Unit::Plain,
                other => other,
            },
            ..candidate.clone()
        })
        .collect()
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
    /// The figure carries its own sign (`+`, `-`, `−`, a triangle mark or
    /// accounting brackets), which a fall stated in words must agree with.
    pub signed: bool,
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

/// The text after a leading sign, whether the sign is a minus (`-`, `−`,
/// `–`), and whether there was one.
fn strip_sign(text: &str) -> (&str, bool, bool) {
    if let Some(rest) = text.strip_prefix(['-', '−', '–']) {
        (rest, true, true)
    } else if let Some(rest) = text.strip_prefix('+') {
        (rest, false, true)
    } else {
        (text, false, false)
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
        let code = &text[..3];
        let after = &text[3..];
        if is_currency_marker(code)
            && after.starts_with(|c: char| {
                c == ' ' || c.is_ascii_digit() || matches!(c, '-' | '−' | '–')
            })
        {
            return Some(after);
        }
    }
    None
}

/// What may follow an amount: nothing, a currency word or code, or "per share".
fn amount_tail(text: &str) -> bool {
    let raw = text.trim();
    let lower = raw.to_ascii_lowercase();
    let text = lower
        .strip_suffix("per share")
        .or_else(|| lower.strip_suffix("a share"))
        .or_else(|| lower.strip_suffix("/share"))
        .unwrap_or(&lower)
        .trim();
    text.is_empty()
        || raw.get(..text.len()).is_some_and(is_currency_marker)
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
            Some(sign < 0.0),
        ),
        None => (text, None),
    };
    let (mut rest, mut negative, mut signed) = strip_sign(text);
    negative |= bracketed || marked == Some(true);
    signed |= bracketed || marked.is_some();
    let currency = match strip_currency(rest) {
        Some(after) => {
            let (after, minus, sign) = strip_sign(after.trim_start());
            negative |= minus;
            signed |= sign;
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
            signed,
        });
    }
    let value = sign * first;
    let gap = rest.len() != rest.trim_start().len();
    let tail = rest.trim();
    let lower = tail.to_ascii_lowercase();
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
                if !amount_tail(&tail[lower.len() - remainder.len()..]) {
                    return None;
                }
                Unit::Amount(found)
            } else if amount_tail(tail) {
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
        signed,
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
        // A figure's own sign meets a fall the evidence states in words; an
        // unsigned figure compares the magnitude, its own words stating the fall.
        let stated = if printed.signed && candidate.stated_fall {
            -candidate.value
        } else {
            candidate.value
        };
        for (value, converted) in readings(printed.unit, candidate.unit, stated, calculated) {
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
    fn reference_currency_prose_preserves_plain_passage_checks() {
        for (passage, number) in [
            ("Please try 12,000 combinations.", "12,000"),
            ("Please Try 12,000 combinations.", "12,000"),
            ("The budget is 12,000 ron.", "12,000"),
            ("The budget is 12,000 Ron.", "12,000"),
            ("php 8.2 is the current version.", "8.2"),
            ("Php 8.2 is the current version.", "8.2"),
        ] {
            assert_eq!(
                verdict(number, &passage_candidates(passage), false),
                Verdict::Matched,
                "{passage}"
            );
        }
    }

    #[test]
    fn reference_currency_markers_require_case_in_all_number_text() {
        for code in [
            "CZK", "HUF", "PLN", "RON", "ISK", "TRY", "IDR", "ILS", "MYR", "PHP", "THB",
        ] {
            for marker in [
                code.to_ascii_lowercase(),
                format!("{}{}", &code[..1], code[1..].to_ascii_lowercase()),
            ] {
                for passage in [format!("{marker} 12,000"), format!("12,000 {marker}")] {
                    assert_eq!(
                        verdict("12,000", &passage_candidates(&passage), false),
                        Verdict::Matched,
                        "{passage}"
                    );
                    assert!(parse_printed(&passage).is_none(), "{passage}");
                }
                assert_eq!(Unit::of_line(Some(&marker)).unwrap(), Unit::Plain);
                assert_eq!(currency_code(&marker), None);
            }
            for passage in [
                format!("{code} 12,000"),
                format!("12,000 {code}"),
                format!("12,000 million {code}"),
                format!("12,000 {code} per share"),
            ] {
                assert_eq!(
                    verdict(&passage, &passage_candidates(&passage), false),
                    Verdict::Matched,
                    "{passage}"
                );
                assert!(
                    matches!(printed(&passage).unit, Unit::Amount(_)),
                    "{passage}"
                );
            }
            assert_eq!(Unit::of_line(Some(code)).unwrap(), Unit::Amount(1.0));
            assert_eq!(currency_code(code), Some(code.into()));
        }
    }

    #[test]
    fn legacy_currency_codes_and_words_keep_their_case_behavior() {
        for code in &CURRENCY_CODES[..21] {
            for marker in [code.to_string(), code.to_ascii_lowercase()] {
                for passage in [format!("{marker} 12,000"), format!("12,000 {marker}")] {
                    assert_eq!(
                        verdict(&passage, &passage_candidates(&passage), false),
                        Verdict::Matched,
                        "{passage}"
                    );
                    assert_eq!(printed(&passage).unit, Unit::Amount(1.0));
                }
            }
        }
        for word in CURRENCY_WORDS {
            for marker in [word.to_string(), word.to_ascii_uppercase()] {
                let suffix = format!("12,000 {marker}");
                assert_eq!(
                    verdict(&suffix, &passage_candidates(&suffix), false),
                    Verdict::Matched
                );
                assert_eq!(printed(&suffix).unit, Unit::Amount(1.0));
                let prefix = format!("{marker} 12,000");
                assert_eq!(
                    verdict("12,000", &passage_candidates(&prefix), false),
                    Verdict::Matched
                );
            }
        }
    }

    #[test]
    fn uppercase_php_version_remains_an_ambiguous_amount() {
        let passage = passage_candidates("PHP 8.2 is the current version.");
        assert_eq!(verdict("8.2", &passage, false), Verdict::NotFound);
        assert_eq!(verdict("PHP 8.2", &passage, false), Verdict::Matched);
        for passage in [
            "Please TRY 12,000 combinations.",
            "The budget is 12,000 RON.",
        ] {
            assert_eq!(
                verdict("12,000", &passage_candidates(passage), false),
                Verdict::NotFound
            );
        }
    }

    #[test]
    fn reference_rate_fields_are_raw_numbers() {
        let payload = json!({"from":"USD","to":"KRW","to_per_from":1334.87,"date":"2026-11-16","revision":"ecb:2026-11-16","source":{"id":"ecb","name":"European Central Bank euro foreign exchange reference rates"},"legs":{"KRW_per_EUR":1470.23,"USD_per_EUR":1.1014},"computed":"cross rate through EUR"});
        let found = data_candidates(&payload);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found.iter().all(|candidate| candidate.unit == Unit::Raw));
        for number in [1334.87, 1470.23, 1.1014] {
            assert!(found.iter().any(|candidate| candidate.value == number));
        }
    }

    #[test]
    fn reference_currency_codes_are_amounts() {
        for code in [
            "CZK", "HUF", "PLN", "RON", "ISK", "TRY", "IDR", "ILS", "MYR", "PHP", "THB",
        ] {
            assert!(
                matches!(printed(&format!("{code} 12")).unit, Unit::Amount(1.0)),
                "{code}"
            );
            assert_eq!(currency_code(code), Some(code.into()));
        }
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
    fn currency_prefixes_require_a_word_boundary() {
        for text in [
            "fraud 12",
            "fraud  12",
            "entrepreneur 5",
            "éAUD 12",
            "_EUR 5",
            "USDKRW   12",
            "USD\n12",
            "USD \n12",
            "USD\t12",
        ] {
            let value = if text.contains("12") { 12.0 } else { 5.0 };
            assert_eq!(find(value, Unit::Amount(1.0), &json!(text)), None, "{text}");
            assert_eq!(
                find(value, Unit::Plain, &json!(text)),
                Some(value),
                "{text}"
            );
        }
        for text in ["AUD 12", "aud 12", "Revenue: EUR 12", "(USD 12)", "AUD  12"] {
            assert_eq!(
                find(12.0, Unit::Amount(1.0), &json!(text)),
                Some(12.0),
                "{text}"
            );
        }
        for (text, value) in [("USDKRW 1,350", 1350.0), ("EURUSD 1.08", 1.08)] {
            assert_eq!(
                find(value, Unit::Amount(1.0), &json!(text)),
                Some(value),
                "{text}"
            );
            assert_eq!(find(value, Unit::Plain, &json!(text)), None, "{text}");
        }
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

    /// rc1 T-045: `calculate` summed flows with `"unit": "KRW 100 million"`;
    /// reading that as millions marked correct 억 figures as differing.
    #[test]
    fn a_number_in_an_amount_unit_multiplies_its_scale() {
        for (unit, scale) in [
            ("KRW 100 million", 1e8),
            ("100 million won", 1e8),
            ("KRW 100 millions", 1e8),
            ("억 원", 1e8),
            ("억원", 1e8),
            ("십억 원", 1e9),
            ("조 원", 1e12),
            ("thousand", 1e3),
            ("10억원", 1e9),
            ("100억 원", 1e10),
            ("KRW 1,000", 1e3),
            ("$100m", 1e8),
            ("KRW 억", 1e8),
            ("KRW millions", 1e6),
            ("KRW bn", 1e9),
            ("$m", 1e6),
            ("백만원", 1e6),
        ] {
            assert_eq!(Unit::of_line(Some(unit)), Ok(Unit::Amount(scale)), "{unit}");
        }
        assert_eq!(currency_code("KRW 100 million").as_deref(), Some("KRW"));
        assert_eq!(currency_code("100 million won").as_deref(), Some("KRW"));
        // Only one whole power of ten multiplies an amount.
        for unit in [
            "KRW 5 million",
            "1.5 billion",
            "KRW 100 10 million",
            "100%",
            "100 shares",
            "% of 2025 revenue",
            "2.5x",
            "KRW 0 million",
        ] {
            assert_eq!(
                Unit::of_line(Some(unit)),
                Err(AmbiguousUnit(unit.into())),
                "{unit}"
            );
        }
        // A denominator never scales the amount.
        for (unit, read) in [
            ("KRW per 1,000 shares", Unit::Amount(1.0)),
            ("won per thousand shares", Unit::Amount(1.0)),
            ("KRW/100 shares", Unit::Amount(1.0)),
            ("per 1,000 people", Unit::Plain),
            ("tonnes per unit", Unit::Plain),
        ] {
            assert_eq!(Unit::of_line(Some(unit)), Ok(read), "{unit}");
        }
        let flows = json!("외국인 누적 123.4억원, 기관 -41.2억원");
        let eok = Unit::of_line(Some("KRW 100 million")).unwrap();
        assert_eq!(find(123.4, eok, &flows), Some(123.4));
        assert_eq!(find(-41.2, eok, &flows), Some(-41.2));
        assert_eq!(find(1.234, eok, &flows), None);
    }

    /// rc1 T-047: an OpenDART dividend notice states each row's unit in its
    /// label (`5. 배당금총액(원) 6,000,000,000`).
    const DIVIDEND_NOTICE: &str =
        "현금ㆍ현물배당결정 가상의 솔숲식품 2025년 결산 현금배당 결정입니다. \
        1. 배당구분 결산배당 2. 배당종류 현금배당 3. 1주당 배당금(원) 보통주식 300 \
        4. 시가배당율(%) 보통주식 1.4 5. 배당금총액(원) 6,000,000,000 6. 배당기준일 2025-12-31 \
        7. 배당금지급 예정일자 2026-04-10 8. 이사회결의일(결정일) 2026-02-05";

    #[test]
    fn a_row_labels_unit_reaches_the_numbers_in_its_row() {
        let notice = passage_candidates(DIVIDEND_NOTICE);
        for (text, expected) in [
            ("₩6 billion", Verdict::Rounded),
            ("6,000,000,000원", Verdict::Matched),
            ("₩6,000,000,000", Verdict::Matched),
            // The number as printed still matches without a unit.
            ("6,000,000,000", Verdict::Matched),
            ("1.4%", Verdict::Matched),
            ("1.4", Verdict::Matched),
            ("₩300 per share", Verdict::Matched),
            ("300원", Verdict::Matched),
            // A different figure, and items' numbers, never take a row's unit.
            ("₩6.5 billion", Verdict::NotFound),
            ("1.5%", Verdict::NotFound),
            ("5%", Verdict::NotFound),
            ("₩4", Verdict::NotFound),
            ("₩6", Verdict::NotFound),
            ("₩2,025", Verdict::NotFound),
        ] {
            assert_eq!(verdict(text, &notice, false), expected, "{text}");
        }
        let (_, index) = check(&printed("₩6 billion"), &notice, false);
        let Place::Span { start, end } = notice[index.unwrap()].place else {
            panic!("passage candidates carry spans");
        };
        assert_eq!(&DIVIDEND_NOTICE[start..end], "6,000,000,000");
        // A data payload's text reads the same way.
        let filing = json!({"data": {"document": {"text": DIVIDEND_NOTICE}}});
        assert_eq!(find(6e9, Unit::Amount(1.0), &filing), Some(6e9));
        assert_eq!(find(1.4, Unit::Percent, &filing), Some(1.4));
        assert_eq!(find(5.0, Unit::Percent, &filing), None);
        for (text, figure, expected) in [
            // A row ends at the next label after its numbers.
            (
                "배당금(원) 300 주식수 1,000,000",
                "₩1,000,000",
                Verdict::NotFound,
            ),
            ("배당금(원) 300 주식수 1,000,000", "₩300", Verdict::Matched),
            ("배당금(원) 300 | 250", "₩250", Verdict::Matched),
            // Sub-labels may sit between a label and its numbers, but not many.
            (
                "배당금(원) 보통주식 우선주 기타 종류 300",
                "₩300",
                Verdict::NotFound,
            ),
            ("배당금(원)\n300", "₩300", Verdict::NotFound),
            ("배당금(원); 300", "₩300", Verdict::NotFound),
            // A number glued to a word has a counter of its own.
            ("임직원(원) 1,820명", "₩1,820", Verdict::NotFound),
            // A unit right after a number states that number's unit.
            ("보통주식 300 (원) 200", "₩300", Verdict::Matched),
            ("보통주식 300 (원) 200", "₩200", Verdict::NotFound),
            ("배당률 1.4(%)", "1.4%", Verdict::Matched),
            // Brackets that name no unit are not labels.
            ("(주)솔숲식품 300", "₩300", Verdict::NotFound),
            ("(연결) 매출 300", "₩300", Verdict::NotFound),
            ("(USD 10) 25", "$250", Verdict::NotFound),
            ("(원, %) 300", "₩300", Verdict::NotFound),
            (
                "Revenue (KRW million) 452,312",
                "₩452.3bn",
                Verdict::Rounded,
            ),
        ] {
            assert_eq!(
                verdict(figure, &passage_candidates(text), false),
                expected,
                "{figure} in {text}"
            );
        }
        let units = |text: &str| {
            passage_candidates(text)
                .into_iter()
                .map(|candidate| (candidate.value, candidate.unit))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            units("PER(배) 12.3"),
            [(12.3, Unit::Plain), (12.3, Unit::Multiple)]
        );
        // A lone letter in brackets is an item, not a unit.
        assert_eq!(units("옵션 (x) 30"), [(30.0, Unit::Plain)]);
        assert_eq!(
            units("(m) 30 (b) 40"),
            [(30.0, Unit::Plain), (40.0, Unit::Plain)]
        );
    }

    #[test]
    fn filing_table_revenues_match_only_at_the_stated_scale_and_precision() {
        for (passage, printed) in [
            ("(in millions)\nNet revenue $ 35,926", "$35.926bn"),
            ("(in\u{a0}millions)\nNet revenue $ 35,926", "$35.926bn"),
            ("| (Millions) | 2024 | 2023\nNet revenue | 65,949 | 60,515", "$65.949bn"),
            ("(in millions, except percentages)\nU.S. | $ | 14,780 | 5 | %\nNet revenue | $ | 35,926", "USD 35.926bn"),
            ("(in\u{a0}millions, except percentages and per share data)\nNet revenue | $ | 28,167 | 12%", "$28.167bn"),
            ("(Millions, except per share amounts) | 2024 | 2023\nTotal revenues net of interest expense | 65,949 | 60,515", "$65.949bn"),
        ] {
            assert_eq!(verdict(printed, &passage_candidates(passage), false), Verdict::Rounded, "{passage}");
        }
        let table = "(in millions, except percentages and per share data)\nNet revenue $ 35,926 | 10 | %\nEarnings per share | $ 14.04";
        for wrong in [
            "$35.926m",
            "$35,926",
            "$35.927bn",
            "-$35.926bn",
            "$14.04m",
            "$10m",
        ] {
            assert_eq!(
                verdict(wrong, &passage_candidates(table), false),
                Verdict::NotFound,
                "{wrong}"
            );
        }
        assert_eq!(
            verdict(
                "$35.926bn",
                &passage_candidates("Net revenue | $ | 35,926"),
                false
            ),
            Verdict::NotFound
        );
    }

    #[test]
    fn a_tables_unit_reaches_its_bare_numbers_until_the_table_ends() {
        let table = "(단위: 백만원) 과목 당기 전기 매출액 67,500 72,300 \
            주당순이익(원) 1,050 975 영업이익 5,200 8,000\n\n임직원 1,820 명";
        let candidates = passage_candidates(table);
        for (text, expected) in [
            ("₩67.5 billion", Verdict::Rounded),
            ("675억원", Verdict::Rounded),
            ("67,500백만원", Verdict::Matched),
            ("₩72.3 billion", Verdict::Rounded),
            // A row's own label overrides the table for its row only.
            ("₩1,050", Verdict::Matched),
            ("₩975", Verdict::Matched),
            ("₩1,050 million", Verdict::NotFound),
            ("₩5.2 billion", Verdict::Rounded),
            ("₩8 billion", Verdict::Rounded),
            ("₩67.6 billion", Verdict::NotFound),
            // A blank line ends the table.
            ("₩1,820 million", Verdict::NotFound),
            ("1,820", Verdict::Matched),
        ] {
            assert_eq!(verdict(text, &candidates, false), expected, "{text}");
        }
        for (text, figure, expected) in [
            (
                "단위 : 백만원 과 목 매출액 67,500",
                "₩67.5 billion",
                Verdict::Rounded,
            ),
            ("[단위: 억원] 매출액 675", "675억원", Verdict::Matched),
            (
                "(Unit: KRW million) Revenue 67,500",
                "₩67.5 billion",
                Verdict::Rounded,
            ),
            (
                "(in KRW millions) Revenue 67,500",
                "₩67.5 billion",
                Verdict::Rounded,
            ),
            (
                "(단위: 1,000원) 매출액 67,500",
                "₩67.5 million",
                Verdict::Rounded,
            ),
            // A sentence end closes the table, so prose after it keeps its numbers.
            (
                "(단위: 백만원) 매출액 67,500. 직원은 1,820 명이다",
                "₩1,820 million",
                Verdict::NotFound,
            ),
            (
                "(단위: 백만원) 매출액 67,500입니다. 직원 1,820",
                "₩1,820 million",
                Verdict::NotFound,
            ),
            // A list of units, or other words, places no unit for the table.
            (
                "(단위: 원, %) 배당금 300 배당률 1.4",
                "₩300",
                Verdict::NotFound,
            ),
            (
                "(단위: 원, %) 배당금 300 배당률 1.4",
                "1.4%",
                Verdict::NotFound,
            ),
            ("단위: 원, 주 배당금 300", "₩300", Verdict::NotFound),
            (
                "(In millions, except per share data) Revenue 67,500",
                "$67.5 billion",
                Verdict::Rounded,
            ),
            // The next table's unit replaces the last one's.
            (
                "(단위: 백만원) 매출 67,500 (단위: 원) 주가 41,200",
                "₩41,200",
                Verdict::Matched,
            ),
            (
                "(단위: 백만원) 매출 67,500 (단위: 원) 주가 41,200",
                "₩41.2 billion",
                Verdict::NotFound,
            ),
            (
                "(단위: 백만원) 매출 67,500 (단위: 주) 주식수 1,000",
                "₩1 billion",
                Verdict::NotFound,
            ),
        ] {
            assert_eq!(
                verdict(figure, &passage_candidates(text), false),
                expected,
                "{figure} in {text}"
            );
        }
    }

    /// rc1 L-032: a saved source states a backlog's unit in words after it.
    #[test]
    fn a_unit_stated_in_words_describes_only_the_number_before_it() {
        let source = "https://fictional.example/solsup/backlog-2025\n\n솔숲식품은 2025-03-12 \
            현재 수주잔고 수치가 184.6이고 단위는 십억 원이라고 알렸다.\n";
        let candidates = passage_candidates(source);
        for (text, expected) in [
            ("₩184.6 billion", Verdict::Matched),
            ("1,846억원", Verdict::Rounded),
            ("184.6", Verdict::Matched),
            ("₩184.6 million", Verdict::NotFound),
            ("₩185.6 billion", Verdict::NotFound),
            ("₩2,025 billion", Verdict::NotFound),
        ] {
            assert_eq!(verdict(text, &candidates, false), expected, "{text}");
        }
        for (text, figure, expected) in [
            (
                "매출 675이고 수주잔고는 184.6이고 단위는 십억 원이다",
                "₩675 billion",
                Verdict::NotFound,
            ),
            (
                "수주잔고는 184.6이다. 단위는 십억 원이다",
                "₩184.6 billion",
                Verdict::Matched,
            ),
            ("수주잔고 184.6, 단위는 %", "184.6%", Verdict::Matched),
            (
                "Backlog was 184.6, units are KRW billion.",
                "₩184.6 billion",
                Verdict::Matched,
            ),
            // Another word between them, or a unit of the number's own, wins.
            (
                "수주잔고 184.6이고 그 단위는 십억 원",
                "₩184.6 billion",
                Verdict::NotFound,
            ),
            (
                "수주잔고 184.6%이고 단위는 십억 원",
                "₩184.6 billion",
                Verdict::NotFound,
            ),
            (
                "수주잔고 184.6이고 단위는 바뀌었다",
                "₩184.6 billion",
                Verdict::NotFound,
            ),
        ] {
            assert_eq!(
                verdict(figure, &passage_candidates(text), false),
                expected,
                "{figure} in {text}"
            );
        }
    }

    #[test]
    fn stated_units_read_long_text_in_one_pass() {
        // Each shape repeats near the 1 MiB text bound; none rescans the text.
        for (repeated, count) in [
            ("배당금(원) 300 ", 70_000),
            ("1 단위는단위는", 40_000),
            ("(단위: 백만원) 1,000 ", 50_000),
            ("1이고 단위는 ", 60_000),
            ("1, units are ", 70_000),
        ] {
            let text = repeated.repeat(count);
            let found = passage_candidates(&text);
            assert!(found.len() <= 2 * count, "{repeated}");
        }
        let digits = format!("a{}. 1", "1".repeat(900_000));
        assert_eq!(passage_candidates(&digits).len(), 1);
    }

    #[test]
    fn as_printed_reads_an_amount_in_its_own_scale() {
        let filing =
            passage_candidates("매출액은 675억원(전년동기 723억원), 영업이익 52억원, 12.3%");
        let plain = |value: f64| Printed {
            value,
            unit: Unit::Plain,
            precision: 0.5,
            signed: false,
        };
        assert_eq!(check(&plain(52.0), &filing, false).0, Verdict::NotFound);
        let printed = as_printed(&filing);
        assert_eq!(check(&plain(52.0), &printed, false).0, Verdict::Matched);
        assert_eq!(check(&plain(675.0), &printed, false).0, Verdict::Matched);
        assert_eq!(check(&plain(53.0), &printed, false).0, Verdict::NotFound);
        // A share is not an amount.
        assert_eq!(check(&plain(12.3), &printed, false).0, Verdict::NotFound);
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
                stated_fall: false,
            },
            Candidate {
                value: 38_100_000_000.0,
                unit: Unit::Plain,
                place: Place::Pointer("/inputs/0".into()),
                stated_fall: false,
            },
            Candidate {
                value: 0.084_173,
                unit: Unit::Plain,
                place: Place::Pointer("/inputs/9".into()),
                stated_fall: false,
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

    /// `−` (U+2212) is a minus wherever a number is read: an answer's figure,
    /// evidence text and a provider's text field.
    #[test]
    fn the_minus_sign_is_a_minus_in_figures_text_and_fields() {
        for text in ["−3.1%", "-3.1%"] {
            let found = printed(text);
            assert_eq!((found.value, found.signed), (-3.1, true), "{text}");
        }
        assert_eq!(printed("KRW−5,000").value, -5000.0);
        assert_eq!(printed("₩−5,000").value, -5000.0);
        assert_eq!(printed("−₩5,000").value, -5000.0);
        for evidence in [
            "shares fell −3.1% on the day",
            "shares fell -3.1% on the day",
        ] {
            let found = passage_candidates(evidence);
            assert_eq!(
                verdict("−3.1%", &found, false),
                Verdict::Matched,
                "{evidence}"
            );
            assert_eq!(
                verdict("-3.1%", &found, false),
                Verdict::Matched,
                "{evidence}"
            );
            assert_eq!(
                verdict("3.1%", &found, false),
                Verdict::NotFound,
                "{evidence}"
            );
        }
        assert_eq!(field_number("−1.20"), Some(-1.2));
        assert_eq!(field_number("(−1,234)"), None);
        let krx = json!({"rows": [{"FLUC_RT": "−1.20"}]});
        let found = data_candidates(&krx);
        assert_eq!(found[0].unit, Unit::Percent);
        assert_eq!(verdict("−1.2%", &found, false), Verdict::Matched);
    }

    #[test]
    fn a_fall_is_stated_only_by_a_word_right_after_the_number() {
        let falls = |text: &str| {
            passage_candidates(text)
                .iter()
                .map(|candidate| (candidate.value, candidate.stated_fall))
                .collect::<Vec<_>>()
        };
        for text in [
            "판매량이 약 9% 줄었고",
            "판매량 9%감소",
            "가격이 9%  하락했다",
            "주가가 9% 떨어졌다",
            "마진이 9%p 낮아졌다",
            "9% lower than Q2",
            "a 9% decline",
            "a 9% drop in volume",
        ] {
            assert!(falls(text)[0].1, "{text}");
        }
        assert_eq!(falls("이익 1조 2,345억원 감소"), [(12_345.0, true)]);
        for text in [
            // Words of increase, particles and other words between.
            "판매량이 9% 늘었고",
            "52억원으로 줄었다",
            "248달러보다 낮아졌다",
            "9% 이상 줄었다",
            "9%가량 줄었다",
            "9%   하락",
            "9%\n감소",
            "9% higher",
            "9% down payment",
            "9% lowered",
            // A number with its own sign keeps it.
            "−9% 감소",
            "△9 감소",
            "(1,234) 감소",
        ] {
            assert!(falls(text).iter().all(|(_, fall)| !fall), "{text}");
        }
        // A signed figure reads the fall; an unsigned one the magnitude.
        let found = passage_candidates("판매량이 약 9% 줄었고");
        assert_eq!(verdict("−9%", &found, false), Verdict::Matched);
        assert_eq!(verdict("9%", &found, false), Verdict::Matched);
        assert_eq!(verdict("+9%", &found, false), Verdict::NotFound);
        assert_eq!(verdict("−9.0%", &found, false), Verdict::Matched);
        let rise = passage_candidates("판매량이 약 9% 늘었고");
        assert_eq!(verdict("−9%", &rise, false), Verdict::NotFound);
        assert_eq!(verdict("+9%", &rise, false), Verdict::Matched);
        // `−9% 감소` prints its sign: the figure must carry it.
        let printed_sign = passage_candidates("판매량 −9% 감소");
        assert_eq!(verdict("−9%", &printed_sign, false), Verdict::Matched);
        assert_eq!(verdict("9%", &printed_sign, false), Verdict::NotFound);
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
