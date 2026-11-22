//! Explain a final not_found verdict using only the cited result. This pass
//! never supplies candidates to the verifier.
use super::*;
use crate::domain::answer_checks::ComparisonReason as Reason;
use std::collections::BTreeSet;

#[derive(Clone, Copy)]
enum Location<'a> {
    Excerpt(Option<&'a ChatEvidenceMatch>),
    Content,
    Field(&'a str),
}

struct Suggestion {
    numeric_value: f64,
    reason: Reason,
    display: String,
    unit: Option<String>,
    scale: Option<i16>,
    span: Option<ChatEvidenceMatch>,
    context: Option<NumberCheckContext>,
    pointer: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Similarity {
    SelectedLine,
    OneDigit,
    Transposition,
    SameDigits,
    SameMagnitude,
}

#[derive(Clone, Copy, PartialEq)]
struct Rank {
    evidence: Similarity,
    anchored: bool,
    distance: f64,
}

impl Rank {
    fn cmp(self, other: Self, slack: f64) -> std::cmp::Ordering {
        self.evidence
            .cmp(&other.evidence)
            .then(self.anchored.cmp(&other.anchored))
            .then_with(|| {
                if (self.distance - other.distance).abs() <= slack {
                    std::cmp::Ordering::Equal
                } else {
                    other.distance.total_cmp(&self.distance)
                }
            })
    }
}

struct Search<'a> {
    printed: &'a numbers::Printed,
    python: bool,
    passage: bool,
    best: Option<(Rank, Suggestion)>,
    tied: bool,
}

fn aligned(value: f64, target: f64) -> f64 {
    if value == 0.0 || target == 0.0 {
        return value;
    }
    value * 10f64.powi((target.log10() - value.log10()).round() as i32)
}

// At the answer's displayed precision, count differing digits and recognize
// a single adjacent transposition. Avoid inferring a typo from short counts.
fn digit_similarity(a: f64, b: f64, precision: f64) -> Option<Similarity> {
    let digits = |value: f64| {
        let n = (value / (2.0 * precision)).round();
        (100.0..=9_007_199_254_740_991.0)
            .contains(&n)
            .then(|| (n as u64).to_string())
    };
    let (a, b) = (digits(a)?, digits(b)?);
    if a.len() != b.len() {
        return None;
    }
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut different = (0..a.len()).filter(|&i| a[i] != b[i]);
    let i = different.next()?;
    match different.next() {
        None => Some(Similarity::OneDigit),
        Some(j) if j == i + 1 && a[i] == b[j] && a[j] == b[i] && different.next().is_none() => {
            Some(Similarity::Transposition)
        }
        _ => None,
    }
}

impl Search<'_> {
    fn score(
        &self,
        c: &Candidate,
        measure: Option<&str>,
        anchored: bool,
    ) -> Option<(Rank, Reason)> {
        let p = self.printed;
        let target = p.value.abs();
        let source = c.value.abs();
        if !target.is_finite() || !source.is_finite() {
            return None;
        }
        let shifted = aligned(source, target);
        let near = |v: f64| (v - target).abs() <= p.precision + 4.0 * f64::EPSILON * target;
        let answer_scale = match p.unit {
            Unit::Amount(scale) if scale != 1.0 => Some(scale),
            _ => None,
        };
        let source_scale = match c.unit {
            Unit::Amount(scale) if !c.inferred_scale => Some(scale),
            _ => None,
        };
        let scale_conflict = match (answer_scale, source_scale) {
            (Some(a), Some(b)) => a != b && near(shifted),
            (Some(scale), None) | (None, Some(scale)) if scale != 1.0 => {
                let equal = |value: f64| {
                    (value - target).abs() <= 4.0 * f64::EPSILON * value.abs().max(target)
                };
                equal(source * scale) || equal(source / scale)
            }
            _ => false,
        };
        let mut comparable_source = source;
        let value = match (p.unit, c.unit) {
            (Unit::Amount(_), _) if matches!(measure, Some("shares" | "count" | "index")) => None,
            (Unit::Amount(scale), _) if measure == Some("per_share") && scale != 1.0 => None,
            (Unit::Amount(a), Unit::Amount(b))
                if !numbers::currencies_agree(p.currency, c.currency) =>
            {
                comparable_source = source * b / a;
                None
            }
            (Unit::Amount(a), Unit::Amount(b)) => Some(source * b / a),
            (Unit::Amount(a), Unit::Plain) if a != 1.0 => Some(shifted),
            // A JSON scalar in passage content is still a cell with no stated scale.
            (Unit::Amount(a), Unit::Raw) if self.passage && a != 1.0 => Some(shifted),
            (Unit::Amount(a), Unit::Raw) => Some(source / a),
            (Unit::Percent | Unit::Points, Unit::Raw | Unit::Plain) if self.python => Some(
                if (source - target).abs() <= (100.0 * source - target).abs() {
                    source
                } else {
                    100.0 * source
                },
            ),
            (Unit::Percent, Unit::Raw | Unit::Plain) => Some(100.0 * source),
            (Unit::Plain, Unit::Amount(1.0)) => Some(source),
            (Unit::Plain | Unit::Amount(_) | Unit::Multiple, Unit::Raw | Unit::Plain) => {
                Some(source)
            }
            (Unit::Points, Unit::BasisPoints) => Some(source / 100.0),
            (Unit::BasisPoints, Unit::Points) => Some(source * 100.0),
            (a, b) if a == b => Some(source),
            _ => None,
        };
        let (weight, distance, reason) = if let Some(value) = value {
            let signed = c.value_for(p);
            if signed.is_sign_negative() != p.value.is_sign_negative() && near(value) {
                (
                    Similarity::SameMagnitude,
                    (value - target).abs(),
                    Reason::SignConflict,
                )
            } else if scale_conflict
                && signed.is_sign_negative() == p.value.is_sign_negative()
                && target >= 20.0 * p.precision
                && !near(value)
                && near(shifted)
                && source != 0.0
                && target != 0.0
            {
                (
                    Similarity::SameDigits,
                    (shifted - target).abs(),
                    Reason::ScaleConflict,
                )
            } else if signed.is_sign_negative() == p.value.is_sign_negative() {
                let gap = (value - target).abs();
                let similarity = digit_similarity(value, target, p.precision);
                let rank = match similarity {
                    Some(Similarity::Transposition) => Similarity::Transposition,
                    Some(Similarity::OneDigit) if gap <= 0.02 * target => Similarity::OneDigit,
                    _ if anchored && gap <= 0.15 * target && gap > 0.0 => Similarity::SelectedLine,
                    _ => return None,
                };
                (rank, gap, Reason::DigitsDiffer)
            } else {
                return None;
            }
        } else if near(source)
            || near(comparable_source)
            || (target > 0.0 && source > 0.0 && near(shifted) && target >= 200.0 * p.precision)
        {
            (
                Similarity::SameDigits,
                (source - target)
                    .abs()
                    .min((shifted - target).abs())
                    .min((comparable_source - target).abs()),
                Reason::IncompatibleUnit,
            )
        } else {
            return None;
        };
        Some((
            Rank {
                evidence: weight,
                anchored,
                distance,
            },
            reason,
        ))
    }

    fn offer(&mut self, rank: Rank, suggestion: impl FnOnce() -> Suggestion) {
        if let Some((best, existing)) = &self.best {
            match rank.cmp(
                *best,
                4.0 * f64::EPSILON * self.printed.value.abs().max(1.0),
            ) {
                std::cmp::Ordering::Less => return,
                std::cmp::Ordering::Equal => {
                    let next = suggestion();
                    self.tied |= next.numeric_value != existing.numeric_value
                        || next.reason != existing.reason
                        || (next.unit.is_some()
                            && existing.unit.is_some()
                            && next.unit != existing.unit)
                        || (next.scale.is_some()
                            && existing.scale.is_some()
                            && next.scale != existing.scale);
                    return;
                }
                _ => {}
            }
        }
        self.best = Some((rank, suggestion()));
        self.tied = false;
    }

    fn text(&mut self, text: &str, location: Location<'_>) {
        let anchor = match location {
            Location::Excerpt(Some(span)) if span.is_valid_for(text) => Some((
                text[..span.start].rfind('\n').map_or(0, |i| i + 1),
                text[span.end..]
                    .find('\n')
                    .map_or(text.len(), |i| span.end + i),
            )),
            _ => None,
        };
        let line = text.trim();
        let single_line =
            line.len() <= 240 && !line.contains('\n') && line.starts_with(char::is_alphabetic);
        numbers::diagnostic_tokens(text, |mut candidate, measure, heading| {
            let Place::Span { start, end } = candidate.place else {
                return;
            };
            if end - start > 256
                || (matches!(location, Location::Content)
                    && text[start..end].chars().count() > MAX_NUMBER_CHECK_CONTEXT_CHARS)
            {
                return;
            }
            let labelled_value = single_line
                && text[..start].split_whitespace().count() <= 4
                && !text[..start].contains(|c: char| c.is_ascii_digit())
                && !text[end..].contains(|c: char| c.is_ascii_digit());
            let anchored = labelled_value || anchor.is_some_and(|(a, b)| start >= a && end <= b);
            let price_header = (candidate.unit == Unit::Plain)
                .then(|| numbers::preceding_price_unit(text, start))
                .flatten();
            if price_header.is_some() {
                candidate.unit = Unit::Amount(1.0);
                candidate.currency = price_header.and_then(numbers::stated_currency);
            }
            let Some((rank, reason)) = self.score(&candidate, measure, anchored) else {
                return;
            };
            self.offer(rank, || {
                let raw = &text[start..end];
                let (unit, scale) = source_unit(
                    candidate.unit,
                    candidate.currency,
                    measure,
                    heading.or(price_header),
                );
                Suggestion {
                    numeric_value: candidate.value,
                    reason,
                    display: clip(raw, 256),
                    unit,
                    scale: scale.filter(|_| !candidate.inferred_scale),
                    span: matches!(location, Location::Excerpt(_))
                        .then_some(ChatEvidenceMatch { start, end }),
                    context: if matches!(location, Location::Content) {
                        context_at(text, start, end)
                    } else {
                        None
                    },
                    pointer: if let Location::Field(path) = location {
                        Some(clip(path, 2048))
                    } else {
                        None
                    },
                }
            });
        });
    }

    fn fields(
        &mut self,
        payload: &Value,
        candidates: &[Candidate],
        passage: bool,
        excerpt: Option<&str>,
    ) {
        let mut visited = BTreeSet::new();
        let mut budget = numbers::MAX_TEXT_BYTES;
        for c in candidates {
            let Place::Pointer(path) = &c.place else {
                continue;
            };
            if path.len() > 2048 || !visited.insert(path) {
                continue;
            }
            let Some(field) = payload.pointer(path) else {
                continue;
            };
            if let Some(text) = field
                .as_str()
                .filter(|s| s.len() > 128 || s.trim().replace(',', "").parse::<f64>().is_err())
            {
                if budget == 0 || excerpt == Some(text) {
                    continue;
                }
                let mut end = text.len().min(budget);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                budget -= end;
                self.text(
                    &text[..end],
                    if passage {
                        Location::Content
                    } else {
                        Location::Field(path)
                    },
                );
                continue;
            }
            let row = split_pointer(path).and_then(|(parent, _)| payload.pointer(parent));
            let field_name = path.rsplit('/').next().unwrap_or_default();
            let heading = row
                .and_then(|r| r["unit"].as_str().or_else(|| r["currency"].as_str()))
                .or_else(|| {
                    payload["structuredContent"]["extra"]["results_metadata"][field_name]["units"]
                        .as_str()
                });
            let measure = heading.and_then(|unit| {
                let label = unit.to_ascii_lowercase();
                if label.contains("per share") || label.contains("/share") {
                    Some("per_share")
                } else if label.contains("shares") {
                    Some("shares")
                } else if label == "index" {
                    Some("index")
                } else {
                    None
                }
            });
            let Some((rank, reason)) = self.score(c, measure, false) else {
                continue;
            };
            self.offer(rank, || {
                let raw = printed(field).unwrap_or_default();
                let (unit, scale) = source_unit(c.unit, c.currency, measure, heading);
                Suggestion {
                    numeric_value: c.value,
                    reason,
                    display: clip(&raw, 256),
                    unit,
                    scale,
                    span: None,
                    context: if passage {
                        context_at(&raw, 0, raw.len())
                    } else {
                        None
                    },
                    pointer: (!passage).then(|| clip(path, 2048)),
                }
            });
        }
    }
}

fn source_unit(
    unit: Unit,
    currency: Option<numbers::Currency>,
    measure: Option<&str>,
    heading: Option<&str>,
) -> (Option<String>, Option<i16>) {
    let heading = heading.unwrap_or_default();
    if measure == Some("shares") {
        let thousands = heading
            .to_ascii_lowercase()
            .contains("shares, which are reflected in thousands");
        return (Some("shares".into()), thousands.then_some(3));
    }
    let label = match unit {
        Unit::Percent => Some("%".into()),
        Unit::Points => Some("pp".into()),
        Unit::BasisPoints => Some("bp".into()),
        Unit::Multiple => Some("multiple".into()),
        Unit::Amount(_) => currency.map(str::to_owned),
        _ => None,
    };
    let scale = if let Unit::Amount(scale) = unit {
        let exponent = scale.log10().round();
        (scale.is_finite()
            && scale > 0.0
            && (10f64.powi(exponent as i32) / scale - 1.0).abs() < 1e-10)
            .then_some(exponent as i16)
    } else {
        None
    };
    (measure.map(str::to_owned).or(label), scale)
}

pub(super) fn explain(
    check: &mut NumberCheck,
    printed: &numbers::Printed,
    result: &RunResult,
    evidence: Option<&ChatEvidenceRef>,
    saved_page: Option<&Value>,
    candidates: &[Candidate],
) {
    let mut search = Search {
        printed,
        python: result.producer.tool_name == "run_python",
        passage: check.kind == NumberCheckKind::Passage,
        best: None,
        tied: false,
    };
    let payload = &result.payload;
    if check.kind == NumberCheckKind::Passage {
        let excerpt = evidence.filter(|_| saved_page.is_none());
        if let Some(e) = excerpt {
            search.text(&e.excerpt, Location::Excerpt(e.excerpt_match.as_ref()));
        }
        let payload = saved_page.unwrap_or(payload);
        let cells = numbers::content_candidates(payload, None);
        search.fields(payload, &cells, true, excerpt.map(|e| e.excerpt.as_str()));
    } else if result.producer.tool_name == "run_python" {
        if let Some(stdout) = payload["stdout"].as_str() {
            search.text(stdout, Location::Field("/stdout"));
        }
    } else if result.producer.runtime_id == "openbb"
        && result.producer.tool_name == "economy_fred_series"
    {
        search.fields(
            payload,
            &numbers::fred_observations(payload, None),
            false,
            None,
        );
    } else {
        search.fields(payload, candidates, false, None);
    }
    check.comparison_reason = Some(Reason::NoCandidate);
    if let Some((_, best)) = search.best.filter(|_| !search.tied) {
        check.nearest = Some(best.display.clone());
        check.compared_value = Some(best.display);
        check.nearest_source_unit = best.unit;
        check.nearest_source_scale = best.scale;
        check.comparison_reason = Some(best.reason);
        check.context = best.context;
        check.span = best.span;
        check.pointer = best.pointer;
    }
}
