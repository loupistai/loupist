//! Currency tokens shared by printed figures, evidence and unit declarations.
use crate::finance_data::fx::{is_currency_marker, CURRENCY_CODES};

pub(crate) type Currency = &'static str;

const ALIASES: &[(&str, Currency)] = &[
    ("미국 달러", "USD"),
    ("대만 달러", "TWD"),
    ("홍콩 달러", "HKD"),
    ("人民币", "CNY"),
    ("人民幣", "CNY"),
    ("新台幣", "TWD"),
    ("新台币", "TWD"),
    ("파운드", "GBP"),
    ("달러", "USD"),
    ("ドル", "USD"),
    ("위안", "CNY"),
    ("유로", "EUR"),
    ("원", "KRW"),
    ("엔", "JPY"),
    ("円", "JPY"),
    ("元", "元"),
    ("RMB", "CNY"),
    ("HK$", "HKD"),
    ("NT$", "TWD"),
    ("US$", "USD"),
    ("NZ$", "NZD"),
    ("A$", "AUD"),
    ("C$", "CAD"),
    ("S$", "SGD"),
    ("dollars", "USD"),
    ("dollar", "USD"),
    ("euros", "EUR"),
    ("euro", "EUR"),
    ("pounds", "GBP"),
    ("pound", "GBP"),
    ("won", "KRW"),
    ("yen", "JPY"),
    ("yuan", "CNY"),
    ("$", "$"),
    ("¥", "¥"),
    ("￥", "¥"),
    ("₩", "KRW"),
    ("￦", "KRW"),
    ("€", "EUR"),
    ("£", "GBP"),
];

fn word_character(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn code(token: &str) -> Option<Currency> {
    if token.len() != 3 || !is_currency_marker(token) {
        return None;
    }
    CURRENCY_CODES
        .iter()
        .copied()
        .find(|code| code.eq_ignore_ascii_case(token))
}

pub(super) fn structured(text: &str) -> Option<Currency> {
    let text = text.trim();
    CURRENCY_CODES
        .iter()
        .copied()
        .find(|code| code.eq_ignore_ascii_case(text))
        .or_else(|| exact(text))
}

pub(super) fn leading(text: &str) -> Option<(Currency, usize)> {
    if let Some(code) = text.get(..3).and_then(code) {
        if !text[3..].starts_with(word_character) {
            return Some((code, 3));
        }
    }
    ALIASES.iter().find_map(|&(name, currency)| {
        let length = name.len();
        (text
            .get(..length)
            .is_some_and(|token| token.eq_ignore_ascii_case(name))
            && !(name.ends_with(|c: char| c.is_ascii_alphabetic())
                && text[length..].starts_with(word_character)))
        .then_some((currency, length))
    })
}

pub(super) fn exact(text: &str) -> Option<Currency> {
    code(text).or_else(|| {
        ALIASES
            .iter()
            .find_map(|&(name, currency)| text.eq_ignore_ascii_case(name).then_some(currency))
    })
}

/// Recognize a prefix directly beside the figure or across up to two spaces.
/// A glued FX pair gives the following figure the quote currency.
pub(super) fn before(text: &str) -> Option<(Currency, usize)> {
    let trimmed = text.trim_end_matches([' ', '\u{a0}', '\u{3000}']);
    if text[trimmed.len()..].chars().count() > 2 {
        return None;
    }
    let first = trimmed.ceil_char_boundary(trimmed.len().saturating_sub(24));
    for (index, c) in trimmed[first..].char_indices() {
        let start = first + index;
        if c.is_alphabetic()
            && trimmed[..start].ends_with(|c: char| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        let tail = &trimmed[start..];
        if let Some(currency) = exact(tail) {
            return Some((currency, start));
        }
        if let Some(currency) = tail
            .split_at_checked(3)
            .filter(|(base, _)| code(base).is_some())
            .and_then(|(_, quote)| code(quote))
        {
            return Some((currency, start));
        }
    }
    None
}

/// An ISO or symbol prefix joined to a one-letter printed scale such as USDm
/// or $m. The caller interprets the scale so the same currency tokens apply.
pub(super) fn scaled_prefix(text: &str) -> Option<(Currency, f64)> {
    let (last, suffix) = text.char_indices().next_back()?;
    let scale = match suffix.to_ascii_lowercase() {
        'k' => 1e3,
        'm' => 1e6,
        'b' => 1e9,
        't' => 1e12,
        _ => return None,
    };
    Some((exact(&text[..last])?, scale))
}

/// Look for currency tokens in a unit phrase, allowing them alongside scales
/// and digits. FX conversion excludes shared signs and legacy generic dollar words.
pub(super) fn markers(text: &str, fx_unambiguous: bool) -> Vec<Currency> {
    let mut markers = Vec::new();
    let mut offset = 0;
    let mut end_of_scale = 0;
    while offset < text.len() {
        let tail = &text[offset..];
        let c = tail.chars().next().unwrap();
        let boundary = !text[..offset].ends_with(word_character) || offset == end_of_scale;
        if boundary {
            if let Some((scale, _)) = super::CJK_SCALES
                .iter()
                .find(|(word, _)| tail.starts_with(word))
            {
                end_of_scale = offset + scale.len();
                offset = end_of_scale;
                continue;
            }
            if let Some((currency, length)) = leading(tail).filter(|&(_, length)| {
                !tail[..length].ends_with(char::is_alphabetic)
                    || !tail[length..].starts_with(word_character)
            }) {
                let token = &tail[..length];
                let ambiguous = matches!(currency, "$" | "¥" | "元")
                    || token.eq_ignore_ascii_case("dollar")
                    || token.eq_ignore_ascii_case("dollars")
                    || token == "달러";
                if (!fx_unambiguous || !ambiguous) && !markers.contains(&currency) {
                    markers.push(currency);
                }
                offset += length;
                continue;
            }
            let word_end = tail
                .bytes()
                .take(5)
                .take_while(u8::is_ascii_alphabetic)
                .count();
            if let Some((currency, _)) = scaled_prefix(&tail[..word_end]) {
                if !markers.contains(&currency) {
                    markers.push(currency);
                }
                offset += word_end;
                continue;
            }
        }
        offset += c.len_utf8();
    }
    markers
}

pub(crate) fn agrees(left: Option<Currency>, right: Option<Currency>) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return true;
    };
    let admits = |mark, code| match mark {
        "$" => matches!(code, "USD" | "HKD" | "TWD" | "CAD" | "NZD" | "AUD" | "SGD"),
        "¥" => matches!(code, "JPY" | "CNY" | "CNH" | "元"),
        "元" => matches!(code, "CNY" | "CNH" | "JPY" | "TWD" | "HKD"),
        _ => false,
    };
    left == right || admits(left, right) || admits(right, left)
}

/// An explicit currency determines a shared symbol in the same quantity.
pub(super) fn contextual(local: Option<Currency>, context: Option<Currency>) -> Option<Currency> {
    match local {
        Some("$" | "¥" | "元") if agrees(local, context) => context.or(local),
        _ => local.or(context),
    }
}

/// Uppercase-only reference codes can also be ordinary text such as PHP 8.2.
pub(super) fn can_support_bare(currency: Currency) -> bool {
    is_currency_marker(&currency.to_ascii_lowercase()) || matches!(currency, "$" | "¥" | "元")
}
