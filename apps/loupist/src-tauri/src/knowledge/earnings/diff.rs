//! Differences between what was expected and what was reported, computed by
//! Rust, never by the model (finance-earnings-snapshot.md §3.3 rule 4). Every
//! number is a calculation with its formula; words come first and there is no
//! beat/miss label.
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::Unit;
use crate::datasets::DatasetRef;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CalcInput {
    pub name: String,
    pub display: String,
}

/// One calculation behind a number on the card.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CalcView {
    pub title: String,
    pub formulas: Vec<String>,
    pub inputs: Vec<CalcInput>,
    pub display: String,
    pub value: f64,
    pub unit: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Position {
    Above,
    Within,
    Below,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GuidanceChange {
    New,
    Raised,
    Lowered,
    Kept,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum NotComparable {
    Basis,
    Period,
    Unit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DifferenceView {
    Range {
        position: Position,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        calc: Option<CalcView>,
    },
    Change {
        calc: CalcView,
    },
    Guidance {
        change: GuidanceChange,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        calc: Option<CalcView>,
    },
    NotComparable {
        reason: NotComparable,
    },
}

/// What a difference needs from a stored value.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparable<'a> {
    pub measure: &'a str,
    pub basis: &'a str,
    pub period: &'a str,
    pub unit: Unit,
    pub currency: Option<&'a str>,
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub point: Option<f64>,
}

impl Comparable<'_> {
    /// A point, or the middle of a range.
    pub fn middle(&self) -> Option<f64> {
        self.point.or(match (self.low, self.high) {
            (Some(low), Some(high)) => Some((low + high) / 2.0),
            _ => None,
        })
    }
}

struct NoDatasets;

impl crate::calculations::DatasetSource for NoDatasets {
    fn dataset(&mut self, _: &str, _: u32) -> Result<(DatasetRef, Arc<Value>), String> {
        Err("this calculation reads no dataset".into())
    }
}

/// Plain text for a formula: up to four decimals, no noise.
pub fn plain(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.').to_owned();
    if text == "-0" {
        "0".into()
    } else {
        text.replace('-', "−")
    }
}

/// "+3.3%", "−1.2 pp": one decimal and a real minus sign.
pub fn signed(value: f64, unit: &str) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    let sign = if rounded > 0.0 {
        "+"
    } else if rounded < 0.0 {
        "−"
    } else {
        ""
    };
    let space = if unit == "pp" { " " } else { "" };
    format!("{sign}{:.1}{space}{unit}", rounded.abs())
}

/// `percent_change` through the host calculation surface (positive base), else
/// the plain difference.
pub fn percent_change(from: f64, to: f64, from_label: &str, to_label: &str) -> CalcView {
    let request = json!({
        "operation": "percent_change",
        "inputs": {"from": from, "to": to},
        "labels": {"from": from_label, "to": to_label},
    });
    if let Ok(result) = crate::calculations::calculate(&request, &mut NoDatasets) {
        let value = result["values"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|item| item["name"] == "percent_change")
            .and_then(|item| item["value"].as_f64());
        if let Some(value) = value {
            return CalcView {
                title: result["title"].as_str().unwrap_or("Percent change").into(),
                formulas: result["formulas"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
                inputs: result["inputs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|input| CalcInput {
                        name: match input["name"].as_str() {
                            Some("from") => from_label.into(),
                            Some("to") => to_label.into(),
                            other => other.unwrap_or_default().into(),
                        },
                        display: input["display"].as_str().unwrap_or_default().into(),
                    })
                    .collect(),
                display: signed(value, "%"),
                value,
                unit: "%".into(),
            };
        }
    }
    let change = to - from;
    CalcView {
        title: "Difference".into(),
        formulas: vec![format!(
            "{to_label} − {from_label} = {} − {} = {}",
            plain(to),
            plain(from),
            plain(change)
        )],
        inputs: vec![
            CalcInput {
                name: from_label.into(),
                display: plain(from),
            },
            CalcInput {
                name: to_label.into(),
                display: plain(to),
            },
        ],
        display: plain(change),
        value: change,
        unit: String::new(),
    }
}

/// The difference of two percentages in percentage points.
pub fn points(from: f64, to: f64, from_label: &str, to_label: &str) -> CalcView {
    let value = to - from;
    CalcView {
        title: "Difference in percentage points".into(),
        formulas: vec![format!(
            "{to_label} − {from_label} = {}% − {}% = {} pp",
            plain(to),
            plain(from),
            plain(value)
        )],
        inputs: vec![
            CalcInput {
                name: from_label.into(),
                display: format!("{}%", plain(from)),
            },
            CalcInput {
                name: to_label.into(),
                display: format!("{}%", plain(to)),
            },
        ],
        display: signed(value, "pp"),
        value,
        unit: "pp".into(),
    }
}

fn change(unit: Unit, from: f64, to: f64, from_label: &str, to_label: &str) -> CalcView {
    if unit == Unit::Percent {
        points(from, to, from_label, to_label)
    } else {
        percent_change(from, to, from_label, to_label)
    }
}

/// Same measure, basis, period, unit and currency, or the reason they differ.
pub fn comparable(a: &Comparable<'_>, b: &Comparable<'_>) -> Result<(), NotComparable> {
    if !a.basis.is_empty() && !b.basis.is_empty() && !a.basis.eq_ignore_ascii_case(b.basis) {
        return Err(NotComparable::Basis);
    }
    if a.period != b.period {
        return Err(NotComparable::Period);
    }
    let currency = |value: Option<&str>| value.map(str::to_ascii_uppercase);
    if a.unit != b.unit
        || (a.currency.is_some()
            && b.currency.is_some()
            && currency(a.currency) != currency(b.currency))
    {
        return Err(NotComparable::Unit);
    }
    Ok(())
}

/// What was reported against one expected value.
pub fn difference(expected: &Comparable<'_>, reported: &Comparable<'_>) -> DifferenceView {
    if let Err(reason) = comparable(expected, reported) {
        return DifferenceView::NotComparable { reason };
    }
    let Some(actual) = reported.middle() else {
        return DifferenceView::NotComparable {
            reason: NotComparable::Unit,
        };
    };
    match (expected.low, expected.high, expected.point) {
        (Some(low), Some(high), _) => {
            if actual > high {
                DifferenceView::Range {
                    position: Position::Above,
                    calc: Some(change(
                        expected.unit,
                        high,
                        actual,
                        "Top of the range",
                        "Reported",
                    )),
                }
            } else if actual < low {
                DifferenceView::Range {
                    position: Position::Below,
                    calc: Some(change(
                        expected.unit,
                        low,
                        actual,
                        "Bottom of the range",
                        "Reported",
                    )),
                }
            } else {
                DifferenceView::Range {
                    position: Position::Within,
                    calc: None,
                }
            }
        }
        (_, _, Some(point)) => DifferenceView::Change {
            calc: change(expected.unit, point, actual, "Expected", "Reported"),
        },
        _ => DifferenceView::NotComparable {
            reason: NotComparable::Unit,
        },
    }
}

/// New guidance against earlier guidance for the same period, by midpoint.
pub fn guidance(earlier: Option<&Comparable<'_>>, new: &Comparable<'_>) -> DifferenceView {
    let Some(earlier) = earlier else {
        return DifferenceView::Guidance {
            change: GuidanceChange::New,
            calc: None,
        };
    };
    if let Err(reason) = comparable(earlier, new) {
        return DifferenceView::NotComparable { reason };
    }
    let (Some(before), Some(after)) = (earlier.middle(), new.middle()) else {
        return DifferenceView::Guidance {
            change: GuidanceChange::New,
            calc: None,
        };
    };
    let same = (after - before).abs() <= 1e-9 * before.abs().max(after.abs()).max(1.0);
    if same {
        return DifferenceView::Guidance {
            change: GuidanceChange::Kept,
            calc: None,
        };
    }
    DifferenceView::Guidance {
        change: if after > before {
            GuidanceChange::Raised
        } else {
            GuidanceChange::Lowered
        },
        calc: Some(change(
            new.unit,
            before,
            after,
            "Earlier guidance (midpoint)",
            "New guidance (midpoint)",
        )),
    }
}

/// Half a unit of the last printed digit of a decimal string, times its scale.
pub fn half_unit(printed: Option<&str>, scale: f64) -> f64 {
    let decimals = printed
        .and_then(|text| text.split_once('.'))
        .map_or(0, |(_, fraction)| fraction.trim().len()) as i32;
    0.5 * 10f64.powi(-decimals) * scale.max(f64::MIN_POSITIVE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(low: Option<f64>, high: Option<f64>, point: Option<f64>) -> Comparable<'static> {
        Comparable {
            measure: "revenue",
            basis: "",
            period: "Q3 2026",
            unit: Unit::Currency,
            currency: Some("USD"),
            low,
            high,
            point,
        }
    }

    #[test]
    fn ranges_say_above_within_or_below_with_the_gap_to_the_edge() {
        let reported = value(None, None, Some(1.27e9));
        let range = value(Some(1.20e9), Some(1.25e9), None);
        let DifferenceView::Range { position, calc } = difference(&range, &reported) else {
            panic!("a range difference");
        };
        assert_eq!(position, Position::Above);
        let calc = calc.unwrap();
        assert_eq!(calc.display, "+1.6%");
        assert!(calc
            .formulas
            .iter()
            .any(|line| line.contains("Top of the range")));
        let below = difference(&range, &value(None, None, Some(1.1e9)));
        assert!(matches!(
            below,
            DifferenceView::Range {
                position: Position::Below,
                calc: Some(_)
            }
        ));
        let within = difference(&range, &value(None, None, Some(1.22e9)));
        assert_eq!(
            within,
            DifferenceView::Range {
                position: Position::Within,
                calc: None
            }
        );
        let point = difference(&value(None, None, Some(1.23e9)), &reported);
        let DifferenceView::Change { calc } = point else {
            panic!("a point difference");
        };
        assert_eq!(calc.display, "+3.3%");
    }

    #[test]
    fn percentages_differ_in_points_and_other_bases_are_not_compared() {
        let margin = |point: f64, basis: &'static str| Comparable {
            measure: "operatingMargin",
            basis,
            period: "Q3 2026",
            unit: Unit::Percent,
            currency: None,
            low: None,
            high: None,
            point: Some(point),
        };
        let DifferenceView::Change { calc } = difference(&margin(16.0, ""), &margin(14.8, ""))
        else {
            panic!("a points difference");
        };
        assert_eq!(
            (calc.display.as_str(), calc.unit.as_str()),
            ("−1.2 pp", "pp")
        );
        assert_eq!(
            difference(&margin(16.0, "GAAP"), &margin(14.8, "adjusted")),
            DifferenceView::NotComparable {
                reason: NotComparable::Basis
            }
        );
        let mut later = margin(14.8, "");
        later.period = "Q4 2026";
        assert_eq!(
            difference(&margin(16.0, ""), &later),
            DifferenceView::NotComparable {
                reason: NotComparable::Period
            }
        );
        let mut won = value(None, None, Some(1.27e9));
        won.currency = Some("KRW");
        assert_eq!(
            difference(&value(None, None, Some(1.23e9)), &won),
            DifferenceView::NotComparable {
                reason: NotComparable::Unit
            }
        );
    }

    #[test]
    fn guidance_is_new_raised_lowered_or_kept_by_midpoint() {
        let next = value(Some(1.30e9), Some(1.35e9), None);
        assert!(matches!(
            guidance(None, &next),
            DifferenceView::Guidance {
                change: GuidanceChange::New,
                ..
            }
        ));
        let earlier = value(Some(1.25e9), Some(1.30e9), None);
        assert!(matches!(
            guidance(Some(&earlier), &next),
            DifferenceView::Guidance {
                change: GuidanceChange::Raised,
                calc: Some(_)
            }
        ));
        assert!(matches!(
            guidance(Some(&next), &earlier),
            DifferenceView::Guidance {
                change: GuidanceChange::Lowered,
                ..
            }
        ));
        assert!(matches!(
            guidance(Some(&next), &next.clone()),
            DifferenceView::Guidance {
                change: GuidanceChange::Kept,
                calc: None
            }
        ));
        assert_eq!(half_unit(Some("14.8"), 1.0), 0.05);
        assert_eq!(half_unit(Some("1.27"), 1e9), 5e6);
        assert_eq!(half_unit(None, 1.0), 0.5);
    }
}
