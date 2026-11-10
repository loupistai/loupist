//! Number checks at finalization (finance-show-the-math.md §7.2). Each
//! `[text](#evidence-…)` link in the answer whose whole text reads as one
//! number is compared with the evidence it links, using this turn's retained
//! results. The model's text never sets a status; Rust reads the number it
//! printed and looks for it in the source.
use super::*;
use crate::domain::{
    answer_checks::{bounded_calc, bounded_fields, clip},
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

/// Evidence links outside code, in reading order.
pub(in crate::commands) fn evidence_links(content: &str) -> Vec<EvidenceLink> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut links = Vec::new();
    let mut open: Option<(String, String)> = None;
    for event in Parser::new(content) {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                open = dest_url
                    .strip_prefix("#evidence-")
                    .filter(|id| {
                        !id.is_empty()
                            && id.len() <= 128
                            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    })
                    .map(|id| (id.to_owned(), String::new()));
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
    } else if result.payload.get("dataset_ref").is_some() {
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

/// A calculation's values first, then its inputs.
fn calc_candidates(payload: &Value) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (list, unit_of) in [("values", true), ("inputs", false)] {
        for (index, item) in payload
            .get(list)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let Some(value) = item.get("value").and_then(Value::as_f64) else {
                continue;
            };
            let unit = if unit_of {
                calc_unit(item.get("unit").and_then(Value::as_str))
            } else {
                Unit::Plain
            };
            out.push(Candidate {
                value,
                unit,
                place: Place::Pointer(format!("/{list}/{index}/value")),
            });
        }
    }
    out
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
fn split_pointer(pointer: &str) -> Option<(&str, String)> {
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
    printed_number: numbers::Printed,
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
    let (mut verdict, index) = numbers::check(&printed_number, &candidates, calculated);
    let mut found = index.and_then(|index| candidates.get(index));
    if kind == NumberCheckKind::Passage && verdict == Verdict::NotFound {
        // The retained excerpt is bounded; the full retained result still
        // decides the status, without a span to mark.
        // Only content fields count, never the result's metadata.
        let (whole, _) = numbers::check(
            &printed_number,
            &numbers::content_candidates(payload),
            false,
        );
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
            let display = pointer.trim_end_matches("/value").to_owned() + "/display";
            payload
                .pointer(&display)
                .and_then(Value::as_str)
                .map(str::to_owned)
        }
        Place::Pointer(pointer) => payload.pointer(pointer).and_then(printed),
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

    /// The popover reads each input's label and unit from the stored check,
    /// copied from the host's own calculation (B98).
    #[test]
    fn calculated_checks_keep_each_inputs_label_and_unit() {
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
        assert_eq!(status("12,345"), NumberCheckStatus::NotFound);
    }
}
