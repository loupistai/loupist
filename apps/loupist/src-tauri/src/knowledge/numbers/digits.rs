//! How a run of digits and separators reads. `1,234.5`, `1.234,5`,
//! `1 234,5`, `12,34,567` and `3,4` are each one number. A lone `1,234` or
//! `1.234` reads by the text's own convention, or by a scale word after it
//! (`1,25 Mrd.`, `1.234 Mio.`, `1,234억`, `1,234 million`). When neither
//! settles it the default reading (`1,234` = 1234, `1.234` = 1.234) is kept
//! and the run is marked unsure. A run in no shape listed here (`1'234`,
//! `1,234,5`) is unrecognized: the scanner reads no number from it.

/// Which separator marks the decimals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Convention {
    /// `1,234.5`
    Point,
    /// `1.234,5`
    Comma,
}

/// A run's digit groups and the separators between them.
pub(super) struct Run {
    /// Digits in each group, in order.
    pub groups: Vec<String>,
    /// The separator before each group after the first.
    pub separators: Vec<char>,
}

#[derive(Debug, PartialEq)]
pub(super) enum Reading {
    Number {
        value: f64,
        /// Digits after the decimal separator.
        decimals: u32,
        grouped: bool,
        /// Neither the text nor a scale word settled a lone `1,234` or `1.234`.
        unsure: bool,
        /// The shape alone says which convention the text uses.
        shows: Option<Convention>,
    },
    /// Dates and versions such as `2026.03.18`: not a number.
    Skip,
    Unrecognized,
}

/// Grouping separators between digits other than `,` and `.`.
pub(super) const GROUP_SPACES: [char; 4] = [' ', '\u{a0}', '\u{202f}', '\u{2009}'];

fn number(
    integer: &str,
    fraction: &str,
    grouped: bool,
    unsure: bool,
    shows: Option<Convention>,
) -> Reading {
    let text = if fraction.is_empty() {
        integer.to_owned()
    } else {
        format!("{integer}.{fraction}")
    };
    match text.parse::<f64>() {
        Ok(value) => Reading::Number {
            value,
            decimals: fraction.len() as u32,
            grouped,
            unsure,
            shows,
        },
        Err(_) => Reading::Unrecognized,
    }
}

/// Groups after the first form thousands (`1,234,567`) or Indian lakh
/// grouping (`12,34,567`): the last has three digits and the others two or three.
fn thousands(groups: &[String]) -> bool {
    let Some((last, middle)) = groups[1..].split_last() else {
        return false;
    };
    groups[0].len() <= 3
        && last.len() == 3
        && middle.iter().all(|group| matches!(group.len(), 2 | 3))
}

fn strict_thousands(groups: &[String]) -> bool {
    groups[0].len() <= 3 && groups[1..].iter().all(|group| group.len() == 3)
}

/// Read `run` in `settled` convention, if the text or a scale word settled one.
pub(super) fn read(run: &Run, settled: Option<Convention>) -> Reading {
    let groups = &run.groups;
    let separators = &run.separators;
    if separators.is_empty() {
        return number(&groups[0], "", false, false, None);
    }
    if separators.iter().any(|c| matches!(c, '\'' | '’')) {
        return Reading::Unrecognized;
    }
    let (last, init) = separators.split_last().expect("separators");
    let joined = |parts: &[String]| parts.concat();
    let integer = &groups[..groups.len() - 1];
    let fraction = &groups[groups.len() - 1];
    let all = |c: char| separators.iter().all(|s| *s == c);
    if separators.iter().all(|c| GROUP_SPACES.contains(c)) {
        return if strict_thousands(groups) {
            number(&joined(groups), "", true, false, None)
        } else {
            Reading::Unrecognized
        };
    }
    if all(',') || all('.') {
        let comma = *last == ',';
        if separators.len() == 1 {
            // `0.125` and `0,125`: a leading zero never starts thousands.
            let lone = groups[0].len() <= 3 && groups[0] != "0" && fraction.len() == 3;
            if !lone {
                // `3,4`, `12,34`, `2.5`, `1234.567`: only a decimal separator.
                let shows = Some(if comma {
                    Convention::Comma
                } else {
                    Convention::Point
                });
                return number(&groups[0], fraction, false, false, shows);
            }
            let decimal_comma = settled == Some(Convention::Comma);
            let unsure = settled.is_none();
            return if comma == decimal_comma {
                number(&groups[0], fraction, false, unsure, None)
            } else {
                number(&joined(groups), "", true, unsure, None)
            };
        }
        if comma && thousands(groups) {
            return number(
                &joined(groups),
                "",
                true,
                false,
                strict_thousands(groups).then_some(Convention::Point),
            );
        }
        if !comma && strict_thousands(groups) {
            return number(&joined(groups), "", true, false, Some(Convention::Comma));
        }
        return if comma {
            Reading::Unrecognized
        } else {
            Reading::Skip
        };
    }
    // One decimal separator after grouping by the other (`1,234.5`, `1.234,5`)
    // or by spaces (`1 234,5`).
    let group = init[0];
    if *last == group || !init.iter().all(|c| *c == group) {
        return Reading::Unrecognized;
    }
    let shape_fits = match group {
        ',' => *last == '.' && thousands(integer),
        '.' => *last == ',' && strict_thousands(integer),
        _ => *last == ',' && GROUP_SPACES.contains(&group) && strict_thousands(integer),
    };
    if !shape_fits {
        return Reading::Unrecognized;
    }
    let shows = Some(if *last == ',' {
        Convention::Comma
    } else {
        Convention::Point
    });
    number(&joined(integer), fraction, true, false, shows)
}

/// The convention a scale word printed after a number implies: German and
/// Portuguese scale words write decimals with a comma; English, Indian and
/// CJK scale words and pence with a point.
/// The convention a prefix before a lone `1.234` states: a thousands
/// prefix (`TEUR`) belongs to statements that print a decimal comma.
pub(super) fn prefix_cue(before: &str) -> Option<Convention> {
    let token = before
        .trim_end_matches(super::UNIT_SPACES)
        .rsplit(|c: char| !c.is_ascii_alphabetic())
        .next()?;
    super::currency::thousands_prefix(token).map(|_| Convention::Comma)
}

pub(super) fn scale_cue(rest: &str) -> Option<Convention> {
    let tail = rest.trim_start_matches(super::UNIT_SPACES);
    if super::CJK_SCALES
        .iter()
        .any(|(word, _)| tail.starts_with(word))
    {
        return Some(Convention::Point);
    }
    let word: String = tail
        .chars()
        .take_while(|c| c.is_alphabetic())
        .collect::<String>()
        .to_lowercase();
    if super::comma_scale(&word).is_some() {
        return Some(Convention::Comma);
    }
    if super::english_scale(&word).is_some() || matches!(word.as_str(), "bn" | "pence" | "p") {
        return Some(Convention::Point);
    }
    None
}
