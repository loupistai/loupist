//! Display text for calculation results. Values keep 15 significant digits;
//! display text is rounded for reading and never fed back into arithmetic.

/// Remove binary-representation noise (0.1 + 0.2) without truncating
/// monetary amounts: binary64 round-trips 15 significant decimal digits.
pub(super) fn round15(value: f64) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return if value == 0.0 { 0.0 } else { value };
    }
    format!("{value:.14e}").parse().unwrap_or(value)
}

/// Amount or plain number: grouped thousands, two decimals from 1,000, four
/// from 1, otherwise six significant digits. Negative values use ASCII `-`
/// so they stay distinct from the `−` operator in formulas.
pub(super) fn amount(value: f64) -> String {
    let value = round15(value);
    let magnitude = value.abs();
    let digits = if magnitude >= 1_000.0 {
        2
    } else if magnitude >= 1.0 {
        4
    } else if magnitude == 0.0 {
        0
    } else {
        (5 - magnitude.log10().floor() as i32).clamp(0, 15) as usize
    };
    signed(value < 0.0, &group(&trim(format!("{magnitude:.digits$}"))))
}

/// A percent quantity already expressed in percent units (12.5 → "12.50%").
pub(super) fn percent(value: f64) -> String {
    format!("{}%", fixed(value, 2))
}

/// A multiple such as leverage or a valuation ratio (2.5 → "2.50x").
pub(super) fn multiple(value: f64) -> String {
    format!("{}x", fixed(value, 2))
}

/// A percent input as the caller wrote it (8 → "8%", 2.5 → "2.5%").
pub(super) fn rate(value: f64) -> String {
    format!("{}%", amount(value))
}

fn fixed(value: f64, digits: usize) -> String {
    let value = round15(value);
    let text = format!("{:.digits$}", value.abs());
    let negative = value < 0.0 && text.chars().any(|c| c.is_ascii_digit() && c != '0');
    signed(negative, &group(&text))
}

fn signed(negative: bool, text: &str) -> String {
    if negative {
        format!("-{text}")
    } else {
        text.to_owned()
    }
}

fn trim(text: String) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

fn group(text: &str) -> String {
    let (integer, fraction) = text
        .split_once('.')
        .map_or((text, None), |(i, f)| (i, Some(f)));
    let mut grouped = String::with_capacity(text.len() + integer.len() / 3);
    for (index, digit) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if let Some(fraction) = fraction {
        grouped.push('.');
        grouped.push_str(fraction);
    }
    grouped
}
