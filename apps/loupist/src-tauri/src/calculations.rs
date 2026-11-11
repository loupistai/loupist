//! Deterministic finance arithmetic behind the `calculate` tool.
//!
//! Inputs are explicit numbers (JSON numbers or decimal strings) or values
//! read from a pinned dataset revision through [`DatasetSource`], which must
//! verify the revision's integrity. Every result returns its values with
//! units, a one-line formula with the inputs substituted, and each input's
//! provenance. No decimal crate is bundled, so arithmetic is IEEE-754
//! binary64; reported values are rounded to 15 significant digits.
mod dcf;
mod format;
mod retail;
mod series;
#[cfg(test)]
mod tests;

use crate::datasets::DatasetRef;
use crate::domain::{CalcInputUnit, CalcInputUnitKind};
use crate::knowledge::numbers::{currency_code, Unit};
use serde_json::{json, Map, Value};
use std::sync::Arc;

pub(crate) const MAX_SERIES: usize = 20_000;
const MAX_MAGNITUDE: f64 = 1e18;
const MAX_LABEL_CHARS: usize = 64;
const ARITHMETIC: &str =
    "IEEE-754 binary64; values rounded to 15 significant digits, display text rounded further";

/// A pinned dataset revision whose bytes passed the store's digest check.
pub(crate) trait DatasetSource {
    fn dataset(
        &mut self,
        dataset_id: &str,
        revision: u32,
    ) -> Result<(DatasetRef, Arc<Value>), String>;
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub(crate) enum CalcError {
    #[error("{field}: not used here; expected {expected}")]
    UnknownField { field: String, expected: String },
    #[error("{field}: required; {expected}")]
    Missing { field: String, expected: String },
    #[error("{field}: {expected}")]
    Invalid { field: String, expected: String },
    #[error("{field}: expected a finite number (JSON number or decimal string such as \"1234.5\") within ±1e18")]
    NotANumber { field: String },
    #[error("{field}: is zero, so the ratio is undefined")]
    ZeroDenominator { field: String },
    #[error("{field}: {reason}")]
    OutOfDomain { field: String, reason: String },
    #[error("{field}: {reason}")]
    Dataset { field: String, reason: String },
    /// The dataset store refused the revision: absent, foreign or failed its digest check.
    #[error("{field}: {reason}")]
    Unavailable { field: String, reason: String },
}

impl CalcError {
    pub(crate) fn field(&self) -> &str {
        match self {
            Self::UnknownField { field, .. }
            | Self::Missing { field, .. }
            | Self::Invalid { field, .. }
            | Self::NotANumber { field }
            | Self::ZeroDenominator { field }
            | Self::OutOfDomain { field, .. }
            | Self::Dataset { field, .. }
            | Self::Unavailable { field, .. } => field,
        }
    }

    /// The message without its field prefix, for field-addressed tool errors.
    pub(crate) fn detail(&self) -> String {
        let message = self.to_string();
        message
            .strip_prefix(&format!("{}: ", self.field()))
            .map_or(message.clone(), str::to_owned)
    }

    fn domain(field: &str, reason: impl Into<String>) -> Self {
        Self::OutOfDomain {
            field: field.into(),
            reason: reason.into(),
        }
    }

    fn invalid(field: &str, expected: impl Into<String>) -> Self {
        Self::Invalid {
            field: field.into(),
            expected: expected.into(),
        }
    }
}

/// A `calculate` operation; its snake_case name is the stable id the
/// renderer words results by (`NumberCheckCalc.operation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "CalcOperation"))]
#[serde(rename_all = "snake_case")]
pub(crate) enum Operation {
    PercentChange,
    Cagr,
    GrowthSeries,
    ReturnsSummary,
    MovingAverage,
    Ratio,
    Margin,
    PayoutRatio,
    NetDebtToEbitda,
    FreeCashFlow,
    DcfGrid,
    ReverseDcf,
    Sum,
    HoldingsOverlap,
    GoalGrid,
    DcaHistory,
}

impl Operation {
    const ALL: [(&'static str, Self); 16] = [
        ("percent_change", Self::PercentChange),
        ("cagr", Self::Cagr),
        ("growth_series", Self::GrowthSeries),
        ("returns_summary", Self::ReturnsSummary),
        ("moving_average", Self::MovingAverage),
        ("ratio", Self::Ratio),
        ("margin", Self::Margin),
        ("payout_ratio", Self::PayoutRatio),
        ("net_debt_to_ebitda", Self::NetDebtToEbitda),
        ("free_cash_flow", Self::FreeCashFlow),
        ("dcf_grid", Self::DcfGrid),
        ("reverse_dcf", Self::ReverseDcf),
        ("sum", Self::Sum),
        ("holdings_overlap", Self::HoldingsOverlap),
        ("goal_grid", Self::GoalGrid),
        ("dca_history", Self::DcaHistory),
    ];

    fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, operation)| *operation == self)
            .map(|(name, _)| *name)
            .expect("every operation has a name")
    }

    /// Reader title for the retained result.
    fn title(self) -> &'static str {
        match self {
            Self::PercentChange => "Percent change",
            Self::Cagr => "Compound annual growth",
            Self::GrowthSeries => "Period growth",
            Self::ReturnsSummary => "Return and risk summary",
            Self::MovingAverage => "Moving average",
            Self::Ratio => "Ratio",
            Self::Margin => "Margin",
            Self::PayoutRatio => "Payout ratio",
            Self::NetDebtToEbitda => "Net debt to EBITDA",
            Self::FreeCashFlow => "Free cash flow",
            Self::DcfGrid => "DCF sensitivity grid",
            Self::ReverseDcf => "Reverse DCF grid",
            Self::Sum => "Sum",
            Self::HoldingsOverlap => "Holdings overlap",
            Self::GoalGrid => "Savings goal grid",
            Self::DcaHistory => "Regular investing history",
        }
    }

    /// Required and optional `inputs` names.
    fn inputs(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Self::PercentChange => (&["from", "to"], &[]),
            Self::Cagr => (&["start", "end", "years"], &[]),
            Self::Ratio => (&["numerator", "denominator"], &[]),
            Self::Margin => (&["profit", "revenue"], &[]),
            Self::PayoutRatio => (&["dividends"], &["earnings", "free_cash_flow"]),
            Self::NetDebtToEbitda => (&["total_debt", "cash", "ebitda"], &[]),
            Self::FreeCashFlow => (&["operating_cash_flow", "capex"], &[]),
            Self::DcfGrid => (&["cash_flow", "growth_pct"], &["net_debt", "shares"]),
            Self::ReverseDcf => (
                &["cash_flow", "price", "shares"],
                &["net_debt", "reference_growth_pct"],
            ),
            Self::GoalGrid => (&["years"], &["start", "contribution", "target"]),
            Self::DcaHistory => (&["contribution"], &["fee_pct"]),
            Self::GrowthSeries
            | Self::ReturnsSummary
            | Self::MovingAverage
            | Self::Sum
            | Self::HoldingsOverlap => (&[], &[]),
        }
    }

    /// Names `labels` may describe: the inputs, and the two weight sets.
    fn label_names(self) -> Vec<&'static str> {
        let (required, optional) = self.inputs();
        let mut names = [required, optional].concat();
        if self == Self::HoldingsOverlap {
            names.extend(["weights_a", "weights_b"]);
        }
        names
    }

    /// Top-level fields besides `operation`, `inputs`, `unit` and `labels`.
    fn fields(self) -> &'static [&'static str] {
        match self {
            Self::GrowthSeries => &["series", "dates", "skip_missing"],
            Self::ReturnsSummary => &[
                "series",
                "dates",
                "skip_missing",
                "series_kind",
                "periods_per_year",
            ],
            Self::MovingAverage => &["series", "dates", "skip_missing", "window"],
            Self::Ratio => &["as_percent"],
            Self::DcfGrid | Self::ReverseDcf => {
                &["wacc_pct", "terminal_growth_pct", "forecast_years"]
            }
            Self::Sum => &["series", "dates", "skip_missing"],
            Self::HoldingsOverlap => &["weights_a", "weights_b"],
            Self::GoalGrid => &["periods_per_year", "return_pct"],
            Self::DcaHistory => &["series", "dates", "skip_missing", "schedule"],
            _ => &[],
        }
    }
}

/// One resolved number and where it came from.
#[derive(Clone, Debug)]
struct Number {
    value: f64,
    provenance: Value,
}

/// Parsed request with operands resolved against datasets.
struct Request<'a> {
    operation: Operation,
    object: &'a Map<String, Value>,
    inputs: Map<String, Value>,
    labels: Map<String, Value>,
    unit: Option<String>,
}

/// Collects the result fields every operation returns.
#[derive(Default)]
struct Outcome {
    values: Vec<Value>,
    formulas: Vec<String>,
    inputs: Vec<Value>,
    notes: Vec<String>,
    extra: Map<String, Value>,
}

impl Outcome {
    fn value(&mut self, name: &str, value: f64, unit: &str) -> Result<(), CalcError> {
        self.values.push(output(name, value, unit)?);
        Ok(())
    }

    fn input(&mut self, name: &str, number: &Number) {
        self.inputs.push(json!({
            "name": name,
            "value": format::round15(number.value),
            "display": format::amount(number.value),
            "provenance": number.provenance,
        }));
    }
}

fn output(name: &str, value: f64, unit: &str) -> Result<Value, CalcError> {
    if !value.is_finite() {
        return Err(CalcError::domain(
            name,
            "the result is not a finite number; check the input scale",
        ));
    }
    Ok(json!({
        "name": name,
        "value": format::round15(value),
        "display": display(value, unit),
        "unit": unit,
    }))
}

fn display(value: f64, unit: &str) -> String {
    match unit {
        "%" => format::percent(value),
        "x" => format::multiple(value),
        _ => format::amount(value),
    }
}

/// The host's word for an input when the request gives no label; formulas and
/// the retained inputs both use it, so they always agree.
fn default_label(name: &str) -> &str {
    match name {
        "profit" => "Profit",
        "revenue" => "Revenue",
        "dividends" => "Dividends",
        "earnings" => "Earnings",
        "free_cash_flow" => "FCF",
        "total_debt" => "Total debt",
        "cash" => "Cash",
        "ebitda" => "EBITDA",
        "operating_cash_flow" => "OCF",
        "cash_flow" => "cash flow",
        "growth_pct" => "growth",
        "net_debt" => "net debt",
        "reference_growth_pct" => "reference growth",
        "fee_pct" => "fee",
        "weights_a" => "A",
        "weights_b" => "B",
        // from, to, start, end, years, numerator, denominator, capex, shares,
        // price, contribution, target and series read as their own names.
        other => other,
    }
}

/// What an input measures, by name; the request's `unit` states amounts only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputKind {
    /// Years, shares, forecast years and holdings counts.
    Count,
    /// A `*_pct` input: 8 means 8%.
    Percent,
    /// A per-share price in the request's currency, never its scale.
    Price,
    /// An amount in the request's `unit`.
    Amount,
}

pub(crate) fn input_kind(name: &str) -> InputKind {
    match name {
        "years" | "shares" | "forecast_years" | "weights_a" | "weights_b" => InputKind::Count,
        "price" => InputKind::Price,
        _ if name.ends_with("_pct") => InputKind::Percent,
        _ => InputKind::Amount,
    }
}

/// Run one calculation request. `request` is the tool's argument object.
pub(crate) fn calculate(
    request: &Value,
    datasets: &mut dyn DatasetSource,
) -> Result<Value, CalcError> {
    let object = request
        .as_object()
        .ok_or_else(|| CalcError::invalid("arguments", "an object with operation"))?;
    let name = object
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| CalcError::Missing {
            field: "operation".into(),
            expected: operation_names(),
        })?;
    let operation = Operation::ALL
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, operation)| *operation)
        .ok_or_else(|| CalcError::invalid("operation", format!("one of {}", operation_names())))?;
    let allowed = operation.fields();
    for key in object.keys() {
        if !matches!(key.as_str(), "operation" | "inputs" | "unit" | "labels")
            && !allowed.contains(&key.as_str())
        {
            return Err(CalcError::UnknownField {
                field: key.clone(),
                expected: list(&[&["operation", "inputs", "unit", "labels"], allowed].concat()),
            });
        }
    }
    let inputs = optional_object(object, "inputs")?;
    let (required, optional) = operation.inputs();
    for key in inputs.keys() {
        if !required.contains(&key.as_str()) && !optional.contains(&key.as_str()) {
            return Err(CalcError::UnknownField {
                field: format!("inputs.{key}"),
                expected: list(&[required, optional].concat()),
            });
        }
    }
    for key in required {
        if !inputs.contains_key(*key) {
            return Err(CalcError::Missing {
                field: format!("inputs.{key}"),
                expected: format!("{} needs inputs {}", operation.name(), list(required)),
            });
        }
    }
    let labels = optional_object(object, "labels")?;
    let label_names = operation.label_names();
    for (key, label) in &labels {
        if !label_names.contains(&key.as_str()) {
            return Err(CalcError::UnknownField {
                field: format!("labels.{key}"),
                expected: list(&label_names),
            });
        }
        bounded_text(label, &format!("labels.{key}"))?;
    }
    let unit = object
        .get("unit")
        .map(|unit| bounded_text(unit, "unit"))
        .transpose()?;
    let request = Request {
        operation,
        object,
        inputs,
        labels,
        unit,
    };
    let mut outcome = Outcome::default();
    match operation {
        Operation::PercentChange => percent_change(&request, datasets, &mut outcome)?,
        Operation::Cagr => cagr(&request, datasets, &mut outcome)?,
        Operation::Ratio => ratio(&request, datasets, &mut outcome)?,
        Operation::Margin => margin(&request, datasets, &mut outcome)?,
        Operation::PayoutRatio => payout_ratio(&request, datasets, &mut outcome)?,
        Operation::NetDebtToEbitda => net_debt_to_ebitda(&request, datasets, &mut outcome)?,
        Operation::FreeCashFlow => free_cash_flow(&request, datasets, &mut outcome)?,
        Operation::GrowthSeries => series::growth(&request, datasets, &mut outcome)?,
        Operation::ReturnsSummary => series::returns(&request, datasets, &mut outcome)?,
        Operation::MovingAverage => series::moving_average(&request, datasets, &mut outcome)?,
        Operation::DcfGrid | Operation::ReverseDcf => dcf::grid(&request, datasets, &mut outcome)?,
        Operation::Sum => series::sum(&request, datasets, &mut outcome)?,
        Operation::HoldingsOverlap => retail::holdings_overlap(&request, &mut outcome)?,
        Operation::GoalGrid => retail::goal_grid(&request, datasets, &mut outcome)?,
        Operation::DcaHistory => retail::dca_history(&request, datasets, &mut outcome)?,
    }
    // Every input names itself as its formula does and says how it is printed,
    // so a reader never recovers either from the formula or the evidence.
    for input in &mut outcome.inputs {
        let name = input["name"].as_str().unwrap_or_default().to_owned();
        input["label"] = json!(request.input_label(&name));
        input["unit"] = json!(request.input_unit(&name));
        if name == "reference_growth_pct" {
            if let Some(period) = request.labels.get(&name) {
                input["period"] = period.clone();
            }
        }
    }
    let mut result = Map::new();
    result.insert("operation".into(), json!(operation.name()));
    result.insert("title".into(), json!(operation.title()));
    result.insert("values".into(), json!(outcome.values));
    result.extend(outcome.extra);
    result.insert("formulas".into(), json!(outcome.formulas));
    result.insert("inputs".into(), json!(outcome.inputs));
    if !outcome.notes.is_empty() {
        result.insert("notes".into(), json!(outcome.notes));
    }
    result.insert("arithmetic".into(), json!(ARITHMETIC));
    Ok(Value::Object(result))
}

/// A number as calculation formulas print it ("30,000,000,000", "6,370"), for
/// a host formula written beside a calculation's own.
pub(crate) fn display_amount(value: f64) -> String {
    format::amount(value)
}

fn operation_names() -> String {
    list(&Operation::ALL.map(|(name, _)| name))
}

fn list(names: &[&str]) -> String {
    names.join(", ")
}

fn optional_object(
    object: &Map<String, Value>,
    field: &str,
) -> Result<Map<String, Value>, CalcError> {
    match object.get(field) {
        None => Ok(Map::new()),
        Some(Value::Object(map)) => Ok(map.clone()),
        Some(_) => Err(CalcError::invalid(field, "an object")),
    }
}

fn bounded_text(value: &Value, field: &str) -> Result<String, CalcError> {
    value
        .as_str()
        .filter(|text| {
            !text.trim().is_empty()
                && text.chars().count() <= MAX_LABEL_CHARS
                && !text.chars().any(char::is_control)
        })
        .map(str::to_owned)
        .ok_or_else(|| {
            CalcError::invalid(
                field,
                format!("non-empty text up to {MAX_LABEL_CHARS} characters"),
            )
        })
}

impl Request<'_> {
    fn number(&self, name: &str, datasets: &mut dyn DatasetSource) -> Result<Number, CalcError> {
        let field = format!("inputs.{name}");
        let value = self.inputs.get(name).ok_or_else(|| CalcError::Missing {
            field: field.clone(),
            expected: format!(
                "{} needs inputs {}",
                self.operation.name(),
                list(self.operation.inputs().0)
            ),
        })?;
        scalar(value, &field, datasets)
    }

    fn optional_number(
        &self,
        name: &str,
        datasets: &mut dyn DatasetSource,
    ) -> Result<Option<Number>, CalcError> {
        self.inputs
            .contains_key(name)
            .then(|| self.number(name, datasets))
            .transpose()
    }

    /// The name a formula prints for an input: its `labels` entry, else the
    /// host's own word for it.
    fn label(&self, name: &str) -> String {
        self.labels
            .get(name)
            .and_then(Value::as_str)
            .map_or_else(|| default_label(name).to_owned(), str::to_owned)
    }

    /// An input's label. A reverse DCF's `labels.reference_growth_pct` is the
    /// period the reference covers, not a name: that input keeps the host's
    /// label and carries the period as `period`.
    fn input_label(&self, name: &str) -> String {
        if name == "reference_growth_pct" {
            default_label(name).to_owned()
        } else {
            self.label(name)
        }
    }

    /// How a reader prints an input: a percent, a count, a per-share price in
    /// the request's currency, or an amount in the currency and scale `unit`
    /// states. A unit the host cannot place stays as text after the number.
    fn input_unit(&self, name: &str) -> CalcInputUnit {
        let kind = if name == "series"
            && self.field("series_kind").and_then(Value::as_str) == Some("returns_pct")
        {
            InputKind::Percent
        } else {
            input_kind(name)
        };
        let unit = self.unit.as_deref();
        let currency = || unit.and_then(currency_code);
        let of = CalcInputUnit::of;
        match kind {
            InputKind::Count => of(CalcInputUnitKind::Number),
            InputKind::Percent => of(CalcInputUnitKind::Percent),
            InputKind::Price => CalcInputUnit {
                currency: currency(),
                ..of(CalcInputUnitKind::Price)
            },
            InputKind::Amount => match unit.map(|text| (text, Unit::of_line(Some(text)))) {
                None => of(CalcInputUnitKind::Amount),
                Some((_, Ok(Unit::Percent))) => of(CalcInputUnitKind::Percent),
                Some((_, Ok(Unit::Points))) => of(CalcInputUnitKind::Points),
                Some((_, Ok(Unit::BasisPoints))) => of(CalcInputUnitKind::BasisPoints),
                Some((_, Ok(Unit::Multiple))) => of(CalcInputUnitKind::Multiple),
                Some((_, Ok(Unit::Amount(scale)))) => CalcInputUnit {
                    currency: currency(),
                    scale: scale.log10().round().clamp(0.0, 18.0) as u8,
                    ..of(CalcInputUnitKind::Amount)
                },
                Some((text, _)) => CalcInputUnit {
                    text: Some(text.to_owned()),
                    ..of(CalcInputUnitKind::Number)
                },
            },
        }
    }

    fn amount_unit(&self) -> String {
        self.unit.clone().unwrap_or_else(|| "input units".into())
    }

    fn field(&self, name: &str) -> Option<&Value> {
        self.object.get(name)
    }

    fn integer(
        &self,
        name: &str,
        minimum: u64,
        maximum: u64,
        default: u64,
    ) -> Result<u64, CalcError> {
        match self.field(name) {
            None => Ok(default),
            Some(value) => value
                .as_u64()
                .filter(|value| (minimum..=maximum).contains(value))
                .ok_or_else(|| {
                    CalcError::invalid(name, format!("an integer from {minimum} to {maximum}"))
                }),
        }
    }

    fn boolean(&self, name: &str) -> Result<bool, CalcError> {
        match self.field(name) {
            None => Ok(false),
            Some(value) => value
                .as_bool()
                .ok_or_else(|| CalcError::invalid(name, "true or false")),
        }
    }
}

/// A JSON number, a plain decimal string or a dataset cell
/// `{dataset_id, revision, pointer}`.
fn scalar(
    value: &Value,
    field: &str,
    datasets: &mut dyn DatasetSource,
) -> Result<Number, CalcError> {
    let provided = || json!({ "kind": "provided" });
    match value {
        Value::Number(_) | Value::String(_) => Ok(Number {
            value: plain_number(value, field)?,
            provenance: provided(),
        }),
        Value::Object(cell) => {
            for key in cell.keys() {
                if !matches!(key.as_str(), "dataset_id" | "revision" | "pointer") {
                    return Err(CalcError::UnknownField {
                        field: format!("{field}.{key}"),
                        expected: "dataset_id, revision, pointer".into(),
                    });
                }
            }
            let (reference, payload) = dataset(cell, field, datasets)?;
            let pointer = pointer(cell.get("pointer"), &format!("{field}.pointer"), true)?;
            let selected = payload
                .pointer(&pointer)
                .ok_or_else(|| CalcError::Dataset {
                    field: format!("{field}.pointer"),
                    reason: "no value at this JSON Pointer in the dataset revision".into(),
                })?;
            if selected.is_null() {
                return Err(CalcError::Dataset {
                    field: format!("{field}.pointer"),
                    reason: "the dataset value is null; a missing value is not zero".into(),
                });
            }
            Ok(Number {
                value: plain_number(selected, &format!("{field}.pointer"))?,
                provenance: json!({
                    "kind": "dataset",
                    "dataset_id": reference.dataset_id,
                    "revision": reference.revision,
                    "digest": reference.digest,
                    "pointer": pointer,
                }),
            })
        }
        _ => Err(CalcError::NotANumber {
            field: field.into(),
        }),
    }
}

/// Finite JSON number or decimal string, never NaN, infinity or grouped text.
fn plain_number(value: &Value, field: &str) -> Result<f64, CalcError> {
    let number = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => decimal(text.trim()),
        _ => None,
    };
    number
        .filter(|number| number.is_finite() && number.abs() <= MAX_MAGNITUDE)
        .ok_or_else(|| CalcError::NotANumber {
            field: field.into(),
        })
}

fn decimal(text: &str) -> Option<f64> {
    if text.is_empty() || text.len() > 64 {
        return None;
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (unsigned, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    let exponent_ok = exponent.is_none_or(|exponent| {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        !exponent.is_empty() && digits(exponent)
    });
    (digits(whole) && digits(fraction) && !(whole.is_empty() && fraction.is_empty()) && exponent_ok)
        .then(|| text.parse().ok())
        .flatten()
}

fn dataset(
    reference: &Map<String, Value>,
    field: &str,
    datasets: &mut dyn DatasetSource,
) -> Result<(DatasetRef, Arc<Value>), CalcError> {
    let dataset_id = reference
        .get("dataset_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .ok_or_else(|| {
            CalcError::invalid(
                &format!("{field}.dataset_id"),
                "the exact dataset_id from a receipt",
            )
        })?;
    let revision = reference
        .get("revision")
        .and_then(Value::as_u64)
        .and_then(|revision| u32::try_from(revision).ok())
        .filter(|revision| *revision >= 1)
        .ok_or_else(|| {
            CalcError::invalid(
                &format!("{field}.revision"),
                "the exact revision integer from a receipt",
            )
        })?;
    datasets
        .dataset(dataset_id, revision)
        .map_err(|reason| CalcError::Unavailable {
            field: field.into(),
            reason,
        })
}

fn pointer(value: Option<&Value>, field: &str, required: bool) -> Result<String, CalcError> {
    match value {
        None if !required => Ok(String::new()),
        Some(Value::String(pointer))
            if pointer.len() <= 2048 && (pointer.is_empty() || pointer.starts_with('/')) =>
        {
            Ok(pointer.clone())
        }
        _ => Err(CalcError::invalid(
            field,
            "a JSON Pointer such as \"/rows/0/value\"",
        )),
    }
}

fn positive(number: &Number, field: &str, reason: &str) -> Result<f64, CalcError> {
    if number.value > 0.0 {
        Ok(number.value)
    } else {
        Err(CalcError::domain(field, reason))
    }
}

fn percent_change(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let from = request.number("from", datasets)?;
    let to = request.number("to", datasets)?;
    let base = positive(
        &from,
        "inputs.from",
        "must be positive; a percent change from zero or a negative base is not meaningful, so report the absolute change",
    )?;
    let change = to.value - base;
    let percent = change / base * 100.0;
    let (from_label, to_label) = (request.label("from"), request.label("to"));
    outcome.value("change", change, &request.amount_unit())?;
    outcome.value("percent_change", percent, "%")?;
    outcome.formulas.push(format!(
        "{to_label} − {from_label} = {} − {} = {}",
        format::amount(to.value),
        format::amount(base),
        format::amount(change)
    ));
    outcome.formulas.push(format!(
        "({to_label} − {from_label}) / {from_label} = {} / {} = {}",
        format::amount(change),
        format::amount(base),
        format::percent(percent)
    ));
    outcome.input("from", &from);
    outcome.input("to", &to);
    Ok(())
}

fn cagr(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let start = request.number("start", datasets)?;
    let end = request.number("end", datasets)?;
    let years = request.number("years", datasets)?;
    let first = positive(
        &start,
        "inputs.start",
        "must be positive for a compound growth rate",
    )?;
    if end.value < 0.0 {
        return Err(CalcError::domain(
            "inputs.end",
            "must be zero or positive; compound growth across a sign change is not meaningful",
        ));
    }
    if !(years.value > 0.0 && years.value <= 200.0) {
        return Err(CalcError::domain(
            "inputs.years",
            "must be greater than 0 and at most 200",
        ));
    }
    let rate = ((end.value / first).powf(1.0 / years.value) - 1.0) * 100.0;
    let (start_label, end_label) = (request.label("start"), request.label("end"));
    outcome.value("cagr", rate, "%")?;
    outcome.formulas.push(format!(
        "({end_label} / {start_label})^(1 / years) − 1 = ({} / {})^(1 / {}) − 1 = {}",
        format::amount(end.value),
        format::amount(first),
        format::amount(years.value),
        format::percent(rate)
    ));
    outcome.input("start", &start);
    outcome.input("end", &end);
    outcome.input("years", &years);
    Ok(())
}

fn ratio(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let numerator = request.number("numerator", datasets)?;
    let denominator = request.number("denominator", datasets)?;
    if denominator.value == 0.0 {
        return Err(CalcError::ZeroDenominator {
            field: "inputs.denominator".into(),
        });
    }
    let as_percent = request.boolean("as_percent")?;
    let raw = numerator.value / denominator.value;
    let (value, unit) = if as_percent {
        (raw * 100.0, "%")
    } else {
        (raw, "x")
    };
    if denominator.value < 0.0 {
        outcome
            .notes
            .push("The denominator is negative; the ratio's sign may not be meaningful.".into());
    }
    outcome.value("ratio", value, unit)?;
    outcome.formulas.push(format!(
        "{} / {} = {} / {} = {}",
        request.label("numerator"),
        request.label("denominator"),
        format::amount(numerator.value),
        format::amount(denominator.value),
        display(value, unit)
    ));
    outcome.input("numerator", &numerator);
    outcome.input("denominator", &denominator);
    Ok(())
}

fn margin(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let profit = request.number("profit", datasets)?;
    let revenue = request.number("revenue", datasets)?;
    let base = positive(&revenue, "inputs.revenue", "must be positive for a margin")?;
    let value = profit.value / base * 100.0;
    outcome.value("margin", value, "%")?;
    outcome.formulas.push(format!(
        "{} / {} = {} / {} = {}",
        request.label("profit"),
        request.label("revenue"),
        format::amount(profit.value),
        format::amount(base),
        format::percent(value)
    ));
    outcome.input("profit", &profit);
    outcome.input("revenue", &revenue);
    Ok(())
}

/// Cash-flow statements often report outflows as negative numbers.
fn outflow(number: &Number, label: &str, outcome: &mut Outcome) -> f64 {
    if number.value < 0.0 {
        outcome.notes.push(format!(
            "{label} was negative and is used as an outflow of {}.",
            format::amount(-number.value)
        ));
    }
    number.value.abs()
}

fn payout_ratio(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let dividends = request.number("dividends", datasets)?;
    let earnings = request.optional_number("earnings", datasets)?;
    let free_cash_flow = request.optional_number("free_cash_flow", datasets)?;
    if earnings.is_none() && free_cash_flow.is_none() {
        return Err(CalcError::Missing {
            field: "inputs.earnings".into(),
            expected: "payout_ratio needs earnings, free_cash_flow, or both".into(),
        });
    }
    let dividend_label = request.label("dividends");
    let paid = outflow(&dividends, &dividend_label, outcome);
    outcome.input("dividends", &dividends);
    for (name, basis, output_name) in [
        ("earnings", &earnings, "payout_on_earnings"),
        (
            "free_cash_flow",
            &free_cash_flow,
            "payout_on_free_cash_flow",
        ),
    ] {
        let Some(basis) = basis else { continue };
        let denominator = positive(
            basis,
            &format!("inputs.{name}"),
            "must be positive; a payout ratio on zero or negative earnings or cash flow is not meaningful",
        )?;
        let value = paid / denominator * 100.0;
        outcome.value(output_name, value, "%")?;
        outcome.formulas.push(format!(
            "{dividend_label} / {} = {} / {} = {}",
            request.label(name),
            format::amount(paid),
            format::amount(denominator),
            format::percent(value)
        ));
        outcome.input(name, basis);
    }
    Ok(())
}

fn net_debt_to_ebitda(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let debt = request.number("total_debt", datasets)?;
    let cash = request.number("cash", datasets)?;
    let ebitda = request.number("ebitda", datasets)?;
    for (number, field) in [(&debt, "inputs.total_debt"), (&cash, "inputs.cash")] {
        if number.value < 0.0 {
            return Err(CalcError::domain(field, "must be zero or positive"));
        }
    }
    let earnings = positive(
        &ebitda,
        "inputs.ebitda",
        "must be positive; leverage on zero or negative EBITDA is not meaningful",
    )?;
    let net_debt = debt.value - cash.value;
    let value = net_debt / earnings;
    if net_debt < 0.0 {
        outcome
            .notes
            .push("Cash exceeds debt (a net cash position), so the ratio is negative.".into());
    }
    let (debt_label, cash_label, ebitda_label) = (
        request.label("total_debt"),
        request.label("cash"),
        request.label("ebitda"),
    );
    outcome.value("net_debt", net_debt, &request.amount_unit())?;
    outcome.value("net_debt_to_ebitda", value, "x")?;
    outcome.formulas.push(format!(
        "Net debt = {debt_label} − {cash_label} = {} − {} = {}",
        format::amount(debt.value),
        format::amount(cash.value),
        format::amount(net_debt)
    ));
    outcome.formulas.push(format!(
        "Net debt / {ebitda_label} = {} / {} = {}",
        format::amount(net_debt),
        format::amount(earnings),
        format::multiple(value)
    ));
    outcome.input("total_debt", &debt);
    outcome.input("cash", &cash);
    outcome.input("ebitda", &ebitda);
    Ok(())
}

fn free_cash_flow(
    request: &Request,
    datasets: &mut dyn DatasetSource,
    outcome: &mut Outcome,
) -> Result<(), CalcError> {
    let operating = request.number("operating_cash_flow", datasets)?;
    let capex = request.number("capex", datasets)?;
    let (ocf_label, capex_label) = (request.label("operating_cash_flow"), request.label("capex"));
    let spent = outflow(&capex, &capex_label, outcome);
    let value = operating.value - spent;
    outcome.value("free_cash_flow", value, &request.amount_unit())?;
    outcome.formulas.push(format!(
        "FCF = {ocf_label} − {capex_label} = {} − {} = {}",
        format::amount(operating.value),
        format::amount(spent),
        format::amount(value)
    ));
    outcome.input("operating_cash_flow", &operating);
    outcome.input("capex", &capex);
    Ok(())
}
