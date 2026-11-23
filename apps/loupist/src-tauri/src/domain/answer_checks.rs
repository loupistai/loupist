//! What Rust checked about a finished answer: each number link against the
//! evidence it names (finance-show-the-math.md §7.2) and, for an as-of answer,
//! what the cutoff left out (finance-point-in-time.md §7). Both are written at
//! finalization; the renderer only reads them and model text cannot set them.
use serde::{Deserialize, Serialize};

use super::{ChatEvidenceMatch, DomainError};

/// Checked number links per message; later links render as numbered citations.
pub const MAX_NUMBER_CHECKS: usize = 200;
const MAX_CHECK_TEXT_BYTES: usize = 256;
pub const MAX_NUMBER_CHECK_CONTEXT_CHARS: usize = 240;
const MAX_FORMULA_LINES: usize = 8;
const MAX_CALC_INPUTS: usize = 12;
const MAX_ROW_FIELDS: usize = 4;
/// Items each as-of list names; the note shows "+n more" past four.
pub const MAX_AS_OF_ITEMS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum NumberCheckKind {
    /// A `calculate` result.
    Calculated,
    /// A retained passage (web page, filing text, saved Source).
    Passage,
    /// A structured data row (OpenBB, DART, KRX, KIS, World Bank).
    Data,
    /// A dataset query or another step's output in this answer.
    Step,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum NumberCheckStatus {
    Matched,
    Rounded,
    NotFound,
    Differs,
    /// The link names no result this reply retained, so nothing could be checked.
    Unlinked,
}

/// Why a numeric citation did not verify against its retained evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ComparisonReason {
    DigitsDiffer,
    ScaleConflict,
    SignConflict,
    IncompatibleUnit,
    NoCandidate,
}

/// Read-time diagnostic for an unresolved check. The saved comparison
/// remains compatible with readers that know the earlier reason values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum ComparisonReplay {
    NoCandidate { searched: ComparisonScope },
    EvidenceUnavailable,
}

/// Material that the host searched while replaying a saved comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComparisonScope {
    RetainedPassage { title: String },
}

/// One field of a matched data row, as the provider printed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NumberCheckField {
    pub key: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub matched: bool,
}

/// What a `calculate` input measures, as the host printed it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum CalcInputUnitKind {
    /// A count or a level: years, shares, or a unit kept as `text`.
    #[default]
    Number,
    Percent,
    /// Percentage points.
    Points,
    BasisPoints,
    Multiple,
    /// An amount in `currency`, stated in units of 10^`scale`.
    Amount,
    /// A per-share price in `currency`.
    Price,
}

/// How a `calculate` input is printed: Rust reads its kind from the input's
/// name and the request's `unit` (finance-show-the-math.md §2.3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CalcInputUnit {
    pub kind: CalcInputUnitKind,
    /// ISO 4217 code the request's `unit` names ("KRW").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub currency: Option<String>,
    /// Power of ten the amount is stated in: 9 for "USD billions", 8 for 억원.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub scale: u8,
    /// A unit the host cannot place ("stores"), printed after the number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub text: Option<String>,
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

impl CalcInputUnit {
    pub fn of(kind: CalcInputUnitKind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    fn valid(&self) -> bool {
        self.scale <= 18
            && self
                .currency
                .as_deref()
                .is_none_or(|code| code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()))
            && self.text.as_deref().is_none_or(|text| bounded(text, 128))
    }
}

/// One `calculate` input and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NumberCheckInput {
    pub name: String,
    /// The name its formula prints ("Operating income", "from").
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub unit: CalcInputUnit,
    pub display: String,
    /// The evidence the input was cited from or read out of; absent when the
    /// model provided the value without a source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub evidence_id: Option<String>,
    /// Rust's check of a cited input against its evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub status: Option<NumberCheckStatus>,
    /// Whether a conversion uses the amount currency declared in the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub currency_basis: Option<String>,
}

/// What the calculated popover shows, copied from the host's own result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NumberCheckCalc {
    /// The `calculate` operation; missing on answers saved before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub operation: Option<crate::calculations::Operation>,
    pub title: String,
    pub formulas: Vec<String>,
    pub inputs: Vec<NumberCheckInput>,
    /// Rows × columns of a DCF or reverse-DCF grid, which has no single value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub grid: Option<[u32; 2]>,
}

/// A bounded part of retained content containing the matched or compared figure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NumberCheckContext {
    pub text: String,
    /// End-exclusive UTF-8 byte offsets of that number inside `text`.
    pub start: usize,
    pub end: usize,
}

/// Powers of ten in an exact conversion from the source to the answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NumberCheckScaleConversion {
    pub source_scale: i16,
    pub answer_scale: i16,
    /// Currency from the evidence, or from the answer's stated unit when
    /// the retained source window has only the base-unit values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub currency: Option<String>,
}

/// The side that leaves the scale of an exact digit match unstated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum UnstatedScale {
    Source,
    Answer,
}

/// How the source signs a matched amount that the answer states unsigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SourceSign {
    /// Negative or in brackets, in a row or field that is spending or a
    /// deduction by nature.
    Outflow,
}

/// One number link in an answer, checked against the evidence it links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NumberCheck {
    /// Index among the answer's `[text](#evidence-…)` links, in order.
    pub link_index: u32,
    pub text: String,
    pub evidence_id: String,
    /// The evidence id the answer wrote, when the host rebound a link that
    /// named no retained result to the one this reply retained near it.
    #[serde(
        rename = "repairedFrom",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(test, ts(optional))]
    pub repaired_from: Option<String>,
    pub kind: NumberCheckKind,
    pub status: NumberCheckStatus,
    /// Nearest calculation result or plausible source figure for an unmatched citation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub nearest: Option<String>,
    /// The unmatched candidate exactly as printed in the evidence.
    #[serde(
        rename = "comparedValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(test, ts(optional))]
    pub compared_value: Option<String>,
    /// Unit of the unmatched candidate when the source identifies it.
    #[serde(
        rename = "nearestSourceUnit",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(test, ts(optional))]
    pub nearest_source_unit: Option<String>,
    /// Power of ten of the stated source scale (6 for millions).
    #[serde(
        rename = "nearestSourceScale",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(test, ts(optional))]
    pub nearest_source_scale: Option<i16>,
    /// Why a numeric citation did not verify, including on historical replay.
    #[serde(
        rename = "comparisonReason",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(test, ts(optional))]
    pub comparison_reason: Option<ComparisonReason>,
    #[serde(
        rename = "comparisonReplay",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(test, ts(optional))]
    pub comparison_replay: Option<ComparisonReplay>,
    /// For a rounded or rescaled match: the value as the evidence prints it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub exact: Option<String>,
    /// Present on a matched citation that changes monetary scale exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub scale_conversion: Option<NumberCheckScaleConversion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub unstated_scale: Option<UnstatedScale>,
    /// Present on a match the source prints as an outflow.
    #[serde(
        rename = "sourceSign",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(test, ts(optional))]
    pub source_sign: Option<SourceSign>,
    /// The matched or compared cell (JSON Pointer into the retained result).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub pointer: Option<String>,
    /// The matched or compared figure (UTF-8 byte offsets into the excerpt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub span: Option<ChatEvidenceMatch>,
    /// The location in retained content when the excerpt has no span.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub context: Option<NumberCheckContext>,
    /// Up to four fields of the matched data row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<NumberCheckField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub calc: Option<NumberCheckCalc>,
}

/// A read a tool refused because it cannot show what was known on the date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AsOfRefusal {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub connection: Option<String>,
}

/// A read that had nothing dated on or before the cutoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AsOfEmpty {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<String>,
}

/// Saved Memory the cutoff left out, by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AsOfLeftOut {
    pub facts: u32,
    pub sources: u32,
    pub notes: u32,
    pub methods: u32,
}

/// What an as-of answer could not use: refusals, empty reads and later Memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AsOfReport {
    pub cutoff: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refused: Vec<AsOfRefusal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub empty: Vec<AsOfEmpty>,
    #[serde(default)]
    pub left_out: AsOfLeftOut,
}

fn bounded(text: &str, maximum: usize) -> bool {
    text.len() <= maximum && !text.contains('\0')
}

/// A calendar date (`YYYY-MM-DD`).
pub fn valid_as_of(value: &str) -> bool {
    value.len() == 10 && chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
}

pub(crate) fn validate_number_checks(checks: &[NumberCheck]) -> Result<(), DomainError> {
    let invalid = checks.len() > MAX_NUMBER_CHECKS
        || checks.iter().any(|check| {
            !bounded(&check.text, MAX_CHECK_TEXT_BYTES)
                || check.text.trim().is_empty()
                || !bounded(&check.evidence_id, 128)
                || check
                    .repaired_from
                    .as_ref()
                    .is_some_and(|written| !bounded(written, 128))
                || [
                    &check.nearest,
                    &check.exact,
                    &check.compared_value,
                    &check.nearest_source_unit,
                ]
                .into_iter()
                .flatten()
                .any(|text| !bounded(text, MAX_CHECK_TEXT_BYTES))
                || check
                    .comparison_replay
                    .as_ref()
                    .is_some_and(|scope| match scope {
                        ComparisonReplay::NoCandidate {
                            searched: ComparisonScope::RetainedPassage { title },
                        } => !bounded(title, 512),
                        ComparisonReplay::EvidenceUnavailable => false,
                    })
                || check
                    .pointer
                    .as_ref()
                    .is_some_and(|pointer| !bounded(pointer, 2048))
                || check
                    .span
                    .as_ref()
                    .is_some_and(|span| span.start >= span.end)
                || (check.unstated_scale.is_some()
                    && (check.scale_conversion.is_some()
                        || check.status != NumberCheckStatus::Matched))
                || check.scale_conversion.as_ref().is_some_and(|conversion| {
                    check.status != NumberCheckStatus::Matched
                        || check.exact.is_none()
                        || conversion.source_scale == conversion.answer_scale
                        || !(0..=18).contains(&conversion.source_scale)
                        || !(0..=18).contains(&conversion.answer_scale)
                        || conversion
                            .currency
                            .as_ref()
                            .is_some_and(|currency| !bounded(currency, 8))
                })
                || check.context.as_ref().is_some_and(|context| {
                    context.text.chars().count() > MAX_NUMBER_CHECK_CONTEXT_CHARS
                        || context.start >= context.end
                        || context.text.get(context.start..context.end).is_none()
                })
                || check.fields.len() > MAX_ROW_FIELDS
                || check.fields.iter().any(|field| {
                    !bounded(&field.key, 128) || !bounded(&field.value, MAX_CHECK_TEXT_BYTES)
                })
                || check.calc.as_ref().is_some_and(|calc| {
                    !bounded(&calc.title, 256)
                        || calc.formulas.len() > MAX_FORMULA_LINES
                        || calc.formulas.iter().any(|line| !bounded(line, 2048))
                        || calc.inputs.len() > MAX_CALC_INPUTS
                        || calc.inputs.iter().any(|input| {
                            !bounded(&input.name, 128)
                                || !bounded(&input.label, 128)
                                || !bounded(&input.display, 128)
                                || !input.unit.valid()
                                || input
                                    .currency_basis
                                    .as_deref()
                                    .is_some_and(|basis| !bounded(basis, 128))
                        })
                })
        });
    if invalid {
        return Err(DomainError::Invalid("number checks exceed their bounds"));
    }
    Ok(())
}

pub(super) fn validate_as_of_report(report: &AsOfReport) -> Result<(), DomainError> {
    if !valid_as_of(&report.cutoff)
        || report.refused.len() > MAX_AS_OF_ITEMS
        || report.empty.len() > MAX_AS_OF_ITEMS
        || report.refused.iter().any(|item| {
            !bounded(&item.tool, 128)
                || item
                    .connection
                    .as_ref()
                    .is_some_and(|name| !bounded(name, 256))
        })
        || report.empty.iter().any(|item| {
            !bounded(&item.tool, 128)
                || item
                    .target
                    .as_ref()
                    .is_some_and(|target| !bounded(target, 256))
        })
    {
        return Err(DomainError::Invalid("as-of report exceeds its bounds"));
    }
    Ok(())
}

/// Formula lines and inputs shown in the popover, bounded.
pub fn bounded_calc(mut calc: NumberCheckCalc) -> NumberCheckCalc {
    calc.title = clip(&calc.title, 256);
    calc.formulas.truncate(MAX_FORMULA_LINES);
    for line in &mut calc.formulas {
        *line = clip(line, 2048);
    }
    calc.inputs.truncate(MAX_CALC_INPUTS);
    for input in &mut calc.inputs {
        input.name = clip(&input.name, 128);
        input.label = clip(&input.label, 128);
        input.display = clip(&input.display, 128);
        if let Some(basis) = &mut input.currency_basis {
            *basis = clip(basis, 128);
        }
        if let Some(text) = &mut input.unit.text {
            *text = clip(text, 128);
        }
        if !input.unit.valid() {
            input.unit = CalcInputUnit::default();
        }
    }
    calc
}

/// Row fields shown in the data popover, bounded.
pub fn bounded_fields(mut fields: Vec<NumberCheckField>) -> Vec<NumberCheckField> {
    fields.truncate(MAX_ROW_FIELDS);
    for field in &mut fields {
        field.key = clip(&field.key, 128);
        field.value = clip(&field.value, MAX_CHECK_TEXT_BYTES);
    }
    fields
}

/// `text` cut at a character boundary to at most `maximum` bytes.
pub fn clip(text: &str, maximum: usize) -> String {
    let mut end = text.len().min(maximum);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].replace('\0', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_check_context_bounds_characters_and_utf8_byte_offsets() {
        let mut check: NumberCheck = serde_json::from_value(serde_json::json!({
            "link_index":0, "text":"4,523", "evidence_id":"ev_page", "kind":"passage", "status":"matched"
        })).unwrap();
        assert!(check.context.is_none());
        let valid = NumberCheckContext {
            text: "한".repeat(240),
            start: 3,
            end: 6,
        };
        check.context = Some(valid.clone());
        assert!(validate_number_checks(std::slice::from_ref(&check)).is_ok());
        for context in [
            NumberCheckContext {
                text: "a".repeat(241),
                start: 0,
                end: 1,
            },
            NumberCheckContext {
                text: "한".repeat(241),
                start: 0,
                end: 3,
            },
            NumberCheckContext {
                start: 1,
                ..valid.clone()
            },
            NumberCheckContext {
                end: 5,
                ..valid.clone()
            },
            NumberCheckContext {
                start: 6,
                end: 3,
                ..valid.clone()
            },
            NumberCheckContext {
                start: 3,
                end: 3,
                ..valid.clone()
            },
            NumberCheckContext { end: 721, ..valid },
            NumberCheckContext {
                text: String::new(),
                start: 0,
                end: 0,
            },
        ] {
            check.context = Some(context);
            assert!(
                validate_number_checks(std::slice::from_ref(&check)).is_err(),
                "{:?}",
                check.context
            );
        }
    }
}
