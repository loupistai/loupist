//! `grid_publish` (finance-grid-answers.md §3.2 and
//! finance-korea-retail-disagreement.md §3.3). A grid compares assets on
//! measures; its `conflict` layout shows where sources disagree. Every value
//! names this turn's acknowledged evidence and Rust checks it with the number
//! matcher, as it checks a number chip: the model's text never sets a status.
//! Under an as-of date every cell must be known by then.
use serde_json::Map;

use super::number_checks::{check_printed, kind_of, EvidenceLink};
use super::*;
use crate::chat::grid::*;
use crate::domain::{
    ChatEvidenceMatch, ChatEvidenceRef, NumberCheck, NumberCheckKind, NumberCheckStatus,
};
use crate::knowledge::numbers::{self, Printed, Unit};

/// What to do when an edit token does not match a current read.
const GRID_REVISE_NEXT: &str =
    "Call artifact_read for this artifact_id, then revise using its returned edit_token.";
const GRID_CHECK_NEXT: &str = "Cells marked differs or not_found do not match the source they cite: correct the value or cite the result it came from, then revise the grid with artifact_read's edit_token.";
/// Leaves of a payload a literal (date or text) cell is looked for in.
const MAX_LITERAL_LEAVES: usize = 50_000;

type Object = Map<String, Value>;

fn object_at<'a>(
    value: &'a Value,
    path: &str,
    required: &[&str],
    optional: &[&str],
) -> Result<&'a Object, BrokerError> {
    let object = value
        .as_object()
        .ok_or_else(|| BrokerError::invalid(path, "expected an object"))?;
    let at = |key: &str| {
        if path.is_empty() {
            key.to_owned()
        } else {
            format!("{path}.{key}")
        }
    };
    if let Some(key) = required.iter().find(|key| !object.contains_key(**key)) {
        return Err(BrokerError::invalid(at(key), "required field is missing"));
    }
    if let Some(key) = object
        .keys()
        .find(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(BrokerError::invalid(at(key), "unexpected field; omit it"));
    }
    Ok(object)
}

fn text_at(value: Option<&Value>, path: &str, chars: usize) -> Result<String, BrokerError> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| valid_text(text, chars))
        .map(str::to_owned)
        .ok_or_else(|| BrokerError::invalid(path, format!("expected 1–{chars} characters of text")))
}

fn optional_text_at(
    value: Option<&Value>,
    path: &str,
    chars: usize,
) -> Result<Option<String>, BrokerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => text_at(Some(value), path, chars).map(Some),
    }
}

fn array_at<'a>(
    value: Option<&'a Value>,
    path: &str,
    range: std::ops::RangeInclusive<usize>,
    what: &str,
) -> Result<&'a Vec<Value>, BrokerError> {
    value
        .and_then(Value::as_array)
        .filter(|items| range.contains(&items.len()))
        .ok_or_else(|| {
            BrokerError::invalid(
                path,
                format!(
                    "expected {}–{} {what}; narrow or split the table",
                    range.start(),
                    range.end()
                ),
            )
        })
}

fn enum_at<T: serde::de::DeserializeOwned>(
    value: Option<&Value>,
    path: &str,
    expected: &str,
) -> Result<T, BrokerError> {
    serde_json::from_value(value.cloned().unwrap_or(Value::Null))
        .map_err(|_| BrokerError::invalid(path, expected))
}

/// A grid value read as a number in its column's unit. A bare number in a
/// percent or multiple column is that unit; a small integer the answer parser
/// leaves to numbered citations is still a value in a grid. Dates and words
/// are not numbers.
fn read_value(value: &str, unit: GridUnit) -> Option<Printed> {
    if unit == GridUnit::Date {
        return None;
    }
    let column = |printed: Unit| match (printed, unit) {
        (Unit::Plain, GridUnit::Percent) => Unit::Percent,
        (Unit::Plain, GridUnit::Multiple) => Unit::Multiple,
        (other, _) => other,
    };
    if let Some(mut printed) = numbers::parse_printed(value) {
        printed.unit = column(printed.unit);
        return Some(printed);
    }
    let trimmed = value.trim();
    if trimmed.is_empty()
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_digit() || b"+-.,".contains(&byte))
    {
        return None;
    }
    let number = trimmed
        .replace(',', "")
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())?;
    let decimals = trimmed
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    Some(Printed {
        value: number,
        unit: column(Unit::Plain),
        precision: 0.5 * 10f64.powi(-(decimals as i32)),
    })
}

/// Whether any string or number leaf of `payload` contains `needle`.
fn payload_contains(payload: &Value, needle: &str) -> bool {
    let mut stack = vec![payload];
    let mut seen = 0usize;
    while let Some(value) = stack.pop() {
        seen += 1;
        if seen > MAX_LITERAL_LEAVES {
            return false;
        }
        match value {
            Value::String(text) if text.contains(needle) => return true,
            Value::Number(number) if number.to_string() == needle => return true,
            Value::Array(items) => stack.extend(items.iter()),
            Value::Object(map) => stack.extend(map.values()),
            _ => {}
        }
    }
    false
}

/// A date or a claim, found as printed in its source or not.
fn literal_check(
    link: &EvidenceLink,
    result: &RunResult,
    evidence: Option<&ChatEvidenceRef>,
) -> NumberCheck {
    let needle = link.text.trim();
    let kind = kind_of(result);
    let span = evidence
        .and_then(|evidence| evidence.excerpt.find(needle))
        .map(|start| ChatEvidenceMatch {
            start,
            end: start + needle.len(),
        });
    let found = span.is_some() || payload_contains(&result.payload, needle);
    NumberCheck {
        link_index: link.index,
        text: link.text.clone(),
        evidence_id: link.evidence_id.clone(),
        kind,
        status: if found {
            NumberCheckStatus::Matched
        } else {
            NumberCheckStatus::NotFound
        },
        nearest: None,
        exact: None,
        pointer: None,
        span: span.filter(|_| kind == NumberCheckKind::Passage),
        fields: Vec::new(),
        calc: None,
    }
}

/// `YYYY-MM-DD` from an ISO timestamp, a date or a compact `YYYYMMDD`.
fn day_of(text: &str) -> Option<chrono::NaiveDate> {
    let text = text.trim();
    let digits = text
        .get(..8)
        .filter(|day| day.bytes().all(|b| b.is_ascii_digit()));
    text.get(..10)
        .and_then(|day| chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
        .or_else(|| digits.and_then(|day| chrono::NaiveDate::parse_from_str(day, "%Y%m%d").ok()))
}

/// When the cited material was published or filed: the evidence's publication
/// date, then the matched row's filing date, then the result's. Retrieval time
/// never dates a report.
fn evidence_date(
    result: &RunResult,
    evidence: Option<&ChatEvidenceRef>,
    check: Option<&NumberCheck>,
) -> Option<chrono::NaiveDate> {
    const ROW_FIELDS: &[&str] = &["filed_at", "published_at", "rcept_dt"];
    let row = check
        .and_then(|check| check.pointer.as_deref())
        .and_then(|pointer| pointer.rsplit_once('/'))
        .and_then(|(parent, _)| result.payload.pointer(parent));
    evidence
        .and_then(|evidence| evidence.published_at.as_deref())
        .and_then(day_of)
        .or_else(|| {
            ROW_FIELDS
                .iter()
                .find_map(|field| row?.get(*field).and_then(Value::as_str).and_then(day_of))
        })
        .or_else(|| {
            ["/published_at", "/filed_at", "/provenance/published_at"]
                .iter()
                .find_map(|pointer| {
                    result
                        .payload
                        .pointer(pointer)
                        .and_then(Value::as_str)
                        .and_then(day_of)
                })
        })
}

/// Normalized quantity of a printed number: a comparable class, its value and
/// its rounding.
fn quantity(printed: &Printed) -> (u8, f64, f64) {
    match printed.unit {
        Unit::Amount(scale) => (0, printed.value * scale, printed.precision * scale),
        Unit::Percent => (1, printed.value, printed.precision),
        Unit::Points => (2, printed.value, printed.precision),
        Unit::BasisPoints => (2, printed.value / 100.0, printed.precision / 100.0),
        Unit::Multiple | Unit::Plain | Unit::Raw => (3, printed.value, printed.precision),
    }
}

/// Reports that say the same thing after normalization: one quantity within
/// the rounding of how each prints it, or the same words.
fn reports_agree(says: &[String]) -> bool {
    let printed: Vec<Option<Printed>> = says
        .iter()
        .map(|text| numbers::parse_printed(text))
        .collect();
    if printed.iter().all(Option::is_some) {
        let quantities: Vec<_> = printed.iter().flatten().map(quantity).collect();
        return quantities.iter().all(|(class, value, precision)| {
            quantities
                .iter()
                .all(|(other_class, other, other_precision)| {
                    class == other_class
                        && (value - other).abs()
                            <= precision.max(*other_precision)
                                + 1e-9 * value.abs().max(other.abs()).max(1.0)
                })
        });
    }
    if printed.iter().any(Option::is_some) {
        return false;
    }
    let words = |text: &str| {
        text.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    says.windows(2)
        .all(|pair| words(&pair[0]) == words(&pair[1]))
}

/// One cited result with the evidence projected from it.
struct Cited<'a> {
    result: &'a RunResult,
    evidence: Option<&'a ChatEvidenceRef>,
}

/// The acknowledged result a citation_url names, or the field error.
fn cited<'a>(
    registry: &'a RunResultRegistry,
    evidence_refs: &'a [ChatEvidenceRef],
    url: Option<&Value>,
    path: &str,
) -> Result<Cited<'a>, BrokerError> {
    let not_cited = || BrokerError::invalid(path, "not a citation_url from this turn");
    let url = url
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("#evidence-") && url.len() <= 256)
        .ok_or_else(not_cited)?;
    let result =
        evidence::cited_result(registry, url).map_err(|why| BrokerError::invalid(path, why))?;
    let id = evidence::evidence_id(&result.result_ref);
    let evidence = evidence_refs.iter().find(|item| item.id == id);
    Ok(Cited { result, evidence })
}

impl AppToolExecutor {
    /// Under an as-of date a cell shows only what was known by then: a local
    /// file has no publication date, and later material is left out.
    fn cell_within_cutoff(
        &self,
        policy: &ToolPolicy,
        path: &str,
        cited: &Cited<'_>,
        dated: Option<chrono::NaiveDate>,
    ) -> Result<(), BrokerError> {
        let Some(cutoff) = parse_as_of_date(policy.as_of.as_deref())? else {
            return Ok(());
        };
        if cited.result.producer.origin == RunResultOrigin::WorkspaceFile {
            return Err(BrokerError::Execution(format!(
                "as_of_unsupported: {path} cites a local file, which has no publication date to hold against {cutoff}"
            )));
        }
        if dated.is_some_and(|date| date > cutoff) {
            return Err(BrokerError::Execution(format!(
                "as_of_later: {path} cites material published after the as-of date {cutoff}; leave the cell empty with not_checked or cite what was known then"
            )));
        }
        Ok(())
    }

    async fn assets_grid(
        &self,
        policy: &ToolPolicy,
        params: &Object,
    ) -> Result<AssetsGrid, BrokerError> {
        let rows = array_at(
            params.get("rows"),
            "rows",
            MIN_GRID_ROWS..=MAX_GRID_ROWS,
            "rows",
        )?;
        let columns = array_at(
            params.get("columns"),
            "columns",
            1..=MAX_GRID_COLUMNS,
            "columns",
        )?;
        let mut grid_rows = Vec::with_capacity(rows.len());
        let mut identities = BTreeSet::new();
        for (index, row) in rows.iter().enumerate() {
            let path = format!("rows[{index}]");
            let object = object_at(row, &path, &["symbol", "market", "label"], &[])?;
            let symbol = text_at(
                object.get("symbol"),
                &format!("{path}.symbol"),
                SYMBOL_CHARS,
            )?;
            let market = text_at(
                object.get("market"),
                &format!("{path}.market"),
                MARKET_CHARS,
            )?;
            let label = text_at(
                object.get("label"),
                &format!("{path}.label"),
                ROW_LABEL_CHARS,
            )?;
            if !identities.insert((symbol.to_uppercase(), market.to_uppercase())) {
                return Err(BrokerError::invalid(
                    path,
                    "duplicate row: each symbol and market appears once",
                ));
            }
            // An exchange-style identity opens (or starts) the asset's page.
            let token = |text: &str| {
                text.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            };
            let asset_id = (token(&symbol) && token(&market))
                .then(|| crate::knowledge::assets::asset_id(&symbol, &market));
            grid_rows.push(GridRow {
                symbol,
                market,
                label,
                asset_id,
            });
        }
        let mut grid_columns = Vec::with_capacity(columns.len());
        let mut ids = BTreeSet::new();
        for (index, column) in columns.iter().enumerate() {
            let path = format!("columns[{index}]");
            let object = object_at(
                column,
                &path,
                &["id", "label", "unit"],
                &["period", "basis"],
            )?;
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| valid_column_id(id))
                .ok_or_else(|| {
                    BrokerError::invalid(
                        format!("{path}.id"),
                        "expected 1–64 letters, digits, _, . or -",
                    )
                })?
                .to_owned();
            if !ids.insert(id.clone()) {
                return Err(BrokerError::invalid(
                    format!("{path}.id"),
                    "duplicate column id",
                ));
            }
            grid_columns.push(GridColumn {
                id,
                label: text_at(
                    object.get("label"),
                    &format!("{path}.label"),
                    COLUMN_LABEL_CHARS,
                )?,
                period: optional_text_at(
                    object.get("period"),
                    &format!("{path}.period"),
                    PERIOD_CHARS,
                )?,
                basis: optional_text_at(
                    object.get("basis"),
                    &format!("{path}.basis"),
                    BASIS_CHARS,
                )?,
                unit: enum_at(
                    object.get("unit"),
                    &format!("{path}.unit"),
                    "expected amount, percent, multiple, number or date",
                )?,
            });
        }
        let cells = params
            .get("cells")
            .and_then(Value::as_array)
            .filter(|cells| {
                cells.len() == rows.len()
                    && cells
                        .iter()
                        .all(|row| row.as_array().is_some_and(|row| row.len() == columns.len()))
            })
            .ok_or_else(|| {
                BrokerError::invalid(
                    "cells",
                    format!(
                        "expected {} rows of {} cells, one per row and column in order",
                        rows.len(),
                        columns.len()
                    ),
                )
            })?;
        let evidence_refs = self.capture.evidence_refs.lock().await.clone();
        let registry = self.capture.run_results.lock().await;
        let mut grid_cells = Vec::with_capacity(cells.len());
        for (row_index, row) in cells.iter().enumerate() {
            let mut grid_row = Vec::with_capacity(columns.len());
            for (column_index, cell) in row.as_array().into_iter().flatten().enumerate() {
                let path = format!("cells[{row_index}][{column_index}]");
                let object = cell.as_object().ok_or_else(|| {
                    BrokerError::invalid(&path, "expected {value, citation_url} or {empty, note?}")
                })?;
                if object.contains_key("empty") {
                    let object = object_at(cell, &path, &["empty"], &["note"])?;
                    grid_row.push(GridCell::Empty(GridEmpty {
                        empty: enum_at(
                            object.get("empty"),
                            &format!("{path}.empty"),
                            "expected not_reported, not_applicable, unavailable or not_checked",
                        )?,
                        note: optional_text_at(
                            object.get("note"),
                            &format!("{path}.note"),
                            EMPTY_NOTE_CHARS,
                        )?,
                    }));
                    continue;
                }
                if !object.contains_key("value") {
                    return Err(BrokerError::invalid(
                        path,
                        "an empty cell needs a reason: {empty: not_reported | not_applicable | unavailable | not_checked}",
                    ));
                }
                let object = object_at(cell, &path, &["value", "citation_url"], &[])?;
                let value = text_at(object.get("value"), &format!("{path}.value"), VALUE_CHARS)?;
                let cited = cited(
                    &registry,
                    &evidence_refs,
                    object.get("citation_url"),
                    &format!("{path}.citation_url"),
                )?;
                let evidence_id = evidence::evidence_id(&cited.result.result_ref);
                let link = EvidenceLink {
                    index: (row_index * columns.len() + column_index) as u32,
                    text: value.clone(),
                    evidence_id: evidence_id.clone(),
                };
                let unit = grid_columns[column_index].unit;
                let printed = read_value(&value, unit);
                let sort = printed
                    .as_ref()
                    .map(|printed| quantity(printed).1)
                    .filter(|sort| sort.is_finite());
                let check = match printed {
                    Some(printed) => {
                        check_printed(&link, printed, cited.result, cited.evidence, &registry)
                    }
                    None => literal_check(&link, cited.result, cited.evidence),
                };
                let dated = evidence_date(cited.result, cited.evidence, Some(&check));
                self.cell_within_cutoff(policy, &path, &cited, dated)?;
                grid_row.push(GridCell::Value(Box::new(GridValue {
                    value,
                    evidence_id,
                    check,
                    sort,
                })));
            }
            grid_cells.push(grid_row);
        }
        drop(registry);
        let notes = match params.get("notes") {
            None | Some(Value::Null) => Vec::new(),
            Some(value) => {
                let notes = array_at(Some(value), "notes", 0..=MAX_GRID_NOTES, "notes")?;
                notes
                    .iter()
                    .enumerate()
                    .map(|(index, note)| {
                        text_at(Some(note), &format!("notes[{index}]"), NOTE_CHARS)
                    })
                    .collect::<Result<_, _>>()?
            }
        };
        Ok(AssetsGrid {
            subtitle: optional_text_at(params.get("subtitle"), "subtitle", SUBTITLE_CHARS)?,
            notes,
            rows: grid_rows,
            columns: grid_columns,
            cells: grid_cells,
            as_of: policy.as_of.clone(),
        })
    }

    /// World node ids this turn's acknowledged World reads delivered, and
    /// whether a disputed claim touches each.
    async fn world_entity(&self, entity_id: &str) -> Option<bool> {
        let reads = self.capture.staged_reads.lock().await;
        let mut delivered = false;
        let mut disputed = false;
        for read in reads.iter() {
            let AckedPayload::World { view_json, .. } = &read.payload else {
                continue;
            };
            let Ok(view) = serde_json::from_str::<Value>(view_json) else {
                continue;
            };
            delivered |= view
                .get("nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|node| node.get("id").and_then(Value::as_str) == Some(entity_id));
            disputed |= view
                .get("edges")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|edge| {
                    edge.get("status").and_then(Value::as_str) == Some("disputed")
                        && edge
                            .get("endpoints")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .any(|end| end.get("nodeId").and_then(Value::as_str) == Some(entity_id))
                });
        }
        delivered.then_some(disputed)
    }

    async fn conflict_grid(
        &self,
        policy: &ToolPolicy,
        params: &Object,
    ) -> Result<ConflictGrid, BrokerError> {
        let measure = object_at(
            params.get("measure").unwrap_or(&Value::Null),
            "measure",
            &["label", "entity_label"],
            &["period"],
        )?;
        let measure = ConflictMeasure {
            label: text_at(measure.get("label"), "measure.label", MEASURE_CHARS)?,
            entity_label: text_at(
                measure.get("entity_label"),
                "measure.entity_label",
                MEASURE_CHARS,
            )?,
            period: optional_text_at(measure.get("period"), "measure.period", PERIOD_CHARS)?,
        };
        let reports = array_at(
            params.get("reports"),
            "reports",
            MIN_CONFLICT_REPORTS..=MAX_CONFLICT_REPORTS,
            "reports",
        )?;
        let evidence_refs = self.capture.evidence_refs.lock().await.clone();
        let registry = self.capture.run_results.lock().await;
        let mut sources = BTreeSet::new();
        let mut conflict_reports = Vec::with_capacity(reports.len());
        for (index, report) in reports.iter().enumerate() {
            let path = format!("reports[{index}]");
            let object = object_at(report, &path, &["citation_url", "says"], &["basis"])?;
            let cited = cited(
                &registry,
                &evidence_refs,
                object.get("citation_url"),
                &format!("{path}.citation_url"),
            )?;
            let evidence_id = evidence::evidence_id(&cited.result.result_ref);
            if !sources.insert(evidence_id.clone()) {
                return Err(BrokerError::invalid(
                    format!("{path}.citation_url"),
                    "each report cites a different source",
                ));
            }
            let says = text_at(object.get("says"), &format!("{path}.says"), SAYS_CHARS)?;
            let link = EvidenceLink {
                index: index as u32,
                text: says.clone(),
                evidence_id: evidence_id.clone(),
            };
            // A number is checked against its own source; a claim is quoted as the report.
            let check = numbers::parse_printed(&says).map(|printed| {
                check_printed(&link, printed, cited.result, cited.evidence, &registry)
            });
            let dated = evidence_date(cited.result, cited.evidence, check.as_ref());
            self.cell_within_cutoff(policy, &path, &cited, dated)?;
            conflict_reports.push(ConflictReport {
                evidence_id,
                says,
                basis: optional_text_at(
                    object.get("basis"),
                    &format!("{path}.basis"),
                    BASIS_CHARS,
                )?,
                dated: dated.map(|date| date.to_string()),
                check,
            });
        }
        drop(registry);
        let says: Vec<String> = conflict_reports
            .iter()
            .map(|report| report.says.clone())
            .collect();
        if reports_agree(&says) {
            return Err(BrokerError::invalid(
                "reports",
                "these reports agree (within rounding); there is no disagreement to show",
            ));
        }
        let used = match params.get("used") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .filter(|index| (*index as usize) < conflict_reports.len())
                    .map(|index| index as u32)
                    .ok_or_else(|| {
                        BrokerError::invalid(
                            "used",
                            "expected the index of a report, or null when you cannot tell",
                        )
                    })?,
            ),
        };
        let (world_entity_id, map_link) = match params.get("world_entity_id") {
            None | Some(Value::Null) => (None, false),
            Some(value) => {
                let id = value
                    .as_str()
                    .filter(|id| !id.is_empty() && id.len() <= 256)
                    .ok_or_else(|| {
                        BrokerError::invalid("world_entity_id", "expected a World node id")
                    })?;
                let disputed = self.world_entity(id).await.ok_or_else(|| {
                    BrokerError::invalid(
                        "world_entity_id",
                        "not a World entity this turn's world_read delivered; omit it",
                    )
                })?;
                (Some(id.to_owned()), disputed)
            }
        };
        Ok(ConflictGrid {
            measure,
            reports: conflict_reports,
            reason: enum_at(
                params.get("reason"),
                "reason",
                "expected basis, period, preliminary_vs_final, estimate_vs_reported, currency_or_unit, adjusted_vs_reported, timing or unclear",
            )?,
            explanation: text_at(params.get("explanation"), "explanation", EXPLANATION_CHARS)?,
            used,
            used_reason: optional_text_at(params.get("used_reason"), "used_reason", USED_REASON_CHARS)?,
            world_entity_id,
            map_link,
            as_of: policy.as_of.clone(),
        })
    }

    pub(super) async fn grid_publish(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        let layout = params.get("layout").and_then(Value::as_str);
        let (required, optional): (&[&str], &[&str]) = match layout {
            Some("assets") => (
                &["mode", "layout", "title", "rows", "columns", "cells"],
                &["subtitle", "notes", "artifact_id", "edit_token"],
            ),
            Some("conflict") => (
                &[
                    "mode",
                    "layout",
                    "title",
                    "measure",
                    "reports",
                    "reason",
                    "explanation",
                ],
                &[
                    "used",
                    "used_reason",
                    "world_entity_id",
                    "artifact_id",
                    "edit_token",
                ],
            ),
            _ => {
                return Err(BrokerError::invalid(
                    "layout",
                    "expected assets or conflict",
                ))
            }
        };
        let object = object_at(&params, "", required, optional)?;
        let title = text_at(object.get("title"), "title", TITLE_CHARS)?;
        let mode = object.get("mode").and_then(Value::as_str);
        // Revising checks the edit token before the new content.
        let revising = match mode {
            Some("create") => {
                if object.contains_key("artifact_id") || object.contains_key("edit_token") {
                    return Err(BrokerError::invalid(
                        "mode",
                        "create does not accept artifact_id or edit_token; omit them or use revise",
                    ));
                }
                None
            }
            Some("revise") => Some(self.grid_to_revise(policy, object).await?),
            _ => return Err(BrokerError::invalid("mode", "expected create or revise")),
        };
        let grid = if layout == Some("assets") {
            GridDocument::Assets(self.assets_grid(policy, object).await?)
        } else {
            GridDocument::Conflict(self.conflict_grid(policy, object).await?)
        };
        grid.validate()
            .map_err(|error| BrokerError::Execution(error.to_string()))?;
        let (cell_checks, empty_cells) = grid_summary(&grid);
        let source_message_id = self
            .capture
            .source_message_id
            .clone()
            .ok_or(BrokerError::MethodDenied)?;
        let timestamp = now_ms();
        let payload = ChatArtifactPayload::Grid {
            schema: GRID_SCHEMA.into(),
            grid: Box::new(grid),
        };
        let commit = match revising {
            None => {
                let artifact_id = new_id("artifact");
                let revision = ChatArtifactRevision::new(
                    artifact_id.clone(),
                    1,
                    payload,
                    source_message_id,
                    timestamp,
                )
                .map_err(|error| BrokerError::Execution(error.to_string()))?;
                ArtifactCommit {
                    artifact: ChatArtifact {
                        id: artifact_id,
                        chat_session_id: policy.session_id.clone(),
                        kind: ChatArtifactKind::Grid,
                        title,
                        current_revision: 1,
                        created_at_ms: timestamp,
                        updated_at_ms: timestamp,
                    },
                    revision,
                    datasets: Vec::new(),
                }
            }
            Some(mut artifact) => {
                let created_at_ms = timestamp.max(artifact.updated_at_ms.saturating_add(1));
                artifact.title = title;
                artifact.current_revision =
                    artifact.current_revision.checked_add(1).ok_or_else(|| {
                        BrokerError::Execution("artifact revision limit reached".into())
                    })?;
                artifact.updated_at_ms = created_at_ms;
                let revision = ChatArtifactRevision::new(
                    artifact.id.clone(),
                    artifact.current_revision,
                    payload,
                    source_message_id,
                    created_at_ms,
                )
                .map_err(|error| BrokerError::Execution(error.to_string()))?;
                ArtifactCommit {
                    artifact,
                    revision,
                    datasets: Vec::new(),
                }
            }
        };
        let to_check = cell_checks.iter().any(|check| {
            matches!(
                check.get("status").and_then(Value::as_str),
                Some("differs" | "not_found")
            )
        });
        let mut staged = self
            .stage_artifact(
                commit,
                delivery_id,
                "The table will be saved only if this Chat turn completes.",
            )
            .await?;
        staged["layout"] = json!(layout);
        staged["cell_checks"] = json!(cell_checks);
        staged["empty_cells"] = json!(empty_cells);
        if to_check {
            staged["next"] = json!(GRID_CHECK_NEXT);
        }
        Ok(staged)
    }

    /// The grid a revise names, after its edit token matches a current read.
    async fn grid_to_revise(
        &self,
        policy: &ToolPolicy,
        object: &Object,
    ) -> Result<ChatArtifact, BrokerError> {
        let artifact_id = object
            .get("artifact_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                BrokerError::invalid(
                    "artifact_id",
                    "required when mode is revise; use artifact_read first",
                )
            })?;
        let edit_token = object
            .get("edit_token")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                BrokerError::invalid(
                    "edit_token",
                    "required when mode is revise; use the token returned by artifact_read",
                )
            })?;
        let artifact = self
            .state
            .store
            .get_chat_artifact(artifact_id)
            .map_err(|error| BrokerError::Execution(error.to_string()))?
            .ok_or_else(|| BrokerError::Execution("artifact was not found".into()))?;
        if artifact.chat_session_id != policy.session_id || artifact.kind != ChatArtifactKind::Grid
        {
            return Err(BrokerError::MethodDenied);
        }
        let current = self
            .state
            .store
            .get_chat_artifact_current(&artifact.id)
            .map_err(|error| BrokerError::Execution(error.to_string()))?
            .ok_or_else(|| BrokerError::Execution("artifact content was not found".into()))?;
        if chart_edit_token(&current) != edit_token
            || !self
                .capture
                .artifact_reads
                .lock()
                .await
                .contains(&(artifact.id.clone(), current.revision))
        {
            return Err(BrokerError::Execution(
                "edit_token does not match an acknowledged current grid read".into(),
            )
            .with_next(GRID_REVISE_NEXT));
        }
        Ok(artifact)
    }

    /// Under an as-of date a grid shows what was known then only when it was
    /// built under a cutoff on or before it.
    pub(super) fn grid_within_cutoff(
        &self,
        policy: &ToolPolicy,
        artifact_id: &str,
        grid: &GridDocument,
    ) -> Result<(), BrokerError> {
        let Some(cutoff) = parse_as_of_date(policy.as_of.as_deref())? else {
            return Ok(());
        };
        let built = grid
            .as_of()
            .and_then(|date| parse_as_of_date(Some(date)).ok().flatten());
        if built.is_some_and(|built| built <= cutoff) {
            return Ok(());
        }
        Err(BrokerError::Execution(format!(
            "as_of_unsupported: table {artifact_id} was not built under an as-of date on or before {cutoff}, so it cannot show only what was known then"
        ))
        .with_next("Read the sources again in this turn, where the as-of cutoff bounds each read, and publish a new grid."))
    }
}

/// Disputed claims a delivered World view holds (sources reporting opposite
/// things), so the read says so and the answer can explain them with a
/// conflict card (finance-korea-retail-disagreement.md §3.3).
pub(super) fn sources_disagree(view_json: &str) -> Option<Value> {
    const MAX_ENTITIES: usize = 8;
    let view = serde_json::from_str::<Value>(view_json).ok()?;
    let mut claims = 0usize;
    let mut entities: Vec<String> = Vec::new();
    for edge in view
        .get("edges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if edge.get("status").and_then(Value::as_str) != Some("disputed") {
            continue;
        }
        claims += 1;
        for end in edge
            .get("endpoints")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = end.get("nodeId").and_then(Value::as_str) {
                if entities.len() < MAX_ENTITIES && !entities.iter().any(|known| known == id) {
                    entities.push(id.to_owned());
                }
            }
        }
    }
    (claims > 0).then(|| {
        json!({
            "claims": claims,
            "entity_ids": entities,
            "next": "Sources disagree here. If the answer uses one of these claims, read each source (sourceTargets readArgs) and show the difference with grid_publish layout conflict, world_entity_id set to the entity; never average or silently pick one.",
        })
    })
}

/// Each value's status, and each empty cell's reason, for the model and evals.
fn grid_summary(grid: &GridDocument) -> (Vec<Value>, Vec<Value>) {
    let status = |check: &NumberCheck| serde_json::to_value(check.status).unwrap_or(Value::Null);
    match grid {
        GridDocument::Assets(grid) => {
            let mut checks = Vec::new();
            let mut empty = Vec::new();
            for (row, cells) in grid.cells.iter().enumerate() {
                for (column, cell) in cells.iter().enumerate() {
                    let cell_path = format!("cells[{row}][{column}]");
                    match cell {
                        GridCell::Value(value) => checks.push(json!({
                            "cell": cell_path,
                            "value": value.value,
                            "status": status(&value.check),
                        })),
                        GridCell::Empty(reason) => empty.push(json!({
                            "cell": cell_path,
                            "reason": reason.empty.as_str(),
                        })),
                    }
                }
            }
            (checks, empty)
        }
        GridDocument::Conflict(grid) => (
            grid.reports
                .iter()
                .enumerate()
                .filter_map(|(index, report)| {
                    report.check.as_ref().map(|check| {
                        json!({
                            "cell": format!("reports[{index}]"),
                            "value": report.says,
                            "status": status(check),
                        })
                    })
                })
                .collect(),
            Vec::new(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_value_reads_in_its_columns_unit_and_dates_are_not_numbers() {
        let read =
            |text: &str, unit| read_value(text, unit).map(|printed| (printed.value, printed.unit));
        assert_eq!(read("42.0", GridUnit::Percent), Some((42.0, Unit::Percent)));
        assert_eq!(
            read("1.2×", GridUnit::Multiple),
            Some((1.2, Unit::Multiple))
        );
        assert_eq!(read("1.2", GridUnit::Multiple), Some((1.2, Unit::Multiple)));
        // A small count is a value in a grid, not a numbered citation.
        assert_eq!(read("12", GridUnit::Number), Some((12.0, Unit::Plain)));
        assert_eq!(read("2026-03-18", GridUnit::Date), None);
        assert_eq!(read("2026-03-18", GridUnit::Number), None);
        assert_eq!(
            read("4,523억 원", GridUnit::Amount).map(|(_, unit)| unit),
            Some(Unit::Amount(1e8))
        );
    }

    #[test]
    fn reports_that_agree_after_normalization_are_not_a_conflict() {
        let says = |items: &[&str]| {
            items
                .iter()
                .map(|text| (*text).to_owned())
                .collect::<Vec<_>>()
        };
        assert!(reports_agree(&says(&["₩452.3bn", "4,523억 원"])));
        assert!(reports_agree(&says(&["14.8%", "14.80%"])));
        assert!(!reports_agree(&says(&["14.8%", "16.1%"])));
        assert!(!reports_agree(&says(&["₩452.3bn", "₩455.0bn"])));
        // A percent and an amount are different measures, never agreement.
        assert!(!reports_agree(&says(&["14.8%", "₩14.8"])));
        assert!(reports_agree(&says(&[
            "Raised guidance",
            "raised  guidance"
        ])));
        assert!(!reports_agree(&says(&["Raised guidance", "Cut guidance"])));
    }

    #[test]
    fn report_dates_come_from_the_evidence_never_retrieval() {
        assert_eq!(
            day_of("2026-03-18T06:00:00Z"),
            chrono::NaiveDate::from_ymd_opt(2026, 3, 18)
        );
        assert_eq!(
            day_of("20260318"),
            chrono::NaiveDate::from_ymd_opt(2026, 3, 18)
        );
        assert_eq!(day_of("Mar 18"), None);
    }
}
