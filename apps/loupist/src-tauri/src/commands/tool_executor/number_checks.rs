//! Number checks at finalization (finance-show-the-math.md §7.2). Each
//! `[text](#evidence-…)` link in the answer whose whole text reads as one
//! number is compared with the evidence it links, using this turn's retained
//! results. The model's text never sets a status; Rust reads the number it
//! printed and looks for it in the source.
use super::*;
use crate::domain::{
    answer_checks::{
        bounded_calc, bounded_fields, clip, NumberCheckContext, MAX_NUMBER_CHECK_CONTEXT_CHARS,
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

/// Evidence links outside code, in reading order.
pub(in crate::commands) fn evidence_links(content: &str) -> Vec<EvidenceLink> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut links = Vec::new();
    let mut open: Option<(String, String)> = None;
    for event in Parser::new(content) {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                open = evidence_id(&dest_url).map(|id| (id.to_owned(), String::new()));
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, label)) = &mut open {
                    label.push_str(&text);
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some((evidence_id, text)) = open.take() {
                    links.push(EvidenceLink {
                        index: links.len() as u32,
                        text,
                        evidence_id,
                    });
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
            out.push(Candidate {
                value,
                unit,
                place: Place::Pointer(format!("/{list}/{index}/value")),
                stated_fall: false,
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
    for (row, cells) in grid["values"].as_array().into_iter().flatten().enumerate() {
        for (column, cell) in cells.as_array().into_iter().flatten().enumerate() {
            if let Some(value) = cell.as_f64() {
                out.push(Candidate {
                    value,
                    unit,
                    place: Place::Pointer(format!("/grid/values/{row}/{column}")),
                    stated_fall: false,
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
                    place: Place::Pointer(format!("/grid/{axis}/{index}")),
                    stated_fall: false,
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
                place: Place::Pointer("/formulas/0".into()),
                stated_fall: false,
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
fn locate(text: &str, candidate: &Candidate) -> Option<(usize, usize)> {
    let candidates = numbers::passage_candidates(text);
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
fn printed_at(payload: &Value, pointer: &str, candidate: &Candidate) -> Option<String> {
    match payload.pointer(pointer)? {
        Value::String(text) => Some(
            locate(text, candidate)
                .map_or_else(|| text.clone(), |(start, end)| text[start..end].to_owned()),
        ),
        other => printed(other),
    }
}

/// Locate the selected fallback candidate again in its content field. The
/// candidate's pointer came from `content_candidates`, never from metadata.
fn retained_context(payload: &Value, candidate: &Candidate) -> Option<NumberCheckContext> {
    let Place::Pointer(pointer) = &candidate.place else {
        return None;
    };
    let text = printed(payload.pointer(pointer)?)?;
    let (start, end) = if let Some(found) = locate(&text, candidate) {
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

/// Checks one printed number against one retained result.
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
    mut printed_number: numbers::Printed,
    result: &RunResult,
    evidence: Option<&ChatEvidenceRef>,
    registry: &RunResultRegistry,
) -> NumberCheck {
    let kind = kind_of(result);
    let payload = &result.payload;
    let excerpt = evidence.map(|evidence| evidence.excerpt.as_str());
    let candidates = match kind {
        NumberCheckKind::Calculated => calc_candidates(payload),
        NumberCheckKind::Data | NumberCheckKind::Step => numbers::data_candidates(payload),
        NumberCheckKind::Passage => numbers::passage_candidates(excerpt.unwrap_or_default()),
    };
    let calculated = kind == NumberCheckKind::Calculated;
    let target_currency = (calculated && payload["operation"] == "convert")
        .then(|| payload["conversion"]["to"].as_str())
        .flatten();
    if target_currency.is_some() && printed_number.unit == Unit::Plain {
        printed_number.unit = Unit::Amount(1.0);
    }
    let (mut verdict, mut index) = numbers::check(&printed_number, &candidates, calculated);
    // The agent's own code prints a percent or point change as a bare number,
    // in the figure's unit (`'qoq_pp': -1.374…`) or as a fraction (`0.0770…`
    // for 7.70%). Read a bare number in its output both ways: as printed in
    // the figure's unit (matched), else as a fraction ×100 (rounded). The
    // step's other fields, such as its exit code, are not output.
    if kind == NumberCheckKind::Step
        && result.producer.tool_name == "run_python"
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
            .any(|currency| currency != target)
    }) {
        verdict = Verdict::Differs;
    }
    // Unsigned data magnitudes may describe a fall in words. Explicit signs
    // (including accounting marks) always retain their ordinary meaning.
    let explicit_sign = link
        .text
        .chars()
        .any(|c| matches!(c, '+' | '-' | '−' | '△' | '▽' | '▼' | '▲' | '('));
    if kind == NumberCheckKind::Data
        && verdict == Verdict::NotFound
        && !explicit_sign
        && printed_number.value > 0.0
    {
        let negative = numbers::Printed {
            value: -printed_number.value,
            ..printed_number
        };
        (verdict, index) = numbers::check(&negative, &candidates, false);
    }
    let mut found = index.and_then(|index| candidates.get(index));
    let mut context = None;
    if kind == NumberCheckKind::Passage && verdict == Verdict::NotFound {
        // The retained excerpt is bounded; the full retained result still
        // decides the status. Keep context when it locates the fallback match.
        // Only content fields count, never the result's metadata.
        let content = numbers::content_candidates(payload);
        let (whole, index) = numbers::check(&printed_number, &content, false);
        if matches!(whole, Verdict::Matched | Verdict::Rounded) {
            context = index
                .and_then(|index| content.get(index))
                .and_then(|candidate| retained_context(payload, candidate));
        }
        if whole != Verdict::NotFound {
            verdict = whole;
            found = None;
        }
    }
    let mut check = NumberCheck {
        link_index: link.index,
        text: clip(&link.text, 256),
        evidence_id: link.evidence_id.clone(),
        kind,
        status: status_of(verdict),
        nearest: None,
        exact: None,
        pointer: None,
        span: None,
        context,
        fields: Vec::new(),
        calc: calculated.then(|| calc_detail(payload, registry)),
    };
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
        Place::Pointer(pointer) => printed_at(payload, pointer, found),
    }
    .map(|text| clip(text.trim(), 256));
    match verdict {
        Verdict::Differs => check.nearest = as_printed,
        Verdict::Rounded => check.exact = as_printed,
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
                check.pointer = Some(clip(pointer, 2048));
                if kind == NumberCheckKind::Data {
                    check.fields = row_fields(payload, pointer);
                }
            }
        }
    }
    check
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
        let links = evidence_links(content);
        if links.is_empty() {
            return Vec::new();
        }
        let registry = self.run_results.lock().await;
        let by_id: BTreeMap<String, &RunResult> = registry
            .values()
            .map(|result| (evidence::evidence_id(&result.result_ref), result))
            .collect();
        let mut checks = Vec::new();
        for link in &links {
            if checks.len() >= MAX_NUMBER_CHECKS {
                break;
            }
            let Some(result) = by_id.get(&link.evidence_id) else {
                continue;
            };
            let evidence = evidence.iter().find(|item| item.id == link.evidence_id);
            if let Some(check) = check_link(link, result, evidence, &registry) {
                checks.push(check);
            }
        }
        checks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(buys.status, NumberCheckStatus::Rounded);
        assert_eq!(
            buys.pointer.as_deref(),
            Some("/data/response/output2/0/frgn_ntby_tr_pbmn")
        );
        assert_eq!(check("-6.1억 원").status, NumberCheckStatus::Rounded);
        assert_eq!(check("+214억 원").status, NumberCheckStatus::NotFound);
        assert_eq!(check("+2140원").status, NumberCheckStatus::NotFound);
    }

    #[test]
    fn kis_uses_only_documented_operation_and_field_units() {
        let registry = RunResultRegistry::default();
        for (operation, container, field, expected) in [
            (
                "domestic_stock.investor_trade_by_stock_daily",
                "output2",
                "etc_corp_ntby_tr_pbmn",
                NumberCheckStatus::Rounded,
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
                NumberCheckStatus::Rounded,
            ),
            (
                "Revenue was $12 thousand then $12 million.",
                "$12,000,000",
                "$12 million",
                NumberCheckStatus::Rounded,
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
    fn excerpt_matches_missing_numbers_and_data_never_carry_fallback_context() {
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
            "매출 [4,523억원](#evidence-ev_page)",
        );
        message.number_checks.push(check.clone());
        chat.messages.push(message);
        state.store.create_chat(&chat).unwrap();
        let loaded = state.store.get_chat("chat-a").unwrap().unwrap();
        assert_eq!(loaded.messages[0].number_checks, [check]);
        let mut older = serde_json::to_value(&loaded.messages[0]).unwrap();
        older["numberChecks"][0]
            .as_object_mut()
            .unwrap()
            .remove("context");
        let older: crate::domain::ChatMessage = serde_json::from_value(older).unwrap();
        older.validate().unwrap();
        assert!(older.number_checks[0].context.is_none());
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
        assert_eq!(billion.status, NumberCheckStatus::Rounded);
        assert_eq!(billion.pointer.as_deref(), Some("/data/document/text"));
        // The popover's "rounded from" names the number, not the whole text.
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
