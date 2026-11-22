//! Number checks at finalization (finance-show-the-math.md §7.2). Each
//! `[text](#evidence-…)` link in the answer whose whole text reads as one
//! number is compared with the evidence it links, using this turn's retained
//! results. The model's text never sets a status; Rust reads the number it
//! printed and looks for it in the source.
mod answer_units;
mod diagnostics;
mod observation;
pub(in crate::commands) use observation::SavedFigureChecks;
pub(crate) mod replay;
use super::*;
use crate::domain::{
    answer_checks::{
        bounded_calc, bounded_fields, clip, NumberCheckContext, NumberCheckScaleConversion,
        MAX_NUMBER_CHECK_CONTEXT_CHARS,
    },
    ChatEvidenceMatch, ChatEvidenceRef, NumberCheck, NumberCheckCalc, NumberCheckField,
    NumberCheckInput, NumberCheckKind, NumberCheckStatus, MAX_NUMBER_CHECKS,
};
use crate::knowledge::numbers::{self, Candidate, Place, Unit, Verdict};

/// One `[text](#evidence-…)` link, in answer order.
pub(in crate::commands) struct EvidenceLink {
    pub index: u32,
    pub text: String,
    pub evidence_id: String,
}

/// Restore the citation target when the model copied its result-field name.
/// Use Markdown offsets so labels, titles, code spans and fences stay verbatim.
pub(in crate::commands) fn normalize_evidence_link_aliases(content: &str) -> String {
    use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};
    let parser = Parser::new(content);
    let alias = |target: &str| {
        let plain = target
            .strip_prefix("citation_url=")
            .or_else(|| target.strip_prefix("citation_url:"))?;
        evidence_id(plain).map(|_| target.len() - plain.len())
    };
    let mut removals = Vec::new();
    for (_, definition) in parser.reference_definitions().iter() {
        if let Some(prefix_len) = alias(&definition.dest) {
            let source = &content[definition.span.clone()];
            if let Some(label_end) = source.find("]:") {
                if let Some(offset) = source[label_end + 2..].find(definition.dest.as_ref()) {
                    let start = definition.span.start + label_end + 2 + offset;
                    removals.push(start..start + prefix_len);
                }
            }
        }
    }
    let mut open = None;
    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::Link {
                dest_url,
                link_type: LinkType::Inline,
                ..
            }) => {
                open = alias(&dest_url)
                    .map(|prefix_len| (dest_url, range.clone(), range.start, prefix_len));
            }
            Event::End(TagEnd::Link) => {
                if let Some((target, link, label_end, prefix_len)) = open.take() {
                    if let Some(offset) = content[label_end..link.end].find(target.as_ref()) {
                        let start = label_end + offset;
                        removals.push(start..start + prefix_len);
                    }
                }
            }
            _ => {
                if let Some((_, _, label_end, _)) = &mut open {
                    *label_end = range.end;
                }
            }
        }
    }
    removals.sort_by_key(|range| range.start);
    removals.dedup();
    let mut normalized = content.to_owned();
    for range in removals.into_iter().rev() {
        normalized.replace_range(range, "");
    }
    normalized
}

fn evidence_id(target: &str) -> Option<&str> {
    target.strip_prefix("#evidence-").filter(|id| {
        !id.is_empty()
            && id.len() <= 128
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

fn located_links(content: &str) -> Vec<(usize, EvidenceLink)> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut links = Vec::new();
    let mut open: Option<(usize, String, String)> = None;
    for (event, pos) in Parser::new(content).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                open = evidence_id(&dest_url).map(|id| (pos.start, id.to_owned(), String::new()));
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, _, label)) = &mut open {
                    label.push_str(&text);
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some((at, evidence_id, text)) = open.take() {
                    links.push((
                        at,
                        EvidenceLink {
                            index: links.len() as u32,
                            text,
                            evidence_id,
                        },
                    ));
                }
            }
            _ => {}
        }
    }
    links
}

pub(in crate::commands) fn kind_of(result: &RunResult) -> NumberCheckKind {
    let tool = result.producer.tool_name.as_str();
    if tool == "calculate" {
        NumberCheckKind::Calculated
    } else if matches!(tool, "dataset_query" | "dataset_describe")
        || matches!(
            result.producer.origin,
            RunResultOrigin::Derived | RunResultOrigin::WorkspaceFile
        )
    {
        NumberCheckKind::Step
    } else if tool == "fx_rate" || result.payload.get("dataset_ref").is_some() {
        NumberCheckKind::Data
    } else {
        NumberCheckKind::Passage
    }
}

fn calc_unit(unit: Option<&str>) -> Unit {
    match unit {
        Some("%") => Unit::Percent,
        Some("x") => Unit::Multiple,
        other => Unit::of_line(other).unwrap_or(Unit::Plain),
    }
}

/// A calculation's values, inputs and DCF grid. A conversion verifies its
/// converted output alone, in base currency units.
fn calc_candidates(payload: &Value) -> Vec<Candidate> {
    let mut out = Vec::new();
    let conversion = payload["operation"] == "convert";
    for (list, unit_of) in [("values", true), ("inputs", false)] {
        if conversion && !unit_of {
            break;
        }
        for (index, item) in payload
            .get(list)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            if conversion && item["name"] != "converted" {
                continue;
            }
            let Some(value) = item.get("value").and_then(Value::as_f64) else {
                continue;
            };
            let unit = if conversion {
                Unit::Amount(1.0)
            } else if unit_of {
                calc_unit(item.get("unit").and_then(Value::as_str))
            } else {
                Unit::Plain
            };
            let currency = (!conversion && matches!(unit, Unit::Amount(_)))
                .then(|| item["unit"].as_str().and_then(numbers::stated_currency))
                .flatten();
            out.push(Candidate {
                value,
                unit,
                currency,
                place: Place::Pointer(format!("/{list}/{index}/value")),
                inferred_scale: false,
                direction: None,
            });
        }
    }
    if matches!(
        payload["operation"].as_str(),
        Some("reverse_dcf" | "dcf_grid")
    ) {
        extend_dcf_candidates(payload, &mut out);
    }
    out
}

fn extend_dcf_candidates(payload: &Value, out: &mut Vec<Candidate>) {
    let grid = &payload["grid"];
    let unit = calc_unit(grid["unit"].as_str());
    let currency = grid["unit"].as_str().and_then(numbers::stated_currency);
    for (row, cells) in grid["values"].as_array().into_iter().flatten().enumerate() {
        for (column, cell) in cells.as_array().into_iter().flatten().enumerate() {
            if let Some(value) = cell.as_f64() {
                out.push(Candidate {
                    value,
                    unit,
                    currency,
                    place: Place::Pointer(format!("/grid/values/{row}/{column}")),
                    inferred_scale: false,
                    direction: None,
                });
            }
        }
    }
    for axis in ["row_values", "column_values"] {
        for (index, cell) in grid[axis].as_array().into_iter().flatten().enumerate() {
            if let Some(value) = cell.as_f64() {
                out.push(Candidate {
                    value,
                    unit: Unit::Percent,
                    currency: None,
                    place: Place::Pointer(format!("/grid/{axis}/{index}")),
                    inferred_scale: false,
                    direction: None,
                });
            }
        }
    }

    // The first reverse DCF formula prints the enterprise value the solver
    // targets. Use its retained inputs so the formula's prose isn't parsed.
    if payload["operation"] == "reverse_dcf" && payload["formulas"][0].is_string() {
        let input = |name: &str| {
            payload["inputs"]
                .as_array()?
                .iter()
                .find(|item| item["name"] == name)?["value"]
                .as_f64()
        };
        if let (Some(price), Some(shares)) = (input("price"), input("shares")) {
            out.push(Candidate {
                value: crate::calculations::reverse_dcf_target(
                    price,
                    shares,
                    input("net_debt").unwrap_or_default(),
                ),
                unit: calc_unit(grid["amount_unit"].as_str()),
                currency: grid["amount_unit"]
                    .as_str()
                    .and_then(numbers::stated_currency),
                place: Place::Pointer("/formulas/0".into()),
                inferred_scale: false,
                direction: None,
            });
        }
    }
}

/// The text a JSON value prints as.
fn printed(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// The parent pointer and last token of a JSON Pointer.
pub(super) fn split_pointer(pointer: &str) -> Option<(&str, String)> {
    let (parent, last) = pointer.rsplit_once('/')?;
    Some((parent, last.replace("~1", "/").replace("~0", "~")))
}

/// Row fields that say what a value is, in the order the popover names them:
/// the entity, the concept, then the period and basis.
const DESCRIPTIVE_FIELDS: &[&str] = &[
    "corp_name",
    "entity",
    "company",
    "name",
    "symbol",
    "canonical_concept",
    "concept",
    "account_nm",
    "label",
    "metric",
    "period",
    "fiscal_year",
    "bsns_year",
    "date",
    "fs_div",
    "basis",
];

/// Up to four short fields of the matched row: the matched cell and the
/// fields that say what it is, descriptive ones first.
fn row_fields(payload: &Value, pointer: &str) -> Vec<NumberCheckField> {
    let Some((parent, key)) = split_pointer(pointer) else {
        return Vec::new();
    };
    let Some(row) = payload.pointer(parent).and_then(Value::as_object) else {
        return Vec::new();
    };
    let short = |value: &Value| printed(value).filter(|text| text.chars().count() <= 64);
    let mut fields: Vec<NumberCheckField> = DESCRIPTIVE_FIELDS
        .iter()
        .filter(|name| **name != key)
        .filter_map(|name| {
            Some(NumberCheckField {
                key: (*name).to_owned(),
                value: short(row.get(*name)?)?,
                matched: false,
            })
        })
        .take(3)
        .collect();
    if let Some(value) = row.get(&key).and_then(short) {
        fields.push(NumberCheckField {
            key,
            value,
            matched: true,
        });
    }
    bounded_fields(fields)
}

fn evidence_for_dataset<'a>(
    registry: &'a RunResultRegistry,
    dataset: &Value,
) -> Option<&'a RunResult> {
    registry.values().find(|result| {
        !matches!(
            result.producer.tool_name.as_str(),
            "dataset_query" | "dataset_describe" | "calculate"
        ) && result.payload.get("dataset_ref").is_some_and(|reference| {
            reference.get("dataset_id") == dataset.get("dataset_id")
                && reference.get("revision") == dataset.get("revision")
        })
    })
}

fn calc_detail(payload: &Value, registry: &RunResultRegistry) -> NumberCheckCalc {
    let text_list = |key: &str| {
        payload
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let inputs = payload
        .get("inputs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|input| {
            let provenance = input.get("provenance").unwrap_or(&Value::Null);
            let (evidence_id, status) = match provenance.get("kind").and_then(Value::as_str) {
                Some("cited") => (
                    provenance
                        .get("evidence_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    provenance
                        .get("status")
                        .cloned()
                        .and_then(|status| serde_json::from_value(status).ok()),
                ),
                Some("dataset") => (
                    evidence_for_dataset(registry, provenance)
                        .map(|result| evidence::evidence_id(&result.result_ref)),
                    None,
                ),
                _ => (None, None),
            };
            NumberCheckInput {
                label: input
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                unit: input
                    .get("unit")
                    .cloned()
                    .and_then(|unit| serde_json::from_value(unit).ok())
                    .unwrap_or_default(),
                name: input
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                display: input
                    .get("display")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                evidence_id,
                status,
                currency_basis: input["currency_basis"].as_str().map(str::to_owned),
            }
        })
        .collect();
    let grid = payload.get("grid").and_then(|grid| {
        let count = |key: &str| grid.get(key).and_then(Value::as_array).map(Vec::len);
        Some([count("row_values")? as u32, count("column_values")? as u32])
    });
    bounded_calc(NumberCheckCalc {
        operation: payload
            .get("operation")
            .cloned()
            .and_then(|operation| serde_json::from_value(operation).ok()),
        title: payload
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Calculation")
            .to_owned(),
        formulas: text_list("formulas"),
        inputs,
        grid,
    })
}

fn status_of(verdict: Verdict) -> NumberCheckStatus {
    match verdict {
        Verdict::Matched => NumberCheckStatus::Matched,
        Verdict::Rounded => NumberCheckStatus::Rounded,
        Verdict::Differs => NumberCheckStatus::Differs,
        Verdict::NotFound => NumberCheckStatus::NotFound,
    }
}

/// Where a candidate read from a text field sits in that text, as UTF-8 byte
/// offsets: the number with the same value and unit, or the field's only one.
fn locate(
    text: &str,
    candidate: &Candidate,
    amount: Option<&numbers::Printed>,
) -> Option<(usize, usize)> {
    let candidates = numbers::checked_passage_candidates(text, amount);
    let found = candidates.iter().find(|found| {
        found.value == candidate.value && (found.unit == candidate.unit || candidates.len() == 1)
    })?;
    match found.place {
        Place::Span { start, end } => Some((start, end)),
        Place::Pointer(_) => None,
    }
}

/// How a data cell prints the candidate read from it: a text field's number
/// as printed in that text, never the whole text.
fn printed_at(
    payload: &Value,
    pointer: &str,
    candidate: &Candidate,
    amount: Option<&numbers::Printed>,
) -> Option<String> {
    match payload.pointer(pointer)? {
        Value::String(text) => Some(
            locate(text, candidate, amount)
                .map_or_else(|| text.clone(), |(start, end)| text[start..end].to_owned()),
        ),
        other => printed(other),
    }
}

/// Locate the selected fallback candidate again in its content field. The
/// candidate's pointer came from `content_candidates`, never from metadata.
fn retained_context(
    payload: &Value,
    candidate: &Candidate,
    amount: Option<&numbers::Printed>,
) -> Option<NumberCheckContext> {
    let Place::Pointer(pointer) = &candidate.place else {
        return None;
    };
    let text = printed(payload.pointer(pointer)?)?;
    let (start, end) = if let Some(found) = locate(&text, candidate, amount) {
        found
    } else if numbers::passage_candidates(&text).is_empty() {
        // Scalar content values can be numbers that passage parsing excludes,
        // such as a bare year. Their whole trimmed field is the candidate.
        let trimmed = text.trim();
        let start = text.len() - text.trim_start().len();
        (start, start + trimmed.len())
    } else {
        return None;
    };
    context_at(&text, start, end)
}

fn context_at(text: &str, start: usize, end: usize) -> Option<NumberCheckContext> {
    let matched_chars = text.get(start..end)?.chars().count();
    let remaining = MAX_NUMBER_CHECK_CONTEXT_CHARS.checked_sub(matched_chars)?;
    let before = text[..start].chars().rev().take(remaining).count();
    let after = text[end..].chars().take(remaining).count();
    let right = after.min(remaining - before.min(remaining / 2));
    let left = before.min(remaining - right);
    let from = if left == 0 {
        start
    } else {
        text[..start].char_indices().rev().nth(left - 1)?.0
    };
    let through = text[end..]
        .char_indices()
        .nth(right)
        .map_or(text.len(), |(offset, _)| end + offset);
    Some(NumberCheckContext {
        text: text[from..through].to_owned(),
        start: start - from,
        end: end - from,
    })
}

/// A host-verified literal can be just an accession or period inside an
/// SEC observation. In that case the selection covers its record.
fn citation_range(evidence: &ChatEvidenceRef) -> Option<ChatEvidenceMatch> {
    let anchor = evidence
        .excerpt_match
        .as_ref()
        .filter(|anchor| anchor.is_valid_for(&evidence.excerpt))?;
    let text = &evidence.excerpt;
    let left = text.ceil_char_boundary(anchor.start.saturating_sub(512));
    let right = text.floor_char_boundary((anchor.end + 512).min(text.len()));
    let record = text[left..anchor.start]
        .rfind('{')
        .zip(text[anchor.end..right].find('}'))
        .map(|(start, end)| ChatEvidenceMatch {
            start: left + start,
            end: anchor.end + end + 1,
        })
        .filter(|record| {
            serde_json::from_str::<Value>(&text[record.start..record.end]).is_ok_and(|row| {
                row["val"].is_number() && row["end"].is_string() && row["accn"].is_string()
            })
        });
    Some(record.unwrap_or_else(|| anchor.clone()))
}

/// Earlier adjacent bytes in the same retained web capture may contain the
/// table heading. Only candidates inside the cited excerpt can then verify.
fn passage_window<'a>(
    excerpt: &'a str,
    result: &'a RunResult,
    registry: &'a RunResultRegistry,
) -> Option<(std::borrow::Cow<'a, str>, usize)> {
    let content = result.payload["content"].as_str()?;
    if excerpt.is_empty() {
        return None;
    }
    let offset = content.find(excerpt)?;
    if content.rfind(excerpt) != Some(offset) {
        return None;
    }
    if offset > 0 {
        return Some((content.into(), offset));
    }
    if result.producer.runtime_id != "native-web" || result.producer.tool_name != "web_fetch" {
        return None;
    }
    let start = usize::try_from(result.payload["content_offset"].as_u64()?).ok()?;
    let digest = result.payload["content_sha256"].as_str()?;
    let source_id = result.payload["source_id"].as_str()?;
    result.upstream_result_refs.iter().find_map(|parent| {
        let parent = registry.get(parent)?;
        let payload = &parent.payload;
        if parent.producer.runtime_id != "native-web"
            || payload["content_sha256"] != digest
            || payload["source_id"] != source_id
        {
            return None;
        }
        let earlier = payload["content"].as_str()?;
        let earlier_start = usize::try_from(payload["content_offset"].as_u64()?).ok()?;
        let offset = start.checked_sub(earlier_start)?;
        let prefix = earlier.get(..offset).filter(|prefix| !prefix.is_empty())?;
        let tail = earlier.get(offset..)?;
        if !content.starts_with(tail) && !tail.starts_with(content) {
            return None;
        }
        Some((format!("{prefix}{content}").into(), offset))
    })
}

fn passage_candidates(
    excerpt: &str,
    amount: Option<&numbers::Printed>,
    result: &RunResult,
    registry: &RunResultRegistry,
    captured: Option<&str>,
) -> Vec<Candidate> {
    let window = captured
        .and_then(|window| {
            let prefix = window.strip_suffix(excerpt)?;
            Some((std::borrow::Cow::Borrowed(window), prefix.len()))
        })
        .or_else(|| passage_window(excerpt, result, registry));
    let Some((window, offset)) = window else {
        return numbers::checked_passage_candidates(excerpt, amount);
    };
    numbers::checked_passage_candidates(&window, amount)
        .into_iter()
        .filter_map(|mut candidate| {
            let Place::Span { start, end } = candidate.place else {
                return None;
            };
            let (start, end) = (start.checked_sub(offset)?, end.checked_sub(offset)?);
            excerpt.get(start..end)?;
            candidate.place = Place::Span { start, end };
            Some(candidate)
        })
        .collect()
}

/// Checks one printed number against one retained result.
#[cfg(test)]
pub(in crate::commands) fn check_link(
    link: &EvidenceLink,
    result: &RunResult,
    evidence: Option<&ChatEvidenceRef>,
    registry: &RunResultRegistry,
) -> Option<NumberCheck> {
    let printed_number = numbers::parse_printed(&link.text)?;
    Some(check_printed(
        link,
        printed_number,
        result,
        evidence,
        registry,
    ))
}

/// Checks a number already read from `link.text` (a grid cell reads its
/// column's unit into a bare value) against one retained result.
pub(in crate::commands) fn check_printed(
    link: &EvidenceLink,
    printed_number: numbers::Printed,
    result: &RunResult,
    evidence: Option<&ChatEvidenceRef>,
    registry: &RunResultRegistry,
) -> NumberCheck {
    check_with_window(link, printed_number, result, evidence, registry, None)
}

fn check_with_window(
    link: &EvidenceLink,
    mut printed_number: numbers::Printed,
    result: &RunResult,
    evidence: Option<&ChatEvidenceRef>,
    registry: &RunResultRegistry,
    captured: Option<&str>,
) -> NumberCheck {
    let kind = kind_of(result);
    let payload = &result.payload;
    let excerpt = evidence.map(|evidence| evidence.excerpt.as_str());
    let fred_series = result.producer.runtime_id == "openbb"
        && result.producer.tool_name == "economy_fred_series";
    let python = kind == NumberCheckKind::Step && result.producer.tool_name == "run_python";
    let amount = (matches!(printed_number.unit, Unit::Amount(_))
        && (kind == NumberCheckKind::Passage || python))
        .then_some(printed_number);
    // Saved Sources can embed either a web capture or a dataset. Preserve
    // decoded passage bytes for web results and select the row for datasets.
    let saved = (result.producer.tool_name == "world_read")
        .then(|| payload["text"].as_str())
        .flatten()
        .filter(|text| text.trim_start().starts_with('{'))
        .and_then(|text| serde_json::from_str::<Value>(text).ok());
    let saved_page = saved.as_ref().filter(|page| page["content"].is_string());
    let saved_data = saved.as_ref().filter(|data| {
        result.producer.origin == RunResultOrigin::SavedContext
            && payload["scope"] == "source"
            && data["dataset_ref"].is_object()
            && (data["data"].is_object()
                || data["data"].is_array()
                || data["structuredContent"].is_object())
    });
    let mut candidates = match kind {
        NumberCheckKind::Data if fred_series => {
            numbers::series_observations(payload, &printed_number)
        }
        NumberCheckKind::Calculated => calc_candidates(payload),
        NumberCheckKind::Step if python => {
            let mut out = numbers::checked_passage_candidates(
                payload["stdout"].as_str().unwrap_or_default(),
                amount.as_ref(),
            );
            for candidate in &mut out {
                candidate.place = Place::Pointer("/stdout".into());
            }
            out
        }
        NumberCheckKind::Step
            if matches!(
                result.producer.tool_name.as_str(),
                "dataset_query" | "chart_query"
            ) =>
        {
            numbers::query_candidates(payload)
        }
        NumberCheckKind::Data | NumberCheckKind::Step => numbers::data_candidates(payload),
        NumberCheckKind::Passage => match (saved_data, saved_page) {
            (Some(data), _) => numbers::data_candidates(data),
            (_, Some(_)) => Vec::new(),
            _ => passage_candidates(
                excerpt.unwrap_or_default(),
                amount.as_ref(),
                result,
                registry,
                captured,
            ),
        },
    };
    let calculated = kind == NumberCheckKind::Calculated;
    let target_currency = (calculated && payload["operation"] == "convert")
        .then(|| payload["conversion"]["to"].as_str())
        .flatten();
    if target_currency.is_some() && printed_number.unit == Unit::Plain {
        printed_number.unit = Unit::Amount(1.0);
    }
    // A literal passage selection identifies the occurrence the citation
    // targeted. Other figures in the surrounding window remain citable.
    let preferred = evidence.and_then(citation_range).map_or(0, |anchor| {
        let inside = |candidate: &Candidate| {
            matches!(candidate.place,
                Place::Span { start, end } if start < anchor.end && end > anchor.start)
        };
        candidates.sort_by_key(|candidate| !inside(candidate));
        candidates
            .iter()
            .take_while(|candidate| inside(candidate))
            .count()
    });
    let selected = numbers::check_citation(&printed_number, &candidates[..preferred], false);
    let (mut verdict, mut index) = if selected.0 == Verdict::NotFound {
        numbers::check_citation(&printed_number, &candidates, calculated)
    } else {
        selected
    };
    // The agent's own code prints a percent or point change as a bare number,
    // in the figure's unit (`'qoq_pp': -1.374…`) or as a fraction (`0.0770…`
    // for 7.70%). Read a bare number in its output both ways: as printed in
    // the figure's unit (matched), else as a fraction ×100 (rounded). The
    // step's other fields, such as its exit code, are not output.
    if python
        && matches!(printed_number.unit, Unit::Percent | Unit::Points)
        && verdict != Verdict::Matched
    {
        let output = Place::Pointer("/stdout".into());
        let bare_in_unit = |scale: f64| {
            candidates
                .iter()
                .map(|candidate| {
                    if matches!(candidate.unit, Unit::Plain | Unit::Raw)
                        && candidate.place == output
                    {
                        Candidate {
                            value: candidate.value * scale,
                            unit: printed_number.unit,
                            ..candidate.clone()
                        }
                    } else {
                        candidate.clone()
                    }
                })
                .collect::<Vec<_>>()
        };
        for (scale, reading) in [(1.0, Verdict::Matched), (100.0, Verdict::Rounded)] {
            if let (Verdict::Matched, at) =
                numbers::check(&printed_number, &bare_in_unit(scale), false)
            {
                (verdict, index) = (reading, at);
                break;
            }
        }
    }
    if target_currency.is_some_and(|target| {
        numbers::unambiguous_currency_markers(&link.text)
            .iter()
            .any(|currency| *currency != target)
    }) {
        verdict = Verdict::Differs;
    }
    // Unsigned data magnitudes may describe a fall in words. Explicit signs
    // (including accounting marks) always retain their ordinary meaning.
    if kind == NumberCheckKind::Data
        && !fred_series
        && verdict == Verdict::NotFound
        && !printed_number.signed
        && printed_number.value > 0.0
    {
        let negative = numbers::Printed {
            value: -printed_number.value,
            ..printed_number
        };
        (verdict, index) = numbers::check_citation(&negative, &candidates, false);
        if verdict != Verdict::NotFound {
            printed_number = negative;
        }
    }
    let mut found = index.and_then(|index| candidates.get(index));
    let mut context = None;
    let full_payload = saved_page.unwrap_or(payload);
    let full_candidates;
    let mut matched_payload = saved_data.unwrap_or(payload);
    let mut matched_saved_data = saved_data.is_some();
    if kind == NumberCheckKind::Passage && verdict == Verdict::NotFound {
        // The retained excerpt is bounded. Only content fields count, never
        // the result's metadata.
        full_candidates = numbers::content_candidates(full_payload, amount.as_ref());
        let (whole, index) = numbers::check_citation(&printed_number, &full_candidates, false);
        if matches!(whole, Verdict::Matched | Verdict::Rounded) {
            found = index.and_then(|index| full_candidates.get(index));
            context = found
                .and_then(|candidate| retained_context(full_payload, candidate, amount.as_ref()));
            matched_payload = full_payload;
            matched_saved_data = false;
            verdict = whole;
        }
    }
    let mut check = new_check(link, kind, status_of(verdict));
    check.context = context;
    check.calc = calculated.then(|| calc_detail(payload, registry));
    if verdict == Verdict::NotFound {
        diagnostics::explain(
            &mut check,
            &printed_number,
            result,
            evidence,
            saved_page,
            &candidates,
        );
        return check;
    }
    let Some(found) = found else {
        return check;
    };
    // How the evidence prints the matched (or nearest) value.
    let as_printed = match &found.place {
        Place::Span { start, end } => excerpt
            .and_then(|text| text.get(*start..*end))
            .map(str::to_owned),
        Place::Pointer(pointer) if calculated => {
            if pointer.starts_with("/grid/values/") {
                let unit = payload["grid"]["unit"].as_str().unwrap_or_default();
                Some(crate::calculations::display(found.value, unit))
            } else if pointer.starts_with("/grid/") {
                Some(crate::calculations::display(found.value, "%"))
            } else if pointer == "/formulas/0" {
                Some(crate::calculations::display_amount(found.value))
            } else {
                let display = pointer.trim_end_matches("/value").to_owned() + "/display";
                payload
                    .pointer(&display)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }
        }
        Place::Pointer(pointer) => printed_at(matched_payload, pointer, found, amount.as_ref()),
    }
    .map(|text| clip(text.trim(), 256));
    match verdict {
        Verdict::Matched => {
            check.unstated_scale = numbers::unstated_scale(&printed_number, found);
        }
        Verdict::Differs => check.nearest = as_printed,
        Verdict::Rounded => {
            check.scale_conversion = as_printed.as_ref().and_then(|_| {
                let (source_scale, answer_scale) =
                    numbers::exact_scale_conversion(&printed_number, found)?;
                Some(NumberCheckScaleConversion {
                    source_scale,
                    answer_scale,
                    currency: found
                        .currency
                        .or(printed_number.currency)
                        .map(str::to_owned),
                })
            });
            if check.scale_conversion.is_some() {
                check.status = NumberCheckStatus::Matched;
            }
            check.exact = as_printed;
        }
        _ => {}
    }
    if verdict != Verdict::Differs {
        match &found.place {
            Place::Span { start, end } => {
                check.span = Some(ChatEvidenceMatch {
                    start: *start,
                    end: *end,
                })
            }
            Place::Pointer(pointer) => {
                if kind != NumberCheckKind::Passage {
                    check.pointer = Some(clip(pointer, 2048));
                }
                if kind == NumberCheckKind::Data {
                    check.fields = row_fields(payload, pointer);
                }
                if matched_saved_data
                    || kind == NumberCheckKind::Data
                    || result.producer.tool_name == "dataset_query"
                {
                    check.context = observation::row_context(matched_payload, pointer);
                }
            }
        }
    }
    check
}

fn new_check(link: &EvidenceLink, kind: NumberCheckKind, status: NumberCheckStatus) -> NumberCheck {
    NumberCheck {
        link_index: link.index,
        text: clip(&link.text, 256),
        evidence_id: link.evidence_id.clone(),
        kind,
        status,
        compared_value: None,
        nearest_source_unit: None,
        nearest_source_scale: None,
        comparison_reason: None,
        comparison_replay: None,
        nearest: None,
        exact: None,
        scale_conversion: None,
        unstated_scale: None,
        pointer: None,
        span: None,
        context: None,
        fields: Vec::new(),
        calc: None,
    }
}

impl ToolCapture {
    /// Checks every number link in the finished answer against this turn's
    /// evidence. Links to evidence this turn did not retain are left to the
    /// renderer's "not available in this reply".
    pub(in crate::commands) async fn number_checks(
        &self,
        content: &str,
        evidence: &[ChatEvidenceRef],
    ) -> Vec<NumberCheck> {
        let links = located_links(content);
        if links.is_empty() {
            return Vec::new();
        }
        let governing = answer_units::governing_units(content);
        let registry = self.run_results.lock().await;
        let by_id: BTreeMap<String, &RunResult> = registry
            .values()
            .map(|result| (evidence::evidence_id(&result.result_ref), result))
            .collect();
        let mut checks = Vec::new();
        for (at, link) in &links {
            if checks.len() >= MAX_NUMBER_CHECKS {
                break;
            }
            let Some(result) = by_id.get(&link.evidence_id) else {
                continue;
            };
            let evidence = evidence.iter().find(|item| item.id == link.evidence_id);
            if let Some(printed) = numbers::parse_with_unit(&link.text, governing.get(at).copied())
            {
                let mut check = check_printed(link, printed, result, evidence, &registry);
                if result.producer.runtime_id == "native-web"
                    && check.kind == NumberCheckKind::Passage
                    && (check.unstated_scale
                        == Some(crate::domain::answer_checks::UnstatedScale::Source)
                        || (matches!(printed.unit, Unit::Amount(_))
                            && check.status == NumberCheckStatus::NotFound))
                {
                    // A literal selection can start below a caption even while the
                    // original acquired representation is still retained this turn.
                    let wider = || {
                        let evidence = evidence?;
                        let offset =
                            usize::try_from(result.payload["content_offset"].as_u64()?).ok()?;
                        let source_id = result.payload["source_id"].as_str()?;
                        let digest = result.payload["content_sha256"].as_str()?;
                        let snapshots = self.web_fetch_snapshots.try_lock().ok()?;
                        let original = &snapshots.get(source_id)?.fetched;
                        if !original.matches_source(source_id, digest) {
                            return None;
                        }
                        let window = original.number_check_window(offset, &evidence.excerpt)?;
                        if window.len() == evidence.excerpt.len() {
                            return None;
                        }
                        Some(check_with_window(
                            link,
                            printed,
                            result,
                            Some(evidence),
                            &registry,
                            Some(window),
                        ))
                    };
                    if let Some(verified) = wider() {
                        check = verified;
                    }
                }
                checks.push(check);
            }
        }
        checks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence_links(content: &str) -> Vec<EvidenceLink> {
        located_links(content)
            .into_iter()
            .map(|(_, link)| link)
            .collect()
    }

    struct NoDatasets;
    impl crate::calculations::DatasetSource for NoDatasets {
        fn dataset(
            &mut self,
            _: &str,
            _: u32,
        ) -> Result<(crate::datasets::DatasetRef, std::sync::Arc<Value>), String> {
            Err("no datasets".into())
        }
    }

    fn result(tool: &str, origin: RunResultOrigin, payload: Value) -> RunResult {
        RunResult {
            result_ref: format!("result:{tool}"),
            producer: RunResultProducer {
                origin,
                runtime_id: "native".into(),
                tool_name: tool.into(),
                provider: None,
            },
            origin: None,
            request_digest: "a".repeat(64),
            response_digest: "b".repeat(64),
            retrieved_at: "2026-03-18T00:00:00Z".into(),
            payload,
            model_delivery: None,
            warnings: vec![],
            upstream_result_refs: vec![],
        }
    }

    #[test]
    fn v007_world_bank_observation_reads_the_row_in_a_saved_source() {
        let data = json!({
            "data": {
                "series": {
                    "indicator_id": "SP.POP.TOTL", "indicator_name": "Population, total",
                    "economy_id": "JPN", "economy_name": "Japan", "unit": ""
                },
                "observations": [
                    {"date":"2023","period":"2023","value":"124516650","decimal":0},
                    {"date":"2024","period":"2024","value":"123975371","decimal":0}
                ]
            },
            "dataset_ref": {"dataset_id":"internal-dataset-id","revision":1}
        });
        let direct = result(
            "worldbank_series",
            RunResultOrigin::AcquiredOriginal,
            data.clone(),
        );
        let saved = result(
            "world_read",
            RunResultOrigin::SavedContext,
            json!({
                "scope":"source", "text":data.to_string()
            }),
        );
        for source in [&direct, &saved] {
            let evidence = evidence::project(source);
            let check = check_link(
                &link("123,975,371"),
                source,
                Some(&evidence),
                &RunResultRegistry::default(),
            )
            .unwrap();
            assert_eq!(check.status, NumberCheckStatus::Matched);
            assert!(check.span.is_none());
            let context = check.context.unwrap();
            assert_eq!(
                context.text,
                "Population, total · Japan · 2024 · 123,975,371"
            );
            assert_eq!(&context.text[context.start..context.end], "123,975,371");
            assert!(evidence.excerpt.contains("internal-dataset-id"));
        }
    }

    #[test]
    fn v007_market_data_excerpt_uses_the_matched_cell_and_unit() {
        let source = result(
            "equity_price_historical",
            RunResultOrigin::AcquiredOriginal,
            json!({
                "structuredContent":{"results":[
                    {"symbol":"ACME","date":"2026-01-12","close":"1220.58","currency":"USD"},
                    {"symbol":"ACME","date":"2026-01-13","close":"1234.567","currency":"USD","request_id":"internal-row"}
                ]},
                "dataset_ref":{"dataset_id":"internal-dataset-id","revision":1}
            }),
        );
        let evidence = evidence::project(&source);
        let check = check_link(
            &link("USD 1,234.567"),
            &source,
            Some(&evidence),
            &RunResultRegistry::default(),
        )
        .unwrap();
        assert_eq!(check.status, NumberCheckStatus::Matched);
        assert_eq!(
            check.pointer.as_deref(),
            Some("/structuredContent/results/1/close")
        );
        let context = check.context.unwrap();
        assert_eq!(context.text, "close · ACME · 2026-01-13 · 1,234.567 USD");
        assert_eq!(&context.text[context.start..context.end], "1,234.567");
    }

    #[test]
    fn v008_pasted_revenue_binds_the_scale_to_each_amount() {
        let text = concat!(
            "The 2024 training dataset records equipment revenue of 30 and services revenue of 20 USD million. ",
            "The 2025 dataset records equipment revenue of 40 and services revenue of 25 USD million."
        );
        let source = result(
            "pasted_capture",
            RunResultOrigin::UserProvided,
            json!({"content": text}),
        );
        let evidence = evidence::project(&source);
        for (answer, original) in [
            ("$20 million", "20 USD million"),
            ("$25 million", "25 USD million"),
        ] {
            let check = check_link(
                &link(answer),
                &source,
                Some(&evidence),
                &RunResultRegistry::default(),
            )
            .unwrap();
            assert_eq!(check.status, NumberCheckStatus::Matched, "{check:?}");
            let span = check.span.unwrap();
            assert_eq!(&text[span.start..span.end], original);
        }
    }

    #[test]
    fn aliases_normalize_only_link_targets_outside_code() {
        let content = concat!(
            "가격 [$162.85](citation_url=#evidence-ev_price), [8.4%](<citation_url:#evidence-ev_margin> \"citation_url=#evidence-ev_title\").\n",
            "[`citation_url=#evidence-ev_label`](citation_url=#evidence-ev_label).\n",
            "[1,234][ref]\n\n[ref]: citation_url:#evidence-ev_ref \"citation_url=#evidence-ev_title\"\n\n",
            "`[9%](citation_url=#evidence-ev_code)`\n\n```md\n[9%](citation_url=#evidence-ev_fence)\n```\n\n",
            "    [9%](citation_url=#evidence-ev_indent)\n\n",
            "citation_url=#evidence-ev_prose [invalid](citation_url=#evidence-ev-bad) [url](https://example.com/citation_url=#evidence-ev_url)"
        );
        let expected = content
            .replace(
                "](citation_url=#evidence-ev_price)",
                "](#evidence-ev_price)",
            )
            .replace(
                "<citation_url:#evidence-ev_margin>",
                "<#evidence-ev_margin>",
            )
            .replace(
                "](citation_url=#evidence-ev_label)",
                "](#evidence-ev_label)",
            )
            .replace(
                "[ref]: citation_url:#evidence-ev_ref",
                "[ref]: #evidence-ev_ref",
            );
        let normalized = normalize_evidence_link_aliases(content);
        assert_eq!(normalized, expected);
        assert_eq!(normalize_evidence_link_aliases(&normalized), normalized);
        let links = evidence_links(&normalized);
        assert_eq!(
            links
                .iter()
                .map(|link| link.evidence_id.as_str())
                .collect::<Vec<_>>(),
            ["ev_price", "ev_margin", "ev_label", "ev_ref"]
        );
    }

    #[test]
    fn links_are_read_in_order_outside_code() {
        let links = evidence_links(
            "Margin was [8.4%](#evidence-ev_a) on [revenue](#evidence-ev_b) [1](#evidence-ev_c).\n\n```\n[9%](#evidence-ev_d)\n```\n",
        );
        let texts: Vec<_> = links.iter().map(|link| link.text.as_str()).collect();
        assert_eq!(texts, ["8.4%", "revenue", "1"]);
        assert_eq!(links[2].index, 2);
        assert_eq!(links[0].evidence_id, "ev_a");
    }

    #[test]
    fn kis_investor_trade_values_verify_the_cited_scale() {
        let kis = result(
            "kis_read",
            RunResultOrigin::AcquiredOriginal,
            json!({"dataset_ref": {"dataset_id": "ds_flow", "revision": 1},
                "source_id": crate::finance_data::KIS_SOURCE_ID,
                "query": {"operation_id": "domestic_stock.investor_trade_by_stock_daily"},
                "data": {"response": {"output2": [{"frgn_ntby_tr_pbmn": "2140", "orgn_ntby_tr_pbmn": -610}]}}}),
        );
        let registry = RunResultRegistry::default();
        let check = |text| check_link(&link(text), &kis, None, &registry).unwrap();
        let buys = check("+21.4억 원");
        assert_eq!(buys.status, NumberCheckStatus::Matched);
        assert_eq!(
            buys.pointer.as_deref(),
            Some("/data/response/output2/0/frgn_ntby_tr_pbmn")
        );
        assert_eq!(check("-6.1억 원").status, NumberCheckStatus::Matched);
        assert_eq!(check("+214억 원").status, NumberCheckStatus::NotFound);
        let unstated = check("+2140원");
        assert_eq!(unstated.status, NumberCheckStatus::Matched);
        assert_eq!(
            unstated.unstated_scale,
            Some(crate::domain::answer_checks::UnstatedScale::Answer)
        );
    }

    #[test]
    fn kis_uses_only_documented_operation_and_field_units() {
        let registry = RunResultRegistry::default();
        for (operation, container, field, expected) in [
            (
                "domestic_stock.investor_trade_by_stock_daily",
                "output2",
                "etc_corp_ntby_tr_pbmn",
                NumberCheckStatus::Matched,
            ),
            (
                "domestic_stock.investor_trade_by_stock_daily",
                "output2",
                "frgn_shnu_tr_pbmn",
                NumberCheckStatus::NotFound,
            ),
            (
                "domestic_stock.investor_trade_by_stock_daily",
                "output2",
                "unknown_ntby_tr_pbmn",
                NumberCheckStatus::NotFound,
            ),
            (
                "domestic_stock.investor_trade_by_stock_daily",
                "output1",
                "frgn_ntby_tr_pbmn",
                NumberCheckStatus::NotFound,
            ),
            (
                "domestic_stock.inquire_investor",
                "output",
                "frgn_ntby_tr_pbmn",
                NumberCheckStatus::NotFound,
            ),
        ] {
            let kis = result(
                "kis_read",
                RunResultOrigin::AcquiredOriginal,
                json!({
                    "dataset_ref": {"dataset_id": "ds_flow", "revision": 1},
                    "source_id": crate::finance_data::KIS_SOURCE_ID,
                    "query": {"operation_id": operation},
                    "data": {"response": {(container): [{(field): "2140"}]}}
                }),
            );
            let check = check_link(&link("+21.4억 원"), &kis, None, &registry).unwrap();
            assert_eq!(check.status, expected, "{operation} {container} {field}");
        }
    }

    #[test]
    fn data_checks_point_at_the_matched_row() {
        let dart = result(
            "dart_financials",
            RunResultOrigin::AcquiredOriginal,
            json!({"dataset_ref": {"dataset_id": "ds_1", "revision": 1}, "facts": [
                {"corp_name": "Solsup Foods", "canonical_concept": "revenue", "period": "FY2025", "fs_div": "CFS",
                 "value": "452312000000", "unit": "KRW", "scale": "1", "rcept_no": "20260318000123"}
            ]}),
        );
        let registry = RunResultRegistry::default();
        let link = EvidenceLink {
            index: 0,
            text: "4,523억 원".into(),
            evidence_id: "ev".into(),
        };
        let check = check_link(&link, &dart, None, &registry).unwrap();
        assert_eq!(check.kind, NumberCheckKind::Data);
        assert_eq!(check.status, NumberCheckStatus::Rounded);
        assert_eq!(check.pointer.as_deref(), Some("/facts/0/value"));
        assert_eq!(check.exact.as_deref(), Some("452312000000"));
        assert_eq!(check.fields.len(), 4);
        assert!(check
            .fields
            .iter()
            .any(|field| field.matched && field.key == "value"));
        // A link whose text is not a number is not checked.
        let words = EvidenceLink {
            text: "revenue".into(),
            ..link
        };
        assert!(check_link(&words, &dart, None, &registry).is_none());
    }

    #[test]
    fn calculated_checks_carry_the_formula_and_report_differences() {
        let calc = result(
            "calculate",
            RunResultOrigin::Derived,
            json!({
                "operation": "margin", "title": "Margin",
                "values": [{"name": "margin", "value": 8.4234, "display": "8.42%", "unit": "%"}],
                "formulas": ["Operating income / Revenue = 38.1 / 452.3 = 8.42%"],
                "inputs": [
                    {"name": "profit", "value": 38.1, "display": "38.1", "provenance": {"kind": "cited", "evidence_id": "ev_dart", "status": "rounded"}},
                    {"name": "revenue", "value": 452.3, "display": "452.3", "provenance": {"kind": "provided"}}
                ]
            }),
        );
        let registry = RunResultRegistry::default();
        let link = |text: &str| EvidenceLink {
            index: 1,
            text: text.into(),
            evidence_id: "ev_calc".into(),
        };
        let matched = check_link(&link("8.4%"), &calc, None, &registry).unwrap();
        assert_eq!(matched.status, NumberCheckStatus::Matched);
        let detail = matched.calc.unwrap();
        assert_eq!(detail.formulas.len(), 1);
        assert_eq!(detail.inputs[0].evidence_id.as_deref(), Some("ev_dart"));
        assert_eq!(detail.inputs[0].status, Some(NumberCheckStatus::Rounded));
        assert_eq!(detail.inputs[1].evidence_id, None);
        let differs = check_link(&link("9.1%"), &calc, None, &registry).unwrap();
        assert_eq!(differs.status, NumberCheckStatus::Differs);
        assert_eq!(differs.nearest.as_deref(), Some("8.42%"));
    }

    #[test]
    fn dcf_grid_checks_the_r5_t050_answer_and_rejects_a_wrong_cell() {
        let fixture: Value =
            serde_json::from_str(include_str!("fixtures/r5-t050-calculation.json")).unwrap();
        let mut payload =
            crate::calculations::calculate(&fixture["request"], &mut NoDatasets).unwrap();
        payload["grid"] = fixture["grid"].clone();
        let calc = result("calculate", RunResultOrigin::Derived, payload);
        let answer = include_str!("fixtures/r5-t050-answer.md");
        let registry = RunResultRegistry::default();
        let checks = |answer| {
            evidence_links(answer)
                .iter()
                .filter(|link| link.evidence_id == "ev_reverse_dcf")
                .map(|link| check_link(link, &calc, None, &registry).unwrap())
                .collect::<Vec<_>>()
        };
        let correct = checks(answer);
        assert_eq!(correct.len(), 13);
        let differs: Vec<_> = correct
            .iter()
            .filter(|check| check.status == NumberCheckStatus::Differs)
            .map(|check| &check.text)
            .collect();
        assert!(differs.is_empty(), "correct figures flagged: {differs:?}");
        let wrong = answer.replace("[5.3%]", "[5.5%]");
        let wrong_checks = checks(&wrong);
        let differs: Vec<_> = wrong_checks
            .iter()
            .filter(|check| check.status == NumberCheckStatus::Differs)
            .collect();
        assert_eq!(differs.len(), 1);
        assert_eq!(differs[0].text, "5.5%");
        assert_eq!(differs[0].nearest.as_deref(), Some("5.27%"));
    }

    #[test]
    fn dcf_grid_checks_forward_amounts_and_percent_axes() {
        let payload = crate::calculations::calculate(
            &json!({"operation": "dcf_grid", "unit": "KRW billions",
                "wacc_pct": [7, 8, 9], "terminal_growth_pct": [1, 2],
                "inputs": {"cash_flow": 100, "growth_pct": 2}}),
            &mut NoDatasets,
        )
        .unwrap();
        let calc = result("calculate", RunResultOrigin::Derived, payload);
        let registry = RunResultRegistry::default();
        for (text, pointer) in [
            ("₩1.7tn", "/grid/values/1/1"),
            ("8%", "/grid/row_values/1"),
            ("1%", "/grid/column_values/0"),
        ] {
            let link = EvidenceLink {
                index: 0,
                text: text.into(),
                evidence_id: "ev_calc".into(),
            };
            let check = check_link(&link, &calc, None, &registry).unwrap();
            assert!(
                matches!(
                    check.status,
                    NumberCheckStatus::Matched | NumberCheckStatus::Rounded
                ),
                "{text}: {:?}",
                check.status
            );
            assert_eq!(check.pointer.as_deref(), Some(pointer), "{text}");
        }
    }

    /// The popover reads each input's label and unit from the stored check,
    /// copied from the host's own calculation (B98).
    #[test]
    fn calculated_checks_keep_each_inputs_label_and_unit() {
        let payload = crate::calculations::calculate(
            &json!({"operation": "margin", "unit": "KRW billions",
                "inputs": {"profit": "38.1", "revenue": "452.3"},
                "labels": {"profit": "Operating income"}}),
            &mut NoDatasets,
        )
        .unwrap();
        let calc = result("calculate", RunResultOrigin::Derived, payload);
        let link = EvidenceLink {
            index: 0,
            text: "8.4%".into(),
            evidence_id: "ev_calc".into(),
        };
        let check = check_link(&link, &calc, None, &RunResultRegistry::default()).unwrap();
        let inputs = check.calc.unwrap().inputs;
        let amount = crate::domain::CalcInputUnit {
            currency: Some("KRW".into()),
            scale: 9,
            ..crate::domain::CalcInputUnit::of(crate::domain::CalcInputUnitKind::Amount)
        };
        assert_eq!(
            inputs
                .iter()
                .map(|input| (input.label.as_str(), &input.unit, input.display.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Operating income", &amount, "38.1"),
                ("Revenue", &amount, "452.3")
            ]
        );
        // The stored answer round-trips them, and an older check without them still reads.
        let stored = serde_json::to_value(&inputs[0]).unwrap();
        assert_eq!(
            stored["unit"],
            json!({"kind": "amount", "currency": "KRW", "scale": 9})
        );
        let older: crate::domain::NumberCheckInput =
            serde_json::from_value(json!({"name": "profit", "display": "38.1"})).unwrap();
        assert_eq!(
            (older.label.as_str(), older.unit.kind),
            ("", crate::domain::CalcInputUnitKind::Number)
        );
    }

    #[test]
    fn a_passage_falls_back_to_the_results_content_never_its_metadata() {
        let page = result(
            "web_fetch",
            RunResultOrigin::AcquiredOriginal,
            json!({"status": 200, "content_bytes": 45231, "content_offset": 1200,
                "title": "Solsup Foods annual report", "url": "https://solsup.example/ir/2026",
                "content": "The company sold 4,523 cars; the plant employs 1,820 people."}),
        );
        let registry = RunResultRegistry::default();
        let link = |text: &str| EvidenceLink {
            index: 0,
            text: text.into(),
            evidence_id: "ev_page".into(),
        };
        // The bounded excerpt is empty here, so the whole page's content decides.
        let check = |text: &str| check_link(&link(text), &page, None, &registry).unwrap();
        assert_eq!(check("4,523").status, NumberCheckStatus::Matched);
        assert_eq!(check("1,820").status, NumberCheckStatus::Matched);
        // Sizes and offsets of the page are not numbers in the source.
        assert_eq!(check("45,231").status, NumberCheckStatus::NotFound);
        assert_eq!(check("1,200").status, NumberCheckStatus::NotFound);
    }

    #[test]
    fn fallback_matches_carry_the_first_matched_number_as_printed() {
        for (content, answer, matched, status) in [
            (
                "The company sold 4,523 cars; later it sold 4,523 more.",
                "4,523",
                "4,523",
                NumberCheckStatus::Matched,
            ),
            (
                "매출은 1조 2,345억원이며 증가했다.",
                "1조 2,345억원",
                "1조 2,345억원",
                NumberCheckStatus::Matched,
            ),
            (
                "(in millions)\nNet revenue................ $ 35,926",
                "$35.926bn",
                "$ 35,926",
                NumberCheckStatus::Matched,
            ),
            (
                "Revenue was $12 thousand then $12 million.",
                "$12,000,000",
                "$12 million",
                NumberCheckStatus::Matched,
            ),
        ] {
            let page = result(
                "web_fetch",
                RunResultOrigin::AcquiredOriginal,
                json!({"content": content, "text": "Later: 4,523."}),
            );
            let check = check_link(
                &EvidenceLink {
                    index: 0,
                    text: answer.into(),
                    evidence_id: "ev_page".into(),
                },
                &page,
                None,
                &RunResultRegistry::default(),
            )
            .unwrap();
            assert_eq!(check.status, status);
            assert!(check.span.is_none());
            let context = check.context.unwrap();
            assert_eq!(context.text, content);
            assert_eq!(&context.text[context.start..context.end], matched);
            assert_eq!(context.start, content.find(matched).unwrap());
        }
        for (payload, answer, source) in [
            (json!({"content": 4523}), "4,523", "4523"),
            (json!({"value": " 4523 "}), "4,523", "4523"),
            (json!({"value": 2025}), "2,025", "2025"),
        ] {
            let page = result("web_fetch", RunResultOrigin::AcquiredOriginal, payload);
            let check = check_link(
                &EvidenceLink {
                    index: 0,
                    text: answer.into(),
                    evidence_id: "ev_page".into(),
                },
                &page,
                None,
                &RunResultRegistry::default(),
            )
            .unwrap();
            assert_eq!(check.status, NumberCheckStatus::Matched);
            let context = check.context.unwrap();
            assert_eq!(&context.text[context.start..context.end], source);
        }
    }

    #[test]
    fn fallback_context_cuts_long_multibyte_content_around_the_match() {
        let content = format!(
            "{}매출 4,523억원 기록{}",
            "앞🌲".repeat(180),
            "뒤🌳".repeat(180)
        );
        let page = result(
            "web_fetch",
            RunResultOrigin::AcquiredOriginal,
            json!({"passages": [{"text": content}]}),
        );
        let check = check_link(
            &EvidenceLink {
                index: 0,
                text: "4,523억원".into(),
                evidence_id: "ev_page".into(),
            },
            &page,
            None,
            &RunResultRegistry::default(),
        )
        .unwrap();
        assert_eq!(check.status, NumberCheckStatus::Matched);
        let context = check.context.unwrap();
        assert_eq!(context.text.chars().count(), 240);
        assert!(content.contains(&context.text));
        assert_eq!(&context.text[context.start..context.end], "4,523억원");
        assert!(context.start > 0 && context.end < context.text.len());
    }

    #[test]
    fn excerpt_matches_missing_numbers_and_non_row_data_keep_their_locations() {
        let page = result(
            "web_fetch",
            RunResultOrigin::AcquiredOriginal,
            json!({"content": "Sales were 4,523."}),
        );
        let evidence: ChatEvidenceRef = serde_json::from_value(json!({
            "id":"ev_page", "title":"Sales", "kind":"original", "tool_name":"web_fetch",
            "retrieved_at":"2026-11-17T00:00:00Z", "excerpt":"Sales were 4,523.",
            "excerpt_truncated":false, "content_sha256":"a".repeat(64), "warnings":[]
        }))
        .unwrap();
        let link = |text: &str| EvidenceLink {
            index: 0,
            text: text.into(),
            evidence_id: "ev_page".into(),
        };
        let registry = RunResultRegistry::default();
        let excerpt = check_link(&link("4,523"), &page, Some(&evidence), &registry).unwrap();
        assert_eq!(excerpt.status, NumberCheckStatus::Matched);
        let span = excerpt.span.unwrap();
        assert_eq!(&evidence.excerpt[span.start..span.end], "4,523");
        assert!(excerpt.context.is_none());
        let missing = check_link(&link("9,999"), &page, None, &registry).unwrap();
        assert_eq!(missing.status, NumberCheckStatus::NotFound);
        assert!(missing.context.is_none());
        let data = result(
            "dart_financials",
            RunResultOrigin::AcquiredOriginal,
            json!({"dataset_ref":{},"value":4523}),
        );
        let data = check_link(&link("4,523"), &data, None, &registry).unwrap();
        assert_eq!(data.kind, NumberCheckKind::Data);
        assert_eq!(data.status, NumberCheckStatus::Matched);
        assert!(data.context.is_none());
        // The same content outside the excerpt does carry context.
        assert!(check_link(&link("4,523"), &page, None, &registry)
            .unwrap()
            .context
            .is_some());
    }

    #[test]
    fn a_stored_message_round_trips_fallback_context_and_reads_older_checks() {
        let page = result(
            "web_fetch",
            RunResultOrigin::AcquiredOriginal,
            json!({"content":"매출은 4,523억원이다."}),
        );
        let check = check_link(
            &EvidenceLink {
                index: 0,
                text: "4,523억원".into(),
                evidence_id: "ev_page".into(),
            },
            &page,
            None,
            &RunResultRegistry::default(),
        )
        .unwrap();
        assert!(check.context.is_some());
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        state
            .store
            .create_loupist(&crate::test_fixtures::loupist_profile("loupist-a"))
            .unwrap();
        let mut chat = crate::test_fixtures::chat_session("chat-a", "loupist-a");
        let mut message = crate::test_fixtures::chat_message(
            "answer",
            crate::domain::ChatRole::Assistant,
            "매출 [4,523억원](#evidence-ev_page), [4,532억원](#evidence-ev_page)",
        );
        let missing = check_link(
            &EvidenceLink {
                index: 1,
                text: "4,532억원".into(),
                evidence_id: "ev_page".into(),
            },
            &page,
            None,
            &RunResultRegistry::default(),
        )
        .unwrap();
        assert_eq!(missing.status, NumberCheckStatus::NotFound);
        assert!(missing.compared_value.is_some());
        message
            .number_checks
            .extend([check.clone(), missing.clone()]);
        chat.messages.push(message);
        state.store.create_chat(&chat).unwrap();
        let loaded = state.store.get_chat("chat-a").unwrap().unwrap();
        assert_eq!(loaded.messages[0].number_checks, [check, missing]);
        let mut older = serde_json::to_value(&loaded.messages[0]).unwrap();
        older["numberChecks"][0]
            .as_object_mut()
            .unwrap()
            .remove("context");
        older["numberChecks"][1] = json!({"link_index":1,"text":"4,532억원","evidence_id":"ev_page","kind":"passage","status":"not_found"});
        let older: crate::domain::ChatMessage = serde_json::from_value(older).unwrap();
        older.validate().unwrap();
        assert!(older.number_checks[0].context.is_none());
        let legacy = &older.number_checks[1];
        assert!(
            legacy.comparison_reason.is_none()
                && legacy.compared_value.is_none()
                && legacy.nearest_source_scale.is_none()
                && legacy.nearest_source_unit.is_none()
        );
    }

    #[test]
    fn data_checks_never_match_identifier_fields() {
        let filings = result(
            "dart_financials",
            RunResultOrigin::AcquiredOriginal,
            json!({"dataset_ref": {"dataset_id": "ds_1", "revision": 1}, "facts": [
                {"rcept_no": "20260318000123", "corp_code": "00990221", "stock_code": "990221",
                 "bsns_year": "2025", "ord": 1234, "value": "(12,345)"}
            ]}),
        );
        let registry = RunResultRegistry::default();
        let link = |text: &str| EvidenceLink {
            index: 0,
            text: text.into(),
            evidence_id: "ev_dart".into(),
        };
        let status = |text: &str| {
            check_link(&link(text), &filings, None, &registry)
                .unwrap()
                .status
        };
        assert_eq!(status("990,221"), NumberCheckStatus::NotFound);
        assert_eq!(status("1,234"), NumberCheckStatus::NotFound);
        assert_eq!(status("20,260,318,000,123"), NumberCheckStatus::NotFound);
        // The accounting negative is the value.
        assert_eq!(status("-12,345"), NumberCheckStatus::Matched);
        assert_eq!(status("12,345"), NumberCheckStatus::Matched);
        assert_eq!(status("+12,345"), NumberCheckStatus::NotFound);
        let passage = result(
            "web_fetch",
            RunResultOrigin::AcquiredOriginal,
            json!({"content":"The loss was -12,345."}),
        );
        assert_eq!(
            check_link(&link("12,345"), &passage, None, &registry)
                .unwrap()
                .status,
            NumberCheckStatus::NotFound
        );
        assert_eq!(
            check_link(&link("-12,345"), &passage, None, &registry)
                .unwrap()
                .status,
            NumberCheckStatus::Matched
        );
    }

    #[test]
    fn conversion_checks_only_the_output_and_unambiguous_currency_markers() {
        let registry = RunResultRegistry::default();
        let payload = json!({"operation": "convert", "conversion":{"from":"USD","to":"KRW"},
            "values":[{"name":"converted","value":5_246_039_100_000_000_u64,"unit":"KRW",
                "display":"5,246,039,100,000,000"}],
            "inputs":[{"name":"amount","value":3930,"currency_basis":"declared"},
                {"name":"rate","value":1334.87}]});
        let calc = result("calculate", RunResultOrigin::Derived, payload.clone());
        let check = |text: &str, source: &RunResult| {
            check_link(
                &EvidenceLink {
                    index: 0,
                    text: text.into(),
                    evidence_id: "ev".into(),
                },
                source,
                None,
                &registry,
            )
            .unwrap()
        };
        let figures = [
            ("₩5,246tn", NumberCheckStatus::Rounded),
            ("KRW 5,246,039,100,000,000", NumberCheckStatus::Matched),
            ("5,246,039,100,000,000", NumberCheckStatus::Matched),
            ("$5,246tn", NumberCheckStatus::Rounded),
            ("¥5,246tn", NumberCheckStatus::Rounded),
            ("5,246tn dollars", NumberCheckStatus::Rounded),
            ("5,246tn 달러", NumberCheckStatus::Rounded),
            ("EUR 5,246tn", NumberCheckStatus::Differs),
            ("EUR5,246tn", NumberCheckStatus::Differs),
            ("US$5,246tn", NumberCheckStatus::Differs),
            ("HK$5,246tn", NumberCheckStatus::Differs),
            ("5,246tn yen", NumberCheckStatus::Differs),
            ("5,246tn pounds", NumberCheckStatus::Differs),
            ("5,246tn 유로", NumberCheckStatus::Differs),
            ("€5,246tn", NumberCheckStatus::Differs),
            ("£5,246tn", NumberCheckStatus::Differs),
            ("USD 5,246tn KRW", NumberCheckStatus::Differs),
            ("3,930", NumberCheckStatus::Differs),
        ];
        let outcomes = figures.map(|(text, _)| {
            let checked = check(text, &calc);
            (text, checked.status, checked.pointer)
        });
        assert_eq!(
            outcomes,
            figures.map(|(text, status)| (
                text,
                status,
                (status != NumberCheckStatus::Differs).then(|| "/values/0/value".to_owned()),
            ))
        );

        let mut thb = payload;
        thb["conversion"]["to"] = json!("THB");
        thb["values"][0]["unit"] = json!("THB");
        let thb = result("calculate", RunResultOrigin::Derived, thb);
        assert_eq!(
            numbers::parse_printed("THB 5,246,039,100,000,000")
                .unwrap()
                .unit,
            Unit::Amount(1.0)
        );
        assert_eq!(
            check("THB 5,246,039,100,000,000", &thb).status,
            NumberCheckStatus::Matched
        );
        assert_eq!(
            check("KRW 5,246,039,100,000,000", &thb).status,
            NumberCheckStatus::Differs
        );
    }

    fn link(text: &str) -> EvidenceLink {
        EvidenceLink {
            index: 0,
            text: text.into(),
            evidence_id: "ev".into(),
        }
    }

    // The 16 chips in the owner's saved replies, including repeated citations.
    // Keep the actual selected windows and serialized tool-result text intact.
    #[test]
    fn owner_apple_figures_match_their_recorded_evidence() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/owner-number-checks.json"))
                .unwrap();
        let mut missing = Vec::new();
        let cases = fixture["cases"].as_array().unwrap();
        for case in cases {
            let id = case["evidence"].as_str().unwrap();
            let entry = &fixture["evidence"][id];
            let excerpt = entry["excerpt"].as_str().unwrap();
            let tool = entry["tool"].as_str().unwrap();
            let (origin, payload) = match tool {
                "run_python" => (
                    RunResultOrigin::Derived,
                    serde_json::from_str(excerpt).unwrap(),
                ),
                "world_read" => (
                    RunResultOrigin::SavedContext,
                    json!({"scope":"source","text":excerpt}),
                ),
                "web_fetch" => (
                    RunResultOrigin::AcquiredOriginal,
                    json!({"content":excerpt}),
                ),
                _ => panic!("unexpected tool {tool}"),
            };
            let read = result(tool, origin, payload);
            let evidence: ChatEvidenceRef = serde_json::from_value(json!({
                "id":id,"title":"Apple","kind":entry["kind"],"tool_name":tool,
                "retrieved_at":"2026-11-20T21:00:00Z","excerpt":excerpt,
                "excerpt_truncated":entry["excerpt_truncated"],"excerpt_match":entry["excerpt_match"],
                "content_sha256":"a".repeat(64),"warnings":[]
            }))
            .unwrap();
            let figure = case["figure"].as_str().unwrap();
            let check = check_link(
                &link(figure),
                &read,
                Some(&evidence),
                &RunResultRegistry::default(),
            )
            .unwrap();
            if !matches!(
                check.status,
                NumberCheckStatus::Matched | NumberCheckStatus::Rounded
            ) {
                missing.push((id, figure));
            } else if tool == "run_python" {
                assert_eq!(check.pointer.as_deref(), Some("/stdout"), "{figure}");
                if let Some(exact) = check.exact {
                    assert_eq!(exact, case["sourceNumber"].as_str().unwrap(), "{figure}");
                }
            } else {
                let located = match (check.span, check.context.as_ref()) {
                    (Some(span), _) => &excerpt[span.start..span.end],
                    (_, Some(ctx)) => &ctx.text[ctx.start..ctx.end],
                    _ => panic!("missing source location for {figure}"),
                };
                assert_eq!(
                    located.trim_start_matches(['$', ' ']),
                    case["sourceNumber"].as_str().unwrap(),
                    "{figure}"
                );
            }
        }
        assert_eq!(cases.len(), 16);
        assert!(
            missing.is_empty(),
            "{} of 16 missing: {missing:?}",
            missing.len()
        );
    }

    #[test]
    fn owner_unverified_figures_explain_the_compared_candidate() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/owner-number-checks.json"))
                .unwrap();
        let stored = |case: usize| {
            fixture["evidence"][fixture["cases"][case]["evidence"].as_str().unwrap()]["excerpt"]
                .as_str()
                .unwrap()
        };
        let source = result(
            "world_read",
            RunResultOrigin::SavedContext,
            json!({"text":stored(2)}),
        );
        let step = result(
            "run_python",
            RunResultOrigin::Derived,
            serde_json::from_str(stored(11)).unwrap(),
        );
        let quote = stored(1).split("At close:").next().unwrap();
        let price = page(quote);
        let excerpt: ChatEvidenceRef = serde_json::from_value(json!({
            "id":"ev","title":"Closing price","kind":"original","tool_name":"web_fetch",
            "retrieved_at":"2026-11-20T21:00:00Z","excerpt":quote,
            "excerpt_truncated":true,"content_sha256":"a".repeat(64),"warnings":[]
        }))
        .unwrap();
        for (figure, read, evidence, reason, printed) in [
            (
                "575.38억 달러",
                &source,
                None,
                "digits_differ",
                Some("$ 57,583"),
            ),
            (
                "50.4억 달러",
                &step,
                None,
                "scale_conflict",
                Some("50368.7433"),
            ),
            (
                "-342.56달러",
                &price,
                Some(&excerpt),
                "sign_conflict",
                Some("342.56"),
            ),
            (
                "14,570,195달러",
                &source,
                None,
                "incompatible_unit",
                Some("14,570,195"),
            ),
            ("8,761.43달러", &source, None, "no_candidate", None),
        ] {
            let check =
                check_link(&link(figure), read, evidence, &RunResultRegistry::default()).unwrap();
            let record = serde_json::to_value(&check).unwrap();
            assert_eq!(record["status"], "not_found", "{figure}");
            // --show-output exposes these five bridge records for renderer fixtures.
            println!("{record}");
            assert_eq!(record["comparisonReason"], reason, "{figure}");
            assert_eq!(record["comparedValue"].as_str(), printed, "{figure}");
            assert_eq!(check.nearest.as_deref(), printed);
            match reason {
                "digits_differ" | "incompatible_unit" => {
                    let ctx = check.context.unwrap();
                    assert_eq!(Some(&ctx.text[ctx.start..ctx.end]), printed);
                }
                "scale_conflict" => {
                    assert_eq!(check.pointer.as_deref(), Some("/stdout"));
                    assert_eq!(record["nearestSourceScale"], 6);
                    assert_eq!(record["nearestSourceUnit"], "USD");
                }
                "sign_conflict" => {
                    let span = check.span.unwrap();
                    assert_eq!(Some(&quote[span.start..span.end]), printed);
                }
                _ => assert!(
                    check.span.is_none() && check.pointer.is_none() && check.context.is_none()
                ),
            }
        }
    }

    #[test]
    fn an_unlabelled_cash_flow_comparison_keeps_the_source_scale_unknown() {
        let read = page("145,097 111,482 118,254\nCash generated by operating activities");
        let check = check_link(
            &link("1,450.79억 달러"),
            &read,
            None,
            &RunResultRegistry::default(),
        )
        .unwrap();
        assert_eq!(check.status, NumberCheckStatus::NotFound);
        assert_eq!(check.compared_value.as_deref(), Some("145,097"));
        assert_eq!(
            serde_json::to_value(&check).unwrap()["comparisonReason"],
            "digits_differ"
        );
        assert!(check.nearest_source_unit.is_none() && check.nearest_source_scale.is_none());
    }

    #[test]
    fn diagnostics_abstain_on_tied_readings_and_unrelated_metadata() {
        let registry = RunResultRegistry::default();
        for (figure, read) in [
            ("$342.55", page("Prices: 342.54 and 342.56")),
            (
                "$342.55",
                result(
                    "web_fetch",
                    RunResultOrigin::AcquiredOriginal,
                    json!({"content":"No quote available.","content_bytes":342.56}),
                ),
            ),
        ] {
            let check = check_link(&link(figure), &read, None, &registry).unwrap();
            assert_eq!(check.status, NumberCheckStatus::NotFound);
            assert_eq!(
                serde_json::to_value(&check).unwrap()["comparisonReason"],
                "no_candidate"
            );
            assert!(check.nearest.is_none() && check.context.is_none());
        }
        let check = check_link(&link("$342.56"), &page("Price: 342.56"), None, &registry).unwrap();
        assert_eq!(check.status, NumberCheckStatus::Matched);
        let record = serde_json::to_value(check).unwrap();
        for key in [
            "comparisonReason",
            "comparedValue",
            "nearestSourceScale",
            "nearestSourceUnit",
            "nearest",
        ] {
            assert!(record.get(key).is_none(), "{key}");
        }
    }

    #[test]
    fn owner_amounts_reject_changed_digits_signs_and_incompatible_cells() {
        let caption = "(In millions, except number of shares, which are reflected in thousands, and per-share amounts)";
        let saved = json!({"content": format!("{caption}\n\n(1) Net sales by category:\niPhone $ 57,583\nServices 31,573\nShares used in computing earnings per share:\nBasic 14,570,195\nEarnings per share:\nDiluted $ 2.10"), "content_bytes": 98654321});
        let wrapped = saved.to_string();
        let source = result(
            "world_read",
            RunResultOrigin::SavedContext,
            json!({"text":wrapped}),
        );
        let quote = page("NasdaqGS - Nasdaq Real Time Price USD\n\n342.56 -6.65 (-1.90%)");
        let cash = page("145,097 111,482 118,254\nCash generated by operating activities");
        let scaled = page("(in millions)\nOperating income 5,040");
        let program = result(
            "run_python",
            RunResultOrigin::Derived,
            json!({"stdout":"EPS 2.0500000000000003\nmultiple 35 9.787428571428572\nop USDm 50368.7433","exit_code":42,"stderr":"77.77"}),
        );
        for (source, wrong) in [
            (
                &source,
                &[
                    "575.38억 달러",
                    "-575.83억 달러",
                    "57.583억 달러",
                    "145,701.95억 달러",
                    "2.10억 달러",
                    "98,654,321달러",
                ][..],
            ),
            (
                &quote,
                &["342.65달러", "-342.56달러", "34.256달러", "1.90달러"][..],
            ),
            (&cash, &["1,450.79억 달러", "-1,450.97억 달러"][..]),
            (&scaled, &["504억 달러", "-50.4억 달러"][..]),
            (
                &program,
                &[
                    "9.78달러",
                    "-2.05달러",
                    "50.4억 달러",
                    "2.05억 달러",
                    "35달러",
                    "42달러",
                    "77.77달러",
                ][..],
            ),
        ] {
            for figure in wrong {
                let check =
                    check_link(&link(figure), source, None, &RunResultRegistry::default()).unwrap();
                assert_eq!(check.status, NumberCheckStatus::NotFound, "{figure}");
            }
        }
    }

    #[test]
    fn monetary_rounding_has_one_midpoint_and_applies_to_scalar_passage_content() {
        for (read, accepted, rejected) in [
            (page("Closing price $2.345"), "$2.35", "$2.34"),
            (page("Closing price -2.345"), "-$2.35", "-$2.34"),
            (
                page("Revenue $999,999,999,999"),
                "$999,999,999,999",
                "$999,999,999,998",
            ),
            (page("145097"), "1,450.97억 달러", "1,450.79억 달러"),
        ] {
            let check = |figure| {
                check_link(&link(figure), &read, None, &RunResultRegistry::default())
                    .unwrap()
                    .status
            };
            assert!(
                matches!(
                    check(accepted),
                    NumberCheckStatus::Rounded | NumberCheckStatus::Matched
                ),
                "{accepted}"
            );
            assert_eq!(check(rejected), NumberCheckStatus::NotFound, "{rejected}");
        }
    }

    /// rc1 T-045 rerun: flows summed in 억 with `"unit": "KRW 100 million"`
    /// were read as millions, so all three correct links differed.
    #[test]
    fn rc1_t045_sums_in_100_million_won_check_their_eok_links() {
        let registry = RunResultRegistry::default();
        for (series, answer, wrong) in [
            (
                json!([21.4, 18.5, 9.6, -4.2, 13.1, 22.8, 14.6, 8.7, 11.9, 7]),
                "+123.4억 원",
                "+124.4억 원",
            ),
            (
                json!([-6.1, -8.8, 2.4, -11.5, -3.2, -4.7, 0.9, -5.6, -2.3, -2.3]),
                "-41.2억 원",
                "-42.2억 원",
            ),
            (
                json!([-15.1, -9.8, -11.85, 15.65, -9.8, -17.85, -15.45, -3.1, -9.4, -4.8]),
                "-81.5억 원",
                "-80.5억 원",
            ),
        ] {
            let payload = crate::calculations::calculate(
                &json!({"operation": "sum", "series": series, "unit": "KRW 100 million"}),
                &mut NoDatasets,
            )
            .unwrap();
            let calc = result("calculate", RunResultOrigin::Derived, payload);
            let check = check_link(&link(answer), &calc, None, &registry).unwrap();
            assert_eq!(check.status, NumberCheckStatus::Matched, "{answer}");
            assert_eq!(check.pointer.as_deref(), Some("/values/0/value"));
            let unit = &check.calc.unwrap().inputs[0].unit;
            assert_eq!((unit.currency.as_deref(), unit.scale), (Some("KRW"), 8));
            let differs = check_link(&link(wrong), &calc, None, &registry).unwrap();
            assert_eq!(differs.status, NumberCheckStatus::Differs, "{wrong}");
            // Millions are not 억.
            let millions = answer.replace("억 원", " million won");
            let millions = check_link(&link(&millions), &calc, None, &registry).unwrap();
            assert_eq!(millions.status, NumberCheckStatus::Differs, "{answer}");
        }
    }

    /// The rc1 T-047 `dart_filing_read` of the FY2025 dividend notice.
    fn dividend_notice() -> RunResult {
        result(
            "dart_filing_read",
            RunResultOrigin::AcquiredOriginal,
            json!({"data": {"document": {"archive_document_count": 1, "chars": 310,
                "media_type": "text/plain", "primary_document": null, "truncated": false,
                "text": "<<untrusted id=30f8d0741715 source=\"OpenDART filing 20260205990221\">>\n현금ㆍ현물배당결정 현금ㆍ현물배당결정 가상의 솔숲식품 2025년 결산 현금배당 결정입니다. 1. 배당구분 결산배당 2. 배당종류 현금배당 3. 1주당 배당금(원) 보통주식 300 4. 시가배당율(%) 보통주식 1.4 5. 배당금총액(원) 6,000,000,000 6. 배당기준일 2025-12-31 7. 배당금지급 예정일자 2026-04-10 8. 이사회결의일(결정일) 2026-02-05 Fictional Solsup Foods: final cash dividend KRW 300 per common share, record date 2025-12-31.\n<<end untrusted id=30f8d0741715>>"},
                "filing": {"filing_id": "20260205990221"}},
                "dataset_ref": {"dataset_id": "dataset-9ef0049d758f4b248d9fdd6c22912160",
                    "digest": "d96670807d3852e49b945fdca471e09782e7ee569701adf33eaab5095e94bb92", "revision": 1},
                "operation": "filing.read", "page": {"available": 1, "returned": 1, "truncated": false},
                "source_id": "opendart.disclosures"}),
        )
    }

    /// rc1 T-047: the notice states each row's unit in its label, so "₩6
    /// billion" and "1.4%" were not found. A filing read is a data result and
    /// its document text is among its candidates already; the unit was missing.
    #[test]
    fn rc1_t047_filing_figures_match_the_unit_in_their_row_label() {
        let notice = dividend_notice();
        let registry = RunResultRegistry::default();
        let check = |text: &str| check_link(&link(text), &notice, None, &registry).unwrap();
        let billion = check("₩6 billion");
        assert_eq!(billion.kind, NumberCheckKind::Data);
        assert_eq!(billion.status, NumberCheckStatus::Matched);
        assert_eq!(billion.pointer.as_deref(), Some("/data/document/text"));
        // The scale comparison keeps the exact printed number.
        assert_eq!(billion.exact.as_deref(), Some("6,000,000,000"));
        for (text, status) in [
            ("1.4%", NumberCheckStatus::Matched),
            ("₩300 per share", NumberCheckStatus::Matched),
            ("6,000,000,000", NumberCheckStatus::Matched),
            ("₩6,000,000,000", NumberCheckStatus::Matched),
            // A different figure, or an item's number, is not in the notice.
            ("₩6.5 billion", NumberCheckStatus::NotFound),
            ("₩60 billion", NumberCheckStatus::NotFound),
            ("1.5%", NumberCheckStatus::NotFound),
            ("5%", NumberCheckStatus::NotFound),
        ] {
            assert_eq!(check(text).status, status, "{text}");
        }
    }

    #[test]
    fn rc1_year_amount_pairs_keep_the_revenue_unit_in_the_retained_passage() {
        let text = "Microsoft 2024 Annual Report, fiscal year ended June 30, 2024. Consolidated revenue (USD millions): 2024 245,122; 2023 211,915.";
        let read = result(
            "world_read",
            RunResultOrigin::AcquiredOriginal,
            json!({"text": text}),
        );
        let evidence: ChatEvidenceRef = serde_json::from_value(json!({
            "id":"ev", "title":"User-provided", "kind":"original", "tool_name":"world_read",
            "retrieved_at":"2026-11-19T00:00:00Z", "excerpt": text,
            "excerpt_truncated":false, "content_sha256":"a".repeat(64), "warnings":[]
        }))
        .unwrap();
        let registry = RunResultRegistry::default();
        for excerpt in [Some(&evidence), None] {
            let check = |figure| {
                check_link(&link(figure), &read, excerpt, &registry)
                    .unwrap()
                    .status
            };
            for figure in ["$245,122 million", "$211,915 million"] {
                assert_eq!(check(figure), NumberCheckStatus::Matched, "{figure}");
            }
            for figure in [
                "$211,915 billion",
                "-$211,915 million",
                "$211,916 million",
                "$2,023 million",
                "$2,024 million",
            ] {
                assert_eq!(check(figure), NumberCheckStatus::NotFound, "{figure}");
            }
        }
    }

    #[test]
    fn v012_apple_pdf_net_sales_use_the_table_caption() {
        // pdf_oxide page 1 of Apple's FY24 Q4 consolidated statements.
        let extracted = concat!(
            "Apple Inc. \n",
            "\n",
            "CONDENSED CONSOLIDATED STATEMENTS OF OPERATIONS (Unaudited) \n",
            "(In millions, except number of shares, which are reflected in thousands, and per-share amounts) \n",
            "  Three Months Ended Twelve Months Ended \n",
            "September 28, September 30, September 28, September 30, \n",
            "  2024 2023 2024 2023 \n",
            "Net sales: \n",
            "\n",
            "(1) Net sales by category:\n",
            "iPhone $ 46,222 $ 43,805 $ 201,183 $ 200,583\n",
            "Mac 7,744 7,614 29,984 29,357\n",
            "iPad 6,950 6,443 26,694 28,300\n",
            "Wearables, Home and Accessories 9,042 9,322 37,005 39,845\n",
            "Services 24,972 22,314 96,169 85,200\n",
            "Total net sales $ 94,930 $ 89,498 $ 391,035 $ 383,285\n",
        );
        // The same page captured with the detected grid retained. Old excerpts
        // keep their original bytes, so exercise both through the chip check.
        let (caption, _) = extracted.split_once("(1) Net sales by category:").unwrap();
        let with_rows = format!("{caption}| (1) Net sales by category: |  |  |  |  |  |  |  |\n| iPhone | $46,222 |  | $43,805 |  | $201,183 |  | $200,583 |\n| Total net sales | $94,930 |  | $89,498 |  | $391,035 |  | $383,285 |\n");
        for extracted in [extracted, with_rows.as_str()] {
            let registry = RunResultRegistry::default();
            let read = page(extracted);
            for (figure, expected) in [
                ("$391,035 million", NumberCheckStatus::Matched),
                ("$391,035 billion", NumberCheckStatus::NotFound),
                ("$391,036 million", NumberCheckStatus::NotFound),
                ("-$391,035 million", NumberCheckStatus::NotFound),
            ] {
                assert_eq!(
                    check_link(&link(figure), &read, None, &registry)
                        .unwrap()
                        .status,
                    expected,
                    "{figure}"
                );
            }
            let no_answer_scale = check_link(&link("$391,035"), &read, None, &registry).unwrap();
            assert_eq!(no_answer_scale.status, NumberCheckStatus::Matched);
            assert_eq!(
                no_answer_scale.unstated_scale,
                Some(crate::domain::answer_checks::UnstatedScale::Answer)
            );
            let unrelated = page(&format!(
                "{extracted}\nOther amounts\nTotal net sales $ 48,765"
            ));
            let match_without_a_caption =
                check_link(&link("$48,765 million"), &unrelated, None, &registry).unwrap();
            assert_eq!(match_without_a_caption.status, NumberCheckStatus::Matched);
            assert_eq!(
                match_without_a_caption.unstated_scale,
                Some(crate::domain::answer_checks::UnstatedScale::Source)
            );
        }
    }

    #[test]
    fn v012_fred_series_checks_the_exact_observation_and_its_metadata() {
        let registry = RunResultRegistry::default();
        for units in [
            None,
            Some("Billions of Dollars"),
            Some("Millions of Dollars"),
            Some("Index"),
        ] {
            let mut body =
                json!({"provider":"fred","results":[{"date":"2024-10-01","GDP":29929.552}]});
            if let Some(unit) = units {
                body["extra"] = json!({"results_metadata":{"GDP":{"units":unit,"title":"Gross Domestic Product"}}});
            }
            let mut read = result(
                "economy_fred_series",
                RunResultOrigin::AcquiredOriginal,
                json!({
                    "dataset_ref":{"dataset_id":"ds_gdp","revision":1},
                    "content":[{"type":"text","text":body.to_string()}],
                    "structuredContent":body
                }),
            );
            read.producer.runtime_id = "openbb".into();
            read.producer.provider = Some("fred".into());
            let check = |figure| check_link(&link(figure), &read, None, &registry).unwrap();
            let matched = check("29,929.552 billion dollars");
            if matches!(units, None | Some("Billions of Dollars")) {
                assert_eq!(matched.status, NumberCheckStatus::Matched, "{units:?}");
                assert_eq!(
                    matched.pointer.as_deref(),
                    Some("/structuredContent/results/0/GDP")
                );
            } else {
                assert_eq!(matched.status, NumberCheckStatus::NotFound, "{units:?}");
            }
            for wrong in ["29,929.553 billion dollars", "-29,929.552 billion dollars"] {
                assert_eq!(check(wrong).status, NumberCheckStatus::NotFound);
            }
            if units == Some("Index") {
                assert_eq!(check("2,992,955.2%").status, NumberCheckStatus::NotFound);
            }
            if units == Some("Millions of Dollars") {
                assert_eq!(
                    check("29.929552 billion dollars").status,
                    NumberCheckStatus::Matched
                );
            }
            if units.is_none() {
                assert_eq!(
                    check("29,929.55 billion dollars").status,
                    NumberCheckStatus::NotFound
                );
            }
        }
    }

    #[test]
    fn rc1_comparison_checks_rounded_billions_only_against_the_attached_issuer_table() {
        let registry = RunResultRegistry::default();
        let apple = concat!(
            "(In millions, except number of shares, which are reflected in thousands, and per-share amounts)\n\n",
            "| Years Ended | 2026 | 2025 |\n| --- | --- | --- |\n",
            "| Total net sales | 478,834 | 416,161 |\n",
            "| Shares used in computing earnings per share | 14,992,000 |"
        );
        let microsoft = concat!(
            "(In millions, except per share amounts)\nYear Ended June 30, | 2026 | 2025\n",
            "Total revenue | 331,839 | 281,724\nDiluted earnings per share | 17.95 | 13.64"
        );
        for (text, figures, wrong) in [
            (
                apple,
                ["$478.83bn", "$416.16bn"],
                ["$478.84bn", "-$416.16bn", "$478.83m", "$14,992bn"],
            ),
            (
                microsoft,
                ["$331.84bn", "$281.72bn"],
                ["$331.83bn", "-$281.72bn", "$331.84m", "$17.95m"],
            ),
        ] {
            let read = result(
                "web_fetch",
                RunResultOrigin::AcquiredOriginal,
                json!({"content":text}),
            );
            let check = |figure| {
                check_link(&link(figure), &read, None, &registry)
                    .unwrap()
                    .status
            };
            for figure in figures {
                assert_eq!(check(figure), NumberCheckStatus::Rounded, "{figure}");
            }
            for figure in wrong {
                assert_eq!(check(figure), NumberCheckStatus::NotFound, "{figure}");
            }
        }
    }

    /// rc1 L-032: the saved source states the backlog's unit in words.
    #[test]
    fn rc1_l032_a_backlog_matches_the_unit_its_source_states_in_words() {
        let text = "https://fictional.example/solsup/backlog-2025\n\n솔숲식품은 2025-03-12 현재 수주잔고 수치가 184.6이고 단위는 십억 원이라고 알렸다.\n";
        let read = result(
            "world_read",
            RunResultOrigin::AcquiredOriginal,
            json!({"recordId": "rec_3d01d8a030bd4da2b0d4f6a6589fd295", "scope": "source",
                "status": "delivered_for_this_turn", "text": text}),
        );
        let evidence: ChatEvidenceRef = serde_json::from_value(json!({
            "id":"ev", "title":"Solsup backlog", "kind":"original", "tool_name":"world_read",
            "retrieved_at":"2026-11-19T00:00:00Z", "excerpt": text,
            "excerpt_truncated":false, "content_sha256":"a".repeat(64), "warnings":[]
        }))
        .unwrap();
        let registry = RunResultRegistry::default();
        for excerpt in [Some(&evidence), None] {
            let check =
                |figure: &str| check_link(&link(figure), &read, excerpt, &registry).unwrap();
            let matched = check("₩184.6 billion");
            assert_eq!(matched.kind, NumberCheckKind::Passage);
            assert_eq!(matched.status, NumberCheckStatus::Matched);
            let located = match (matched.span, matched.context) {
                (Some(span), None) => text[span.start..span.end].to_owned(),
                (None, Some(context)) => context.text[context.start..context.end].to_owned(),
                other => panic!("{other:?}"),
            };
            assert_eq!(located, "184.6");
            assert_eq!(check("₩184.6 million").status, NumberCheckStatus::NotFound);
            assert_eq!(check("₩186 billion").status, NumberCheckStatus::NotFound);
        }
    }

    /// The rc2 RP-002 trial 0 `run_python` step behind the margin changes.
    fn margin_change_step() -> RunResult {
        result(
            "run_python",
            RunResultOrigin::Derived,
            json!({"artifacts": [], "changed_files": [], "conflicts": [], "exit_code": 0,
                "result_ref": "result:d3efceb1ae2e4c50a4147d47e6525c74",
                "snapshot_omitted_count": 0, "snapshot_omitted_paths": [],
                "snapshot_scan_truncated": false, "status": "completed",
                "stderr": "", "stderr_truncated": false,
                "stdout": "{'qoq_pp': -1.37410609456719, 'yoy_pp': -3.3613032119256188, 'adjusted_margin_pct': 9.777777777777779, 'maintenance_direct_pp': 2.074074074074074}\n",
                "stdout_truncated": false, "untrusted_input": false}),
        )
    }

    /// rc2 RP-002: the agent's code printed the margin changes in points and
    /// the adjusted margin in percent as bare numbers, so `−1.37%p`, `−3.36%p`
    /// and `9.8%` were not found.
    #[test]
    fn rc2_rp002_percent_and_point_figures_match_the_bare_numbers_their_code_printed() {
        let step = margin_change_step();
        let registry = RunResultRegistry::default();
        let check = |text: &str| check_link(&link(text), &step, None, &registry).unwrap();
        for text in [
            "−1.37%p",
            "-1.37%p",
            "−1.4pp",
            "−3.36%p",
            "9.8%",
            "9.78%",
            "2.07%p",
        ] {
            let found = check(text);
            assert_eq!(found.kind, NumberCheckKind::Step);
            assert_eq!(found.status, NumberCheckStatus::Matched, "{text}");
            assert_eq!(found.pointer.as_deref(), Some("/stdout"), "{text}");
        }
        for text in [
            // The sign and the shown digits must agree.
            "1.37%p",
            "+1.37%p",
            "−1.38%p",
            "−3.37%p",
            "9.7%",
            "−9.8%",
            // A bare number is never read ÷100, and the exit code is not output.
            "0.098%",
            "0.0%p",
        ] {
            assert_eq!(check(text).status, NumberCheckStatus::NotFound, "{text}");
        }
        // Code also prints a ratio as a fraction (`print(52/675)`): a bare
        // number reads as a fraction ×100 too, as a rounded match.
        let printed = |stdout: &str| {
            result(
                "run_python",
                RunResultOrigin::Derived,
                json!({"exit_code": 0, "status": "completed", "stdout": stdout}),
            )
        };
        let margin = printed("0.07703703703703704\n");
        let change = printed("-0.0137410609456719\n");
        for (source, text, status) in [
            (&margin, "7.70%", NumberCheckStatus::Rounded),
            (&margin, "7.7%", NumberCheckStatus::Rounded),
            (&margin, "7.71%", NumberCheckStatus::NotFound),
            (&margin, "77.0%", NumberCheckStatus::NotFound),
            (&change, "−1.37%p", NumberCheckStatus::Rounded),
            (&change, "-1.37pp", NumberCheckStatus::Rounded),
            (&change, "−1.38%p", NumberCheckStatus::NotFound),
            (&change, "1.37%p", NumberCheckStatus::NotFound),
        ] {
            let found = check_link(&link(text), source, None, &registry).unwrap();
            assert_eq!(found.status, status, "{text}");
            if status == NumberCheckStatus::Rounded {
                assert_eq!(found.pointer.as_deref(), Some("/stdout"), "{text}");
                let exact = source.payload["stdout"].as_str().unwrap().trim();
                assert_eq!(found.exact.as_deref(), Some(exact), "{text}");
            }
        }
        // A number printed in the figure's unit is an exact match and beats a
        // fraction that reads the same.
        let both = printed("fraction 0.0978\nmargin_pct 9.78\n");
        let found = check_link(&link("9.78%"), &both, None, &registry).unwrap();
        assert_eq!(found.status, NumberCheckStatus::Matched);
        let fraction = printed("fraction 0.0978\n");
        let found = check_link(&link("9.78%"), &fraction, None, &registry).unwrap();
        assert_eq!(found.status, NumberCheckStatus::Rounded);
        // Other steps keep their units: an unlabelled dataset value is raw.
        let query = result(
            "dataset_query",
            RunResultOrigin::Derived,
            json!({"rows": [{"margin": 9.777777777777779}]}),
        );
        assert_eq!(
            check_link(&link("9.8%"), &query, None, &registry)
                .unwrap()
                .status,
            NumberCheckStatus::NotFound
        );
    }

    /// The rc2 RP-002 trial 0 `web_fetch` of the earnings article.
    const EARNINGS_ARTICLE: &str = "<<untrusted id=ef29c647a71a source=\"https://fictional.example/news/hanbit-chem-q3-2026\">>\n가상의 화학기업 한빛화학은 12일 3분기 분기보고서를 내고 연결 기준 영업이익이 52억원으로 1년 전보다 줄었다고 밝혔다. 매출액은 675억원이었다.\n\n회사는 같은 날 열린 실적 설명회에서 여수공장 정기보수로 3분기 판매량이 2분기보다 약 9% 줄었고, 정기보수와 관련한 일회성 비용 약 14억원이 영업이익에 반영됐다고 설명했다.\n\n주력 제품 가격과 원재료 가격의 차이인 스프레드는 3분기 평균 톤당 212달러로 2분기의 248달러보다 낮아졌다. 회사는 역내 신규 설비가 가동을 시작해 스프레드 약세가 당분간 이어질 수 있다고 봤다.\n\n회사는 정기보수를 마친 뒤 4분기 공장 가동률이 90% 이상으로 회복될 것으로 예상했다.\n<<end untrusted id=ef29c647a71a>>";

    fn page(content: &str) -> RunResult {
        result(
            "web_fetch",
            RunResultOrigin::AcquiredOriginal,
            json!({"content": content, "content_bytes": content.len(), "status": 200,
                "url": "https://fictional.example/news/hanbit-chem-q3-2026"}),
        )
    }

    #[test]
    fn c010_samsung_dividends_in_a_share_class_row_match_across_scripts() {
        for (label, source) in [
            ("372원", "Common shares: KRW 372 (0.2%)"),
            ("374원", "Common shares: KRW 374 (0.1%)"),
            ("4,601원", "보통주 4,601원"),
        ] {
            let check = check_link(
                &link(label),
                &page(source),
                None,
                &RunResultRegistry::default(),
            )
            .unwrap();
            assert_eq!(check.status, NumberCheckStatus::Matched, "{check:?}");
        }
        let conflict = check_link(
            &link("372원"),
            &page("Dividend: USD 372"),
            None,
            &RunResultRegistry::default(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(conflict).unwrap()["comparisonReason"],
            "incompatible_unit"
        );
    }

    #[test]
    fn c010_currency_aliases_match_before_and_after_the_number() {
        let registry = RunResultRegistry::default();
        let figures = |unit| {
            [
                format!("{unit}372"),
                format!("{unit} 372"),
                format!("372{unit}"),
                format!("372 {unit}"),
            ]
        };
        for (code, aliases) in [
            ("KRW", &["원", "₩", "￦", "won"][..]),
            ("USD", &["$", "US$", "달러", "미국 달러", "dollars"]),
            ("JPY", &["¥", "円", "엔", "yen"]),
            ("EUR", &["€", "유로", "euros"]),
            ("GBP", &["£", "파운드", "pound"]),
            ("CNY", &["RMB", "元", "¥", "人民币", "위안"]),
            ("TWD", &["NT$", "新台幣", "대만 달러", "$", "元"]),
            ("HKD", &["HK$", "홍콩 달러", "$", "元"]),
            ("NZD", &["NZ$", "$"]),
            ("AUD", &["A$", "$"]),
            ("CAD", &["C$", "$"]),
            ("SGD", &["S$", "$"]),
        ] {
            for alias in aliases {
                for named in figures(code) {
                    for variant in figures(alias) {
                        for (answer, source) in [(&named, &variant), (&variant, &named)] {
                            let check = check_link(&link(answer), &page(source), None, &registry)
                                .unwrap_or_else(|| panic!("did not parse {answer}"));
                            assert_eq!(
                                check.status,
                                NumberCheckStatus::Matched,
                                "{answer} against {source}: {check:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn c010_currencies_follow_scales_and_diagnostic_comparisons() {
        use NumberCheckStatus::{Matched, NotFound};
        let registry = RunResultRegistry::default();
        for (label, source, status) in [
            ("372원", "Dividend: 372", Matched),
            ("1,372", "Dividend: KRW 1,372", Matched),
            ("-KRW372", "Dividend: 원-372", Matched),
            ("-372원", "Dividend: 372원", NotFound),
            (
                "3.72억 원",
                "Common shares: KRW 372 million (0.2%)",
                Matched,
            ),
            (
                "3.72억 원",
                "Common shares: 372 million USD (0.2%)",
                NotFound,
            ),
            ("0.372조 원", "(단위: 백만원)\n매출 372,000", Matched),
            ("1.372조 원", "매출 1조 3,720억원", Matched),
            ("372원", "(Unit: KRW)\nCommon shares: 372", Matched),
            ("372원", "Shares outstanding: 372", NotFound),
            ("372원", "Shares: 372", NotFound),
            ("3.72억 원", "Diluted EPS: KRW 372 million", NotFound),
            ("USD 372", "(Unit: CAD)\nDividend: $372", NotFound),
            ("JPY 372", "(Unit: CNY)\nDividend: 372¥", NotFound),
            ("372元", "Dividend: ¥372", Matched),
            ("372달러", "Closing price USD\n372", Matched),
            ("372원", "Closing price USD\n372", NotFound),
        ] {
            let check = check_link(&link(label), &page(source), None, &registry).unwrap();
            assert_eq!(check.status, status, "{label} against {source}: {check:?}");
        }

        for (answer, source, code) in [
            ("372원", "Dividend: USD 372", "USD"),
            ("372달러", "Dividend: 372원", "KRW"),
            ("¥372", "Dividend: EUR 372", "EUR"),
            ("3.72억 원", "Dividend: 372 million USD", "USD"),
        ] {
            let check = check_link(&link(answer), &page(source), None, &registry).unwrap();
            assert_eq!(check.status, NotFound, "{answer} against {source}");
            assert_eq!(
                check.comparison_reason,
                Some(crate::domain::answer_checks::ComparisonReason::IncompatibleUnit)
            );
            assert_eq!(check.nearest_source_unit.as_deref(), Some(code));
            assert_eq!(check.text, answer);
        }
    }

    #[tokio::test]
    async fn c010_answer_and_structured_row_units_use_the_same_currency() {
        for (currency, status) in [
            ("KRW", NumberCheckStatus::Matched),
            ("USD", NumberCheckStatus::NotFound),
        ] {
            let result = result(
                "observations",
                RunResultOrigin::AcquiredOriginal,
                json!({
                    "dataset_ref":{"dataset_id":"ds_dividends"},
                    "data":[{"value":"372", "currency":currency, "scale":1000000}]
                }),
            );
            let answer = format!(
                "| 억 원 | 배당금 |\n|---|---|\n| | [3.72](#evidence-{}) |",
                evidence::evidence_id(&result.result_ref)
            );
            let capture = ToolCapture::default();
            capture
                .run_results
                .lock()
                .await
                .results
                .insert(result.result_ref.clone(), result);
            let checks = capture.number_checks(&answer, &[]).await;
            assert_eq!(checks.len(), 1);
            assert_eq!(checks[0].status, status, "{checks:?}");
            if currency == "USD" {
                assert_eq!(
                    checks[0].comparison_reason,
                    Some(crate::domain::answer_checks::ComparisonReason::IncompatibleUnit)
                );
                assert_eq!(checks[0].nearest_source_unit.as_deref(), Some("USD"));
            }
        }
    }

    #[tokio::test]
    async fn v006_exact_scale_conversion_highlights_the_cited_accession() {
        // The 2023 revenue fact appears in three annual filings. The
        // literal passage from the audit identifies the FY2024 accession.
        let selected = r#""end":"2023-09-30","val":383285000000,"accn":"0000320193-24-000123""#;
        let text = concat!(
            r#"[{"start":"2022-09-25","end":"2023-09-30","val":383285000000,"accn":"0000320193-23-000106","fy":2023},"#,
            r#"{"start":"2022-09-25","end":"2023-09-30","val":383285000000,"accn":"0000320193-24-000123","fy":2024},"#,
            r#"{"start":"2022-09-25","end":"2023-09-30","val":383285000000,"accn":"0000320193-25-000079","fy":2025}]"#
        );
        let mut source = page(text);
        source.producer.runtime_id = "native-web".into();
        source.upstream_result_refs = vec!["result:original".into()];
        let chosen = text.find(selected).unwrap();
        source.payload["content_offset"] = json!(1000);
        source.payload["selection"] = json!({
            "type":"literal_passage",
            "offset_unit":"utf8_bytes",
            "source_result_ref":"result:original",
            "find":selected,
            "match_offset":1000 + chosen
        });
        let evidence = evidence::project(&source);
        let capture = ToolCapture::default();
        capture
            .run_results
            .lock()
            .await
            .results
            .insert(source.result_ref.clone(), source);
        let answer = format!(
            "| Total revenue (USD millions) |\n|---|\n| [383,285](#evidence-{0}) |\n| [383,300](#evidence-{0}) |",
            evidence.id
        );
        let checks = capture
            .number_checks(&answer, std::slice::from_ref(&evidence))
            .await;
        let (exact, rounded) = (&checks[0], &checks[1]);
        assert_eq!(exact.status, NumberCheckStatus::Matched);
        assert_eq!(exact.exact.as_deref(), Some("383285000000"));
        assert_eq!(
            serde_json::to_value(&exact.scale_conversion).unwrap(),
            json!({
                "source_scale":0, "answer_scale":6, "currency":"USD"
            })
        );
        // One decimal of a billion is rounded from the same fact.
        let answer = format!(
            "Revenue in USD billions: [383.3](#evidence-{}).",
            evidence.id
        );
        let real = capture
            .number_checks(&answer, std::slice::from_ref(&evidence))
            .await;
        assert_eq!(real[0].status, NumberCheckStatus::Rounded);
        assert_eq!(real[0].exact.as_deref(), Some("383285000000"));
        assert!(real[0].scale_conversion.is_none());
        assert_eq!(rounded.status, NumberCheckStatus::NotFound);
        let offset = chosen + selected.find("383285000000").unwrap();
        for check in [&checks[0], &real[0]] {
            assert_eq!(check.span.as_ref().unwrap().start, offset);
        }
        let chosen_accession = text.find("0000320193-24-000123").unwrap();
        let mut accession = evidence.clone();
        accession.excerpt_match = Some(ChatEvidenceMatch {
            start: chosen_accession,
            end: chosen_accession + "0000320193-24-000123".len(),
        });
        let selected_row = capture.number_checks(&answer, &[accession]).await;
        assert_eq!(selected_row[0].span.as_ref().unwrap().start, offset);

        let first = ChatEvidenceRef {
            excerpt_match: None,
            ..evidence
        };
        let fallback = capture.number_checks(&answer, &[first]).await;
        assert_eq!(
            fallback[0].span.as_ref().unwrap().start,
            text.find("383285000000").unwrap()
        );
    }

    #[tokio::test]
    async fn c007_microsoft_segment_totals_use_the_filing_table_scale() {
        let text = concat!(
            "SEGMENT RESULTS OF OPERATIONS\n\n",
            "| (In millions, except percentages) |  | 2024 |  |  | 2023 |  |  | Percentage Change |  |\n",
            "| Revenue |  |  |  |  |  |  |  |  |  |\n",
            "| Productivity and Business Processes |  | $77,728 |  |  | $69,274 |  |  | 12% |  |\n",
            "| Intelligent Cloud |  | 105,362 |  |  | 87,907 |  |  | 20% |  |\n",
            "| More Personal Computing |  | 62,032 |  |  | 54,734 |  |  | 13% |  |\n",
            "| Total |  | $245,122 |  |  | $211,915 |  |  | 16% |  |\n"
        );
        let source = page(text);
        let evidence = evidence::project(&source);
        let capture = ToolCapture::default();
        capture
            .run_results
            .lock()
            .await
            .results
            .insert(source.result_ref.clone(), source);
        let answer = format!(
            "| Reported consolidated revenue (USD millions) |\n|---|\n| [211,915](#evidence-{0}) |\n| [245,122](#evidence-{0}) |",
            evidence.id
        );
        let checks = capture.number_checks(&answer, &[evidence]).await;
        assert_eq!(checks.len(), 2);
        for check in checks {
            assert_eq!(check.status, NumberCheckStatus::Matched, "{check:?}");
            assert_eq!(check.unstated_scale, None, "{check:?}");
        }
    }

    #[test]
    fn c007_filing_scale_excludes_share_and_percentage_fields() {
        for (caption, scale) in [
            ("(In millions, except percentages)", 1e6),
            ("(In millions, except per share amounts)", 1e6),
            ("($ in millions)", 1e6),
            ("(Dollars in millions)", 1e6),
            ("(Dollars in millions except per share amounts)", 1e6),
            ("in thousands, except share data", 1e3),
            ("(단위: 백만원)", 1e6),
            ("(単位：百万円)", 1e6),
        ] {
            let text = format!("| {caption} | Current year | Percentage Change |\n| Revenue | 245,122 | 16 |\n| Earnings per share | 4.25 | 3 |\n| Shares outstanding | 390,000 | 1 |");
            let found = numbers::passage_candidates(&text);
            let units_of = |lexeme: &str| {
                found
                    .iter()
                    .filter_map(|candidate| match candidate.place {
                        Place::Span { start, end } if &text[start..end] == lexeme => {
                            Some(candidate.unit)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            assert!(
                units_of("245,122").contains(&Unit::Amount(scale)),
                "{caption}: {found:?}"
            );
            assert!(
                units_of("16").contains(&Unit::Percent),
                "{caption}: {found:?}"
            );
            for skipped in ["16", "4.25", "390,000"] {
                assert!(
                    !units_of(skipped)
                        .iter()
                        .any(|unit| matches!(unit, Unit::Amount(_))),
                    "{caption}: {found:?}"
                );
            }
        }
    }

    #[test]
    fn w002_chart_values_use_the_displayed_column_units() {
        for (label, figures) in [
            (
                "Revenue (USD millions)",
                ["USD 40 million", "USD 48 million"],
            ),
            ("매출 (백만 달러)", ["40백만 달러", "48백만 달러"]),
        ] {
            let chart = result(
                "chart_query",
                RunResultOrigin::Derived,
                json!({
                    "columns": [{"id":"year","label":"Year","kind":"string"},
                                {"id":"revenue","label":label,"kind":"number"}],
                    "rows": [["2023", 40], ["2024", 48]]
                }),
            );
            for (row, figure) in figures.into_iter().enumerate() {
                let check =
                    check_link(&link(figure), &chart, None, &RunResultRegistry::default()).unwrap();
                assert_eq!(
                    check.status,
                    NumberCheckStatus::Matched,
                    "{label}: {check:?}"
                );
                assert_eq!(check.unstated_scale, None);
                assert_eq!(
                    check.pointer.as_deref(),
                    Some(format!("/rows/{row}/1").as_str())
                );
            }
        }
    }

    fn assert_scale_unspecified(tool: &str, origin: RunResultOrigin, payload: Value) {
        let read = result(tool, origin, payload);
        let evidence = evidence::project(&read);
        let check = check_link(
            &link("USD 40 million"),
            &read,
            Some(&evidence),
            &RunResultRegistry::default(),
        )
        .unwrap();
        assert_eq!(check.status, NumberCheckStatus::Matched, "{check:?}");
        assert_eq!(
            check.unstated_scale,
            Some(crate::domain::answer_checks::UnstatedScale::Source)
        );
        assert_eq!(check.comparison_reason, None);
    }

    #[test]
    fn passage_without_scale_keeps_equal_digits() {
        assert_scale_unspecified(
            "web_fetch",
            RunResultOrigin::AcquiredOriginal,
            json!({"content":"Revenue: 40."}),
        );
    }

    #[test]
    fn structured_record_without_scale_keeps_equal_digits() {
        assert_scale_unspecified(
            "worldbank_series",
            RunResultOrigin::AcquiredOriginal,
            json!({"dataset_ref": {"revision":1}, "data": {"value":40}}),
        );
    }

    #[test]
    fn queried_dataset_without_scale_keeps_equal_digits() {
        assert_scale_unspecified(
            "dataset_query",
            RunResultOrigin::Derived,
            json!({"rows":[{"revenue":40}]}),
        );
    }

    #[test]
    fn chart_without_scale_keeps_equal_digits() {
        assert_scale_unspecified(
            "chart_query",
            RunResultOrigin::Derived,
            json!({"columns":[{"id":"revenue","label":"Revenue","kind":"number"}], "rows":[[40]]}),
        );
    }

    #[test]
    fn calculated_result_without_scale_keeps_equal_digits() {
        assert_scale_unspecified(
            "calculate",
            RunResultOrigin::Derived,
            json!({"values":[{"name":"total","value":40,"display":"40"}]}),
        );
    }

    #[test]
    fn grid_value_without_scale_keeps_equal_digits() {
        let read = result(
            "dataset_query",
            RunResultOrigin::Derived,
            json!({
                "columns":{"value":{"unit":"USD millions"}}, "rows":[{"value":40}]
            }),
        );
        let cell = link("40");
        let printed =
            super::super::grid::read_value(&cell.text, crate::chat::grid::GridUnit::Amount)
                .unwrap();
        let check = check_printed(&cell, printed, &read, None, &RunResultRegistry::default());
        assert_eq!(check.status, NumberCheckStatus::Matched);
        assert_eq!(
            check.unstated_scale,
            Some(crate::domain::answer_checks::UnstatedScale::Answer)
        );
        assert_eq!(check.comparison_reason, None);
    }

    #[test]
    fn queried_cells_use_only_their_own_column_metadata() {
        for metadata in [
            json!({"unit":"억 원"}),
            json!({"axis_title":"매출 (억 원)"}),
            json!({"semantic_type":"KRW 100 million"}),
            json!({"label":"매출 (억 원)"}),
        ] {
            let source = result(
                "dataset_query",
                RunResultOrigin::Derived,
                json!({
                    "metadata": {"columns":{"revenue":metadata}}, "rows":[{"year":2024,"revenue":40,"adjustment":48}]
                }),
            );
            let compare = |text| {
                check_link(&link(text), &source, None, &RunResultRegistry::default()).unwrap()
            };
            let amount = compare("40억 원");
            assert_eq!(amount.status, NumberCheckStatus::Matched);
            assert_eq!(amount.unstated_scale, None);
            assert_eq!(amount.pointer.as_deref(), Some("/rows/0/revenue"));
            assert_eq!(
                compare("48억 원").unstated_scale,
                Some(crate::domain::answer_checks::UnstatedScale::Source)
            );
            assert_eq!(
                compare("40조 원").comparison_reason,
                Some(crate::domain::answer_checks::ComparisonReason::ScaleConflict)
            );
            assert_ne!(compare("40백만 달러").status, NumberCheckStatus::Matched);
        }
    }

    #[test]
    fn c007_distinguishes_an_unstated_scale_and_a_captured_heading() {
        use crate::domain::answer_checks::{ComparisonReason, UnstatedScale};
        let excerpt = "| Total | $245,122 |";
        let mut source = page(excerpt);
        let evidence = evidence::project(&source);
        let registry = RunResultRegistry::default();
        let answer = link("245,122 USD million");
        let unknown = check_link(&answer, &source, Some(&evidence), &registry).unwrap();
        assert_eq!(unknown.status, NumberCheckStatus::Matched);
        assert_eq!(unknown.unstated_scale, Some(UnstatedScale::Source));
        let prefix = "| (In millions, except percentages) | 2024 |\n| Revenue | |\n";
        source.payload["content"] = json!(format!("{prefix}{excerpt}"));
        let known = check_link(&answer, &source, Some(&evidence), &registry).unwrap();
        assert_eq!(known.status, NumberCheckStatus::Matched);
        assert_eq!(known.unstated_scale, None);
        assert_eq!(known.span, unknown.span);

        let scaled = page("Revenue: 245,122 USD million.");
        let receipt = evidence::project(&scaled);
        let check = check_link(&link("245,122"), &scaled, Some(&receipt), &registry).unwrap();
        assert_eq!(check.status, NumberCheckStatus::Matched);
        assert_eq!(check.unstated_scale, Some(UnstatedScale::Answer));
        let conflict = check_link(
            &link("245,122 USD billion"),
            &scaled,
            Some(&receipt),
            &registry,
        )
        .unwrap();
        assert_eq!(conflict.status, NumberCheckStatus::NotFound);
        assert_eq!(
            conflict.comparison_reason,
            Some(ComparisonReason::ScaleConflict)
        );
    }

    #[test]
    fn c007_adjacent_window_supplies_the_same_capture_heading() {
        let preceding = "| (In millions, except percentages) | 2024 |\n| Revenue | |\n";
        let text = "| Total | $245,122 |";
        let mut parent = page(preceding);
        parent.producer.runtime_id = "native-web".into();
        parent.payload["source_id"] = json!("one-filing");
        parent.payload["content_sha256"] = json!("same-capture");
        parent.payload["content_offset"] = json!(0);
        let mut current = parent.clone();
        current.result_ref = "result:selected".into();
        current.payload["content_offset"] = json!(preceding.len());
        current.payload["content"] = json!(text);
        current.upstream_result_refs.push(parent.result_ref.clone());
        let mut registry = RunResultRegistry::default();
        registry.results.insert(parent.result_ref.clone(), parent);
        let evidence = evidence::project(&current);
        let check = check_link(
            &link("245,122 USD million"),
            &current,
            Some(&evidence),
            &registry,
        )
        .unwrap();
        assert_eq!(check.status, NumberCheckStatus::Matched);
        assert!(check.unstated_scale.is_none());
        let span = check.span.unwrap();
        assert_eq!(&text[span.start..span.end], "$245,122");
    }

    #[tokio::test]
    async fn c007_revenue_table_uses_its_stated_caption_for_the_raw_sec_dollars() {
        let companies: Vec<Value> =
            serde_json::from_str(include_str!("../tests/fixtures/c007-revenue.json")).unwrap();
        let capture = ToolCapture::default();
        let mut answer = String::from("**Reported consolidated revenue (USD millions)**\n\n| Company | Fiscal 2023 | Fiscal 2024 | 2024 Form 10-K |\n|---|---:|---:|---|\n");
        for company in companies {
            let name = company["company"].as_str().unwrap();
            let content = serde_json::to_string(&company["facts"]).unwrap();
            // The follow-up reused the saved Apple and Microsoft documents.
            let mut source = if name == "Alphabet" {
                page(&content)
            } else {
                result(
                    "world_read",
                    RunResultOrigin::SavedContext,
                    json!({
                        "text": json!({"content":content}).to_string()
                    }),
                )
            };
            source.result_ref = format!("result:{name}");
            let id = evidence::evidence_id(&source.result_ref);
            let a = company["answer"][0].as_str().unwrap();
            let b = company["answer"][1].as_str().unwrap();
            answer.push_str(&format!(
                "| {name} | [{a}](#evidence-{id}) | [{b}](#evidence-{id}) | {name} filing |\n"
            ));
            capture
                .run_results
                .lock()
                .await
                .results
                .insert(source.result_ref.clone(), source);
        }
        let checks = capture.number_checks(&answer, &[]).await;
        assert_eq!(checks.len(), 6);
        for check in checks {
            assert_eq!(check.status, NumberCheckStatus::Matched, "{check:?}");
            assert!(check.scale_conversion.is_some(), "{check:?}");
        }
    }

    #[tokio::test]
    async fn c007_answer_units_respect_table_scopes_and_raw_field_precision() {
        use NumberCheckStatus::{Matched, NotFound, Rounded};
        let cases: &[(&str, &[NumberCheckStatus])] = &[
            ("| Revenue (USD m) | Employees |\n|---|---|\n| [383,285](#ID) | [383,285](#ID) |", &[Matched, NotFound]),
            ("| Fiscal 2023 (USD m) | Change (%) |\n|---|---|\n| [383,285](#ID) | [383,285](#ID) |", &[Matched, NotFound]),
            ("| USD millions | Fiscal 2023 |\n|---|---|\n| Apple | [383,285](#ID) |", &[Matched]),
            ("| Year | Revenue (bn) |\n|---|---|\n| 2023 | [383](#ID) |", &[Rounded]),
            ("| Year | Revenue |\n|---|---|\n| 2023 | [383,285](#ID) |\n\n$ in millions", &[Matched]),
            ("| Metric | 2023 |\n|---|---|\n| Revenue (bn) | [383.3](#ID) |", &[Rounded]),
            ("매출 (억 원) [3,832.85](#ID).", &[Matched]),
            ("매출 (조 원) [0.383285](#ID).", &[Matched]),
            ("매출 (백만 달러) [383,285](#ID).", &[Matched]),
            ("Revenue (百万) [383,285](#ID).", &[Matched]),
            ("Revenue (億) [3,832.85](#ID).", &[Matched]),
            ("| 2023 |\n|---|\n| [383,285](#ID) |", &[NotFound]),
            ("We sold 2 million units. Revenue was [383,285](#ID).", &[NotFound]),
        ];
        for &(markdown, expected) in cases {
            let currency = if markdown.contains("원") {
                "KRW"
            } else {
                "USD"
            };
            let source = result(
                "observations",
                RunResultOrigin::AcquiredOriginal,
                json!({
                    "dataset_ref": {"dataset_id":"ds_revenue"},
                    "data": [{"unit":currency,"val":383285000000_i64}]
                }),
            );
            let id = evidence::evidence_id(&source.result_ref);
            let capture = ToolCapture::default();
            capture
                .run_results
                .lock()
                .await
                .results
                .insert(source.result_ref.clone(), source);
            let answer = markdown.replace("#ID", &format!("#evidence-{id}"));
            let checks = capture.number_checks(&answer, &[]).await;
            let statuses: Vec<_> = checks.iter().map(|c| c.status).collect();
            assert_eq!(statuses, expected, "{markdown}: {checks:?}");
            if expected == [NotFound] {
                assert_eq!(
                    serde_json::to_value(&checks[0]).unwrap()["comparisonReason"],
                    "no_candidate"
                );
                assert!(checks[0].compared_value.is_none());
            }
        }
    }

    #[test]
    fn c002_tsmc_closing_price_and_decline_match_the_cited_passage() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/c002-tsmc-close.json")).unwrap();
        let passage = fixture["excerpt"].as_str().unwrap();
        let read = result(
            "web_fetch",
            RunResultOrigin::AcquiredOriginal,
            json!({"content":passage,"url":fixture["url"]}),
        );
        let evidence: ChatEvidenceRef = serde_json::from_value(json!({
            "id":"ev","title":"TSMC","kind":"original","tool_name":"web_fetch",
            "retrieved_at":"2026-11-21T13:45:00Z","excerpt":passage,
            "excerpt_truncated":false,"content_sha256":"a".repeat(64),"warnings":[]
        }))
        .unwrap();
        let links = evidence_links(fixture["answer"].as_str().unwrap());
        assert_eq!(links.len(), 2);
        for (link, digits) in links.iter().zip(["1,385", "4.81%"]) {
            for excerpt in [Some(&evidence), None] {
                let check =
                    check_link(link, &read, excerpt, &RunResultRegistry::default()).unwrap();
                assert_eq!(check.status, NumberCheckStatus::Matched, "{check:?}");
                assert!(check.comparison_reason.is_none());
                let located = match (check.span, check.context.as_ref()) {
                    (Some(span), _) => &passage[span.start..span.end],
                    (_, Some(ctx)) => &ctx.text[ctx.start..ctx.end],
                    _ => panic!("missing evidence location"),
                };
                assert_eq!(located.trim_end_matches('元'), digits);
            }
        }
        let opposite = check_link(
            &link("+4.81%"),
            &read,
            Some(&evidence),
            &RunResultRegistry::default(),
        )
        .unwrap();
        assert_eq!(opposite.status, NumberCheckStatus::NotFound);
        assert_eq!(
            serde_json::to_value(&opposite).unwrap()["comparisonReason"],
            "sign_conflict"
        );
        assert_eq!(opposite.compared_value.as_deref(), Some("4.81%"));
    }

    /// rc2 RP-002, both trials: `−9%` and `-9%` cite "판매량이 2분기보다 약 9%
    /// 줄었고"; the article states the sign in the word after the number.
    #[test]
    fn rc2_rp002_a_signed_change_matches_a_fall_its_passage_states_in_words() {
        let article = page(EARNINGS_ARTICLE);
        let evidence: ChatEvidenceRef = serde_json::from_value(json!({
            "id":"ev", "title":"한빛화학 3분기", "kind":"original", "tool_name":"web_fetch",
            "retrieved_at":"2026-11-19T18:14:13Z", "excerpt": EARNINGS_ARTICLE,
            "excerpt_truncated":false, "content_sha256":"a".repeat(64), "warnings":[]
        }))
        .unwrap();
        let registry = RunResultRegistry::default();
        for excerpt in [Some(&evidence), None] {
            let check = |text: &str| {
                check_link(&link(text), &article, excerpt, &registry)
                    .unwrap()
                    .status
            };
            for (text, status) in [
                ("−9%", NumberCheckStatus::Matched),
                ("-9%", NumberCheckStatus::Matched),
                // An unsigned figure still matches; its own words state the fall.
                ("9%", NumberCheckStatus::Matched),
                ("14억원", NumberCheckStatus::Matched),
                // The opposite sign, or other digits, do not.
                ("+9%", NumberCheckStatus::NotFound),
                ("−8%", NumberCheckStatus::NotFound),
                // A particle or word between states nothing: `52억원으로 … 줄었다`,
                // `248달러보다 낮아졌다`, `90% 이상으로`, `14억원이 … 반영됐다`.
                ("−52억원", NumberCheckStatus::NotFound),
                ("−248달러", NumberCheckStatus::NotFound),
                ("−90%", NumberCheckStatus::NotFound),
                ("−14억원", NumberCheckStatus::NotFound),
            ] {
                assert_eq!(
                    check(text),
                    status,
                    "{text} with excerpt {}",
                    excerpt.is_some()
                );
            }
            let matched = check_link(&link("−9%"), &article, excerpt, &registry).unwrap();
            let located = match (matched.span, matched.context) {
                (Some(span), None) => EARNINGS_ARTICLE[span.start..span.end].to_owned(),
                (None, Some(context)) => context.text[context.start..context.end].to_owned(),
                other => panic!("{other:?}"),
            };
            assert_eq!(located, "9%");
        }
        // A word of increase leaves the number a rise: `−9%` is not found.
        for (content, rise, fall) in [
            (
                EARNINGS_ARTICLE.replace("9% 줄었고", "9% 늘었고"),
                true,
                false,
            ),
            (
                EARNINGS_ARTICLE.replace("9% 줄었고", "9%감소했고"),
                false,
                true,
            ),
            (
                "Unit sales were 9% lower than in the second quarter.".to_owned(),
                false,
                true,
            ),
            (
                "Sales posted a 9% decline in the quarter.".to_owned(),
                false,
                true,
            ),
            (
                "Unit sales were 9% higher than in the second quarter.".to_owned(),
                true,
                false,
            ),
            // A line break ends the number's words.
            ("Unit sales: 9%\nlower costs".to_owned(), true, false),
        ] {
            let source = page(&content);
            let check = |text: &str| {
                check_link(&link(text), &source, None, &registry)
                    .unwrap()
                    .status
            };
            let found = |yes: bool| {
                if yes {
                    NumberCheckStatus::Matched
                } else {
                    NumberCheckStatus::NotFound
                }
            };
            assert_eq!(check("+9%"), found(rise), "+9% in {content}");
            assert_eq!(check("−9%"), found(fall), "−9% in {content}");
            assert_eq!(check("9%"), NumberCheckStatus::Matched, "9% in {content}");
        }
    }

    #[test]
    fn conversion_currency_basis_is_serialized_in_the_popover_input() {
        let payload = json!({"operation":"convert",
            "inputs":[{"name":"amount","display":"3,930","currency_basis":"declared"},
                {"name":"rate","display":"1,334.87"}]});
        let detail = calc_detail(&payload, &RunResultRegistry::default());
        let stored = serde_json::to_value(&detail).unwrap();
        assert_eq!(stored["inputs"][0]["currency_basis"], "declared");
        assert!(stored["inputs"][1].get("currency_basis").is_none());
        assert_eq!(
            serde_json::from_value::<NumberCheckCalc>(stored).unwrap(),
            detail
        );
    }
}
