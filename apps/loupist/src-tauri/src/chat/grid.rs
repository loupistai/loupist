//! A grid answer (finance-grid-answers.md, W16.10) and its conflict layout
//! (finance-korea-retail-disagreement.md §2.3, W16.9): the stored
//! `loupist-grid/1` document. `grid_publish` builds it from this turn's
//! evidence and checks every value the way a number chip is checked; the
//! renderer only draws it. Holdings never reach a grid (D9).
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::artifacts::ChatArtifactError;
use crate::domain::NumberCheck;

pub const GRID_SCHEMA: &str = "loupist-grid/1";
/// The serialized document, like a Markdown artifact.
pub const MAX_GRID_BYTES: usize = 256 * 1024;
pub const MIN_GRID_ROWS: usize = 2;
pub const MAX_GRID_ROWS: usize = 12;
pub const MAX_GRID_COLUMNS: usize = 8;
pub const MAX_GRID_NOTES: usize = 3;
pub const MIN_CONFLICT_REPORTS: usize = 2;
pub const MAX_CONFLICT_REPORTS: usize = 6;

/// Character limits (finance-grid-answers.md §3.2, finance-korea-retail-disagreement.md §3.3).
pub const TITLE_CHARS: usize = 120;
pub const SUBTITLE_CHARS: usize = 160;
pub const NOTE_CHARS: usize = 200;
pub const ROW_LABEL_CHARS: usize = 80;
pub const SYMBOL_CHARS: usize = 32;
pub const MARKET_CHARS: usize = 16;
pub const COLUMN_LABEL_CHARS: usize = 40;
pub const PERIOD_CHARS: usize = 24;
pub const BASIS_CHARS: usize = 40;
pub const VALUE_CHARS: usize = 32;
pub const EMPTY_NOTE_CHARS: usize = 80;
pub const MEASURE_CHARS: usize = 80;
pub const SAYS_CHARS: usize = 120;
pub const EXPLANATION_CHARS: usize = 280;
pub const USED_REASON_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GridUnit {
    Amount,
    Percent,
    Multiple,
    Number,
    Date,
}

/// Why a cell is empty; an empty cell is never a zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GridEmptyReason {
    NotReported,
    NotApplicable,
    Unavailable,
    NotChecked,
}

impl GridEmptyReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotReported => "not_reported",
            Self::NotApplicable => "not_applicable",
            Self::Unavailable => "unavailable",
            Self::NotChecked => "not_checked",
        }
    }
}

/// Why sources disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ConflictReason {
    Basis,
    Period,
    PreliminaryVsFinal,
    EstimateVsReported,
    CurrencyOrUnit,
    AdjustedVsReported,
    Timing,
    Unclear,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GridRow {
    pub symbol: String,
    pub market: String,
    pub label: String,
    /// Set when Rust resolved the row to an asset page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub asset_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GridColumn {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub period: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub basis: Option<String>,
    pub unit: GridUnit,
}

/// A value with the evidence it links and Rust's check of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GridValue {
    pub value: String,
    pub evidence_id: String,
    pub check: NumberCheck,
    /// The value in base units (an amount times its scale, a percent as
    /// printed), so the renderer sorts without reading numbers itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub sort: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GridEmpty {
    pub empty: GridEmptyReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(untagged)]
pub enum GridCell {
    Value(Box<GridValue>),
    Empty(GridEmpty),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AssetsGrid {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    pub rows: Vec<GridRow>,
    pub columns: Vec<GridColumn>,
    pub cells: Vec<Vec<GridCell>>,
    /// The turn's as-of date: every cell is what was known by then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub as_of: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConflictMeasure {
    pub label: String,
    pub entity_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub period: Option<String>,
}

/// One source's report. `dated` comes from the evidence, never the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConflictReport {
    pub evidence_id: String,
    pub says: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub basis: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dated: Option<String>,
    /// Present when `says` is a number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub check: Option<NumberCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConflictGrid {
    pub measure: ConflictMeasure,
    pub reports: Vec<ConflictReport>,
    pub reason: ConflictReason,
    pub explanation: String,
    /// The report the answer relies on, or none when it cannot tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub used: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub used_reason: Option<String>,
    /// A World entity this turn read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub world_entity_id: Option<String>,
    /// Rust found a disputed claim group touching the entity: the card links the Map.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub map_link: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub as_of: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "layout", rename_all = "snake_case")]
pub enum GridDocument {
    Assets(AssetsGrid),
    Conflict(ConflictGrid),
}

/// Nonblank text of at most `chars` characters without NUL.
pub fn valid_text(value: &str, chars: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= chars && !value.contains('\0')
}

fn valid_optional(value: &Option<String>, chars: usize) -> bool {
    value.as_deref().is_none_or(|text| valid_text(text, chars))
}

fn valid_evidence_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A column id the renderer and exports key on.
pub fn valid_column_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}

fn checks_valid(checks: &[NumberCheck]) -> bool {
    crate::domain::answer_checks::validate_number_checks(checks).is_ok()
}

impl GridDocument {
    /// The evidence every value and report links, for the renderer's popovers.
    pub fn evidence_ids(&self) -> BTreeSet<String> {
        match self {
            Self::Assets(grid) => grid
                .cells
                .iter()
                .flatten()
                .filter_map(|cell| match cell {
                    GridCell::Value(value) => Some(value.evidence_id.clone()),
                    GridCell::Empty(_) => None,
                })
                .collect(),
            Self::Conflict(grid) => grid
                .reports
                .iter()
                .map(|report| report.evidence_id.clone())
                .collect(),
        }
    }

    pub fn as_of(&self) -> Option<&str> {
        match self {
            Self::Assets(grid) => grid.as_of.as_deref(),
            Self::Conflict(grid) => grid.as_of.as_deref(),
        }
    }

    pub fn validate(&self) -> Result<(), ChatArtifactError> {
        let invalid = ChatArtifactError::Invalid;
        if self
            .as_of()
            .is_some_and(|date| !crate::domain::valid_as_of(date))
        {
            return Err(invalid("grid as-of date"));
        }
        match self {
            Self::Assets(grid) => grid.validate()?,
            Self::Conflict(grid) => grid.validate()?,
        }
        let bytes = serde_json::to_vec(self).map_err(|_| ChatArtifactError::Digest)?;
        if bytes.len() > MAX_GRID_BYTES {
            return Err(invalid("grid exceeds 256 KiB"));
        }
        Ok(())
    }
}

impl AssetsGrid {
    fn validate(&self) -> Result<(), ChatArtifactError> {
        let invalid = ChatArtifactError::Invalid;
        if !valid_optional(&self.subtitle, SUBTITLE_CHARS)
            || self.notes.len() > MAX_GRID_NOTES
            || self.notes.iter().any(|note| !valid_text(note, NOTE_CHARS))
        {
            return Err(invalid("grid subtitle or notes"));
        }
        if !(MIN_GRID_ROWS..=MAX_GRID_ROWS).contains(&self.rows.len())
            || !(1..=MAX_GRID_COLUMNS).contains(&self.columns.len())
            || self.cells.len() != self.rows.len()
            || self.cells.iter().any(|row| row.len() != self.columns.len())
        {
            return Err(invalid("grid shape"));
        }
        let mut identities = BTreeSet::new();
        for row in &self.rows {
            if !valid_text(&row.symbol, SYMBOL_CHARS)
                || !valid_text(&row.market, MARKET_CHARS)
                || !valid_text(&row.label, ROW_LABEL_CHARS)
                || row
                    .asset_id
                    .as_deref()
                    .is_some_and(|id| !crate::knowledge::assets::valid_asset_id(id))
                || !identities.insert((
                    row.symbol.trim().to_uppercase(),
                    row.market.trim().to_uppercase(),
                ))
            {
                return Err(invalid("grid row"));
            }
        }
        let mut ids = BTreeSet::new();
        for column in &self.columns {
            if !valid_column_id(&column.id)
                || !ids.insert(column.id.as_str())
                || !valid_text(&column.label, COLUMN_LABEL_CHARS)
                || !valid_optional(&column.period, PERIOD_CHARS)
                || !valid_optional(&column.basis, BASIS_CHARS)
            {
                return Err(invalid("grid column"));
            }
        }
        let mut checks = Vec::new();
        for cell in self.cells.iter().flatten() {
            match cell {
                GridCell::Value(value) => {
                    if !valid_text(&value.value, VALUE_CHARS)
                        || value.sort.is_some_and(|sort| !sort.is_finite())
                        || !valid_evidence_id(&value.evidence_id)
                        || value.check.evidence_id != value.evidence_id
                    {
                        return Err(invalid("grid value cell"));
                    }
                    checks.push(value.check.clone());
                }
                GridCell::Empty(empty) => {
                    if !valid_optional(&empty.note, EMPTY_NOTE_CHARS) {
                        return Err(invalid("grid empty cell"));
                    }
                }
            }
        }
        if !checks_valid(&checks) {
            return Err(invalid("grid cell checks"));
        }
        Ok(())
    }
}

impl ConflictGrid {
    fn validate(&self) -> Result<(), ChatArtifactError> {
        let invalid = ChatArtifactError::Invalid;
        if !valid_text(&self.measure.label, MEASURE_CHARS)
            || !valid_text(&self.measure.entity_label, MEASURE_CHARS)
            || !valid_optional(&self.measure.period, PERIOD_CHARS)
            || !valid_text(&self.explanation, EXPLANATION_CHARS)
            || !valid_optional(&self.used_reason, USED_REASON_CHARS)
            || self
                .world_entity_id
                .as_deref()
                .is_some_and(|id| id.is_empty() || id.len() > 256 || id.contains('\0'))
            || (self.map_link && self.world_entity_id.is_none())
        {
            return Err(invalid("conflict measure or explanation"));
        }
        if !(MIN_CONFLICT_REPORTS..=MAX_CONFLICT_REPORTS).contains(&self.reports.len())
            || self
                .used
                .is_some_and(|index| index as usize >= self.reports.len())
        {
            return Err(invalid("conflict reports"));
        }
        let mut sources = BTreeSet::new();
        let mut checks = Vec::new();
        for report in &self.reports {
            if !valid_evidence_id(&report.evidence_id)
                || !sources.insert(report.evidence_id.as_str())
                || !valid_text(&report.says, SAYS_CHARS)
                || !valid_optional(&report.basis, BASIS_CHARS)
                || report
                    .dated
                    .as_deref()
                    .is_some_and(|date| !crate::domain::valid_as_of(date))
                || report
                    .check
                    .as_ref()
                    .is_some_and(|check| check.evidence_id != report.evidence_id)
            {
                return Err(invalid("conflict report"));
            }
            checks.extend(report.check.clone());
        }
        if !checks_valid(&checks) {
            return Err(invalid("conflict checks"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{NumberCheckKind, NumberCheckStatus};

    fn check(evidence: &str, text: &str) -> NumberCheck {
        NumberCheck {
            link_index: 0,
            text: text.into(),
            evidence_id: evidence.into(),
            kind: NumberCheckKind::Data,
            status: NumberCheckStatus::Matched,
            nearest: None,
            exact: None,
            pointer: None,
            span: None,
            fields: vec![],
            calc: None,
        }
    }

    fn value(text: &str) -> GridCell {
        GridCell::Value(Box::new(GridValue {
            value: text.into(),
            evidence_id: "ev_a".into(),
            check: check("ev_a", text),
            sort: None,
        }))
    }

    fn assets() -> AssetsGrid {
        AssetsGrid {
            subtitle: None,
            notes: vec![],
            rows: vec![
                GridRow {
                    symbol: "990221".into(),
                    market: "KRX".into(),
                    label: "Solsup Foods".into(),
                    asset_id: None,
                },
                GridRow {
                    symbol: "990552".into(),
                    market: "KRX".into(),
                    label: "Iseul Logistics".into(),
                    asset_id: None,
                },
            ],
            columns: vec![GridColumn {
                id: "revenue".into(),
                label: "Revenue".into(),
                period: Some("FY2025".into()),
                basis: None,
                unit: GridUnit::Amount,
            }],
            cells: vec![
                vec![value("₩452.3bn")],
                vec![GridCell::Empty(GridEmpty {
                    empty: GridEmptyReason::NotReported,
                    note: None,
                })],
            ],
            as_of: None,
        }
    }

    #[test]
    fn a_stored_grid_round_trips_and_keeps_its_shape() {
        let document = GridDocument::Assets(assets());
        document.validate().unwrap();
        let json = serde_json::to_value(&document).unwrap();
        assert_eq!(json["layout"], "assets");
        assert_eq!(json["cells"][1][0]["empty"], "not_reported");
        let back: GridDocument = serde_json::from_value(json).unwrap();
        assert_eq!(back, document);
        assert_eq!(document.evidence_ids().len(), 1);

        let mut ragged = assets();
        ragged.cells[1].push(value("1"));
        assert!(GridDocument::Assets(ragged).validate().is_err());
        let mut duplicate = assets();
        duplicate.rows[1].symbol = "990221".into();
        assert!(GridDocument::Assets(duplicate).validate().is_err());
        let mut long = assets();
        long.cells[0][0] = value(&"9".repeat(VALUE_CHARS + 1));
        assert!(GridDocument::Assets(long).validate().is_err());
    }

    #[test]
    fn a_conflict_needs_distinct_sources_and_a_used_report_it_has() {
        let report = |evidence: &str| ConflictReport {
            evidence_id: evidence.into(),
            says: "14.8%".into(),
            basis: None,
            dated: Some("2026-10-30".into()),
            check: Some(check(evidence, "14.8%")),
        };
        let conflict = ConflictGrid {
            measure: ConflictMeasure {
                label: "Operating margin".into(),
                entity_label: "Hanul Precision".into(),
                period: Some("Q3 2026".into()),
            },
            reports: vec![report("ev_a"), report("ev_b")],
            reason: ConflictReason::Basis,
            explanation: "The news report excludes a one-off charge.".into(),
            used: Some(0),
            used_reason: Some("It is the filed figure.".into()),
            world_entity_id: None,
            map_link: false,
            as_of: None,
        };
        GridDocument::Conflict(conflict.clone()).validate().unwrap();
        let mut same = conflict.clone();
        same.reports[1] = report("ev_a");
        assert!(GridDocument::Conflict(same).validate().is_err());
        let mut used = conflict.clone();
        used.used = Some(2);
        assert!(GridDocument::Conflict(used).validate().is_err());
        let mut linked = conflict;
        linked.map_link = true;
        assert!(GridDocument::Conflict(linked).validate().is_err());
    }
}
