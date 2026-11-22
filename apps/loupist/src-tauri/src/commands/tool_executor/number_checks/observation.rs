//! Present a matched observation using the retained row's display fields.
use super::{
    located_links, new_check, numbers, printed, split_pointer, EvidenceLink, NumberCheck,
    NumberCheckContext, NumberCheckKind, NumberCheckStatus, Place, Verdict, MAX_NUMBER_CHECKS,
};
use crate::datasets::saved_rows::SavedRows;
use serde_json::Value;

pub(super) fn row_context(payload: &Value, pointer: &str) -> Option<NumberCheckContext> {
    let (parent, key) = split_pointer(pointer)?;
    // Only the data body or query rows supply a preview. Envelope metadata,
    // the tool argument echo and the MCP text copy aren't observations.
    if ![
        "/data/",
        "/facts/",
        "/structuredContent/results/",
        "/rows/",
        "/results/",
    ]
    .iter()
    .any(|prefix| pointer.starts_with(prefix))
    {
        return None;
    }
    if matches!(
        key.as_str(),
        "text" | "content" | "body" | "markdown" | "excerpt"
    ) {
        return None;
    }
    let row = payload.pointer(parent)?;
    row.as_object()?;
    let value = grouped(&short(row.get(&key)?, 44)?);
    let series = &payload["data"]["series"];
    let observations = parent.starts_with("/data/observations/");
    let metadata = &payload["structuredContent"]["extra"]["results_metadata"][&key];
    let series_label = observations
        .then(|| first(series, &["indicator_name", "name", "label", "metric"], 50))
        .flatten();
    let label = series_label
        .or_else(|| {
            SavedRows::parse(payload)
                .and_then(|data| data.column_label(&key))
                .and_then(|label| short(&Value::String(label.to_owned()), 50))
        })
        .or_else(|| first(metadata, &["title", "name"], 50))
        .or_else(|| {
            matches!(key.as_str(), "value" | "val" | "amount")
                .then(|| first(row, &["label", "metric", "concept", "account_nm"], 50))
                .flatten()
        })
        .or_else(|| {
            (key.chars().count() <= 50 && !key.contains(char::is_control))
                .then(|| key.replace('_', " "))
        })?;
    let entity = ["corp_name", "entity", "company", "name", "symbol"]
        .iter()
        .find_map(|key| short(row.get(*key)?, 36))
        .or_else(|| {
            observations
                .then(|| short(&series["economy_name"], 36))
                .flatten()
        });
    let period = [
        "period",
        "date",
        "end",
        "day",
        "fiscal_year",
        "bsns_year",
        "year",
        "stck_bsop_date",
        "BAS_DD",
    ]
    .iter()
    .find_map(|key| short(row.get(*key)?, 24));
    let scale = row
        .get("scale")
        .and_then(|value| short(value, 18))
        .filter(|value| {
            value
                .parse::<f64>()
                .is_ok_and(|scale| scale.is_finite() && scale > 0.0 && scale != 1.0)
        });
    let unit = ["unit", "currency"]
        .iter()
        .find_map(|key| short(row.get(*key)?, 28))
        .or_else(|| observations.then(|| short(&series["unit"], 28)).flatten())
        .or_else(|| short(&metadata["units"], 28));

    let mut text = label;
    for part in [entity, period].into_iter().flatten() {
        text.push_str(" · ");
        text.push_str(&part);
    }
    text.push_str(" · ");
    let start = text.len();
    text.push_str(&value);
    let end = text.len();
    if let Some(scale) = scale {
        text.push_str(" × ");
        text.push_str(&grouped(&scale));
    }
    if let Some(unit) = unit {
        text.push(' ');
        text.push_str(&unit);
    }
    Some(NumberCheckContext { text, start, end })
}

fn first(row: &Value, keys: &[&str], limit: usize) -> Option<String> {
    keys.iter().find_map(|key| short(row.get(*key)?, limit))
}

fn short(value: &Value, limit: usize) -> Option<String> {
    let value = printed(value)?;
    let text = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty() && text.chars().count() <= limit && !text.contains(char::is_control))
        .then_some(text)
}

/// Group ordinary decimal digits without re-parsing or rounding the value.
fn grouped(text: &str) -> String {
    let sign = text.starts_with(['-', '+', '−']);
    let digits = if sign {
        &text[text.chars().next().unwrap().len_utf8()..]
    } else {
        text
    };
    let (integer, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if integer.is_empty()
        || !integer.bytes().all(|c| c.is_ascii_digit())
        || !fraction.bytes().all(|c| c.is_ascii_digit())
    {
        return text.to_owned();
    }
    let mut value = text[..text.len() - digits.len()].to_owned();
    for (index, digit) in integer.bytes().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            value.push(',');
        }
        value.push(char::from(digit));
    }
    value.push_str(&digits[integer.len()..]);
    value
}

/// Reconstruct the figure location from the exact, admitted saved Source.
/// A single compatible observation supplies the display context and focus.
pub(in crate::commands) struct SavedFigureChecks<'a> {
    data: &'a SavedRows<'a>,
    candidates: Vec<numbers::Candidate>,
}

impl<'a> SavedFigureChecks<'a> {
    pub fn new(data: &'a SavedRows<'a>) -> Self {
        let candidates = numbers::data_candidates(data.payload)
            .into_iter()
            .filter(|candidate| {
                let Place::Pointer(path) = &candidate.place else {
                    return false;
                };
                data.cell(path)
                    .is_some_and(|(_, key)| data.series().is_none() || key == "value")
            })
            .collect();
        Self { data, candidates }
    }

    fn locate(&self, figure: &str) -> Option<(&str, NumberCheckContext, Verdict)> {
        let value = numbers::parse_printed(figure)?;
        let (verdict, index) = numbers::check_citation(&value, &self.candidates, false);
        if !matches!(verdict, Verdict::Matched | Verdict::Rounded) {
            return None;
        }
        let index = index?;
        let matches = self
            .candidates
            .iter()
            .enumerate()
            .any(|(other, candidate)| {
                index != other
                    && numbers::check_citation(&value, std::slice::from_ref(candidate), false).0
                        == verdict
            });
        if matches {
            return None;
        }
        let Place::Pointer(pointer) = &self.candidates[index].place else {
            return None;
        };
        Some((pointer, row_context(self.data.payload, pointer)?, verdict))
    }

    pub fn pointer(&self, content: &str, index: u32, evidence_id: &str) -> Option<&str> {
        let (_, link) = located_links(content).into_iter().nth(index as usize)?;
        if link.evidence_id != evidence_id {
            return None;
        }
        self.locate(&link.text).map(|(pointer, _, _)| pointer)
    }

    pub fn project(&self, content: &str, evidence_id: &str, checks: &mut Vec<NumberCheck>) {
        for (_, link) in located_links(content) {
            if link.evidence_id == evidence_id {
                self.project_link(&link, checks);
            }
        }
        checks.sort_by_key(|check| check.link_index);
    }

    fn project_link(&self, link: &EvidenceLink, checks: &mut Vec<NumberCheck>) {
        let Some((_, context, verdict)) = self.locate(&link.text) else {
            return;
        };
        if let Some(check) = checks.iter_mut().find(|check| {
            check.link_index == link.index
                && check.evidence_id == link.evidence_id
                && check.text == link.text
        }) {
            if matches!(
                check.status,
                NumberCheckStatus::Matched | NumberCheckStatus::Rounded
            ) {
                check.span = None;
                check.context = Some(context);
            }
        } else if verdict == Verdict::Matched && checks.len() < MAX_NUMBER_CHECKS {
            let mut check = new_check(link, NumberCheckKind::Passage, NumberCheckStatus::Matched);
            check.context = Some(context);
            checks.push(check);
        }
    }
}
