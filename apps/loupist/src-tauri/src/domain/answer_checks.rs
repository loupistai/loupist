//! What Rust checked about a finished answer: each number link against the
//! evidence it names (finance-show-the-math.md §7.2) and, for an as-of answer,
//! what the cutoff left out (finance-point-in-time.md §7). Both are written at
//! finalization; the renderer only reads them and model text cannot set them.
use serde::{Deserialize, Serialize};

use super::{ChatEvidenceMatch, DomainError};

/// Checked number links per message; later links render as numbered citations.
pub const MAX_NUMBER_CHECKS: usize = 200;
const MAX_CHECK_TEXT_BYTES: usize = 256;
const MAX_FORMULA_LINES: usize = 8;
const MAX_CALC_INPUTS: usize = 12;
const MAX_ROW_FIELDS: usize = 4;
/// Items each as-of list names; the note shows "+n more" past four.
pub const MAX_AS_OF_ITEMS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[serde(rename_all = "snake_case")]
pub enum NumberCheckStatus {
    Matched,
    Rounded,
    NotFound,
    Differs,
}

/// One field of a matched data row, as the provider printed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumberCheckField {
    pub key: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub matched: bool,
}

/// What a `calculate` input measures, as the host printed it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
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
#[serde(deny_unknown_fields)]
pub struct CalcInputUnit {
    pub kind: CalcInputUnitKind,
    /// ISO 4217 code the request's `unit` names ("KRW").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Power of ten the amount is stated in: 9 for "USD billions", 8 for 억원.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub scale: u8,
    /// A unit the host cannot place ("stores"), printed after the number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
#[serde(deny_unknown_fields)]
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
    pub evidence_id: Option<String>,
    /// Rust's check of a cited input against its evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<NumberCheckStatus>,
}

/// What the calculated popover shows, copied from the host's own result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumberCheckCalc {
    pub title: String,
    pub formulas: Vec<String>,
    pub inputs: Vec<NumberCheckInput>,
    /// Rows × columns of a DCF or reverse-DCF grid, which has no single value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<[u32; 2]>,
}

/// One number link in an answer, checked against the evidence it links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumberCheck {
    /// Index among the answer's `[text](#evidence-…)` links, in order.
    pub link_index: u32,
    pub text: String,
    pub evidence_id: String,
    pub kind: NumberCheckKind,
    pub status: NumberCheckStatus,
    /// For `differs`: the calculated value nearest to the printed number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nearest: Option<String>,
    /// For `rounded`: the value as the evidence prints it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact: Option<String>,
    /// The matched data cell (JSON Pointer into the retained result).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// The matched passage text (UTF-8 byte offsets into the evidence excerpt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<ChatEvidenceMatch>,
    /// Up to four fields of the matched data row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<NumberCheckField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calc: Option<NumberCheckCalc>,
}

/// A read a tool refused because it cannot show what was known on the date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsOfRefusal {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
}

/// A read that had nothing dated on or before the cutoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsOfEmpty {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Saved Memory the cutoff left out, by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsOfLeftOut {
    pub facts: u32,
    pub sources: u32,
    pub notes: u32,
    pub methods: u32,
}

/// What an as-of answer could not use: refusals, empty reads and later Memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsOfReport {
    pub cutoff: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refused: Vec<AsOfRefusal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub empty: Vec<AsOfEmpty>,
    #[serde(default)]
    pub left_out: AsOfLeftOut,
}

/// Grid cards per message (finance-korea-retail-disagreement.md §2.2).
pub const MAX_SENSITIVITY_CARDS: usize = 8;
const MAX_GRID_AXIS: usize = 9;
const MAX_SUBJECT_BYTES: usize = 240;

/// The past growth a reverse-DCF grid is read against (usually a CAGR).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensitivityReference {
    pub value_pct: f64,
    /// The period it covers, as the calculation was labelled ("FY2020–FY2025").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The years it spans, from a cited `cagr` result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub years: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<NumberCheckStatus>,
}

/// The price a reverse-DCF grid solves for, with its date and source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensitivityPrice {
    pub value: f64,
    pub display: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<NumberCheckStatus>,
}

/// One grid input (cash flow, shares, net debt, growth) and its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensitivityInput {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub display: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<NumberCheckStatus>,
}

/// A `dcf_grid` or `reverse_dcf` result as the host computed it; never one
/// point value. Rows are discount rates, columns long-run growth rates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensitivityGrid {
    pub operation: String,
    pub row_values: Vec<f64>,
    pub column_values: Vec<f64>,
    pub values: Vec<Vec<Option<f64>>>,
    pub unit: String,
    pub forecast_years: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<SensitivityReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub above_reference: Option<Vec<Vec<bool>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<SensitivityPrice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount_unit: Option<String>,
    #[serde(default)]
    pub inputs: Vec<SensitivityInput>,
    #[serde(default)]
    pub formulas: Vec<String>,
}

// Every number is finite (the calculation refuses anything else).
impl Eq for SensitivityGrid {}

/// A paragraph that is only a link to a grid result, shown as a card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SensitivityCard {
    #[serde(flatten)]
    pub grid: SensitivityGrid,
    pub link_index: u32,
    pub evidence_id: String,
    pub subject: String,
}

impl SensitivityGrid {
    pub fn valid(&self) -> bool {
        let (rows, columns) = (self.row_values.len(), self.column_values.len());
        let shaped = |cells: usize| cells == columns;
        let finite = |value: &f64| value.is_finite();
        matches!(self.operation.as_str(), "reverse_dcf" | "dcf_grid")
            && (1..=MAX_GRID_AXIS).contains(&rows)
            && (1..=MAX_GRID_AXIS).contains(&columns)
            && self.values.len() == rows
            && self.values.iter().all(|row| shaped(row.len()))
            && self
                .values
                .iter()
                .flatten()
                .flatten()
                .chain(&self.row_values)
                .chain(&self.column_values)
                .all(finite)
            && self.above_reference.as_ref().is_none_or(|above| {
                above.len() == rows && above.iter().all(|row| shaped(row.len()))
            })
            && bounded(&self.unit, 64)
            && (1..=30).contains(&self.forecast_years)
            && self.reference.as_ref().is_none_or(|reference| {
                reference.value_pct.is_finite()
                    && reference
                        .label
                        .as_deref()
                        .is_none_or(|label| bounded(label, 128))
                    && reference
                        .evidence_id
                        .as_deref()
                        .is_none_or(|id| bounded(id, 128))
            })
            && self.price.as_ref().is_none_or(|price| {
                price.value.is_finite()
                    && bounded(&price.display, 64)
                    && price
                        .currency
                        .as_deref()
                        .is_none_or(|code| bounded(code, 8))
                    && price.date.as_deref().is_none_or(valid_as_of)
                    && price
                        .evidence_id
                        .as_deref()
                        .is_none_or(|id| bounded(id, 128))
            })
            && self
                .amount_unit
                .as_deref()
                .is_none_or(|unit| bounded(unit, 64))
            && self.inputs.len() <= MAX_CALC_INPUTS
            && self.inputs.iter().all(|input| {
                bounded(&input.name, 128)
                    && bounded(&input.display, 128)
                    && input
                        .label
                        .as_deref()
                        .is_none_or(|label| bounded(label, 128))
                    && input
                        .evidence_id
                        .as_deref()
                        .is_none_or(|id| bounded(id, 128))
            })
            && self.formulas.len() <= MAX_FORMULA_LINES
            && self.formulas.iter().all(|line| bounded(line, 2048))
    }
}

pub(crate) fn validate_sensitivity_cards(cards: &[SensitivityCard]) -> Result<(), DomainError> {
    let invalid = cards.len() > MAX_SENSITIVITY_CARDS
        || cards.iter().any(|card| {
            !card.grid.valid()
                || !bounded(&card.evidence_id, 128)
                || card.evidence_id.is_empty()
                || card.subject.trim().is_empty()
                || !bounded(&card.subject, MAX_SUBJECT_BYTES)
        });
    if invalid {
        return Err(DomainError::Invalid(
            "sensitivity cards exceed their bounds",
        ));
    }
    Ok(())
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
                || [&check.nearest, &check.exact]
                    .into_iter()
                    .flatten()
                    .any(|text| !bounded(text, MAX_CHECK_TEXT_BYTES))
                || check
                    .pointer
                    .as_ref()
                    .is_some_and(|pointer| !bounded(pointer, 2048))
                || check
                    .span
                    .as_ref()
                    .is_some_and(|span| span.start >= span.end)
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
